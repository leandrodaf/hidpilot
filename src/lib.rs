//! Logitech peripherals: HID++ 2.0 mice and keyboards, Centurion headsets.
//! Linux talks to hidraw directly; Windows and macOS go through hidapi.

pub mod centurion;
pub mod features;
pub mod hidpp;

#[cfg(target_os = "linux")]
pub mod hidraw;
#[cfg(not(target_os = "linux"))]
#[path = "hidapi_io.rs"]
pub mod hidraw;

#[cfg(target_os = "linux")]
pub mod daemon;
#[cfg(target_os = "linux")]
pub mod notify;
pub mod remap;
#[cfg(target_os = "linux")]
pub mod uhid;
