//! The mice and keyboards this machine can hand over, and their Easy-Switch channels.

use crate::proto::DeviceChannel;
use anyhow::Result;

mod imp {
    use super::*;
    use hidpilot::features;
    use hidpilot::hidpp::{self, Device, feature as f};

    pub struct Tracked {
        pub dev: Device,
        pub unit: String,
        pub keyboard: bool,
        /// Host names stored in the device, per channel (empty when it cannot tell).
        pub host_names: Vec<String>,
        pub channel: u8,
    }

    pub struct Devices {
        pub list: Vec<Tracked>,
    }

    impl Devices {
        /// Finds Easy-Switch capable devices that are connected to this machine right now.
        pub fn scan() -> Result<Devices> {
            let mut list = Vec::new();
            for mut dev in hidpp::discover()? {
                if !dev.has(f::CHANGE_HOST) {
                    continue;
                }
                let Ok(unit) = features::unit_id(&mut dev) else { continue };
                let Ok(hosts) = features::hosts(&mut dev) else { continue };
                let keyboard = features::is_keyboard(&mut dev);
                list.push(Tracked {
                    unit,
                    keyboard,
                    host_names: hosts.names.into_iter().map(|(_, n)| n).collect(),
                    channel: hosts.current,
                    dev,
                });
            }
            Ok(Devices { list })
        }

        pub fn switch(&mut self, unit: &str, channel: u8) -> Result<()> {
            if let Some(t) = self.list.iter_mut().find(|t| t.unit == unit) {
                features::switch_host(&mut t.dev, channel)?;
            }
            Ok(())
        }
    }
}

pub use imp::Devices;

impl Devices {
    pub fn channels(&self) -> Vec<DeviceChannel> {
        self.list
            .iter()
            .map(|t| DeviceChannel { unit: t.unit.clone(), keyboard: t.keyboard, channel: t.channel })
            .collect()
    }

    /// Channel a peer occupies on one of our devices: what the peer told us, or else
    /// the channel whose stored host name matches the peer's machine name.
    pub fn peer_channel(&self, unit: &str, peer_name: &str, announced: &[DeviceChannel]) -> Option<u8> {
        if let Some(c) = announced.iter().find(|c| c.unit == unit) {
            return Some(c.channel);
        }
        let t = self.list.iter().find(|t| t.unit == unit)?;
        let want = normalize(peer_name);
        if want.is_empty() {
            return None;
        }
        t.host_names
            .iter()
            .position(|n| {
                let n = normalize(n);
                !n.is_empty() && (n == want || n.starts_with(&want) || want.starts_with(&n))
            })
            .map(|i| i as u8)
    }
}

/// A machine name as the device may have stored it. macOS announces "MacBook-Pro-de-Ana.local"
/// on the network but pairs over Bluetooth as "MacBook Pro de Ana"; Windows and Linux differ in
/// case and punctuation too. Lowercase, drop ".local", and treat any run of punctuation or
/// whitespace as one space.
fn normalize(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let base = lower.strip_suffix(".local").unwrap_or(&lower);
    let mut out = String::with_capacity(base.len());
    let mut space = false;
    for c in base.chars() {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn host_names_match_across_transports() {
        assert_eq!(normalize("MacBook-Pro-de-Ana.local"), "macbook pro de ana");
        assert_eq!(normalize("MacBook Pro de Ana"), "macbook pro de ana");
        assert_eq!(normalize("  DESKTOP-ABC123 "), "desktop abc123");
        assert_eq!(normalize("leandro-ferreira"), "leandro ferreira");
        assert_eq!(normalize("Ana’s MacBook Pro"), "ana s macbook pro");
        assert_eq!(normalize(""), "");
    }
}
