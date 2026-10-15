//! Virtual HID devices through /dev/uhid, used to publish battery levels to the kernel.
//!
//! The kernel turns a "Battery Strength" usage into a power_supply, which UPower and
//! GNOME show like any Bluetooth mouse battery. The virtual device never sends input.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::thread;

const UHID_DESTROY: u32 = 1;
const UHID_GET_REPORT: u32 = 9;
const UHID_GET_REPORT_REPLY: u32 = 10;
const UHID_CREATE2: u32 = 11;
const UHID_INPUT2: u32 = 12;
const UHID_SET_REPORT: u32 = 13;
const UHID_SET_REPORT_REPLY: u32 = 14;
const EVENT_SIZE: usize = 4 + 4372; // u32 type + largest union member (uhid_create2_req)
const BUS_VIRTUAL: u16 = 0x06;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Mouse,
    Keyboard,
}

/// Report 1 = [buttons/modifiers..., battery 0-100, charging flag].
fn descriptor(kind: Kind) -> Vec<u8> {
    let mut d = match kind {
        Kind::Mouse => vec![
            0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x85, 0x01, // Generic Desktop / Mouse, report 1
            0x09, 0x01, 0xA1, 0x00, // Pointer
            0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02,
            0x95, 0x01, 0x75, 0x05, 0x81, 0x03, // padding
            0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x02, 0x81, 0x06,
            0xC0,
        ],
        Kind::Keyboard => vec![
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x85, 0x01, // Generic Desktop / Keyboard, report 1
            0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02,
        ],
    };
    d.extend_from_slice(&[
        0x05, 0x06, 0x09, 0x20, 0x15, 0x00, 0x25, 0x64, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, // Battery Strength
        0x05, 0x85, 0x09, 0x44, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x01, 0x81, 0x02, // Charging
        0x75, 0x07, 0x95, 0x01, 0x81, 0x03, // padding
        0xC0,
    ]);
    d
}

pub struct BatteryDevice {
    file: File,
    kind: Kind,
}

impl BatteryDevice {
    pub fn create(name: &str, uniq: &str, kind: Kind) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/uhid")
            .context("cannot open /dev/uhid (the daemon must run as root)")?;

        let rd = descriptor(kind);
        let mut ev = vec![0u8; EVENT_SIZE];
        ev[0..4].copy_from_slice(&UHID_CREATE2.to_ne_bytes());
        let req = &mut ev[4..];
        put_str(&mut req[0..128], name);
        put_str(&mut req[128..192], "hidpilot");
        put_str(&mut req[192..256], uniq);
        req[256..258].copy_from_slice(&(rd.len() as u16).to_ne_bytes());
        req[258..260].copy_from_slice(&BUS_VIRTUAL.to_ne_bytes());
        req[260..264].copy_from_slice(&(crate::hidpp::LOGITECH as u32).to_ne_bytes());
        req[264..268].copy_from_slice(&0u32.to_ne_bytes()); // product
        req[268..272].copy_from_slice(&1u32.to_ne_bytes()); // version
        req[272..276].copy_from_slice(&0u32.to_ne_bytes()); // country
        req[276..276 + rd.len()].copy_from_slice(&rd);
        file.write_all(&ev).context("UHID_CREATE2")?;

        // The kernel may ask for reports; answer them so it never waits on us.
        let mut reader = file.try_clone()?;
        let mut replier = file.try_clone()?;
        thread::spawn(move || {
            let mut buf = vec![0u8; EVENT_SIZE];
            while let Ok(n) = reader.read(&mut buf) {
                if n < 8 {
                    continue;
                }
                let ty = u32::from_ne_bytes(buf[0..4].try_into().unwrap());
                let reply_ty = match ty {
                    UHID_GET_REPORT => UHID_GET_REPORT_REPLY,
                    UHID_SET_REPORT => UHID_SET_REPORT_REPLY,
                    _ => continue,
                };
                let id = buf[4..8].to_vec();
                let mut r = vec![0u8; EVENT_SIZE];
                r[0..4].copy_from_slice(&reply_ty.to_ne_bytes());
                r[4..8].copy_from_slice(&id);
                r[8..10].copy_from_slice(&(libc::EIO as u16).to_ne_bytes());
                if replier.write_all(&r).is_err() {
                    break;
                }
            }
        });
        Ok(Self { file, kind })
    }

    pub fn update(&mut self, percent: u8, charging: bool) -> Result<()> {
        let report: Vec<u8> = match self.kind {
            Kind::Mouse => vec![0x01, 0, 0, 0, percent.min(100), charging as u8],
            Kind::Keyboard => vec![0x01, 0, percent.min(100), charging as u8],
        };
        let mut ev = vec![0u8; EVENT_SIZE];
        ev[0..4].copy_from_slice(&UHID_INPUT2.to_ne_bytes());
        ev[4..6].copy_from_slice(&(report.len() as u16).to_ne_bytes());
        ev[6..6 + report.len()].copy_from_slice(&report);
        self.file.write_all(&ev).context("UHID_INPUT2")
    }
}

impl Drop for BatteryDevice {
    fn drop(&mut self) {
        let mut ev = vec![0u8; EVENT_SIZE];
        ev[0..4].copy_from_slice(&UHID_DESTROY.to_ne_bytes());
        let _ = self.file.write_all(&ev);
    }
}

fn put_str(dst: &mut [u8], s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&b[..n]);
}
