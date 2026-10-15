//! Windows and macOS stand-in for the hidraw module, on top of hidapi.
//!
//! Those systems expose each HID top-level collection as its own device and do not
//! hand out report descriptors, so the report IDs are inferred from the vendor usage
//! page of the collection.

use anyhow::{Context, Result, anyhow};
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const BUS_USB: u16 = 0x03;
pub const BUS_BLUETOOTH: u16 = 0x05;

#[derive(Debug, Clone)]
pub struct HidrawInfo {
    pub path: PathBuf,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub name: String,
    report_ids: Vec<u8>,
}

impl HidrawInfo {
    pub fn has_report_id(&self, id: u8) -> bool {
        self.report_ids.contains(&id)
    }
}

fn api() -> Result<std::sync::MutexGuard<'static, hidapi::HidApi>> {
    static API: OnceLock<Mutex<hidapi::HidApi>> = OnceLock::new();
    if API.get().is_none() {
        let a = hidapi::HidApi::new().map_err(|e| anyhow!("hidapi: {e}"))?;
        let _ = API.set(Mutex::new(a));
    }
    let mut g = API.get().unwrap().lock().unwrap();
    g.refresh_devices().map_err(|e| anyhow!("hidapi: {e}"))?;
    Ok(g)
}

/// Report IDs carried by a Logitech vendor collection.
fn ids_for(usage_page: u16, usage: u16) -> Vec<u8> {
    match (usage_page, usage) {
        (0xFF00, 0x0001) => vec![0x10],       // receiver, HID++ short
        (0xFF00, 0x0002) => vec![0x11],       // receiver, HID++ long
        (0xFF43, 0x0202) => vec![0x10, 0x11], // Bluetooth HID++
        (0xFF53, 0x0404) => vec![0x22],       // Zone receiver, Centurion
        _ => Vec::new(),
    }
}

pub fn enumerate() -> Result<Vec<HidrawInfo>> {
    let api = api()?;
    let mut out = Vec::new();
    for d in api.device_list() {
        let report_ids = ids_for(d.usage_page(), d.usage());
        if report_ids.is_empty() {
            continue;
        }
        let bus = match d.bus_type() {
            hidapi::BusType::Bluetooth => BUS_BLUETOOTH,
            _ => BUS_USB,
        };
        out.push(HidrawInfo {
            path: PathBuf::from(d.path().to_string_lossy().into_owned()),
            bus,
            vendor: d.vendor_id(),
            product: d.product_id(),
            name: d.product_string().unwrap_or_default().to_string(),
            report_ids,
        });
    }
    Ok(out)
}

pub struct Hidraw {
    dev: hidapi::HidDevice,
    pub path: PathBuf,
}

impl Hidraw {
    pub fn open(path: &PathBuf) -> Result<Self> {
        let c = CString::new(path.to_string_lossy().as_bytes()).context("bad device path")?;
        let dev = api()?.open_path(&c).map_err(|e| anyhow!("cannot open {}: {e}", path.display()))?;
        Ok(Self { dev, path: path.clone() })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.dev.write(data).map_err(|e| anyhow!("write to {}: {e}", self.path.display()))?;
        Ok(())
    }

    pub fn read_until(&mut self, deadline: Instant) -> Result<Option<Vec<u8>>> {
        let mut buf = [0u8; 256];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let ms = left.as_millis().clamp(1, i32::MAX as u128) as i32;
            let n = self.dev.read_timeout(&mut buf, ms).map_err(|e| anyhow!("{} was disconnected: {e}", self.path.display()))?;
            if n > 0 {
                return Ok(Some(buf[..n].to_vec()));
            }
        }
    }

    pub fn read_timeout(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        self.read_until(Instant::now() + timeout)
    }
}
