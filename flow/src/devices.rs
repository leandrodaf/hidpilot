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
        let want = peer_name.to_lowercase();
        t.host_names
            .iter()
            .position(|n| {
                let n = n.to_lowercase();
                !n.is_empty() && (n == want || n.starts_with(&want) || want.starts_with(&n))
            })
            .map(|i| i as u8)
    }
}
