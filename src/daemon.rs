//! Background service: mirrors device batteries into the kernel so GNOME/UPower show them.

use crate::features;
use crate::hidpp::{self, Device, feature as f};
use crate::uhid::{BatteryDevice, Kind};
use anyhow::Result;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(120);

fn kind(dev: &mut Device) -> Kind {
    // DEVICE_NAME fn 2 = getDeviceType: 0 keyboard, 2 numpad, 3 mouse, 4 trackpad, 5 trackball...
    match dev.call(f::DEVICE_NAME, 2, &[]).map(|r| r[0]) {
        Ok(0) | Ok(2) => Kind::Keyboard,
        _ => Kind::Mouse,
    }
}

pub fn run() -> Result<()> {
    let mut mirrors: HashMap<String, BatteryDevice> = HashMap::new();
    let mut last: HashMap<String, (u8, bool)> = HashMap::new();
    loop {
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
                    if !mirrors.contains_key(&name) {
                        let uniq = format!("hidpilot-{}", name.to_lowercase().replace(' ', "-"));
                        match BatteryDevice::create(&name, &uniq, kind(&mut dev)) {
                            Ok(m) => {
                                eprintln!("hidpilot: publishing battery of {name}");
                                mirrors.insert(name.clone(), m);
                            }
                            Err(e) => {
                                eprintln!("hidpilot: {e:#}");
                                continue;
                            }
                        }
                        // Give the kernel a moment to bind the new device before the first report.
                        thread::sleep(Duration::from_millis(300));
                    }
                    if last.get(&name) != Some(&(percent, charging)) {
                        eprintln!("hidpilot: {name} {percent}%{}", if charging { " charging" } else { "" });
                        last.insert(name.clone(), (percent, charging));
                    }
                    if let Some(m) = mirrors.get_mut(&name) {
                        if let Err(e) = m.update(percent, charging) {
                            eprintln!("hidpilot: {name}: {e:#}");
                            mirrors.remove(&name);
                        }
                    }
                }
            }
            Err(e) => eprintln!("hidpilot: discovery failed: {e:#}"),
        }
        // Devices that are asleep keep their last reported level.
        thread::sleep(POLL);
    }
}
