//! Reads and places the local pointer. Each platform provides the desktop bounds
//! (all monitors), the pointer position and a warp, in global coordinates.

use crate::config::Side;
use anyhow::Result;

/// Desktop rectangle: x, y, width, height.
type Rect = (i32, i32, i32, i32);

#[cfg(target_os = "linux")]
mod imp {
    use super::Rect;
    use anyhow::{Context, Result};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, Window};
    use x11rb::rust_connection::RustConnection;

    pub struct Pointer {
        conn: RustConnection,
        root: Window,
    }

    impl Pointer {
        pub fn open() -> Result<Pointer> {
            let (conn, screen) = x11rb::connect(None).context("cannot reach the X server (Flow needs an X11 session)")?;
            let root = conn.setup().roots[screen].root;
            Ok(Pointer { conn, root })
        }

        pub fn bounds(&self) -> Result<Rect> {
            let g = self.conn.get_geometry(self.root)?.reply()?;
            Ok((0, 0, g.width as i32, g.height as i32))
        }

        pub fn position(&self) -> Result<(i32, i32)> {
            let p = self.conn.query_pointer(self.root)?.reply()?;
            Ok((p.root_x as i32, p.root_y as i32))
        }

        pub fn warp(&self, x: i32, y: i32) -> Result<()> {
            self.conn.warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, x as i16, y as i16)?;
            self.conn.flush()?;
            Ok(())
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::Rect;
    use anyhow::{Result, bail};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        SetCursorPos,
    };

    pub struct Pointer;

    impl Pointer {
        pub fn open() -> Result<Pointer> {
            Ok(Pointer)
        }

        pub fn bounds(&self) -> Result<Rect> {
            unsafe {
                Ok((
                    GetSystemMetrics(SM_XVIRTUALSCREEN),
                    GetSystemMetrics(SM_YVIRTUALSCREEN),
                    GetSystemMetrics(SM_CXVIRTUALSCREEN),
                    GetSystemMetrics(SM_CYVIRTUALSCREEN),
                ))
            }
        }

        pub fn position(&self) -> Result<(i32, i32)> {
            let mut p = POINT { x: 0, y: 0 };
            if unsafe { GetCursorPos(&mut p) } == 0 {
                bail!("GetCursorPos failed");
            }
            Ok((p.x, p.y))
        }

        pub fn warp(&self, x: i32, y: i32) -> Result<()> {
            if unsafe { SetCursorPos(x, y) } == 0 {
                bail!("SetCursorPos failed");
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Rect;
    use anyhow::{Result, bail};
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
        fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayBounds(display: u32) -> CGRect;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    pub struct Pointer;

    impl Pointer {
        pub fn open() -> Result<Pointer> {
            Ok(Pointer)
        }

        pub fn bounds(&self) -> Result<Rect> {
            let mut ids = [0u32; 16];
            let mut n = 0u32;
            if unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) } != 0 || n == 0 {
                bail!("no displays");
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for &id in &ids[..n as usize] {
                let r = unsafe { CGDisplayBounds(id) };
                x0 = x0.min(r.origin.x);
                y0 = y0.min(r.origin.y);
                x1 = x1.max(r.origin.x + r.size.width);
                y1 = y1.max(r.origin.y + r.size.height);
            }
            Ok((x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32))
        }

        pub fn position(&self) -> Result<(i32, i32)> {
            unsafe {
                let ev = CGEventCreate(std::ptr::null());
                if ev.is_null() {
                    bail!("CGEventCreate failed");
                }
                let p = CGEventGetLocation(ev);
                CFRelease(ev);
                Ok((p.x as i32, p.y as i32))
            }
        }

        pub fn warp(&self, x: i32, y: i32) -> Result<()> {
            unsafe {
                CGWarpMouseCursorPosition(CGPoint { x: x as f64, y: y as f64 });
                // Without this macOS freezes the pointer for a quarter second after a warp.
                CGAssociateMouseAndMouseCursorPosition(1);
            }
            Ok(())
        }
    }
}

pub use imp::Pointer;

impl Pointer {
    /// Which edge the pointer is pressed against, and where along it (0.0-1.0).
    pub fn at_edge(&self) -> Result<Option<(Side, f64)>> {
        let (bx, by, w, h) = self.bounds()?;
        let (x, y) = self.position()?;
        let (x, y) = (x - bx, y - by);
        let along_y = y as f64 / (h - 1).max(1) as f64;
        let along_x = x as f64 / (w - 1).max(1) as f64;
        Ok(if x <= 0 {
            Some((Side::Left, along_y))
        } else if x >= w - 1 {
            Some((Side::Right, along_y))
        } else if y <= 0 {
            Some((Side::Top, along_x))
        } else if y >= h - 1 {
            Some((Side::Bottom, along_x))
        } else {
            None
        })
    }

    /// Puts the pointer just inside `edge`, at `pos` along it.
    pub fn enter(&self, edge: Side, pos: f64) -> Result<()> {
        let (bx, by, w, h) = self.bounds()?;
        let pos = pos.clamp(0.0, 1.0);
        let (x, y) = match edge {
            Side::Left => (2, (pos * (h - 1) as f64) as i32),
            Side::Right => (w - 3, (pos * (h - 1) as f64) as i32),
            Side::Top => ((pos * (w - 1) as f64) as i32, 2),
            Side::Bottom => ((pos * (w - 1) as f64) as i32, h - 3),
        };
        self.warp(bx + x, by + y)
    }
}
