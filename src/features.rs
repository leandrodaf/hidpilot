//! Typed wrappers for the HID++ 2.0 features of MX mice and keyboards.

use crate::hidpp::{Device, feature as f};
use anyhow::{Result, bail};

// ---------- firmware (0x0003) ----------

pub struct Firmware {
    pub kind: &'static str,
    pub version: String,
}

pub fn firmware(dev: &mut Device) -> Result<Vec<Firmware>> {
    let count = dev.call(f::FIRMWARE, 0, &[])?[0];
    let mut out = Vec::new();
    for entity in 0..count {
        let r = dev.call(f::FIRMWARE, 1, &[entity])?;
        let kind = match r[0] & 0x0F {
            0 => "firmware",
            1 => "bootloader",
            2 => "hardware",
            _ => "other",
        };
        let version = match r[0] & 0x0F {
            0 | 1 => {
                let prefix = String::from_utf8_lossy(&r[1..4]).to_string();
                let build = u16::from_be_bytes([r[6], r[7]]);
                let mut v = format!("{prefix} {:02X}.{:02X}", r[4], r[5]);
                if build != 0 {
                    v += &format!(".B{build:04X}");
                }
                v
            }
            2 => r[1].to_string(),
            _ => continue,
        };
        out.push(Firmware { kind, version });
    }
    Ok(out)
}

/// Unit ID: identifies this physical device across hosts and connections.
pub fn unit_id(dev: &mut Device) -> Result<String> {
    let r = dev.call(f::FIRMWARE, 0, &[])?;
    Ok(r[1..5].iter().map(|b| format!("{b:02X}")).collect())
}

/// Device type from DEVICE_NAME fn 2: 0 keyboard, 2 numpad, 3 mouse, 4 trackpad, 5 trackball.
pub fn is_keyboard(dev: &mut Device) -> bool {
    matches!(dev.call(f::DEVICE_NAME, 2, &[]).map(|r| r[0]), Ok(0) | Ok(2))
}

// ---------- battery (0x1004) ----------

pub struct Battery {
    pub percent: Option<u8>,
    pub level: &'static str,
    pub status: &'static str,
}

pub fn battery(dev: &mut Device) -> Result<Battery> {
    let r = dev.call(f::UNIFIED_BATTERY, 1, &[])?;
    let level = match r[1] {
        8.. => "full",
        4..=7 => "good",
        2..=3 => "low",
        1 => "critical",
        _ => "empty",
    };
    let status = match r[2] {
        0 => "discharging",
        1 => "charging",
        2 => "almost full",
        3 => "full",
        4 => "charging (slow)",
        5 => "invalid battery",
        6 => "thermal error",
        _ => "unknown",
    };
    Ok(Battery { percent: (r[0] != 0).then_some(r[0]), level, status })
}

// ---------- DPI (0x2201) ----------

pub fn dpi_choices(dev: &mut Device) -> Result<Vec<u16>> {
    let mut bytes = Vec::new();
    for page in 0..=255u8 {
        let r = dev.call(f::ADJUSTABLE_DPI, 1, &[0, 0, page])?;
        bytes.extend_from_slice(&r[1..]);
        if bytes.ends_with(&[0, 0]) {
            break;
        }
    }
    let mut list: Vec<u16> = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let v = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
        if v == 0 {
            break;
        }
        // 0b111xxxxx_xxxxxxxx = "step" entry followed by the range end
        if v >> 13 == 0b111 && i + 3 < bytes.len() {
            let step = v & 0x1FFF;
            let last = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]);
            if let (Some(&from), true) = (list.last(), step > 0) {
                list.extend((from + step..=last).step_by(step as usize));
            }
            i += 4;
        } else {
            list.push(v);
            i += 2;
        }
    }
    Ok(list)
}

pub fn dpi(dev: &mut Device) -> Result<u16> {
    let r = dev.call(f::ADJUSTABLE_DPI, 2, &[0])?;
    let current = u16::from_be_bytes([r[1], r[2]]);
    let default = u16::from_be_bytes([r[3], r[4]]);
    Ok(if current == 0 { default } else { current })
}

pub fn set_dpi(dev: &mut Device, value: u16) -> Result<u16> {
    let choices = dpi_choices(dev)?;
    let Some(&nearest) = choices.iter().min_by_key(|&&c| c.abs_diff(value)) else {
        bail!("device reported no DPI values");
    };
    let [hi, lo] = nearest.to_be_bytes();
    dev.call(f::ADJUSTABLE_DPI, 3, &[0, hi, lo])?;
    Ok(nearest)
}

// ---------- SmartShift (0x2110 / 0x2111) ----------

pub struct SmartShift {
    pub ratchet: bool,
    /// Wheel speed at which ratchet disengages (1..=50); 255 means never.
    pub threshold: u8,
}

/// (feature, get fn, set fn) for whichever SmartShift variant the device has.
fn smartshift_feature(dev: &mut Device) -> Result<(u16, u8, u8)> {
    if dev.has(f::SMART_SHIFT_ENHANCED) {
        Ok((f::SMART_SHIFT_ENHANCED, 1, 2))
    } else {
        Ok((f::SMART_SHIFT, 0, 1))
    }
}

pub fn smartshift(dev: &mut Device) -> Result<SmartShift> {
    let (feat, get, _) = smartshift_feature(dev)?;
    let r = dev.call(feat, get, &[])?;
    Ok(SmartShift { ratchet: r[0] == 2, threshold: r[1] })
}

/// `ratchet`: Some(true)=ratchet, Some(false)=free spin, None=unchanged.
/// `threshold`: 1..=50, or None to keep it.
pub fn set_smartshift(dev: &mut Device, ratchet: Option<bool>, threshold: Option<u8>) -> Result<()> {
    let (feat, _, set) = smartshift_feature(dev)?;
    let mode = match ratchet {
        Some(true) => 2,
        Some(false) => 1,
        None => 0,
    };
    let threshold = match threshold {
        Some(t) if t >= 50 => 255,
        Some(t) => t.max(1),
        None => 0,
    };
    dev.call(feat, set, &[mode, threshold])?;
    Ok(())
}

// ---------- high resolution wheel (0x2121) ----------

pub const WHEEL_HIRES: u8 = 0x02;
pub const WHEEL_INVERT: u8 = 0x04;

pub fn wheel_mode(dev: &mut Device) -> Result<u8> {
    Ok(dev.call(f::HIRES_WHEEL, 1, &[])?[0])
}

pub fn set_wheel_flag(dev: &mut Device, flag: u8, on: bool) -> Result<()> {
    let mode = wheel_mode(dev)?;
    let mode = if on { mode | flag } else { mode & !flag };
    dev.call(f::HIRES_WHEEL, 2, &[mode & 0x07])?;
    Ok(())
}

// ---------- thumb wheel (0x2150) ----------

pub fn thumbwheel_inverted(dev: &mut Device) -> Result<bool> {
    Ok(dev.call(f::THUMB_WHEEL, 1, &[])?[1] & 0x01 != 0)
}

pub fn set_thumbwheel_inverted(dev: &mut Device, on: bool) -> Result<()> {
    let r = dev.call(f::THUMB_WHEEL, 1, &[])?;
    dev.call(f::THUMB_WHEEL, 2, &[r[0] & 0x01, on as u8])?;
    Ok(())
}

// ---------- Easy-Switch hosts (0x1814 / 0x1815) ----------

pub struct Hosts {
    pub count: u8,
    pub current: u8,
    /// (paired, name) per host, when the device can report names.
    pub names: Vec<(bool, String)>,
}

pub fn hosts(dev: &mut Device) -> Result<Hosts> {
    let r = dev.call(f::CHANGE_HOST, 0, &[])?;
    let (count, current) = (r[0], r[1]);
    let mut names = Vec::new();
    if dev.has(f::HOSTS_INFO) {
        let caps = dev.call(f::HOSTS_INFO, 0, &[])?[0];
        if caps & 0x01 != 0 {
            for host in 0..count {
                let info = dev.call(f::HOSTS_INFO, 1, &[host])?;
                let (paired, len) = (info[1] != 0, info[4] as usize);
                let mut name = Vec::new();
                while name.len() < len {
                    let piece = dev.call(f::HOSTS_INFO, 3, &[host, name.len() as u8])?;
                    let take = (len - name.len()).min(14);
                    name.extend_from_slice(&piece[2..2 + take]);
                }
                names.push((paired, String::from_utf8_lossy(&name).to_string()));
            }
        }
    }
    Ok(Hosts { count, current, names })
}

/// Switches the device to another host. The device leaves immediately, so no reply is awaited.
pub fn switch_host(dev: &mut Device, host: u8) -> Result<()> {
    match dev.call(f::CHANGE_HOST, 1, &[host]) {
        Ok(_) => Ok(()),
        Err(e) if e.downcast_ref::<crate::hidpp::HidppError>().is_some_and(|e| {
            matches!(e, crate::hidpp::HidppError::Timeout | crate::hidpp::HidppError::Offline)
        }) =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

// ---------- Fn key inversion (0x40a3) ----------

fn current_host_prefix(dev: &mut Device) -> u8 {
    dev.call(f::HOSTS_INFO, 0, &[]).map(|r| r[3]).unwrap_or(0xFF)
}

pub fn fn_inverted(dev: &mut Device) -> Result<bool> {
    let host = current_host_prefix(dev);
    Ok(dev.call(f::K375S_FN_INVERSION, 0, &[host])?[1] & 0x01 != 0)
}

pub fn set_fn_inverted(dev: &mut Device, on: bool) -> Result<()> {
    let host = current_host_prefix(dev);
    dev.call(f::K375S_FN_INVERSION, 1, &[host, on as u8])?;
    Ok(())
}

// ---------- OS key layout, Fn+O / Fn+P (0x4531) ----------

pub struct Platforms {
    pub current: u8,
    pub settable: bool,
    /// (platform id, OS names) as declared by the device.
    pub options: Vec<(u8, Vec<&'static str>)>,
}

const OS_BITS: [(&str, u16); 9] = [
    ("linux", 0x0400),
    ("macos", 0x2000),
    ("windows", 0x0100),
    ("ios", 0x4000),
    ("android", 0x1000),
    ("webos", 0x8000),
    ("chrome", 0x0800),
    ("windows-embedded", 0x0200),
    ("tizen", 0x0001),
];

pub fn platforms(dev: &mut Device) -> Result<Platforms> {
    let r = dev.call(f::MULTIPLATFORM, 0, &[])?;
    let (flags, count, current) = (r[0], r[2], r[6]);
    let mut options: Vec<(u8, Vec<&'static str>)> = Vec::new();
    for i in 0..count {
        let d = dev.call(f::MULTIPLATFORM, 1, &[i])?;
        let (platform, os_flags) = (d[0], u16::from_be_bytes([d[2], d[3]]));
        let names: Vec<&str> = OS_BITS.iter().filter(|(_, bit)| os_flags & bit != 0).map(|(n, _)| *n).collect();
        match options.iter_mut().find(|(p, _)| *p == platform) {
            Some((_, existing)) => {
                for n in names {
                    if !existing.contains(&n) {
                        existing.push(n);
                    }
                }
            }
            None => options.push((platform, names)),
        }
    }
    Ok(Platforms { current, settable: flags & 0x02 != 0, options })
}

/// Switches the key layout to the platform that lists `os` (e.g. "linux", "macos").
pub fn set_platform(dev: &mut Device, os: &str) -> Result<u8> {
    let p = platforms(dev)?;
    if !p.settable {
        bail!("this device does not allow changing its OS layout");
    }
    let os = os.to_lowercase();
    let lists = |os: &str| p.options.iter().find(|(_, names)| names.iter().any(|n| *n == os));
    // Logitech's "Fn+P" layout covers Linux, Android and ChromeOS even when only Windows is declared.
    let found = lists(&os).or_else(|| matches!(os.as_str(), "linux" | "android" | "chrome").then(|| lists("windows")).flatten());
    let Some((id, _)) = found else {
        bail!("unknown OS \"{os}\"; options: {}", p.options.iter().flat_map(|(_, n)| n.clone()).collect::<Vec<_>>().join(", "));
    };
    // 0xFF = the host the device is currently connected to
    dev.call(f::MULTIPLATFORM, 3, &[0xFF, *id])?;
    Ok(*id)
}

// ---------- keyboard backlight (0x1982) ----------

pub struct Backlight {
    enabled: u8,
    options: u8,
    supported: u8,
    pub level: u8,
    dho: u16,
    dhi: u16,
    dpow: u16,
    pub levels: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklightMode {
    Off,
    On,
    Auto,
    Manual,
}

impl Backlight {
    pub fn mode(&self) -> BacklightMode {
        match (self.enabled, (self.options >> 3) & 0x03) {
            (0, _) => BacklightMode::Off,
            (_, 0x03) => BacklightMode::Manual,
            (_, 0x01) => BacklightMode::Auto,
            _ => BacklightMode::On,
        }
    }
    pub fn supports_auto(&self) -> bool {
        self.supported & 0x08 != 0
    }
    pub fn supports_manual(&self) -> bool {
        self.supported & 0x20 != 0
    }
}

pub fn backlight(dev: &mut Device) -> Result<Backlight> {
    let r = dev.call(f::BACKLIGHT2, 0, &[])?;
    let le = |i: usize| u16::from_le_bytes([r[i], r[i + 1]]);
    let levels = dev.call(f::BACKLIGHT2, 2, &[]).map(|r| r[0]).unwrap_or(0);
    Ok(Backlight {
        enabled: r[0],
        options: r[1],
        supported: r[2],
        level: r[5],
        dho: le(6),
        dhi: le(8),
        dpow: le(10),
        levels,
    })
}

pub fn set_backlight(dev: &mut Device, mode: Option<BacklightMode>, level: Option<u8>) -> Result<()> {
    let mut b = backlight(dev)?;
    if let Some(m) = mode {
        match m {
            BacklightMode::Off => b.enabled = 0,
            BacklightMode::On => b.enabled = 1,
            BacklightMode::Auto if !b.supports_auto() => bail!("this device has no automatic backlight mode"),
            BacklightMode::Auto => {
                b.enabled = 1;
                b.options = (b.options & 0x07) | (0x01 << 3);
            }
            BacklightMode::Manual if !b.supports_manual() => bail!("this device has no manual backlight mode"),
            BacklightMode::Manual => {
                b.enabled = 1;
                b.options = (b.options & 0x07) | (0x03 << 3);
            }
        }
    }
    if let Some(l) = level {
        if !b.supports_manual() {
            bail!("this device does not accept a backlight level (use its brightness keys)");
        }
        if b.levels > 1 && l >= b.levels {
            bail!("level must be 0..={}", b.levels - 1);
        }
        b.level = l;
        // A level only takes effect in manual mode.
        b.enabled = 1;
        b.options = (b.options & 0x07) | (0x03 << 3);
    }
    let manual = (b.options >> 3) & 0x03 == 0x03;
    let mut data = vec![b.enabled, b.options, 0xFF, if manual { b.level } else { 0 }];
    data.extend_from_slice(&b.dho.to_le_bytes());
    data.extend_from_slice(&b.dhi.to_le_bytes());
    data.extend_from_slice(&b.dpow.to_le_bytes());
    dev.call(f::BACKLIGHT2, 1, &data)?;
    Ok(())
}

// ---------- reprogrammable controls (0x1b04) ----------

/// Capability bits of a control (getCidInfo bytes 4 and 8 as one field).
pub mod key_flag {
    pub const REPROGRAMMABLE: u16 = 0x0010;
    pub const DIVERTABLE: u16 = 0x0020;
    pub const PERSISTENTLY_DIVERTABLE: u16 = 0x0040;
    pub const VIRTUAL: u16 = 0x0080;
    pub const RAW_XY: u16 = 0x0100;
}

/// Reporting bits (get/setCidReporting byte 2). Each setting has a "valid" bit right
/// above it that tells the device to apply that setting at all.
pub mod report_flag {
    pub const DIVERTED: u8 = 0x01;
    pub const PERSISTENTLY_DIVERTED: u8 = 0x04;
    pub const RAW_XY: u8 = 0x10;
}

pub struct Control {
    pub cid: u16,
    pub task: u16,
    /// key_flag bits
    pub flags: u16,
    pub divertable: bool,
    pub diverted: bool,
    pub raw_xy: bool,
}

impl Control {
    pub fn supports_raw_xy(&self) -> bool {
        self.flags & key_flag::RAW_XY != 0
    }
    pub fn is_virtual(&self) -> bool {
        self.flags & key_flag::VIRTUAL != 0
    }
}

pub fn controls(dev: &mut Device) -> Result<Vec<Control>> {
    let count = dev.call(f::REPROG_CONTROLS_V4, 0, &[])?[0];
    let mut out = Vec::new();
    for i in 0..count {
        let r = dev.call(f::REPROG_CONTROLS_V4, 1, &[i])?;
        let cid = u16::from_be_bytes([r[0], r[1]]);
        let task = u16::from_be_bytes([r[2], r[3]]);
        let flags = r[4] as u16 | (r[8] as u16) << 8;
        let rep = cid_reporting(dev, cid)?;
        out.push(Control {
            cid,
            task,
            flags,
            divertable: flags & key_flag::DIVERTABLE != 0,
            diverted: rep & report_flag::DIVERTED != 0,
            raw_xy: rep & report_flag::RAW_XY != 0,
        });
    }
    Ok(out)
}

/// Current report_flag bits of a control.
pub fn cid_reporting(dev: &mut Device, cid: u16) -> Result<u8> {
    let r = dev.call(f::REPROG_CONTROLS_V4, 2, &cid.to_be_bytes())?;
    Ok(r[2])
}

/// Diverts a control to HID++ events (temporarily: the device forgets on power cycle or
/// host switch) or gives it back. `raw_xy` also reports pointer motion while it is held.
/// Settings passed as None are left as they are; the mapping is never changed.
pub fn set_cid_reporting(dev: &mut Device, cid: u16, divert: Option<bool>, raw_xy: Option<bool>) -> Result<()> {
    let mut bits = 0u8;
    if let Some(on) = divert {
        bits |= (report_flag::DIVERTED << 1) | if on { report_flag::DIVERTED } else { 0 };
    }
    if let Some(on) = raw_xy {
        bits |= (report_flag::RAW_XY << 1) | if on { report_flag::RAW_XY } else { 0 };
    }
    let [hi, lo] = cid.to_be_bytes();
    dev.call(f::REPROG_CONTROLS_V4, 3, &[hi, lo, bits, 0, 0])?;
    Ok(())
}

/// Thumb wheel reporting: true when its rotation comes as HID++ events instead of
/// horizontal scrolling.
pub fn thumbwheel_diverted(dev: &mut Device) -> Result<bool> {
    Ok(dev.call(f::THUMB_WHEEL, 1, &[])?[0] & 0x01 != 0)
}

pub fn set_thumbwheel_diverted(dev: &mut Device, on: bool) -> Result<()> {
    let r = dev.call(f::THUMB_WHEEL, 1, &[])?;
    dev.call(f::THUMB_WHEEL, 2, &[on as u8, r[1] & 0x01])?;
    Ok(())
}

/// Human-readable names for the controls of MX mice and keyboards.
pub fn control_name(cid: u16) -> &'static str {
    match cid {
        0x0034 => "Fn",
        0x0050 => "Left click",
        0x0051 => "Right click",
        0x0052 => "Middle click",
        0x0053 => "Back",
        0x0056 => "Forward",
        0x00C3 => "Gesture button",
        0x00C4 => "SmartShift toggle",
        0x00D1 => "Easy-Switch 1",
        0x00D2 => "Easy-Switch 2",
        0x00D3 => "Easy-Switch 3",
        0x00D4 => "Search",
        0x00D7 => "Virtual gesture button",
        0x00DE => "Fn lock",
        0x00E2 => "Backlight down",
        0x00E3 => "Backlight up",
        0x00E7 => "Mute",
        0x00E8 => "Volume down",
        0x00E9 => "Volume up",
        0x0103 => "Dictation",
        0x0108 => "Emoji",
        0x010A => "Screenshot",
        0x010B => "Grave accent",
        0x010C => "Tab",
        0x010D => "Caps Lock",
        0x010E => "Left Shift",
        0x010F => "Left Ctrl",
        0x0110 => "Left Option / Start",
        0x0111 => "Left Command / Alt",
        0x0112 => "Right Command / Alt",
        0x0115 => "Right Shift",
        0x0117 => "Lock",
        0x013C => "Right Option / Start",
        0x0141 => "Play/Pause",
        0x01A0 => "Thumb wheel click",
        _ => "",
    }
}
