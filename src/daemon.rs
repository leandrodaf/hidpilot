//! Background service: mirrors device batteries into the kernel so GNOME/UPower show them.
//!
//! Levels are re-read every few minutes and right after the devices say something changed:
//! a watcher thread per hidraw node listens for HID++ notifications (battery broadcasts,
//! devices connecting) and Centurion events from the headset, and wakes the main loop.

use crate::centurion;
use crate::features;
use crate::hidpp::{self, Device, LOGITECH, feature as f};
use crate::hidraw::{self, Hidraw};
use crate::uhid::{BatteryDevice, Kind};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(300);
/// Several notifications usually arrive together; refresh once after they settle.
const SETTLE: Duration = Duration::from_secs(2);
/// Never refresh more often than this because of events alone.
const MIN_GAP: Duration = Duration::from_secs(10);

const HIDPP_SHORT: u8 = 0x10;
const HIDPP_LONG: u8 = 0x11;
const CENTURION: u8 = 0x22;
/// HID++ 1.0 "device connection" notification sub id.
const DEVICE_CONNECTION: u8 = 0x41;

fn kind(dev: &mut Device) -> Kind {
    if features::is_keyboard(dev) { Kind::Keyboard } else { Kind::Mouse }
}

/// True for reports the devices send on their own (not replies to our requests).
fn is_event(r: &[u8]) -> bool {
    match r.first() {
        Some(&HIDPP_SHORT) | Some(&HIDPP_LONG) if r.len() >= 4 => {
            // HID++ 1.0 connection notice, or a HID++ 2.0 feature event (software id 0).
            r[2] == DEVICE_CONNECTION || (r[2] != 0xFF && r[2] < 0x80 && r[3] & 0x0F == 0)
        }
        // 22 addr len flags idx fn|swid: events carry software id 0.
        Some(&CENTURION) if r.len() >= 6 => r[5] & 0x0F == 0,
        _ => false,
    }
}

fn watched_nodes() -> Vec<PathBuf> {
    hidraw::enumerate()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| i.vendor == LOGITECH && (i.has_report_id(HIDPP_LONG) || i.has_report_id(CENTURION)))
        .map(|i| i.path)
        .collect()
}

enum Wake {
    Event,
    /// The watcher of this node stopped (device unplugged or re-enumerated).
    Gone(PathBuf),
}

/// Starts a listener for every new node; each one ends by itself when its device goes away.
fn watch(nodes: &mut HashSet<PathBuf>, tx: &Sender<Wake>) {
    for path in watched_nodes() {
        if nodes.contains(&path) {
            continue;
        }
        let Ok(mut io) = Hidraw::open(&path) else { continue };
        nodes.insert(path.clone());
        let tx = tx.clone();
        thread::spawn(move || {
            loop {
                match io.read_timeout(Duration::from_secs(60)) {
                    Ok(Some(r)) if is_event(&r) => {
                        if tx.send(Wake::Event).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    // Unplugged (or the receiver re-enumerated, as the Zone one does when
                    // the headset turns on or off): tell the loop so it rescans.
                    Err(_) => {
                        let _ = tx.send(Wake::Gone(path));
                        return;
                    }
                }
            }
        });
    }
}

struct Mirror {
    dev: BatteryDevice,
    last: Option<(u8, bool)>,
}

#[derive(Default)]
struct Mirrors(HashMap<String, Mirror>);

impl Mirrors {
    fn publish(&mut self, key: &str, name: &str, uniq: &str, kind: Kind, percent: u8, charging: bool) {
        if !self.0.contains_key(key) {
            match BatteryDevice::create(name, uniq, kind) {
                Ok(dev) => {
                    eprintln!("hidpilot: publishing battery of {name}");
                    self.0.insert(key.to_string(), Mirror { dev, last: None });
                    // Give the kernel a moment to bind the new device before the first report.
                    thread::sleep(Duration::from_millis(300));
                }
                Err(e) => {
                    eprintln!("hidpilot: {e:#}");
                    return;
                }
            }
        }
        let m = self.0.get_mut(key).unwrap();
        if m.last != Some((percent, charging)) {
            eprintln!("hidpilot: {name} {percent}%{}", if charging { " charging" } else { "" });
            m.last = Some((percent, charging));
        }
        if let Err(e) = m.dev.update(percent, charging) {
            eprintln!("hidpilot: {name}: {e:#}");
            self.0.remove(key);
        }
    }

    fn drop_key(&mut self, key: &str) {
        if self.0.remove(key).is_some() {
            eprintln!("hidpilot: {key} went away");
        }
    }
}

fn refresh(mirrors: &mut Mirrors) {
    match hidpp::discover() {
        Ok(devices) => {
            for mut dev in devices {
                if !dev.has(f::UNIFIED_BATTERY) {
                    continue;
                }
                let Ok(b) = features::battery(&mut dev) else { continue };
                let percent = b.percent.unwrap_or(match b.level {
                    "full" => 100,
                    "good" => 60,
                    "low" => 20,
                    "critical" => 5,
                    _ => 0,
                });
                let charging = matches!(b.status, "charging" | "almost full" | "charging (slow)");
                let name = dev.name.clone();
                let uniq = format!("hidpilot-{}", name.to_lowercase().replace(' ', "-"));
                let k = kind(&mut dev);
                mirrors.publish(&name, &name, &uniq, k, percent, charging);
            }
            // Devices that are asleep keep their last reported level.
        }
        Err(e) => eprintln!("hidpilot: discovery failed: {e:#}"),
    }

    // The headset has no "asleep" state worth keeping: off means no battery to show.
    const HEADSET: &str = "headset";
    match centurion::find() {
        Ok(Some(mut h)) => match h.battery() {
            Ok(b) => {
                let serial = h.serial().unwrap_or_else(|_| "zone".into());
                let uniq = format!("hidpilot-headset-{}", serial.to_lowercase());
                let name = h.name.clone();
                mirrors.publish(HEADSET, &name, &uniq, Kind::Headset, b.percent, b.charging);
            }
            Err(e) => eprintln!("hidpilot: headset battery: {e:#}"),
        },
        Ok(None) => mirrors.drop_key(HEADSET),
        Err(e) => eprintln!("hidpilot: headset: {e:#}"),
    }
}

fn forget(nodes: &mut HashSet<PathBuf>, w: Wake) {
    if let Wake::Gone(p) = w {
        nodes.remove(&p);
    }
}

pub fn run() -> Result<()> {
    let (tx, rx): (Sender<Wake>, Receiver<Wake>) = mpsc::channel();
    let mut nodes = HashSet::new();
    let mut mirrors = Mirrors::default();
    loop {
        watch(&mut nodes, &tx);
        refresh(&mut mirrors);
        let last_refresh = Instant::now();

        // Sleep until the poll interval or an event; then let a burst of events settle.
        let deadline = Instant::now() + POLL;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(w) => {
                    forget(&mut nodes, w);
                    thread::sleep(SETTLE);
                    while let Ok(w) = rx.try_recv() {
                        forget(&mut nodes, w);
                    }
                    let wait = MIN_GAP.saturating_sub(last_refresh.elapsed());
                    thread::sleep(wait);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => unreachable!("the loop keeps a sender"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_event;

    #[test]
    fn events_and_replies() {
        // Unified battery broadcast from slot 2 (feature index 8, event 0, software id 0).
        assert!(is_event(&[0x11, 0x02, 0x08, 0x00, 0x5A, 0x02, 0x00]));
        // Receiver: device 1 connected.
        assert!(is_event(&[0x10, 0x01, 0x41, 0x0C, 0x40, 0x82, 0xB0]));
        // Our own reply (software id 0xA) and a HID++ 2.0 error: not events.
        assert!(!is_event(&[0x11, 0x02, 0x08, 0x1A, 0x5A]));
        assert!(!is_event(&[0x11, 0x02, 0xFF, 0x08, 0x1A, 0x02]));
        // Headset battery event vs. a reply to one of our requests.
        assert!(is_event(&[0x22, 0xF1, 0x05, 0x00, 0x04, 0x00, 0x58, 0x58, 0x00]));
        assert!(!is_event(&[0x22, 0xF1, 0x05, 0x00, 0x04, 0x0A, 0x58, 0x58, 0x00]));
        assert!(!is_event(&[0x01, 0x00]));
    }
}
