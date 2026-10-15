//! HID++ 2.0 transport for Logitech receivers (Bolt/Unifying) and Bluetooth devices.

use crate::hidraw::{self, BUS_BLUETOOTH, BUS_USB, Hidraw, HidrawInfo};
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const LOGITECH: u16 = 0x046d;
const SHORT: u8 = 0x10;
const LONG: u8 = 0x11;
const LONG_LEN: usize = 20;
const SOFTWARE_ID: u8 = 0x0A;
const TIMEOUT: Duration = Duration::from_secs(2);
// Sleeping devices take over a second to wake; empty slots fail immediately.
const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

/// Well-known HID++ 2.0 feature IDs.
pub mod feature {
    pub const ROOT: u16 = 0x0000;
    pub const FEATURE_SET: u16 = 0x0001;
    pub const FIRMWARE: u16 = 0x0003;
    pub const DEVICE_NAME: u16 = 0x0005;
    pub const UNIFIED_BATTERY: u16 = 0x1004;
    pub const CHANGE_HOST: u16 = 0x1814;
    pub const HOSTS_INFO: u16 = 0x1815;
    pub const BACKLIGHT2: u16 = 0x1982;
    pub const REPROG_CONTROLS_V4: u16 = 0x1b04;
    pub const SMART_SHIFT: u16 = 0x2110;
    pub const SMART_SHIFT_ENHANCED: u16 = 0x2111;
    pub const HIRES_WHEEL: u16 = 0x2121;
    pub const THUMB_WHEEL: u16 = 0x2150;
    pub const ADJUSTABLE_DPI: u16 = 0x2201;
    pub const K375S_FN_INVERSION: u16 = 0x40a3;
    pub const MULTIPLATFORM: u16 = 0x4531;
}

#[derive(Debug)]
pub enum HidppError {
    /// HID++ 2.0 error reply with its error code.
    Feature(u8),
    /// The receiver says the device in that slot is not reachable.
    Offline,
    Timeout,
}

impl std::fmt::Display for HidppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HidppError::Feature(code) => {
                let name = match code {
                    1 => "unknown",
                    2 => "invalid argument",
                    3 => "out of range",
                    4 => "hardware error",
                    5 => "logitech internal",
                    6 => "invalid feature index",
                    7 => "invalid function",
                    8 => "busy",
                    9 => "unsupported",
                    _ => "?",
                };
                write!(f, "device returned error {code:#04x} ({name})")
            }
            HidppError::Offline => write!(f, "device is offline (asleep or switched off)"),
            HidppError::Timeout => write!(f, "device did not answer"),
        }
    }
}

impl std::error::Error for HidppError {}

pub struct Device {
    io: Hidraw,
    /// Device slot: 1..=6 behind a receiver, 0xFF when directly connected.
    pub index: u8,
    pub name: String,
    pub via: Link,
    features: HashMap<u16, u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Receiver,
    Bluetooth,
}

impl Device {
    fn new(path: &PathBuf, index: u8, via: Link) -> Result<Self> {
        Ok(Self { io: Hidraw::open(path)?, index, name: String::new(), via, features: HashMap::new() })
    }

    pub fn path(&self) -> &PathBuf {
        &self.io.path
    }

    /// Sends a request to a feature index and returns the reply parameters (16 bytes).
    pub fn call_index(&mut self, index: u8, function: u8, params: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let mut msg = [0u8; LONG_LEN];
        msg[0] = LONG;
        msg[1] = self.index;
        msg[2] = index;
        msg[3] = (function << 4) | SOFTWARE_ID;
        let n = params.len().min(LONG_LEN - 4);
        msg[4..4 + n].copy_from_slice(&params[..n]);
        self.io.write(&msg)?;

        let deadline = Instant::now() + timeout;
        while let Some(r) = self.io.read_until(deadline)? {
            if r.len() < 4 || (r[0] != SHORT && r[0] != LONG) || r[1] != self.index {
                continue;
            }
            // HID++ 1.0 error from the receiver: 10 idx 8F <sub_id> <addr> <err>
            if r[2] == 0x8F && r.len() >= 6 && r[3] == index {
                return Err(HidppError::Offline.into());
            }
            // HID++ 2.0 error: 11 idx FF <feat_idx> <fn|sw> <err>
            if r[2] == 0xFF && r.len() >= 6 && r[3] == index && r[4] == msg[3] {
                return Err(HidppError::Feature(r[5]).into());
            }
            if r[2] == index && r[3] == msg[3] {
                return Ok(r[4..].to_vec());
            }
        }
        Err(HidppError::Timeout.into())
    }

    /// Feature index for a feature ID, or None if the device lacks it.
    pub fn feature_index(&mut self, id: u16) -> Result<Option<u8>> {
        if id == feature::ROOT {
            return Ok(Some(0));
        }
        if let Some(&i) = self.features.get(&id) {
            return Ok((i != 0).then_some(i));
        }
        let r = self.call_index(0, 0, &id.to_be_bytes(), TIMEOUT)?;
        self.features.insert(id, r[0]);
        Ok((r[0] != 0).then_some(r[0]))
    }

    pub fn has(&mut self, id: u16) -> bool {
        matches!(self.feature_index(id), Ok(Some(_)))
    }

    /// Calls `function` of feature `id`.
    pub fn call(&mut self, id: u16, function: u8, params: &[u8]) -> Result<Vec<u8>> {
        let index = self
            .feature_index(id)?
            .ok_or_else(|| anyhow!("{} does not support feature {id:#06x}", self.name))?;
        self.call_index(index, function, params, TIMEOUT)
    }

    /// Lists all (feature ID, version) pairs the device exposes.
    pub fn feature_list(&mut self) -> Result<Vec<(u16, u8)>> {
        let count = self.call(feature::FEATURE_SET, 0, &[])?[0];
        let mut out = Vec::new();
        for i in 1..=count {
            let r = self.call(feature::FEATURE_SET, 1, &[i])?;
            out.push((u16::from_be_bytes([r[0], r[1]]), r[3]));
        }
        Ok(out)
    }

    fn read_name(&mut self) -> Result<String> {
        let len = self.call(feature::DEVICE_NAME, 0, &[])?[0] as usize;
        let mut name = Vec::new();
        while name.len() < len {
            let chunk = self.call(feature::DEVICE_NAME, 1, &[name.len() as u8])?;
            if chunk.iter().all(|&b| b == 0) {
                break;
            }
            name.extend_from_slice(&chunk);
        }
        name.truncate(len);
        Ok(String::from_utf8_lossy(&name).trim_end_matches('\0').to_string())
    }

    /// Answers a root ping quickly; false for empty or sleeping slots.
    fn ping(&mut self) -> bool {
        match self.call_index(0, 1, &[0, 0, 0x5A], PROBE_TIMEOUT) {
            Ok(_) => true,
            Err(e) => {
                if std::env::var_os("HIDPILOT_DEBUG").is_some() {
                    eprintln!("ping {} slot {}: {e}", self.io.path.display(), self.index);
                }
                false
            }
        }
    }
}

fn is_hidpp_interface(info: &HidrawInfo) -> bool {
    info.vendor == LOGITECH && info.has_report_id(LONG)
}

/// Finds every online HID++ 2.0 device on receivers and Bluetooth.
pub fn discover() -> Result<Vec<Device>> {
    let mut found = Vec::new();
    for info in hidraw::enumerate()? {
        if !is_hidpp_interface(&info) {
            continue;
        }
        let (via, slots): (Link, Vec<u8>) = match info.bus {
            BUS_USB => (Link::Receiver, (1..=6).collect()),
            BUS_BLUETOOTH => (Link::Bluetooth, vec![0xFF]),
            _ => continue,
        };
        for slot in slots {
            let mut dev = match Device::new(&info.path, slot, via) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("warning: {e:#}");
                    break;
                }
            };
            if !dev.ping() {
                continue;
            }
            dev.name = dev.read_name().unwrap_or_else(|_| format!("{} #{slot}", info.name));
            found.push(dev);
        }
    }
    Ok(found)
}

/// Picks one device by list number (1-based) or case-insensitive name fragment.
pub fn select(devices: Vec<Device>, query: &str) -> Result<Device> {
    if let Ok(n) = query.parse::<usize>() {
        return devices
            .into_iter()
            .nth(n.wrapping_sub(1))
            .ok_or_else(|| anyhow!("no device number {n}; run `hidpilot list`"));
    }
    let q = query.to_lowercase();
    let mut matches: Vec<Device> = devices.into_iter().filter(|d| d.name.to_lowercase().contains(&q)).collect();
    match matches.len() {
        0 => bail!("no device matches \"{query}\"; run `hidpilot list`"),
        1 => Ok(matches.remove(0)),
        _ => bail!(
            "\"{query}\" matches several devices: {}",
            matches.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ")
        ),
    }
}
