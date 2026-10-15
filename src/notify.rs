//! Low-battery desktop notifications, run as the logged-in user (notifications belong to
//! the user's session, never to the root daemon).
//!
//! Levels come from the power supplies the battery daemon publishes in sysfs; if the daemon
//! is not running, the devices are read directly.

use crate::centurion;
use crate::features;
use crate::hidpp::{self, feature as f};
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(120);
pub const LOW: u8 = 15;
/// Warn again only after the battery came back above this (or started charging).
const REARM: u8 = 25;

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub name: String,
    pub percent: u8,
    pub charging: bool,
}

/// Batteries mirrored by `hidpilot daemon`: power supplies named hid-hidpilot-*-battery.
fn from_sysfs() -> Vec<Level> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else { return out };
    for e in entries.flatten() {
        let id = e.file_name().to_string_lossy().into_owned();
        if !id.starts_with("hid-hidpilot-") {
            continue;
        }
        let read = |attr: &str| fs::read_to_string(e.path().join(attr)).map(|s| s.trim().to_string()).ok();
        let Some(percent) = read("capacity").and_then(|c| c.parse::<u8>().ok()) else { continue };
        let name = read("model_name").filter(|n| !n.is_empty()).unwrap_or(id);
        let charging = matches!(read("status").as_deref(), Some("Charging") | Some("Full"));
        out.push(Level { name, percent, charging });
    }
    out
}

fn from_devices() -> Vec<Level> {
    let mut out = Vec::new();
    if let Ok(devices) = hidpp::discover() {
        for mut d in devices {
            if !d.has(f::UNIFIED_BATTERY) {
                continue;
            }
            if let Ok(b) = features::battery(&mut d) {
                if let Some(percent) = b.percent {
                    let charging = matches!(b.status, "charging" | "almost full" | "charging (slow)" | "full");
                    out.push(Level { name: d.name.clone(), percent, charging });
                }
            }
        }
    }
    if let Ok(Some(mut h)) = centurion::find() {
        if let Ok(b) = h.battery() {
            out.push(Level { name: h.name.clone(), percent: b.percent, charging: b.charging || b.full });
        }
    }
    out
}

pub fn levels() -> Vec<Level> {
    let mirrored = from_sysfs();
    if mirrored.is_empty() { from_devices() } else { mirrored }
}

fn send(level: &Level) {
    let body = format!("{}% left. Charge it soon.", level.percent);
    let title = format!("{} battery low", level.name);
    let r = Command::new("notify-send")
        .args(["--app-name=hidpilot", "--icon=battery-caution", "--urgency=critical", &title, &body])
        .status();
    if let Err(e) = r {
        eprintln!("hidpilot: notify-send: {e} (install libnotify-bin)");
    }
}

/// Decides which devices to warn about now; `armed` remembers who was already warned.
pub fn due(levels: &[Level], armed: &mut HashMap<String, bool>) -> Vec<Level> {
    let mut out = Vec::new();
    for l in levels {
        let ready = armed.entry(l.name.clone()).or_insert(true);
        if l.charging || l.percent >= REARM {
            *ready = true;
        } else if *ready && l.percent <= LOW {
            *ready = false;
            out.push(l.clone());
        }
    }
    out
}

pub fn run() -> Result<()> {
    let mut armed = HashMap::new();
    loop {
        for l in due(&levels(), &mut armed) {
            send(&l);
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(p: u8, charging: bool) -> Vec<Level> {
        vec![Level { name: "MX Master 3S".into(), percent: p, charging }]
    }

    #[test]
    fn warns_once_per_discharge() {
        let mut armed = HashMap::new();
        assert!(due(&lv(40, false), &mut armed).is_empty());
        assert_eq!(due(&lv(15, false), &mut armed).len(), 1);
        assert!(due(&lv(12, false), &mut armed).is_empty(), "no repeat while still low");
        assert!(due(&lv(12, true), &mut armed).is_empty(), "charging re-arms, no warning");
        assert_eq!(due(&lv(14, false), &mut armed).len(), 1);
    }
}
