mod centurion;
mod daemon;
mod features;
mod hidpp;
#[allow(dead_code)] // feature-report helpers are unused so far
mod hidraw;
mod uhid;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use features::BacklightMode;
use hidpp::{Device, feature as f};

#[derive(Parser)]
#[command(version, about = "Configure Logitech mice, keyboards and headsets on Linux")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List connected devices
    List,
    /// Show everything a device reports (firmware, battery, settings, features)
    Info { device: String },
    /// Show battery level of all devices, or of one
    Battery { device: Option<String> },
    /// Show or set pointer sensitivity (DPI)
    Dpi { device: String, value: Option<u16> },
    /// Show or set the SmartShift scroll wheel
    Smartshift {
        device: String,
        #[arg(long, value_enum)]
        mode: Option<WheelMode>,
        /// Wheel speed that switches to free spin (1-50; 50 = always ratchet)
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=50))]
        threshold: Option<u8>,
    },
    /// Show or set the main scroll wheel
    Wheel {
        device: String,
        /// High resolution (smooth) scrolling
        #[arg(long, value_enum)]
        hires: Option<Toggle>,
        /// Invert scroll direction
        #[arg(long, value_enum)]
        invert: Option<Toggle>,
    },
    /// Show or set the thumb wheel direction
    Thumbwheel {
        device: String,
        #[arg(long, value_enum)]
        invert: Option<Toggle>,
    },
    /// Show or set keyboard backlight
    Backlight {
        device: String,
        #[arg(long, value_enum)]
        mode: Option<LightMode>,
        #[arg(long)]
        level: Option<u8>,
    },
    /// Show or set whether F1-F12 act as function keys without holding Fn
    FnKeys { device: String, state: Option<Toggle> },
    /// Show or set which OS the keys are laid out for (same as holding Fn+O / Fn+P)
    Platform { device: String, os: Option<String> },
    /// Show Easy-Switch hosts, or switch to host N (1-3)
    Host { device: String, number: Option<u8> },
    /// List the device's reprogrammable buttons
    Buttons { device: String },
    /// Show or configure the Zone headset
    Headset {
        /// Minutes of inactivity before it switches off
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..))]
        auto_sleep: Option<u8>,
    },
    /// Run the background service (publishes batteries to GNOME; needs root)
    Daemon,
}

#[derive(Clone, Copy, ValueEnum)]
enum Toggle {
    On,
    Off,
}

impl Toggle {
    fn on(self) -> bool {
        matches!(self, Toggle::On)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum WheelMode {
    Ratchet,
    Freespin,
}

#[derive(Clone, Copy, ValueEnum)]
enum LightMode {
    Off,
    On,
    Auto,
    Manual,
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn open(query: &str) -> Result<Device> {
    hidpp::select(hidpp::discover()?, query)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            let devices = hidpp::discover()?;
            let headset = centurion::find()?;
            if devices.is_empty() && headset.is_none() {
                println!("No Logitech HID++ devices online.");
            }
            for (i, d) in devices.iter().enumerate() {
                let via = match d.via {
                    hidpp::Link::Receiver => format!("receiver slot {}", d.index),
                    hidpp::Link::Bluetooth => "bluetooth".into(),
                };
                println!("{}. {}  ({via}, {})", i + 1, d.name, d.path().display());
            }
            if let Some(h) = headset {
                println!("-. {}  (Zone receiver; use `hidpilot headset`)", h.name);
            }
        }
        Cmd::Info { device } => info(&mut open(&device)?)?,
        Cmd::Battery { device } => {
            let devices = match device {
                Some(q) => vec![open(&q)?],
                None => hidpp::discover()?,
            };
            for mut d in devices {
                match features::battery(&mut d) {
                    Ok(b) => println!("{}: {}", d.name, battery_text(&b)),
                    Err(e) => println!("{}: {e}", d.name),
                }
            }
            if let Some(mut h) = centurion::find()? {
                match h.battery() {
                    Ok(b) => println!("{}: {}", h.name, headset_battery_text(&b)),
                    Err(e) => println!("{}: {e}", h.name),
                }
            }
        }
        Cmd::Headset { auto_sleep } => {
            let Some(mut h) = centurion::find()? else {
                anyhow::bail!("no Zone headset found (is it switched on?)");
            };
            if let Some(m) = auto_sleep {
                h.set_auto_sleep(m)?;
            }
            println!("{}", h.name);
            println!("  serial: {}", h.serial()?);
            println!("  battery: {}", headset_battery_text(&h.battery()?));
            println!("  auto sleep: {} min", h.auto_sleep()?);
        }
        Cmd::Dpi { device, value } => {
            let mut d = open(&device)?;
            match value {
                Some(v) => println!("DPI set to {}", features::set_dpi(&mut d, v)?),
                None => {
                    let choices = features::dpi_choices(&mut d)?;
                    println!("DPI: {}", features::dpi(&mut d)?);
                    if let (Some(min), Some(max)) = (choices.first(), choices.last()) {
                        println!("range: {min}-{max} ({} values)", choices.len());
                    }
                }
            }
        }
        Cmd::Smartshift { device, mode, threshold } => {
            let mut d = open(&device)?;
            if mode.is_some() || threshold.is_some() {
                features::set_smartshift(&mut d, mode.map(|m| matches!(m, WheelMode::Ratchet)), threshold)?;
            }
            print_smartshift(&mut d)?;
        }
        Cmd::Wheel { device, hires, invert } => {
            let mut d = open(&device)?;
            if let Some(t) = hires {
                features::set_wheel_flag(&mut d, features::WHEEL_HIRES, t.on())?;
            }
            if let Some(t) = invert {
                features::set_wheel_flag(&mut d, features::WHEEL_INVERT, t.on())?;
            }
            let m = features::wheel_mode(&mut d)?;
            println!("high resolution: {}", on_off(m & features::WHEEL_HIRES != 0));
            println!("inverted: {}", on_off(m & features::WHEEL_INVERT != 0));
        }
        Cmd::Thumbwheel { device, invert } => {
            let mut d = open(&device)?;
            if let Some(t) = invert {
                features::set_thumbwheel_inverted(&mut d, t.on())?;
            }
            println!("thumb wheel inverted: {}", on_off(features::thumbwheel_inverted(&mut d)?));
        }
        Cmd::Backlight { device, mode, level } => {
            let mut d = open(&device)?;
            if mode.is_some() || level.is_some() {
                let mode = mode.map(|m| match m {
                    LightMode::Off => BacklightMode::Off,
                    LightMode::On => BacklightMode::On,
                    LightMode::Auto => BacklightMode::Auto,
                    LightMode::Manual => BacklightMode::Manual,
                });
                features::set_backlight(&mut d, mode, level)?;
            }
            print_backlight(&mut d)?;
        }
        Cmd::FnKeys { device, state } => {
            let mut d = open(&device)?;
            if let Some(t) = state {
                features::set_fn_inverted(&mut d, t.on())?;
            }
            println!("F1-F12 without Fn: {}", on_off(features::fn_inverted(&mut d)?));
        }
        Cmd::Host { device, number } => {
            let mut d = open(&device)?;
            match number {
                Some(n) => {
                    let hosts = features::hosts(&mut d)?;
                    if n == 0 || n > hosts.count {
                        anyhow::bail!("host must be 1..={}", hosts.count);
                    }
                    features::switch_host(&mut d, n - 1)?;
                    println!("{} switched to host {n}", d.name);
                }
                None => print_hosts(&mut d)?,
            }
        }
        Cmd::Platform { device, os } => {
            let mut d = open(&device)?;
            if let Some(os) = os {
                features::set_platform(&mut d, &os)?;
            }
            print_platforms(&mut d)?;
        }
        Cmd::Daemon => daemon::run()?,
        Cmd::Buttons { device } => {
            let mut d = open(&device)?;
            for c in features::controls(&mut d)? {
                let name = features::control_name(c.cid);
                let flags = match (c.divertable, c.diverted) {
                    (_, true) => "diverted",
                    (true, false) => "divertable",
                    _ => "",
                };
                println!("{:#06x}  {:<24} task {:#06x}  {flags}", c.cid, name, c.task);
            }
        }
    }
    Ok(())
}

fn battery_text(b: &features::Battery) -> String {
    match b.percent {
        Some(p) => format!("{p}% ({})", b.status),
        None => format!("{} ({})", b.level, b.status),
    }
}

fn headset_battery_text(b: &centurion::Battery) -> String {
    let status = if b.full { "full" } else if b.charging { "charging" } else { "discharging" };
    format!("{}% ({status})", b.percent)
}

fn print_smartshift(d: &mut Device) -> Result<()> {
    let s = features::smartshift(d)?;
    println!("mode: {}", if s.ratchet { "ratchet" } else { "free spin" });
    match s.threshold {
        255 => println!("auto switch: never"),
        t => println!("auto switch threshold: {t}"),
    }
    Ok(())
}

fn print_backlight(d: &mut Device) -> Result<()> {
    let b = features::backlight(d)?;
    let mode = match b.mode() {
        BacklightMode::Off => "off",
        BacklightMode::On => "on",
        BacklightMode::Auto => "auto",
        BacklightMode::Manual => "manual",
    };
    if b.supports_manual() && b.levels > 1 {
        println!("backlight: {mode}, level {} (0-{})", b.level, b.levels - 1);
    } else {
        println!("backlight: {mode}");
    }
    Ok(())
}

fn print_platforms(d: &mut Device) -> Result<()> {
    let p = features::platforms(d)?;
    for (id, names) in &p.options {
        let mark = if *id == p.current { "*" } else { " " };
        println!("{mark} {}", names.join(", "));
    }
    Ok(())
}

fn print_hosts(d: &mut Device) -> Result<()> {
    let h = features::hosts(d)?;
    for i in 0..h.count {
        let mark = if i == h.current { "*" } else { " " };
        let (paired, name) = h.names.get(i as usize).cloned().unwrap_or((true, String::new()));
        let state = if paired { name } else { "(not paired)".into() };
        println!("{mark} {}. {state}", i + 1);
    }
    Ok(())
}

fn info(d: &mut Device) -> Result<()> {
    println!("{}  ({})", d.name, d.path().display());
    if let Ok(fw) = features::firmware(d) {
        for x in fw {
            println!("  {}: {}", x.kind, x.version);
        }
    }
    if let Ok(b) = features::battery(d) {
        println!("  battery: {}", battery_text(&b));
    }
    if d.has(f::ADJUSTABLE_DPI) {
        println!("  DPI: {}", features::dpi(d)?);
    }
    if d.has(f::SMART_SHIFT) || d.has(f::SMART_SHIFT_ENHANCED) {
        let s = features::smartshift(d)?;
        let t = if s.threshold == 255 { "never".to_string() } else { s.threshold.to_string() };
        println!("  SmartShift: {}, threshold {t}", if s.ratchet { "ratchet" } else { "free spin" });
    }
    if d.has(f::HIRES_WHEEL) {
        let m = features::wheel_mode(d)?;
        println!(
            "  wheel: hires {}, inverted {}",
            on_off(m & features::WHEEL_HIRES != 0),
            on_off(m & features::WHEEL_INVERT != 0)
        );
    }
    if d.has(f::THUMB_WHEEL) {
        println!("  thumb wheel inverted: {}", on_off(features::thumbwheel_inverted(d)?));
    }
    if d.has(f::BACKLIGHT2) {
        print!("  ");
        print_backlight(d)?;
    }
    if d.has(f::K375S_FN_INVERSION) {
        println!("  F1-F12 without Fn: {}", on_off(features::fn_inverted(d)?));
    }
    if d.has(f::MULTIPLATFORM) {
        println!("  OS key layout:");
        print_platforms(d)?;
    }
    if d.has(f::CHANGE_HOST) {
        println!("  Easy-Switch hosts:");
        print_hosts(d)?;
    }
    let list = d.feature_list()?;
    let ids: Vec<String> = list.iter().map(|(id, _)| format!("{id:04x}")).collect();
    println!("  features: {}", ids.join(" "));
    Ok(())
}
