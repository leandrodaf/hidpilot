//! The Zone headset as BlueZ sees it, for when it talks to this machine over Bluetooth
//! instead of its USB receiver. Read through `bluetoothctl`, so no extra dependency.

use std::process::Command;

/// Bluetooth vendor/product the Zone Vibe announces (PnP record `usb:v046Dp0328`).
const MODALIAS: &str = "usb:v046Dp0328";

#[derive(Debug, Clone)]
pub struct BtHeadset {
    pub address: String,
    pub name: String,
    pub connected: bool,
    /// From the hands-free profile; BlueZ only knows it while connected.
    pub battery: Option<u8>,
}

fn bluetoothctl(args: &[&str]) -> String {
    Command::new("bluetoothctl")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Addresses in `bluetoothctl devices [filter]` output.
fn addresses(filter: Option<&str>) -> Vec<String> {
    let mut args = vec!["devices"];
    args.extend(filter);
    bluetoothctl(&args)
        .lines()
        .filter_map(|l| l.strip_prefix("Device ")?.split_whitespace().next().map(str::to_string))
        .collect()
}

/// Bluetooth devices connected right now; cheap enough to poll.
pub fn connected() -> Vec<String> {
    addresses(Some("Connected"))
}

/// The paired Zone headset, connected or not.
pub fn zone_headset() -> Option<BtHeadset> {
    for address in addresses(Some("Paired")) {
        let info = bluetoothctl(&["info", &address]);
        let field = |key: &str| info.lines().find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_string()));
        let name = field("Name:").unwrap_or_default();
        let is_zone = field("Modalias:").is_some_and(|m| m.starts_with(MODALIAS)) || name.starts_with("Zone ");
        if !is_zone {
            continue;
        }
        // "Battery Percentage: 0x50 (80)"
        let battery = field("Battery Percentage:").and_then(|v| v.split_once('(')?.1.trim_end_matches(')').parse().ok());
        return Some(BtHeadset { address, name, connected: field("Connected:").as_deref() == Some("yes"), battery });
    }
    None
}
