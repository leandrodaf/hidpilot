//! Desktop app: a webview UI over the same device code the CLI uses.

use anyhow::{Context, Result, anyhow};
use hidpilot::centurion::{self, Headset};
use hidpilot::features::{self as feat, BacklightMode};
use hidpilot::hidpp::{self, Device, feature as f};
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Default)]
struct Devices {
    hidpp: Vec<Device>,
    headset: Option<Headset>,
    scanned: bool,
}

struct State(Mutex<Devices>);

impl Devices {
    fn rescan(&mut self) -> Result<()> {
        self.hidpp = hidpp::discover()?;
        self.headset = centurion::find()?;
        self.scanned = true;
        Ok(())
    }

    fn ensure(&mut self) -> Result<()> {
        if !self.scanned { self.rescan() } else { Ok(()) }
    }
}

fn kind(dev: &mut Device) -> &'static str {
    // DEVICE_NAME fn 2 = getDeviceType: 0 keyboard, 2 numpad, 3 mouse, 4 trackpad, 5 trackball
    match dev.call(f::DEVICE_NAME, 2, &[]).map(|r| r[0]) {
        Ok(0) | Ok(2) => "keyboard",
        _ => "mouse",
    }
}

fn battery_json(dev: &mut Device) -> Value {
    match feat::battery(dev) {
        Ok(b) => json!({
            "percent": b.percent,
            "level": b.level,
            "charging": matches!(b.status, "charging" | "almost full" | "charging (slow)"),
        }),
        Err(_) => Value::Null,
    }
}

fn headset_battery_json(h: &mut Headset) -> Value {
    match h.battery() {
        Ok(b) => json!({ "percent": b.percent, "charging": b.charging, "full": b.full }),
        Err(_) => Value::Null,
    }
}

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command(async)]
fn devices(state: tauri::State<State>, rescan: bool) -> Result<Value, String> {
    let mut d = state.0.lock().unwrap();
    if rescan { d.rescan() } else { d.ensure() }.map_err(err)?;
    let mut out = Vec::new();
    for (i, dev) in d.hidpp.iter_mut().enumerate() {
        let kind = kind(dev);
        out.push(json!({
            "id": format!("hidpp:{i}"),
            "name": dev.name,
            "kind": kind,
            "link": match dev.via { hidpp::Link::Receiver => "bolt", hidpp::Link::Bluetooth => "bluetooth" },
            "battery": battery_json(dev),
        }));
    }
    if let Some(h) = d.headset.as_mut() {
        out.push(json!({
            "id": "headset",
            "name": h.name,
            "kind": "headset",
            "link": "zone-receiver",
            "battery": headset_battery_json(h),
        }));
    }
    Ok(Value::Array(out))
}

fn with_device<T>(state: &tauri::State<State>, id: &str, run: impl FnOnce(Target) -> Result<T>) -> Result<T, String> {
    let mut d = state.0.lock().unwrap();
    d.ensure().map_err(err)?;
    let target = if id == "headset" {
        Target::Headset(d.headset.as_mut().ok_or("headset is not connected")?)
    } else {
        let i: usize = id.strip_prefix("hidpp:").and_then(|n| n.parse().ok()).ok_or("bad device id")?;
        Target::Hidpp(d.hidpp.get_mut(i).ok_or("device is gone; rescan")?)
    };
    run(target).map_err(err)
}

enum Target<'a> {
    Hidpp(&'a mut Device),
    Headset(&'a mut Headset),
}

/// Reads one setting, leaving it out of the snapshot when the device lacks it.
fn opt<T: serde::Serialize>(r: Result<T>) -> Value {
    r.map(|v| json!(v)).unwrap_or(Value::Null)
}

fn hidpp_snapshot(dev: &mut Device) -> Value {
    let mut s = serde_json::Map::new();
    s.insert("name".into(), json!(dev.name));
    s.insert("kind".into(), json!(kind(dev)));
    s.insert("battery".into(), battery_json(dev));
    s.insert(
        "firmware".into(),
        opt(feat::firmware(dev).map(|v| v.into_iter().map(|fw| json!({"kind": fw.kind, "version": fw.version})).collect::<Vec<_>>())),
    );
    if dev.has(f::ADJUSTABLE_DPI) {
        s.insert("dpi".into(), opt(feat::dpi(dev)));
        s.insert("dpiChoices".into(), opt(feat::dpi_choices(dev)));
    }
    if dev.has(f::SMART_SHIFT) || dev.has(f::SMART_SHIFT_ENHANCED) {
        s.insert("smartshift".into(), opt(feat::smartshift(dev).map(|v| json!({"ratchet": v.ratchet, "threshold": v.threshold}))));
    }
    if dev.has(f::HIRES_WHEEL) {
        s.insert(
            "wheel".into(),
            opt(feat::wheel_mode(dev).map(|m| json!({"hires": m & feat::WHEEL_HIRES != 0, "invert": m & feat::WHEEL_INVERT != 0}))),
        );
    }
    if dev.has(f::THUMB_WHEEL) {
        s.insert("thumbwheelInvert".into(), opt(feat::thumbwheel_inverted(dev)));
    }
    if dev.has(f::BACKLIGHT2) {
        s.insert(
            "backlight".into(),
            opt(feat::backlight(dev).map(|b| {
                let mode = match b.mode() {
                    BacklightMode::Off => "off",
                    BacklightMode::On => "on",
                    BacklightMode::Auto => "auto",
                    BacklightMode::Manual => "manual",
                };
                json!({"mode": mode, "level": b.level, "levels": b.levels, "auto": b.supports_auto(), "manual": b.supports_manual()})
            })),
        );
    }
    if dev.has(f::K375S_FN_INVERSION) {
        s.insert("fnKeys".into(), opt(feat::fn_inverted(dev)));
    }
    if dev.has(f::MULTIPLATFORM) {
        s.insert(
            "platform".into(),
            opt(feat::platforms(dev).map(|p| json!({"current": p.current, "settable": p.settable, "options": p.options}))),
        );
    }
    if dev.has(f::CHANGE_HOST) {
        s.insert(
            "hosts".into(),
            opt(feat::hosts(dev).map(|h| json!({"count": h.count, "current": h.current, "names": h.names}))),
        );
    }
    Value::Object(s)
}

fn headset_snapshot(h: &mut Headset) -> Value {
    json!({
        "name": h.name,
        "kind": "headset",
        "serial": opt(h.serial()),
        "battery": headset_battery_json(h),
        "autoSleep": opt(h.auto_sleep()),
    })
}

#[tauri::command(async)]
fn device(state: tauri::State<State>, id: String) -> Result<Value, String> {
    with_device(&state, &id, |t| {
        Ok(match t {
            Target::Hidpp(dev) => hidpp_snapshot(dev),
            Target::Headset(h) => headset_snapshot(h),
        })
    })
}

fn as_bool(v: &Value) -> Result<bool> {
    v.as_bool().context("expected true/false")
}

fn as_u8(v: &Value) -> Result<u8> {
    v.as_u64().and_then(|n| u8::try_from(n).ok()).context("expected a number 0-255")
}

#[tauri::command(async)]
fn set(state: tauri::State<State>, id: String, key: String, value: Value) -> Result<Value, String> {
    with_device(&state, &id, |t| {
        match t {
            Target::Hidpp(dev) => {
                match key.as_str() {
                    "dpi" => {
                        feat::set_dpi(dev, value.as_u64().context("expected a number")? as u16)?;
                    }
                    "smartshift.ratchet" => feat::set_smartshift(dev, Some(as_bool(&value)?), None)?,
                    "smartshift.threshold" => feat::set_smartshift(dev, None, Some(as_u8(&value)?))?,
                    "wheel.hires" => feat::set_wheel_flag(dev, feat::WHEEL_HIRES, as_bool(&value)?)?,
                    "wheel.invert" => feat::set_wheel_flag(dev, feat::WHEEL_INVERT, as_bool(&value)?)?,
                    "thumbwheelInvert" => feat::set_thumbwheel_inverted(dev, as_bool(&value)?)?,
                    "fnKeys" => feat::set_fn_inverted(dev, as_bool(&value)?)?,
                    "platform" => {
                        feat::set_platform(dev, value.as_str().context("expected an OS name")?)?;
                    }
                    "host" => feat::switch_host(dev, as_u8(&value)?)?,
                    "backlight.mode" => {
                        let mode = match value.as_str() {
                            Some("off") => BacklightMode::Off,
                            Some("on") => BacklightMode::On,
                            Some("auto") => BacklightMode::Auto,
                            Some("manual") => BacklightMode::Manual,
                            _ => return Err(anyhow!("unknown backlight mode")),
                        };
                        feat::set_backlight(dev, Some(mode), None)?;
                    }
                    "backlight.level" => feat::set_backlight(dev, None, Some(as_u8(&value)?))?,
                    _ => return Err(anyhow!("unknown setting {key}")),
                }
                Ok(hidpp_snapshot(dev))
            }
            Target::Headset(h) => {
                match key.as_str() {
                    "autoSleep" => h.set_auto_sleep(as_u8(&value)?)?,
                    _ => return Err(anyhow!("unknown setting {key}")),
                }
                Ok(headset_snapshot(h))
            }
        }
    })
}

fn main() {
    tauri::Builder::default()
        .manage(State(Mutex::new(Devices::default())))
        .invoke_handler(tauri::generate_handler![devices, device, set])
        .run(tauri::generate_context!())
        .expect("failed to start hidpilot");
}
