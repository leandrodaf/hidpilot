//! LAN discovery over mDNS. Announces only a key fingerprint and a display name;
//! being found grants nothing, the handshake still requires a pinned key.

use crate::config::{Config, PORT};
use anyhow::Result;
use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::SocketAddr;
use tokio::sync::mpsc;

const SERVICE: &str = "_hidpilot-flow._tcp.local.";

#[derive(Debug, Clone)]
pub enum Event {
    Found(Found),
    /// Stopped announcing itself; carries the id (`Config::id_of`) it announced under.
    Gone(String),
}

#[derive(Debug, Clone)]
pub struct Found {
    pub name: String,
    /// Full public key hex as announced; only trusted if it matches a pinned peer.
    pub key: String,
    pub addrs: Vec<SocketAddr>,
}

pub struct Discovery {
    _daemon: ServiceDaemon,
}

pub fn start(cfg: &Config) -> Result<(Discovery, mpsc::UnboundedReceiver<Event>)> {
    let daemon = ServiceDaemon::new()?;
    // Containers, VMs and VPN tunnels are not the LAN; announcing them only makes peers
    // waste time dialing addresses they cannot reach.
    let virtual_ifs: Vec<IfKind> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.name)
        .filter(|n| is_virtual(n))
        .map(IfKind::Name)
        .collect();
    if !virtual_ifs.is_empty() {
        daemon.disable_interface(virtual_ifs)?;
    }
    let host = format!("{}.local.", cfg.my_id());
    let props = [("name", cfg.name.as_str()), ("key", cfg.public_key.as_str())];
    let info = ServiceInfo::new(SERVICE, &cfg.my_id(), &host, "", PORT, &props[..])?.enable_addr_auto();
    daemon.register(info)?;

    let events = daemon.browse(SERVICE)?;
    let me = cfg.public_key.clone();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(ev) = events.recv_async().await {
            let s = match ev {
                ServiceEvent::ServiceResolved(s) => s,
                ServiceEvent::ServiceRemoved(_, full) => {
                    let id = full.split('.').next().unwrap_or_default().to_string();
                    if tx.send(Event::Gone(id)).is_err() {
                        break;
                    }
                    continue;
                }
                _ => continue,
            };
            let Some(key) = s.get_property_val_str("key").map(str::to_string) else { continue };
            if key == me {
                continue;
            }
            let name = s.get_property_val_str("name").unwrap_or("?").to_string();
            let mut addrs: Vec<SocketAddr> =
                s.get_addresses_v4().into_iter().map(|ip| SocketAddr::new(ip.into(), s.get_port())).collect();
            addrs.sort();
            if tx.send(Event::Found(Found { name, key, addrs })).is_err() {
                break;
            }
        }
    });
    Ok((Discovery { _daemon: daemon }, rx))
}

fn is_virtual(name: &str) -> bool {
    const PREFIXES: [&str; 10] = ["docker", "br-", "veth", "virbr", "vmnet", "vboxnet", "tun", "tap", "wg", "utun"];
    let n = name.to_lowercase();
    PREFIXES.iter().any(|p| n.starts_with(p)) || n.contains("warp") || n.contains("tailscale") || n.contains("zerotier")
}
