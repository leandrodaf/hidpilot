//! Reads and places the local pointer.

use anyhow::Result;

#[cfg(target_os = "linux")]
mod imp {
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

        /// Size of the whole desktop (all monitors).
        pub fn bounds(&self) -> Result<(i32, i32)> {
            let g = self.conn.get_geometry(self.root)?.reply()?;
            Ok((g.width as i32, g.height as i32))
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

pub use imp::Pointer;

use crate::config::Side;

impl Pointer {
    /// Which edge the pointer is pressed against, and where along it (0.0-1.0).
    pub fn at_edge(&self) -> Result<Option<(Side, f64)>> {
        let (w, h) = self.bounds()?;
        let (x, y) = self.position()?;
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
        let (w, h) = self.bounds()?;
        let pos = pos.clamp(0.0, 1.0);
        let (x, y) = match edge {
            Side::Left => (2, (pos * (h - 1) as f64) as i32),
            Side::Right => (w - 3, (pos * (h - 1) as f64) as i32),
            Side::Top => ((pos * (w - 1) as f64) as i32, 2),
            Side::Bottom => ((pos * (w - 1) as f64) as i32, h - 3),
        };
        self.warp(x, y)
    }
}
