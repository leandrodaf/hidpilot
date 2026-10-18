//! Persistent state: this machine's identity key and the peers it trusts.
//!
//! Lives in ~/.config/hidpilot/flow.toml, readable only by the user (0600), because
//! the private key in it is what proves this machine's identity to its peers.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub const PORT: u16 = 47_811;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

impl std::str::FromStr for Side {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "left" | "esquerda" => Side::Left,
            "right" | "direita" => Side::Right,
            "top" | "cima" => Side::Top,
            "bottom" | "baixo" => Side::Bottom,
            _ => anyhow::bail!("side must be left, right, top or bottom"),
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Peer {
    pub name: String,
    /// Noise static public key, hex. Pinned at pairing time; nothing else is trusted.
    pub key: String,
    /// Where that screen sits relative to this one. None = paired but not placed.
    pub side: Option<Side>,
    /// Last address it was reached at, used when mDNS is blocked on the network.
    pub addr: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub name: String,
    /// Noise static private key, hex.
    pub private_key: String,
    pub public_key: String,
    #[serde(default = "yes")]
    pub clipboard: bool,
    #[serde(default = "yes")]
    pub keyboard_follows: bool,
    /// Move the headset's sound along with the mouse, and keep the output following the
    /// headset as it comes and goes (see audio.rs).
    #[serde(default = "yes")]
    pub audio_follows: bool,
    /// Device unit id -> Easy-Switch channel (0-based) this machine sits on.
    #[serde(default)]
    pub channels: BTreeMap<String, u8>,
    #[serde(default)]
    pub peers: BTreeMap<String, Peer>,
}

fn yes() -> bool {
    true
}

pub fn path() -> Result<PathBuf> {
    Ok(dirs::config_dir().context("no config directory")?.join("hidpilot").join("flow.toml"))
}

impl Config {
    pub fn load_or_create() -> Result<Config> {
        let p = path()?;
        if let Ok(text) = fs::read_to_string(&p) {
            return toml::from_str(&text).with_context(|| format!("parse {}", p.display()));
        }
        let kp = crate::link::generate_keypair()?;
        let c = Config {
            name: hostname::get().map(|h| h.to_string_lossy().into_owned()).unwrap_or_else(|_| "linux".into()),
            private_key: hex(&kp.private),
            public_key: hex(&kp.public),
            clipboard: true,
            keyboard_follows: true,
            audio_follows: true,
            channels: BTreeMap::new(),
            peers: BTreeMap::new(),
        };
        c.save()?;
        Ok(c)
    }

    pub fn save(&self) -> Result<()> {
        let p = path()?;
        fs::create_dir_all(p.parent().unwrap())?;
        let tmp = p.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&tmp, &p)?;
        Ok(())
    }

    /// Short id of a peer: the first 8 bytes of its key, as shown to the user.
    pub fn id_of(key_hex: &str) -> String {
        key_hex.chars().take(16).collect()
    }

    pub fn my_id(&self) -> String {
        Self::id_of(&self.public_key)
    }

    pub fn private(&self) -> Result<Vec<u8>> {
        unhex(&self.private_key)
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Result<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2).context("odd hex")?, 16).context("bad hex"))
        .collect()
}
