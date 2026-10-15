//! Desktop app: a webview UI over the same device code the CLI uses.

mod logfile;
mod tray;
#[cfg(target_os = "linux")]
mod x11;

use anyhow::{Context, Result, anyhow};
use hidpilot::centurion::{self, Headset};
use hidpilot::features::{self as feat, BacklightMode};
use hidpilot::hidpp::{self, Device, feature as f};
use hidpilot::remap;
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
fn devices(app: tauri::AppHandle, state: tauri::State<State>, rescan: bool) -> Result<Value, String> {
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
    // "MX Master 3S 95% · Zone Vibe Wireless 87%" in the tray menu and tooltip.
    let summary: Vec<String> = out
        .iter()
        .filter_map(|d| Some(format!("{} {}%", d["name"].as_str()?, d["battery"]["percent"].as_u64()?)))
        .collect();
    tray::set_status(&app, &if summary.is_empty() { "Nenhum dispositivo".to_string() } else { summary.join(" · ") });
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
    let eq = h.equalizer().ok().map(|e| {
        let preset = centurion::EQ_PRESETS.iter().find(|(_, b)| *b == e.bands).map(|(n, _)| *n).unwrap_or("Custom");
        json!({"bands": e.bands, "preset": preset})
    });
    json!({
        "name": h.name,
        "kind": "headset",
        "serial": opt(h.serial()),
        "battery": headset_battery_json(h),
        "autoSleep": opt(h.auto_sleep()),
        "equalizer": eq,
        "presets": centurion::EQ_PRESETS.iter().map(|(n, b)| json!({"name": n, "bands": b})).collect::<Vec<_>>(),
        "sidetone": opt(h.sidetone()),
        "sidetoneMax": centurion::SIDETONE_MAX,
        "micMuted": opt(h.mic_muted()),
        "voicePrompts": opt(h.voice_prompts()),
        "rotateToMute": opt(h.rotate_to_mute()),
        "bluetoothConnections": opt(h.bluetooth_connections()),
        "micLevel": mic::level().ok(),
    })
}

/// The headset's microphone level lives in the sound server (PipeWire), as in Logi Tune.
mod mic {
    use anyhow::{Context, Result, bail};
    use std::process::Command;

    /// Node id of the headset microphone, from `wpctl status`.
    fn source() -> Result<String> {
        let out = Command::new("wpctl").arg("status").output().context("wpctl not found")?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut in_sources = false;
        for line in text.lines() {
            let l = line.trim_start_matches([' ', '│', '├', '└', '─', '*']).trim();
            if line.contains("Sources:") {
                in_sources = true;
                continue;
            }
            if in_sources && line.contains(":") && !l.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                in_sources = false;
            }
            if in_sources && l.contains("Zone") {
                if let Some((id, _)) = l.split_once('.') {
                    return Ok(id.trim().to_string());
                }
            }
        }
        bail!("headset microphone not found in PipeWire")
    }

    pub fn level() -> Result<u32> {
        let out = Command::new("wpctl").args(["get-volume", &source()?]).output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let v: f64 = text.split_whitespace().nth(1).context("no volume")?.parse()?;
        Ok((v * 100.0).round() as u32)
    }

    pub fn set_level(percent: u32) -> Result<()> {
        if percent > 100 {
            bail!("level must be 0-100");
        }
        let v = format!("{:.2}", percent as f64 / 100.0);
        if !Command::new("wpctl").args(["set-volume", &source()?, &v]).status()?.success() {
            bail!("wpctl failed");
        }
        Ok(())
    }
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
                    "name" => h.set_name(value.as_str().context("expected text")?)?,
                    "sidetone" => h.set_sidetone(as_u8(&value)?)?,
                    "micMuted" => h.set_mic_muted(as_bool(&value)?)?,
                    "voicePrompts" => h.set_voice_prompts(as_bool(&value)?)?,
                    "rotateToMute" => h.set_rotate_to_mute(as_bool(&value)?)?,
                    "micLevel" => mic::set_level(value.as_u64().context("expected a number")? as u32)?,
                    "equalizer" => {
                        let bands: Vec<i8> = serde_json::from_value(value.clone()).context("expected five gains")?;
                        let bands: [i8; centurion::EQ_BANDS] = bands.try_into().map_err(|_| anyhow!("expected five gains"))?;
                        let mode = if bands == [0; centurion::EQ_BANDS] { 0 } else { h.equalizer().map(|e| e.mode).unwrap_or(0) };
                        h.set_equalizer(centurion::Equalizer { mode, bands })?;
                    }
                    _ => return Err(anyhow!("unknown setting {key}")),
                }
                Ok(headset_snapshot(h))
            }
        }
    })
}

/// Where scripts/fetch-logitech-assets.sh puts fonts and device pictures taken from the
/// user's own copy of the Logitech apps. They are never shipped with hidpilot.
fn assets_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        .unwrap_or_default();
    base.join("hidpilot/logitech")
}

fn serve_asset(path: &str) -> tauri::http::Response<Vec<u8>> {
    let not_found = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    let rel = path.trim_start_matches('/');
    // Plain relative names only: no "..", no absolute paths.
    if rel.is_empty() || rel.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return not_found();
    }
    let Ok(data) = std::fs::read(assets_dir().join(rel)) else { return not_found() };
    let mime = match rel.rsplit('.').next().unwrap_or("") {
        "webp" => "image/webp",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "otf" => "font/otf",
        "ttf" => "font/ttf",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .body(data)
        .unwrap()
}

// ---------- buttons, gestures and thumb wheel (served by `hidpilot remap run`) ----------

#[tauri::command(async)]
fn remap_get(state: tauri::State<State>, id: String) -> Result<Value, String> {
    with_device(&state, &id, |t| {
        let Target::Hidpp(dev) = t else { anyhow::bail!("not a mouse or keyboard") };
        let unit = feat::unit_id(dev)?;
        let buttons = remap::buttons(dev).unwrap_or_default();
        let cfg = remap::Config::load()?;
        let profile = cfg.effective(&unit, "", !feat::is_keyboard(dev));
        Ok(json!({
            "unit": unit,
            "buttons": buttons,
            "bindings": profile.buttons,
            "thumbwheel": profile.thumbwheel,
            "actions": remap::ACTIONS,
            "thumbPresets": remap::THUMB_PRESETS,
        }))
    })
}

/// `binding`: an action id, a gesture table, or null to give the control back.
#[tauri::command(async)]
fn remap_set(state: tauri::State<State>, id: String, cid: String, binding: Value) -> Result<(), String> {
    with_device(&state, &id, |t| {
        let Target::Hidpp(dev) = t else { anyhow::bail!("not a mouse or keyboard") };
        let unit = feat::unit_id(dev)?;
        let is_mouse = !feat::is_keyboard(dev);
        let mut cfg = remap::Config::load()?;
        // Start from what is in effect, so a mouse keeps its default gestures.
        let current = cfg.effective(&unit, "", is_mouse);
        let entry = cfg.devices.entry(unit).or_default();
        entry.name = dev.name.clone();
        if entry.profile.buttons.is_empty() {
            entry.profile.buttons = current.buttons;
        }
        if binding.is_null() {
            entry.profile.buttons.remove(&cid);
        } else {
            let b: remap::Binding = serde_json::from_value(binding).context("bad binding")?;
            entry.profile.buttons.insert(cid, b);
        }
        cfg.save()
    })
}

#[tauri::command(async)]
fn thumbwheel_set(state: tauri::State<State>, id: String, preset: String) -> Result<(), String> {
    with_device(&state, &id, |t| {
        let Target::Hidpp(dev) = t else { anyhow::bail!("not a mouse") };
        let unit = feat::unit_id(dev)?;
        let mut cfg = remap::Config::load()?;
        let entry = cfg.devices.entry(unit).or_default();
        entry.name = dev.name.clone();
        entry.profile.thumbwheel = if preset == "scroll" { None } else { Some(remap::Wheel::Preset(preset)) };
        cfg.save()
    })
}

// ---------- Flow (settings shared with `hidpilot-flow`, which reloads them) ----------

fn flow_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    base.join("hidpilot/flow.toml")
}

fn flow_load() -> Result<toml::Table> {
    let text = std::fs::read_to_string(flow_path()).context("Flow is not set up yet: run hidpilot-flow once")?;
    Ok(text.parse()?)
}

fn flow_save(t: &toml::Table) -> Result<()> {
    let p = flow_path();
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(t)?)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

#[tauri::command(async)]
fn flow_get() -> Result<Value, String> {
    let t = flow_load().map_err(err)?;
    let peers: Vec<Value> = t
        .get("peers")
        .and_then(|p| p.as_table())
        .map(|p| {
            p.iter()
                .map(|(k, v)| json!({"key": k, "name": v.get("name").and_then(|x| x.as_str()), "side": v.get("side").and_then(|x| x.as_str())}))
                .collect()
        })
        .unwrap_or_default();
    // Never hand the private key to the page.
    Ok(json!({
        "name": t.get("name").and_then(|x| x.as_str()),
        "id": t.get("public_key").and_then(|x| x.as_str()).map(|k| k.chars().take(16).collect::<String>()),
        "clipboard": t.get("clipboard").and_then(|x| x.as_bool()).unwrap_or(true),
        "keyboardFollows": t.get("keyboard_follows").and_then(|x| x.as_bool()).unwrap_or(true),
        "peers": peers,
        "running": std::process::Command::new("pgrep").args(["-x", "hidpilot-flow"]).status().map(|s| s.success()).unwrap_or(false),
    }))
}

#[tauri::command(async)]
fn flow_set(key: String, value: Value, peer: Option<String>) -> Result<Value, String> {
    let mut t = flow_load().map_err(err)?;
    match (key.as_str(), peer) {
        ("clipboard", None) => {
            t.insert("clipboard".into(), toml::Value::Boolean(value.as_bool().ok_or("expected true/false")?));
        }
        ("keyboardFollows", None) => {
            t.insert("keyboard_follows".into(), toml::Value::Boolean(value.as_bool().ok_or("expected true/false")?));
        }
        ("side", Some(peer)) => {
            let side = value.as_str().filter(|s| ["left", "right", "top", "bottom"].contains(s));
            let peers = t.get_mut("peers").and_then(|p| p.as_table_mut()).ok_or("no peers")?;
            // One screen per side.
            if let Some(s) = side {
                for (k, v) in peers.iter_mut() {
                    if *k != peer && v.get("side").and_then(|x| x.as_str()) == Some(s) {
                        v.as_table_mut().unwrap().remove("side");
                    }
                }
            }
            let entry = peers.get_mut(&peer).and_then(|v| v.as_table_mut()).ok_or("unknown computer")?;
            match side {
                Some(s) => entry.insert("side".into(), toml::Value::String(s.into())),
                None => entry.remove("side"),
            };
        }
        ("forget", Some(peer)) => {
            if let Some(p) = t.get_mut("peers").and_then(|p| p.as_table_mut()) {
                p.remove(&peer);
            }
        }
        _ => return Err(format!("unknown Flow setting {key}")),
    }
    flow_save(&t).map_err(err)?;
    flow_get()
}

/// Opens the pairing window (`hidpilot-flow pair`) and returns the code it shows.
#[tauri::command(async)]
fn flow_pair() -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    let mut child = std::process::Command::new("hidpilot-flow")
        .arg("pair")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("hidpilot-flow not found: {e}"))?;
    let out = child.stdout.take().unwrap();
    let mut lines = BufReader::new(out).lines();
    let first = lines.next().ok_or("pairing did not start")?.map_err(|e| e.to_string())?;
    // "Pairing code for NAME: 123  456"
    let code: String = first.rsplit(':').next().unwrap_or("").chars().filter(|c| c.is_ascii_digit()).collect();
    // Keep reading so the child never blocks on a full pipe; it exits on its own.
    std::thread::spawn(move || {
        for _ in lines {}
        let _ = child.wait();
    });
    if code.len() != 6 {
        return Err("could not read the pairing code".into());
    }
    Ok(code)
}

/// JSON from the local Logitech assets (hotspot metadata), read here so the page does
/// not depend on cross-origin fetches of the lgasset:// scheme.
#[tauri::command(async)]
fn asset_json(path: String) -> Result<Value, String> {
    if path.split('/').any(|c| c.is_empty() || c == "." || c == "..") || !path.ends_with(".json") {
        return Err("bad asset path".into());
    }
    let text = std::fs::read_to_string(assets_dir().join(&path)).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))
}

/// Background services the page can check and switch on (user services, no root).
const SERVICES: [&str; 2] = ["hidpilot-remap", "hidpilot-flow"];

fn systemctl_user(args: &[&str]) -> Result<std::process::Output> {
    Ok(std::process::Command::new("systemctl").arg("--user").args(args).output()?)
}

#[tauri::command(async)]
fn service_status(name: String) -> Result<Value, String> {
    if !SERVICES.contains(&name.as_str()) {
        return Err("unknown service".into());
    }
    let unit = format!("{name}.service");
    let active = systemctl_user(&["is-active", &unit]).map(|o| o.status.success()).unwrap_or(false);
    let installed = systemctl_user(&["cat", &unit]).map(|o| o.status.success()).unwrap_or(false);
    // Running by hand (e.g. `hidpilot remap run` in a terminal) counts too.
    let process = match name.as_str() {
        "hidpilot-flow" => "hidpilot-flow",
        _ => "hidpilot",
    };
    let by_hand = std::process::Command::new("pgrep")
        .args(["-f", if process == "hidpilot" { "hidpilot remap run" } else { "hidpilot-flow run" }])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    Ok(json!({ "active": active || by_hand, "installed": installed }))
}

#[tauri::command(async)]
fn service_enable(name: String) -> Result<Value, String> {
    if !SERVICES.contains(&name.as_str()) {
        return Err("unknown service".into());
    }
    let unit = format!("{name}.service");
    let out = systemctl_user(&["enable", "--now", &unit]).map_err(err)?;
    logln!("service_enable {unit}: {} {}", out.status, String::from_utf8_lossy(&out.stderr).trim());
    if !out.status.success() {
        return Err(format!("não foi possível ativar {unit}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    service_status(name)
}

// ---------- typing layout (GNOME input sources) ----------

/// Layouts offered for typing Portuguese; ids are xkb "layout+variant".
const LAYOUTS: [(&str, &str, &str); 4] = [
    ("us+intl", "EUA Internacional – acentos do português", "Teclado físico americano. ´ ~ ^ ` ¨ viram acento nas vogais e ´+c dá ç; nas outras letras sai o próprio símbolo."),
    ("us+altgr-intl", "EUA Internacional com AltGr", "Teclado físico americano. Aspas e acentos saem normais; os acentos ficam no AltGr (AltGr+' = ´)."),
    ("br", "Português (Brasil) ABNT2", "Para teclados físicos ABNT2, com tecla Ç."),
    ("us", "EUA", "Sem acentos."),
];

fn gsettings(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("gsettings").args(args).output().context("gsettings not found")?;
    if !out.status.success() {
        anyhow::bail!("gsettings: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[tauri::command(async)]
fn layout_get() -> Result<Value, String> {
    // e.g. [('xkb', 'us+intl')]
    let sources = gsettings(&["get", "org.gnome.desktop.input-sources", "sources"]).map_err(err)?;
    let current = sources.split('\'').nth(3).unwrap_or("").to_string();
    let compose = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".XCompose"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|t| t.contains("hidpilot"));
    Ok(json!({
        "current": current,
        "options": LAYOUTS.iter().map(|(id, name, hint)| json!({"id": id, "name": name, "hint": hint})).collect::<Vec<_>>(),
        "smartAccents": compose,
    }))
}

#[tauri::command(async)]
fn layout_set(id: String) -> Result<Value, String> {
    if !LAYOUTS.iter().any(|(l, _, _)| *l == id) {
        return Err("unknown layout".into());
    }
    gsettings(&["set", "org.gnome.desktop.input-sources", "sources", &format!("[('xkb', '{id}')]")]).map_err(err)?;
    logln!("layout set to {id}");
    layout_get()
}

// ---------- app settings: start with the computer, open the window at login ----------

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
struct AppSettings {
    /// Start hidpilot when the user logs in (in the tray).
    autostart: bool,
    /// Also show the window when started at login; off = tray only.
    open_on_login: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings { autostart: true, open_on_login: true }
    }
}

fn config_home() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_default()
}

fn settings_path() -> std::path::PathBuf {
    config_home().join("hidpilot/app.toml")
}

fn autostart_path() -> std::path::PathBuf {
    config_home().join("autostart/hidpilot.desktop")
}

fn load_settings() -> AppSettings {
    std::fs::read_to_string(settings_path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Writes the settings and makes the login entry match them.
fn apply_settings(s: AppSettings) -> Result<()> {
    let p = settings_path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, toml::to_string(&s)?)?;
    let entry = autostart_path();
    if s.autostart {
        std::fs::create_dir_all(entry.parent().unwrap())?;
        std::fs::write(
            &entry,
            "[Desktop Entry]\nType=Application\nName=hidpilot\nComment=Logitech devices in the tray\n\
             Exec=hidpilot-gui --login\nIcon=hidpilot\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        )?;
    } else if entry.exists() {
        std::fs::remove_file(&entry)?;
    }
    logln!("settings: autostart={} open_on_login={}", s.autostart, s.open_on_login);
    Ok(())
}

#[tauri::command(async)]
fn settings_get() -> Value {
    let s = load_settings();
    json!({ "autostart": s.autostart, "openOnLogin": s.open_on_login })
}

#[tauri::command(async)]
fn settings_set(key: String, value: bool) -> Result<Value, String> {
    let mut s = load_settings();
    match key.as_str() {
        "autostart" => s.autostart = value,
        "openOnLogin" => s.open_on_login = value,
        _ => return Err("unknown setting".into()),
    }
    apply_settings(s).map_err(err)?;
    Ok(settings_get())
}

/// Lets the page write to the app log (errors, failed loads).
#[tauri::command]
fn log_js(msg: String) {
    logln!("page: {}", msg.chars().take(500).collect::<String>());
}

fn main() {
    logfile::init();
    logln!(
        "start {} session={:?} display={:?} desktop={:?}",
        env!("CARGO_PKG_VERSION"),
        std::env::var("XDG_SESSION_TYPE").ok(),
        std::env::var("DISPLAY").ok(),
        std::env::var("XDG_CURRENT_DESKTOP").ok()
    );
    tauri::Builder::default()
        // Opening hidpilot again (app menu, dock) brings back the running window from
        // the tray instead of starting a second, hidden copy.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            logln!("second launch: showing the running window");
            tray::show_window(app);
        }))
        .register_uri_scheme_protocol("lgasset", |_ctx, req| serve_asset(req.uri().path()))
        .setup(|app| {
            if !settings_path().exists() {
                if let Err(e) = apply_settings(AppSettings::default()) {
                    logln!("settings: could not write defaults: {e:#}");
                }
            }
            // Started at login with "open the window" off: stay in the tray.
            let at_login = std::env::args().any(|a| a == "--login");
            if !(at_login && !load_settings().open_on_login) {
                if let Some(w) = tauri::Manager::get_webview_window(app, "main") {
                    let _ = w.show();
                }
            } else {
                logln!("started at login: staying in the tray");
            }
            if let Err(e) = tray::install(app.handle()) {
                logln!("tray: failed to install: {e}");
            }
            // Report how the window manager received the first window.
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(2));
                #[cfg(target_os = "linux")]
                x11::log_state("startup");
            });
            Ok(())
        })
        .on_window_event(tray::hide_on_close)
        .manage(State(Mutex::new(Devices::default())))
        .invoke_handler(tauri::generate_handler![devices, device, set, remap_get, remap_set, thumbwheel_set, flow_get, flow_set, flow_pair, asset_json, service_status, service_enable, log_js, layout_get, layout_set, settings_get, settings_set])
        .run(tauri::generate_context!())
        .expect("failed to start hidpilot");
}
