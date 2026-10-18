//! Centurion transport, the HID++ successor spoken by newer Logitech headsets (Zone Vibe).
//!
//! Frames go out and come back on report 0x22 of the receiver's first interface:
//! `22 | addr | len | flags | feature index | fn<<4 | swid | params...`, padded to 64 bytes,
//! where `len` counts flags, index, function byte and params. See docs/zone-vibe-protocol.md.

use crate::hidpp::LOGITECH;
use crate::hidraw::{self, Hidraw};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const REPORT: u8 = 0x22;
const FRAME_LEN: usize = 64;
/// Address the Zone Vibe answers on. Wrong addresses can stall the receiver until the
/// kernel resets it, so never scan for others.
const ADDR: u8 = 0xF1;
const SOFTWARE_ID: u8 = 0x0A;
const TIMEOUT: Duration = Duration::from_millis(1500);

/// Receivers whose interface 0 carries the Centurion report. Interface 1 is the
/// firmware updater and must never be written to.
const RECEIVERS: [(u16, &str); 2] = [(0x0af0, "Zone Vibe Wireless"), (0x0af1, "Zone Receiver")];

pub mod feature {
    pub const DEVICE_INFO: u16 = 0x0100;
    pub const DEVICE_NAME: u16 = 0x0101;
    pub const BATTERY_SOC: u16 = 0x0104;
    pub const AUTO_SLEEP: u16 = 0x0108;
    pub const EARCON: u16 = 0x0109;
    pub const EQ: u16 = 0x0201;
    pub const AUDIO: u16 = 0x0500;
    pub const BT_CONNECTIONS: u16 = 0x0501;
    /// Also holds factory reset (fn 4): never call functions here that are not wrapped below.
    pub const MISC: u16 = 0x0502;
}

pub const NAME_MAX: usize = 30;
pub const EQ_BANDS: usize = 5;
/// Sidetone steps the headset takes (Logi Tune shows them as 0-100 % in tens).
pub const SIDETONE_MAX: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Equalizer {
    /// Passed through untouched, as Logi Tune does.
    pub mode: u8,
    /// Gain per band, low to high, in the device's own units (-128..=127, 0 = flat).
    pub bands: [i8; EQ_BANDS],
}

/// Built-in presets as Logi Tune defines them for Centurion headsets.
pub const EQ_PRESETS: [(&str, [i8; EQ_BANDS]); 4] = [
    ("Default", [0, 0, 0, 0, 0]),
    ("Volume Boost", [127, 127, 127, 127, 127]),
    ("Podcast", [0, 25, 102, 127, 76]),
    ("Bass Boost", [127, 76, 25, 25, 25]),
];

pub struct Headset {
    io: Hidraw,
    pub name: String,
    features: HashMap<u16, u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    pub full: bool,
}

/// True when a Zone receiver is plugged in, with or without its headset.
pub fn receiver_present() -> bool {
    hidraw::enumerate()
        .map(|l| l.iter().any(|i| i.vendor == LOGITECH && RECEIVERS.iter().any(|(pid, _)| *pid == i.product)))
        .unwrap_or(false)
}

/// Finds a Zone receiver with its headset switched on.
pub fn find() -> Result<Option<Headset>> {
    for info in hidraw::enumerate()? {
        let known = info.vendor == LOGITECH && RECEIVERS.iter().any(|(pid, _)| *pid == info.product);
        if !known || !info.has_report_id(REPORT) {
            continue;
        }
        if info.product == 0x0af1 {
            // The receiver enumerates as 0af1 while the headset is off or out of range.
            return Ok(None);
        }
        let mut h = Headset { io: Hidraw::open(&info.path)?, name: String::new(), features: HashMap::new() };
        h.name = h.read_name().unwrap_or_else(|_| "Zone headset".into());
        return Ok(Some(h));
    }
    Ok(None)
}

impl Headset {
    fn send(&mut self, index: u8, function: u8, params: &[u8]) -> Result<Vec<u8>> {
        let fn_byte = (function << 4) | SOFTWARE_ID;
        let mut frame = vec![REPORT, ADDR, (3 + params.len()) as u8, 0x00, index, fn_byte];
        frame.extend_from_slice(params);
        frame.resize(FRAME_LEN, 0);
        self.io.write(&frame)?;

        let deadline = Instant::now() + TIMEOUT;
        while let Some(r) = self.io.read_until(deadline)? {
            if r.len() < 6 || r[0] != REPORT {
                continue;
            }
            let end = (3 + r[2] as usize).min(r.len());
            let body = &r[4..end];
            if body.len() >= 4 && body[0] == 0xFF && body[1] == index && body[2] == fn_byte {
                return Err(anyhow!("headset returned error {:#04x}", body[3]));
            }
            if body.len() >= 2 && body[0] == index && body[1] == fn_byte {
                return Ok(body[2..].to_vec());
            }
            // Anything else is a notification; the listener handles those.
        }
        bail!("headset did not answer")
    }

    fn index(&mut self, id: u16) -> Result<Option<u8>> {
        if let Some(&i) = self.features.get(&id) {
            return Ok(Some(i));
        }
        let r = self.send(0, 0, &id.to_be_bytes())?;
        let i = r.first().copied().filter(|&i| i != 0 || id == 0);
        if let Some(i) = i {
            self.features.insert(id, i);
        }
        Ok(i)
    }

    pub fn call(&mut self, id: u16, function: u8, params: &[u8]) -> Result<Vec<u8>> {
        let i = self.index(id)?.with_context(|| format!("headset has no feature {id:#06x}"))?;
        self.send(i, function, params)
    }

    pub fn has(&mut self, id: u16) -> bool {
        matches!(self.index(id), Ok(Some(_)))
    }

    fn read_name(&mut self) -> Result<String> {
        let r = self.call(feature::DEVICE_NAME, 0, &[])?;
        let len = *r.get(1).context("short name reply")? as usize;
        let name = String::from_utf8_lossy(&r[2..(2 + len).min(r.len())]).into_owned();
        if name.is_empty() || name.chars().any(|c| c.is_control() || c == char::REPLACEMENT_CHARACTER) {
            bail!("unreadable name {name:?}");
        }
        Ok(name)
    }

    pub fn serial(&mut self) -> Result<String> {
        let r = self.call(feature::DEVICE_INFO, 2, &[])?;
        let len = *r.first().context("short serial reply")? as usize;
        Ok(String::from_utf8_lossy(&r[1..(1 + len).min(r.len())]).into_owned())
    }

    pub fn battery(&mut self) -> Result<Battery> {
        let r = self.call(feature::BATTERY_SOC, 0, &[])?;
        let status = r.get(2).copied().unwrap_or(0);
        Ok(Battery { percent: r[0].min(100), charging: matches!(status, 1 | 2), full: status == 3 })
    }

    /// Minutes of inactivity before the headset switches itself off.
    pub fn auto_sleep(&mut self) -> Result<u8> {
        Ok(self.call(feature::AUTO_SLEEP, 0, &[])?[0])
    }

    pub fn set_auto_sleep(&mut self, minutes: u8) -> Result<()> {
        if minutes == 0 {
            bail!("auto sleep must be at least 1 minute");
        }
        self.call(feature::AUTO_SLEEP, 1, &[minutes])?;
        Ok(())
    }

    // Setters below always send every parameter: with a short frame the firmware reads the
    // missing ones from the zero padding and applies them.

    pub fn set_name(&mut self, name: &str) -> Result<()> {
        if name.is_empty() || name.len() > NAME_MAX || !name.is_ascii() {
            bail!("name must be 1-{NAME_MAX} plain ASCII characters");
        }
        let mut p = vec![name.len() as u8];
        p.extend_from_slice(name.as_bytes());
        p.push(0);
        self.call(feature::DEVICE_NAME, 1, &p)?;
        self.name = name.to_string();
        Ok(())
    }

    pub fn equalizer(&mut self) -> Result<Equalizer> {
        let r = self.call(feature::EQ, 0, &[])?;
        if r.len() < 1 + EQ_BANDS {
            bail!("short equalizer reply");
        }
        let mut bands = [0i8; EQ_BANDS];
        for (b, v) in bands.iter_mut().zip(&r[1..]) {
            *b = *v as i8;
        }
        Ok(Equalizer { mode: r[0], bands })
    }

    pub fn set_equalizer(&mut self, eq: Equalizer) -> Result<()> {
        let mut p = vec![eq.mode];
        p.extend(eq.bands.iter().map(|b| *b as u8));
        self.call(feature::EQ, 1, &p)?;
        Ok(())
    }

    pub fn mic_muted(&mut self) -> Result<bool> {
        Ok(self.call(feature::AUDIO, 0, &[])?[0] != 0)
    }

    pub fn set_mic_muted(&mut self, on: bool) -> Result<()> {
        self.call(feature::AUDIO, 1, &[on as u8])?;
        Ok(())
    }

    /// Sidetone level 0..=SIDETONE_MAX (how much of your own voice you hear).
    pub fn sidetone(&mut self) -> Result<u8> {
        let v = self.call(feature::AUDIO, 2, &[])?[0];
        // High nibble flags a negative value, which this headset never reports.
        Ok(if v & 0xF0 != 0 { 0 } else { v & 0x0F })
    }

    pub fn set_sidetone(&mut self, level: u8) -> Result<()> {
        if level > SIDETONE_MAX {
            bail!("sidetone must be 0-{SIDETONE_MAX}");
        }
        self.call(feature::AUDIO, 3, &[level])?;
        Ok(())
    }

    pub fn voice_prompts(&mut self) -> Result<bool> {
        Ok(self.call(feature::MISC, 2, &[])?[0] != 0)
    }

    pub fn set_voice_prompts(&mut self, on: bool) -> Result<()> {
        self.call(feature::MISC, 3, &[on as u8])?;
        Ok(())
    }

    /// Muting the mic by flipping the boom up.
    pub fn rotate_to_mute(&mut self) -> Result<bool> {
        Ok(self.call(feature::MISC, 5, &[])?[0] != 0)
    }

    pub fn set_rotate_to_mute(&mut self, on: bool) -> Result<()> {
        self.call(feature::MISC, 6, &[on as u8])?;
        Ok(())
    }

    /// Bluetooth hosts connected right now (the USB receiver is not counted).
    pub fn bluetooth_connections(&mut self) -> Result<u8> {
        Ok(self.call(feature::BT_CONNECTIONS, 0, &[])?[0])
    }
}
