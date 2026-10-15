//! Button remapping, like Logi Options+: diverted buttons, thumb-button gestures, thumb
//! wheel actions and keyboard special keys, with per-application profiles.
//!
//! Only controls that have an action in the profile of the focused application are
//! diverted; everything else keeps its native behaviour. Diversion is temporary on the
//! device (lost on power cycle or host switch), so the service re-applies it when the
//! device reconnects and gives every control back when it stops.

use crate::features::{self, report_flag};
use crate::hidpp::{self, Device, feature as f};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

pub const GESTURE_BUTTON: u16 = 0x00C3;
const SMARTSHIFT_BUTTON: u16 = 0x00C4;
/// Pointer travel (raw device counts) that turns a held gesture button into a swipe.
const GESTURE_DISTANCE: i32 = 50;
/// Software id the service uses, so its replies do not mix with the CLI's or the GUI's.
const SERVICE_SOFTWARE_ID: u8 = 0x0B;
const RECHECK: Duration = Duration::from_secs(15);
const FOCUS_POLL: Duration = Duration::from_millis(250);

// ---------- configuration (~/.config/hidpilot/buttons.toml) ----------

/// What a control does. A plain string is an action (see `ACTIONS`); a table is a
/// gesture map for the gesture button.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Binding {
    Action(String),
    Gesture(Gesture),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Gesture {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
}

impl Gesture {
    /// Options+ defaults on Linux terms.
    pub fn options_plus_default() -> Gesture {
        Gesture {
            click: Some("overview".into()),
            up: Some("overview".into()),
            down: Some("show-desktop".into()),
            left: Some("workspace-left".into()),
            right: Some("workspace-right".into()),
        }
    }
}

/// Thumb wheel: a preset ("scroll", "volume", "zoom", "workspaces", "tabs") or one action
/// per direction.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Wheel {
    Preset(String),
    Custom { left: String, right: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// Control id ("0x00c3") -> binding.
    #[serde(default)]
    pub buttons: BTreeMap<String, Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbwheel: Option<Wheel>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DeviceConfig {
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub profile: Profile,
    /// Application (X11 WM_CLASS, lower case) -> overrides while it has focus.
    #[serde(default)]
    pub apps: BTreeMap<String, Profile>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Config {
    /// Device unit id -> settings.
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceConfig>,
}

pub fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("no config directory")?;
    Ok(base.join("hidpilot").join("buttons.toml"))
}

pub fn cid_key(cid: u16) -> String {
    format!("0x{cid:04x}")
}

fn parse_cid(key: &str) -> Option<u16> {
    u16::from_str_radix(key.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
}

impl Config {
    pub fn load() -> Result<Config> {
        let p = config_path()?;
        match fs::read_to_string(&p) {
            Ok(text) => {
                let c: Config = toml::from_str(&text).with_context(|| format!("parse {}", p.display()))?;
                c.validate()?;
                Ok(c)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let p = config_path()?;
        fs::create_dir_all(p.parent().unwrap())?;
        let tmp = p.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(&tmp, &p)?;
        Ok(())
    }

    /// Rejects unknown actions up front, so a typo shows when saving, not when pressing.
    pub fn validate(&self) -> Result<()> {
        let check = |a: &str| Action::parse(a).map(|_| ()).with_context(|| format!("action \"{a}\""));
        for (unit, d) in &self.devices {
            for p in std::iter::once(&d.profile).chain(d.apps.values()) {
                for (k, b) in &p.buttons {
                    parse_cid(k).ok_or_else(|| anyhow!("{unit}: \"{k}\" is not a control id like 0x00c3"))?;
                    match b {
                        Binding::Action(a) => check(a)?,
                        Binding::Gesture(g) => {
                            for a in [&g.click, &g.up, &g.down, &g.left, &g.right].into_iter().flatten() {
                                check(a)?;
                            }
                        }
                    }
                }
                match &p.thumbwheel {
                    Some(Wheel::Preset(s)) if !THUMB_PRESETS.iter().any(|(id, _)| id == s) => {
                        bail!("{unit}: unknown thumb wheel preset \"{s}\"")
                    }
                    Some(Wheel::Custom { left, right }) => {
                        check(left)?;
                        check(right)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Profile in effect for a device while `app` has focus: the app's entries override
    /// the device's own. A mouse with no configuration gets the Options+ gestures.
    pub fn effective(&self, unit: &str, app: &str, is_mouse: bool) -> Profile {
        let mut p = match self.devices.get(unit) {
            Some(d) => d.profile.clone(),
            None if is_mouse => {
                let mut p = Profile::default();
                p.buttons.insert(cid_key(GESTURE_BUTTON), Binding::Gesture(Gesture::options_plus_default()));
                p
            }
            None => Profile::default(),
        };
        if let Some(a) = self.devices.get(unit).and_then(|d| d.apps.get(&app.to_lowercase())) {
            p.buttons.extend(a.buttons.clone());
            if a.thumbwheel.is_some() {
                p.thumbwheel = a.thumbwheel.clone();
            }
        }
        p
    }
}

// ---------- actions ----------

/// Actions offered to the user: (id, label). `key:...` and `cmd:...` take arguments.
pub const ACTIONS: &[(&str, &str)] = &[
    ("none", "Do nothing"),
    ("default", "Native function"),
    ("click:left", "Left click"),
    ("click:right", "Right click"),
    ("click:middle", "Middle click"),
    ("click:back", "Back"),
    ("click:forward", "Forward"),
    ("overview", "Activities overview"),
    ("show-desktop", "Show desktop"),
    ("workspace-left", "Previous workspace"),
    ("workspace-right", "Next workspace"),
    ("app-switcher", "Switch application"),
    ("close-window", "Close window"),
    ("lock", "Lock screen"),
    ("screenshot", "Screenshot"),
    ("emoji", "Emoji picker"),
    ("copy", "Copy"),
    ("paste", "Paste"),
    ("cut", "Cut"),
    ("undo", "Undo"),
    ("redo", "Redo"),
    ("zoom-in", "Zoom in"),
    ("zoom-out", "Zoom out"),
    ("zoom-reset", "Reset zoom"),
    ("tab-next", "Next tab"),
    ("tab-previous", "Previous tab"),
    ("media:play-pause", "Play/Pause"),
    ("media:next", "Next track"),
    ("media:previous", "Previous track"),
    ("media:volume-up", "Volume up"),
    ("media:volume-down", "Volume down"),
    ("media:mute", "Mute"),
    ("media:mic-mute", "Mute microphone"),
    ("smartshift", "Toggle SmartShift (ratchet / free spin)"),
    ("key:", "Keyboard shortcut, e.g. key:ctrl+shift+t"),
    ("cmd:", "Run a command"),
];

pub const THUMB_PRESETS: &[(&str, &str)] = &[
    ("scroll", "Horizontal scroll (native)"),
    ("volume", "Volume"),
    ("zoom", "Zoom"),
    ("workspaces", "Switch workspaces"),
    ("tabs", "Switch tabs"),
];

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    /// Let the control do what it does without software.
    Default,
    /// Held while the control is held (a chord, pressed in order, released in reverse).
    Keys(Vec<u16>),
    Click(u16),
    SmartShift,
    Command(String),
}

pub mod key {
    //! Linux input event codes (linux/input-event-codes.h).
    pub const ESC: u16 = 1;
    pub const TAB: u16 = 15;
    pub const ENTER: u16 = 28;
    pub const LEFTCTRL: u16 = 29;
    pub const LEFTSHIFT: u16 = 42;
    pub const LEFTALT: u16 = 56;
    pub const SPACE: u16 = 57;
    pub const F4: u16 = 62;
    pub const HOME: u16 = 102;
    pub const UP: u16 = 103;
    pub const PAGEUP: u16 = 104;
    pub const LEFT: u16 = 105;
    pub const RIGHT: u16 = 106;
    pub const END: u16 = 107;
    pub const DOWN: u16 = 108;
    pub const PAGEDOWN: u16 = 109;
    pub const INSERT: u16 = 110;
    pub const DELETE: u16 = 111;
    pub const MUTE: u16 = 113;
    pub const VOLUMEDOWN: u16 = 114;
    pub const VOLUMEUP: u16 = 115;
    pub const LEFTMETA: u16 = 125;
    pub const NEXTSONG: u16 = 163;
    pub const PLAYPAUSE: u16 = 164;
    pub const PREVIOUSSONG: u16 = 165;
    pub const SYSRQ: u16 = 99;
    pub const MICMUTE: u16 = 248;
    pub const BTN_LEFT: u16 = 0x110;
    pub const BTN_RIGHT: u16 = 0x111;
    pub const BTN_MIDDLE: u16 = 0x112;
    pub const BTN_SIDE: u16 = 0x113;
    pub const BTN_EXTRA: u16 = 0x114;
}

/// Key names accepted in `key:` shortcuts.
fn key_code(name: &str) -> Option<u16> {
    use key::*;
    let n = name.to_ascii_lowercase();
    // Letters and digits follow the US keyboard rows.
    const ROW_Q: &str = "qwertyuiop";
    const ROW_A: &str = "asdfghjkl";
    const ROW_Z: &str = "zxcvbnm";
    if n.len() == 1 {
        let c = n.chars().next().unwrap();
        if let Some(i) = ROW_Q.find(c) {
            return Some(16 + i as u16);
        }
        if let Some(i) = ROW_A.find(c) {
            return Some(30 + i as u16);
        }
        if let Some(i) = ROW_Z.find(c) {
            return Some(44 + i as u16);
        }
        if let Some(d) = c.to_digit(10) {
            return Some(if d == 0 { 11 } else { 1 + d as u16 });
        }
    }
    if let Some(num) = n.strip_prefix('f').and_then(|x| x.parse::<u16>().ok()) {
        return match num {
            1..=10 => Some(58 + num),
            11 => Some(87),
            12 => Some(88),
            13..=24 => Some(183 + num - 13),
            _ => None,
        };
    }
    Some(match n.as_str() {
        "ctrl" | "control" => LEFTCTRL,
        "shift" => LEFTSHIFT,
        "alt" => LEFTALT,
        "altgr" | "ralt" => 100,
        "super" | "meta" | "win" | "cmd" => LEFTMETA,
        "esc" | "escape" => ESC,
        "tab" => TAB,
        "enter" | "return" => ENTER,
        "space" => SPACE,
        "backspace" => 14,
        "minus" | "-" => 12,
        "equal" | "plus" | "=" => 13,
        "period" | "." => 52,
        "comma" | "," => 51,
        "slash" | "/" => 53,
        "semicolon" | ";" => 39,
        "apostrophe" | "'" => 40,
        "grave" | "`" => 41,
        "backslash" | "\\" => 43,
        "leftbrace" | "[" => 26,
        "rightbrace" | "]" => 27,
        "up" => UP,
        "down" => DOWN,
        "left" => LEFT,
        "right" => RIGHT,
        "home" => HOME,
        "end" => END,
        "pageup" | "page_up" => PAGEUP,
        "pagedown" | "page_down" => PAGEDOWN,
        "insert" => INSERT,
        "delete" | "del" => DELETE,
        "print" | "printscreen" | "sysrq" => SYSRQ,
        "capslock" => 58,
        _ => return None,
    })
}

impl Action {
    pub fn parse(s: &str) -> Result<Action> {
        use key::*;
        let chord = |keys: &[u16]| Action::Keys(keys.to_vec());
        Ok(match s.trim() {
            "none" => Action::None,
            "default" => Action::Default,
            "click:left" => Action::Click(BTN_LEFT),
            "click:right" => Action::Click(BTN_RIGHT),
            "click:middle" => Action::Click(BTN_MIDDLE),
            "click:back" => Action::Click(BTN_SIDE),
            "click:forward" => Action::Click(BTN_EXTRA),
            // GNOME shortcuts (see gsettings org.gnome.desktop.wm.keybindings).
            "overview" => chord(&[LEFTMETA]),
            "show-desktop" => chord(&[LEFTMETA, 32]),
            "workspace-left" => chord(&[LEFTMETA, PAGEUP]),
            "workspace-right" => chord(&[LEFTMETA, PAGEDOWN]),
            "app-switcher" => chord(&[LEFTALT, TAB]),
            "close-window" => chord(&[LEFTALT, F4]),
            "lock" => chord(&[LEFTMETA, 38]),
            "screenshot" => chord(&[SYSRQ]),
            "emoji" => chord(&[LEFTCTRL, 52]),
            "copy" => chord(&[LEFTCTRL, 46]),
            "paste" => chord(&[LEFTCTRL, 47]),
            "cut" => chord(&[LEFTCTRL, 45]),
            "undo" => chord(&[LEFTCTRL, 44]),
            "redo" => chord(&[LEFTCTRL, LEFTSHIFT, 44]),
            "zoom-in" => chord(&[LEFTCTRL, 13]),
            "zoom-out" => chord(&[LEFTCTRL, 12]),
            "zoom-reset" => chord(&[LEFTCTRL, 11]),
            "tab-next" => chord(&[LEFTCTRL, PAGEDOWN]),
            "tab-previous" => chord(&[LEFTCTRL, PAGEUP]),
            "media:play-pause" => chord(&[PLAYPAUSE]),
            "media:next" => chord(&[NEXTSONG]),
            "media:previous" => chord(&[PREVIOUSSONG]),
            "media:volume-up" => chord(&[VOLUMEUP]),
            "media:volume-down" => chord(&[VOLUMEDOWN]),
            "media:mute" => chord(&[MUTE]),
            "media:mic-mute" => chord(&[MICMUTE]),
            "smartshift" => Action::SmartShift,
            other => {
                if let Some(keys) = other.strip_prefix("key:") {
                    let codes: Vec<u16> = keys
                        .split('+')
                        .map(|k| key_code(k.trim()).ok_or_else(|| anyhow!("unknown key \"{}\"", k.trim())))
                        .collect::<Result<_>>()?;
                    if codes.is_empty() {
                        bail!("empty shortcut");
                    }
                    Action::Keys(codes)
                } else if let Some(cmd) = other.strip_prefix("cmd:") {
                    if cmd.trim().is_empty() {
                        bail!("empty command");
                    }
                    Action::Command(cmd.trim().to_string())
                } else {
                    bail!("unknown action \"{other}\"");
                }
            }
        })
    }
}

/// What a control does without software, for controls that are diverted only because
/// an app profile needs them.
fn native_action(cid: u16) -> Action {
    use key::*;
    match cid {
        0x0052 => Action::Click(BTN_MIDDLE),
        0x0053 => Action::Click(BTN_SIDE),
        0x0056 => Action::Click(BTN_EXTRA),
        SMARTSHIFT_BUTTON => Action::SmartShift,
        0x00E7 => Action::Keys(vec![MUTE]),
        0x00E8 => Action::Keys(vec![VOLUMEDOWN]),
        0x00E9 => Action::Keys(vec![VOLUMEUP]),
        0x0141 => Action::Keys(vec![PLAYPAUSE]),
        0x010A => Action::Keys(vec![SYSRQ]),
        _ => Action::None,
    }
}

// ---------- output: virtual keyboard and mouse through /dev/uinput ----------

#[cfg(target_os = "linux")]
pub struct Output {
    keyboard: evdev::uinput::VirtualDevice,
    mouse: evdev::uinput::VirtualDevice,
}

#[cfg(target_os = "linux")]
impl Output {
    pub fn new() -> Result<Output> {
        use evdev::{AttributeSet, KeyCode, RelativeAxisCode};
        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 1..=248u16 {
            keys.insert(KeyCode(code));
        }
        let keyboard = evdev::uinput::VirtualDevice::builder()
            .context("cannot open /dev/uinput (install udev/70-hidpilot.rules)")?
            .name("hidpilot virtual keyboard")
            .with_keys(&keys)?
            .build()?;
        let mut buttons = AttributeSet::<KeyCode>::new();
        for code in key::BTN_LEFT..=key::BTN_EXTRA {
            buttons.insert(KeyCode(code));
        }
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        for a in [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y, RelativeAxisCode::REL_WHEEL, RelativeAxisCode::REL_HWHEEL] {
            axes.insert(a);
        }
        let mouse = evdev::uinput::VirtualDevice::builder()?
            .name("hidpilot virtual mouse")
            .with_keys(&buttons)?
            .with_relative_axes(&axes)?
            .build()?;
        Ok(Output { keyboard, mouse })
    }

    fn key(&mut self, code: u16, down: bool) -> Result<()> {
        let ev = evdev::InputEvent::new(evdev::EventType::KEY.0, code, down as i32);
        if code >= key::BTN_LEFT { self.mouse.emit(&[ev])? } else { self.keyboard.emit(&[ev])? }
        Ok(())
    }

    /// Ctrl + wheel, which every app reads as zoom.
    fn zoom(&mut self, steps: i32) -> Result<()> {
        self.key(key::LEFTCTRL, true)?;
        let wheel = evdev::InputEvent::new(evdev::EventType::RELATIVE.0, evdev::RelativeAxisCode::REL_WHEEL.0, steps);
        self.mouse.emit(&[wheel])?;
        self.key(key::LEFTCTRL, false)
    }

    pub fn press(&mut self, a: &Action, dev: Option<&mut Device>) -> Result<()> {
        match a {
            Action::Keys(keys) => keys.iter().try_for_each(|k| self.key(*k, true)),
            Action::Click(b) => self.key(*b, true),
            Action::SmartShift => {
                if let Some(d) = dev {
                    let s = features::smartshift(d)?;
                    features::set_smartshift(d, Some(!s.ratchet), None)?;
                }
                Ok(())
            }
            Action::Command(c) => {
                std::process::Command::new("sh").arg("-c").arg(c).spawn().with_context(|| format!("run {c}"))?;
                Ok(())
            }
            Action::None | Action::Default => Ok(()),
        }
    }

    pub fn release(&mut self, a: &Action) -> Result<()> {
        match a {
            Action::Keys(keys) => keys.iter().rev().try_for_each(|k| self.key(*k, false)),
            Action::Click(b) => self.key(*b, false),
            _ => Ok(()),
        }
    }

    pub fn tap(&mut self, a: &Action, dev: Option<&mut Device>) -> Result<()> {
        self.press(a, dev)?;
        self.release(a)
    }
}

// ---------- focused application (X11) ----------

#[cfg(target_os = "linux")]
pub struct Focus {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    active: u32,
    class: u32,
}

#[cfg(target_os = "linux")]
impl Focus {
    pub fn open() -> Result<Focus> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let active = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
        let class = conn.intern_atom(false, b"WM_CLASS")?.reply()?.atom;
        Ok(Focus { conn, root, active, class })
    }

    /// WM_CLASS class of the focused window, lower case ("firefox", "code"...).
    pub fn app(&self) -> String {
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
        let get = || -> Result<String> {
            let w = self.conn.get_property(false, self.root, self.active, AtomEnum::WINDOW, 0, 1)?.reply()?;
            let win = w.value32().and_then(|mut v| v.next()).context("no active window")?;
            let c = self.conn.get_property(false, win, self.class, AtomEnum::STRING, 0, 256)?.reply()?;
            // WM_CLASS = "instance\0class\0"
            let parts: Vec<&[u8]> = c.value.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
            let class = parts.get(1).or(parts.first()).context("no WM_CLASS")?;
            Ok(String::from_utf8_lossy(class).to_lowercase())
        };
        get().unwrap_or_default()
    }
}

// ---------- the service ----------

/// One device being remapped.
struct Worker {
    dev: Device,
    unit: String,
    is_mouse: bool,
    reprog: u8,
    thumb: Option<u8>,
    /// Controls the device has and can divert, with raw XY support.
    divertable: HashMap<u16, bool>,
    /// What we diverted, so it can be given back.
    diverted: BTreeSet<u16>,
    raw_xy: bool,
    thumb_diverted: bool,
    profile: Profile,
    pressed: BTreeSet<u16>,
    gesture: Option<GestureState>,
    thumb_acc: i32,
    thumb_step: i32,
    last_check: Instant,
}

struct GestureState {
    dx: i32,
    dy: i32,
    fired: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Swipe {
    Up,
    Down,
    Left,
    Right,
}

/// Which way a held gesture button was dragged, once it went far enough.
pub fn swipe(dx: i32, dy: i32) -> Option<Swipe> {
    if dx.abs().max(dy.abs()) < GESTURE_DISTANCE {
        return None;
    }
    Some(if dx.abs() > dy.abs() {
        if dx > 0 { Swipe::Right } else { Swipe::Left }
    } else if dy > 0 {
        Swipe::Down
    } else {
        Swipe::Up
    })
}

impl Worker {
    fn new(mut dev: Device) -> Result<Worker> {
        dev.set_software_id(SERVICE_SOFTWARE_ID);
        let unit = features::unit_id(&mut dev)?;
        let is_mouse = !features::is_keyboard(&mut dev);
        let reprog = dev.feature_index(f::REPROG_CONTROLS_V4)?.context("no reprogrammable controls")?;
        let thumb = dev.feature_index(f::THUMB_WHEEL)?;
        let mut divertable = HashMap::new();
        for c in features::controls(&mut dev)? {
            if c.divertable {
                divertable.insert(c.cid, c.supports_raw_xy());
            }
        }
        // Diverted resolution of the thumb wheel; one action step per 1/8 of it.
        let thumb_step = match thumb {
            Some(_) => dev.call(f::THUMB_WHEEL, 0, &[]).map(|r| (u16::from_be_bytes([r[2], r[3]]) as i32 / 8).max(1)).unwrap_or(15),
            None => 15,
        };
        Ok(Worker {
            dev,
            unit,
            is_mouse,
            reprog,
            thumb,
            divertable,
            diverted: BTreeSet::new(),
            raw_xy: false,
            thumb_diverted: false,
            profile: Profile::default(),
            pressed: BTreeSet::new(),
            gesture: None,
            thumb_acc: 0,
            thumb_step,
            last_check: Instant::now(),
        })
    }

    fn wants(&self) -> BTreeSet<u16> {
        self.profile
            .buttons
            .iter()
            .filter_map(|(k, b)| {
                let cid = parse_cid(k)?;
                let active = !matches!(b, Binding::Action(a) if a == "default");
                (active && self.divertable.contains_key(&cid)).then_some(cid)
            })
            .collect()
    }

    fn wants_thumb(&self) -> bool {
        self.thumb.is_some() && self.thumb_preset() != "scroll"
    }

    fn thumb_preset(&self) -> &str {
        match &self.profile.thumbwheel {
            Some(Wheel::Preset(p)) => p,
            Some(Wheel::Custom { .. }) => "custom",
            None => "scroll",
        }
    }

    /// Brings the device in line with the profile; `force` re-sends everything (after a
    /// reconnect the device has forgotten it all).
    fn apply(&mut self, profile: Profile, force: bool) -> Result<()> {
        self.profile = profile;
        let want = self.wants();
        if force {
            self.diverted.clear();
            self.raw_xy = false;
            self.thumb_diverted = false;
        }
        for cid in self.diverted.difference(&want).copied().collect::<Vec<_>>() {
            let raw = (cid == GESTURE_BUTTON && self.raw_xy).then_some(false);
            features::set_cid_reporting(&mut self.dev, cid, Some(false), raw)?;
            if raw.is_some() {
                self.raw_xy = false;
            }
            self.diverted.remove(&cid);
        }
        for cid in want.difference(&self.diverted).copied().collect::<Vec<_>>() {
            let gesture = matches!(self.profile.buttons.get(&cid_key(cid)), Some(Binding::Gesture(_)));
            let raw = (gesture && self.divertable[&cid]).then_some(true);
            features::set_cid_reporting(&mut self.dev, cid, Some(true), raw)?;
            if raw.is_some() {
                self.raw_xy = true;
            }
            self.diverted.insert(cid);
        }
        let thumb = self.wants_thumb();
        if thumb != self.thumb_diverted {
            features::set_thumbwheel_diverted(&mut self.dev, thumb)?;
            self.thumb_diverted = thumb;
        }
        Ok(())
    }

    /// Gives every control back to the device.
    fn restore(&mut self) {
        for cid in std::mem::take(&mut self.diverted) {
            let raw = (cid == GESTURE_BUTTON && self.raw_xy).then_some(false);
            if let Err(e) = features::set_cid_reporting(&mut self.dev, cid, Some(false), raw) {
                eprintln!("remap: {}: could not give back {}: {e:#}", self.dev.name, cid_key(cid));
            }
        }
        if self.thumb_diverted {
            let _ = features::set_thumbwheel_diverted(&mut self.dev, false);
            self.thumb_diverted = false;
        }
    }

    /// The device forgets temporary diversion when it reconnects; check one control.
    fn still_diverted(&mut self) -> bool {
        let Some(&cid) = self.diverted.iter().next() else { return true };
        match features::cid_reporting(&mut self.dev, cid) {
            Ok(r) => r & report_flag::DIVERTED != 0,
            Err(_) => true, // asleep or away; nothing to fix until it answers
        }
    }

    fn action_for(&self, cid: u16) -> Action {
        match self.profile.buttons.get(&cid_key(cid)) {
            Some(Binding::Action(a)) => match Action::parse(a) {
                Ok(Action::Default) => native_action(cid),
                Ok(a) => a,
                Err(_) => Action::None,
            },
            Some(Binding::Gesture(g)) => g.click.as_deref().and_then(|a| Action::parse(a).ok()).unwrap_or(Action::None),
            None => native_action(cid),
        }
    }

    fn gesture_action(&self, cid: u16, s: Swipe) -> Action {
        let Some(Binding::Gesture(g)) = self.profile.buttons.get(&cid_key(cid)) else { return Action::None };
        let a = match s {
            Swipe::Up => &g.up,
            Swipe::Down => &g.down,
            Swipe::Left => &g.left,
            Swipe::Right => &g.right,
        };
        a.as_deref().and_then(|a| Action::parse(a).ok()).unwrap_or(Action::None)
    }

    fn handle(&mut self, r: &[u8], out: &mut Output) -> Result<()> {
        // Receiver: device (re)connected, 10 idx 41 ... with bit 6 of byte 4 clear.
        if r[0] == 0x10 && r[2] == 0x41 {
            if r.len() > 4 && r[4] & 0x40 == 0 {
                thread::sleep(Duration::from_millis(300));
                let p = self.profile.clone();
                self.apply(p, true)?;
                println!("remap: {} reconnected, controls re-diverted", self.dev.name);
            }
            return Ok(());
        }
        if r.len() < 8 || r[0] != 0x11 || r[3] & 0x0F != 0 {
            return Ok(());
        }
        let event = r[3] >> 4;
        if r[2] == self.reprog && event == 0 {
            // divertedButtonsEvent: up to four control ids held right now.
            let now: BTreeSet<u16> =
                r[4..12].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).filter(|c| *c != 0).collect();
            for cid in now.difference(&self.pressed).copied().collect::<Vec<_>>() {
                if matches!(self.profile.buttons.get(&cid_key(cid)), Some(Binding::Gesture(_))) {
                    self.gesture = Some(GestureState { dx: 0, dy: 0, fired: false });
                } else {
                    let a = self.action_for(cid);
                    out.press(&a, Some(&mut self.dev))?;
                }
            }
            for cid in self.pressed.difference(&now).copied().collect::<Vec<_>>() {
                if matches!(self.profile.buttons.get(&cid_key(cid)), Some(Binding::Gesture(_))) {
                    if let Some(g) = self.gesture.take() {
                        if !g.fired {
                            let a = self.action_for(cid);
                            out.tap(&a, Some(&mut self.dev))?;
                        }
                    }
                } else {
                    out.release(&self.action_for(cid))?;
                }
            }
            self.pressed = now;
        } else if r[2] == self.reprog && event == 1 {
            // divertedRawMouseXYEvent while the gesture button is held.
            let dx = i16::from_be_bytes([r[4], r[5]]) as i32;
            let dy = i16::from_be_bytes([r[6], r[7]]) as i32;
            if let Some(g) = self.gesture.as_mut() {
                if !g.fired {
                    g.dx += dx;
                    g.dy += dy;
                    if let Some(s) = swipe(g.dx, g.dy) {
                        g.fired = true;
                        let cid = self.pressed.iter().copied().find(|c| {
                            matches!(self.profile.buttons.get(&cid_key(*c)), Some(Binding::Gesture(_)))
                        });
                        if let Some(cid) = cid {
                            let a = self.gesture_action(cid, s);
                            out.tap(&a, Some(&mut self.dev))?;
                        }
                    }
                }
            }
        } else if Some(r[2]) == self.thumb && event == 0 && self.thumb_diverted {
            let rot = i16::from_be_bytes([r[4], r[5]]) as i32;
            self.thumb_acc += rot;
            while self.thumb_acc.abs() >= self.thumb_step {
                let right = self.thumb_acc > 0;
                self.thumb_acc -= self.thumb_step * self.thumb_acc.signum();
                self.thumb_step_action(right, out)?;
            }
        }
        Ok(())
    }

    fn thumb_step_action(&mut self, right: bool, out: &mut Output) -> Result<()> {
        use key::*;
        let pick = |l: &[u16], r: &[u16]| Action::Keys(if right { r.to_vec() } else { l.to_vec() });
        let a = match &self.profile.thumbwheel {
            Some(Wheel::Custom { left, right: r }) => Action::parse(if right { r } else { left })?,
            Some(Wheel::Preset(p)) => match p.as_str() {
                "volume" => pick(&[VOLUMEDOWN], &[VOLUMEUP]),
                "zoom" => return out.zoom(if right { 1 } else { -1 }),
                "workspaces" => pick(&[LEFTMETA, PAGEUP], &[LEFTMETA, PAGEDOWN]),
                "tabs" => pick(&[LEFTCTRL, PAGEUP], &[LEFTCTRL, PAGEDOWN]),
                _ => return Ok(()),
            },
            None => return Ok(()),
        };
        out.tap(&a, Some(&mut self.dev))
    }
}

fn mtime() -> Option<SystemTime> {
    fs::metadata(config_path().ok()?).ok()?.modified().ok()
}

/// Runs until `stop` is set; every diverted control is given back before returning.
#[cfg(target_os = "linux")]
pub fn run(stop: Arc<AtomicBool>) -> Result<()> {
    let mut config = Config::load()?;
    let mut config_time = mtime();
    let out = Arc::new(Mutex::new(Output::new()?));
    let focus = Focus::open().ok();
    let app = Arc::new(Mutex::new(focus.as_ref().map(|f| f.app()).unwrap_or_default()));
    let shared_config = Arc::new(Mutex::new(config.clone()));
    let mut running: HashMap<String, thread::JoinHandle<()>> = HashMap::new();
    let mut last_scan = Instant::now() - Duration::from_secs(3600);

    while !stop.load(Ordering::Relaxed) {
        if last_scan.elapsed() > Duration::from_secs(30) {
            last_scan = Instant::now();
            running.retain(|_, h| !h.is_finished());
            for dev in hidpp::discover()? {
                let mut probe = dev;
                if !probe.has(f::REPROG_CONTROLS_V4) {
                    continue;
                }
                let Ok(unit) = features::unit_id(&mut probe) else { continue };
                if running.contains_key(&unit) {
                    continue;
                }
                let (stop, out, app, cfg) = (stop.clone(), out.clone(), app.clone(), shared_config.clone());
                let name = probe.name.clone();
                running.insert(
                    unit,
                    thread::spawn(move || {
                        if let Err(e) = work(probe, stop, out, app, cfg) {
                            eprintln!("remap: {name}: {e:#}");
                        }
                    }),
                );
            }
        }
        thread::sleep(FOCUS_POLL);
        if let Some(f) = &focus {
            let now = f.app();
            let mut a = app.lock().unwrap();
            if *a != now {
                *a = now;
            }
        }
        let t = mtime();
        if t != config_time {
            config_time = t;
            match Config::load() {
                Ok(c) => {
                    config = c;
                    *shared_config.lock().unwrap() = config.clone();
                    println!("remap: configuration reloaded");
                }
                Err(e) => eprintln!("remap: keeping the previous configuration: {e:#}"),
            }
        }
    }
    for (_, h) in running {
        let _ = h.join();
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn work(dev: Device, stop: Arc<AtomicBool>, out: Arc<Mutex<Output>>, app: Arc<Mutex<String>>, cfg: Arc<Mutex<Config>>) -> Result<()> {
    let mut w = Worker::new(dev)?;
    let profile_now = |w: &Worker| cfg.lock().unwrap().effective(&w.unit, &app.lock().unwrap(), w.is_mouse);
    let p = profile_now(&w);
    w.apply(p, true)?;
    println!("remap: {} ({}) ready, {} controls diverted", w.dev.name, w.unit, w.diverted.len());
    let result = (|| -> Result<()> {
        while !stop.load(Ordering::Relaxed) {
            if let Some(r) = w.dev.next_report(Duration::from_millis(100))? {
                let mut o = out.lock().unwrap();
                if let Err(e) = w.handle(&r, &mut o) {
                    eprintln!("remap: {}: {e:#}", w.dev.name);
                }
                continue;
            }
            // Idle: follow focus / config changes, and re-divert after a host switch.
            let p = profile_now(&w);
            if p != w.profile && w.pressed.is_empty() {
                w.apply(p, false)?;
            }
            if w.last_check.elapsed() > RECHECK {
                w.last_check = Instant::now();
                if !w.still_diverted() {
                    let p = w.profile.clone();
                    w.apply(p, true)?;
                    println!("remap: {} came back, controls re-diverted", w.dev.name);
                }
            }
        }
        Ok(())
    })();
    // Release anything still held, then hand the controls back.
    {
        let mut o = out.lock().unwrap();
        for cid in std::mem::take(&mut w.pressed) {
            let _ = o.release(&w.action_for(cid));
        }
    }
    w.restore();
    println!("remap: {} restored", w.dev.name);
    result
}

// ---------- API for the GUI and CLI ----------

#[derive(Serialize, Clone, Debug)]
pub struct ButtonInfo {
    pub cid: String,
    pub name: String,
    pub gestures: bool,
}

/// Controls of a device that can be given an action.
pub fn buttons(dev: &mut Device) -> Result<Vec<ButtonInfo>> {
    Ok(features::controls(dev)?
        .into_iter()
        .filter(|c| c.divertable && !c.is_virtual())
        .map(|c| ButtonInfo {
            cid: cid_key(c.cid),
            name: match features::control_name(c.cid) {
                "" => format!("Key {}", cid_key(c.cid)),
                n => n.to_string(),
            },
            gestures: c.supports_raw_xy(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions() {
        assert_eq!(Action::parse("key:ctrl+shift+t").unwrap(), Action::Keys(vec![29, 42, 20]));
        assert_eq!(Action::parse("click:back").unwrap(), Action::Click(key::BTN_SIDE));
        assert_eq!(Action::parse("workspace-left").unwrap(), Action::Keys(vec![key::LEFTMETA, key::PAGEUP]));
        assert!(Action::parse("key:ctrl+nope").is_err());
        assert!(Action::parse("fly").is_err());
        for (id, _) in ACTIONS {
            if !id.ends_with(':') {
                Action::parse(id).unwrap();
            }
        }
    }

    #[test]
    fn swipes_need_distance_and_pick_the_main_axis() {
        assert_eq!(swipe(10, -20), None);
        assert_eq!(swipe(5, -80), Some(Swipe::Up));
        assert_eq!(swipe(-90, 30), Some(Swipe::Left));
        assert_eq!(swipe(70, 10), Some(Swipe::Right));
        assert_eq!(swipe(0, 60), Some(Swipe::Down));
    }

    #[test]
    fn app_profiles_override_and_mice_get_default_gestures() {
        let text = r#"
            [devices.ABCD]
            name = "MX Master 3S"
            [devices.ABCD.buttons]
            "0x0053" = "key:alt+left"
            "0x00c3" = { up = "overview", click = "lock" }
            [devices.ABCD.apps.firefox.buttons]
            "0x0053" = "tab-previous"
            [devices.ABCD.apps.firefox]
            thumbwheel = "tabs"
        "#;
        let c: Config = toml::from_str(text).unwrap();
        c.validate().unwrap();
        let base = c.effective("ABCD", "code", true);
        assert_eq!(base.buttons["0x0053"], Binding::Action("key:alt+left".into()));
        let ff = c.effective("ABCD", "Firefox", true);
        assert_eq!(ff.buttons["0x0053"], Binding::Action("tab-previous".into()));
        assert_eq!(ff.thumbwheel, Some(Wheel::Preset("tabs".into())));
        assert!(matches!(ff.buttons["0x00c3"], Binding::Gesture(_)));
        let unknown = c.effective("FFFF", "", true);
        assert_eq!(unknown.buttons[&cid_key(GESTURE_BUTTON)], Binding::Gesture(Gesture::options_plus_default()));
        assert!(c.effective("FFFF", "", false).buttons.is_empty());
        let round: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(round, c);
    }

    #[test]
    fn rejects_bad_config() {
        let c: Config = toml::from_str("[devices.A.buttons]\n\"0x0053\" = \"key:ctrl+zz\"\n").unwrap();
        assert!(c.validate().is_err());
        let c: Config = toml::from_str("[devices.A]\nthumbwheel = \"spin\"\n").unwrap();
        assert!(c.validate().is_err());
    }
}
