//! Plain log of what the app does with its window and tray, for diagnosing desktop
//! issues: ~/.local/state/hidpilot/gui.log (also printed to stderr).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_default();
    base.join("hidpilot/gui.log")
}

pub fn init() {
    let p = path();
    let _ = fs::create_dir_all(p.parent().unwrap_or(&p));
    // Keep the log small: start over once it passes 1 MB.
    if fs::metadata(&p).map(|m| m.len() > 1 << 20).unwrap_or(false) {
        let _ = fs::remove_file(&p);
    }
    let f = OpenOptions::new().create(true).append(true).open(&p).ok();
    let _ = FILE.set(Mutex::new(f));
}

pub fn write(msg: &str) {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let line = format!("{}.{:03} [{}] {msg}\n", t.as_secs(), t.subsec_millis(), std::process::id());
    eprint!("{line}");
    if let Some(m) = FILE.get() {
        if let Some(f) = m.lock().unwrap().as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

#[macro_export]
macro_rules! logln {
    ($($arg:tt)*) => { $crate::logfile::write(&format!($($arg)*)) };
}
