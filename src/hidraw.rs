//! Minimal Linux hidraw access: enumeration through sysfs and raw report I/O.

use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
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
    pub report_descriptor: Vec<u8>,
}

impl HidrawInfo {
    /// True if the report descriptor declares the given report ID.
    pub fn has_report_id(&self, id: u8) -> bool {
        self.report_descriptor.windows(2).any(|w| w == [0x85, id])
    }
}

pub fn enumerate() -> Result<Vec<HidrawInfo>> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/class/hidraw") {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let sys = entry.path();
        let Ok(uevent) = fs::read_to_string(sys.join("device/uevent")) else { continue };
        let mut id = None;
        let mut name = String::new();
        for line in uevent.lines() {
            if let Some(v) = line.strip_prefix("HID_ID=") {
                id = parse_hid_id(v);
            } else if let Some(v) = line.strip_prefix("HID_NAME=") {
                name = v.to_string();
            }
        }
        let Some((bus, vendor, product)) = id else { continue };
        let report_descriptor = fs::read(sys.join("device/report_descriptor")).unwrap_or_default();
        out.push(HidrawInfo {
            path: PathBuf::from("/dev").join(entry.file_name()),
            bus,
            vendor,
            product,
            name,
            report_descriptor,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

// HID_ID=0003:0000046D:0000C548
fn parse_hid_id(v: &str) -> Option<(u16, u16, u16)> {
    let mut it = v.split(':').map(|p| u32::from_str_radix(p, 16).ok());
    Some((it.next()?? as u16, it.next()?? as u16, it.next()?? as u16))
}

pub struct Hidraw {
    file: File,
    pub path: PathBuf,
}

impl Hidraw {
    pub fn open(path: &PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .with_context(|| {
                format!("cannot open {} (is the udev rule installed? see README)", path.display())
            })?;
        Ok(Self { file, path: path.clone() })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.file
            .write_all(data)
            .with_context(|| format!("write to {}", self.path.display()))
    }

    /// Reads one input report, waiting at most until `deadline`. Returns None on timeout.
    pub fn read_until(&mut self, deadline: Instant) -> Result<Option<Vec<u8>>> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let ms = left.as_millis().min(i32::MAX as u128) as i32;
            let r = unsafe { libc::poll(&mut pfd, 1, ms.max(1)) };
            if r < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err.into());
            }
            if r == 0 {
                continue;
            }
            if pfd.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                bail!("{} was disconnected", self.path.display());
            }
            let mut buf = [0u8; 256];
            match self.file.read(&mut buf) {
                Ok(n) => return Ok(Some(buf[..n].to_vec())),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub fn read_timeout(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        self.read_until(Instant::now() + timeout)
    }

    /// HIDIOCGFEATURE: `buf[0]` must hold the report ID; returns the bytes the device filled in.
    pub fn get_feature(&mut self, buf: &mut [u8]) -> Result<usize> {
        let req = ioc(3, 0x07, buf.len());
        let r = unsafe { libc::ioctl(self.file.as_raw_fd(), req, buf.as_mut_ptr()) };
        if r < 0 {
            return Err(std::io::Error::last_os_error()).context("HIDIOCGFEATURE");
        }
        Ok(r as usize)
    }

    /// HIDIOCSFEATURE: `data[0]` is the report ID.
    pub fn set_feature(&mut self, data: &[u8]) -> Result<()> {
        let req = ioc(3, 0x06, data.len());
        let r = unsafe { libc::ioctl(self.file.as_raw_fd(), req, data.as_ptr()) };
        if r < 0 {
            return Err(std::io::Error::last_os_error()).context("HIDIOCSFEATURE");
        }
        Ok(())
    }
}

// _IOC(dir, 'H', nr, size) as used by <linux/hidraw.h>
fn ioc(dir: u64, nr: u64, size: usize) -> libc::c_ulong {
    ((dir << 30) | ((size as u64) << 16) | ((b'H' as u64) << 8) | nr) as libc::c_ulong
}
