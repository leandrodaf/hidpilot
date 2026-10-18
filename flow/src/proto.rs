//! Messages exchanged inside the encrypted link. Nothing here can move the remote
//! cursor freely or type: the only cursor action is "the pointer enters at this edge".

use crate::config::Side;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceChannel {
    /// HID++ unit id, the same on every host the device is paired with.
    pub unit: String,
    pub keyboard: bool,
    /// Easy-Switch channel (0-based) this machine occupies on that device.
    pub channel: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    Hello {
        name: String,
        version: u32,
        devices: Vec<DeviceChannel>,
        /// Only while pairing: where the receiver's screen sits as seen from the sender.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        your_side: Option<Side>,
    },
    /// Sender is handing the mouse over: the pointer should appear on the receiver's
    /// `edge`, at `pos` (0.0-1.0) along it.
    Enter {
        edge: Side,
        pos: f64,
        /// The headset's sound comes along: take it as this computer's output.
        #[serde(default)]
        audio: bool,
    },
    Clipboard(Clip),
    Ping,
    Pong,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Clip {
    Text { text: String },
    /// RGBA8, base64.
    Image { width: usize, height: usize, rgba: String },
}

pub const VERSION: u32 = 1;

impl Msg {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serializable")
    }

    pub fn decode(b: &[u8]) -> anyhow::Result<Msg> {
        Ok(serde_json::from_slice(b)?)
    }
}
