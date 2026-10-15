//! LAN discovery over mDNS. Announces only a key fingerprint and a display name;
//! being found grants nothing, the handshake still requires a pinned key.

use crate::config::{Config, PORT};
use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::SocketAddr;
use tokio::sync::mpsc;

const SERVICE: &str = "_hidpilot-flow._tcp.local.";

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

pub fn start(cfg: &Config) -> Result<(Discovery, mpsc::UnboundedReceiver<Found>)> {
    let daemon = ServiceDaemon::new()?;
    let host = format!("{}.local.", cfg.my_id());
    let props = [("name", cfg.name.as_str()), ("key", cfg.public_key.as_str())];
    let info = ServiceInfo::new(SERVICE, &cfg.my_id(), &host, "", PORT, &props[..])?.enable_addr_auto();
    daemon.register(info)?;

    let events = daemon.browse(SERVICE)?;
    let me = cfg.public_key.clone();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(ev) = events.recv_async().await {
            let ServiceEvent::ServiceResolved(s) = ev else { continue };
            let Some(key) = s.get_property_val_str("key").map(str::to_string) else { continue };
            if key == me {
                continue;
            }
            let name = s.get_property_val_str("name").unwrap_or("?").to_string();
            let mut addrs: Vec<SocketAddr> =
                s.get_addresses_v4().into_iter().map(|ip| SocketAddr::new(ip.into(), s.get_port())).collect();
            addrs.sort();
            if tx.send(Found { name, key, addrs }).is_err() {
                break;
            }
        }
    });
    Ok((Discovery { _daemon: daemon }, rx))
}
