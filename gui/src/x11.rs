//! Bringing the window back on X11 in a way GNOME accepts.
//!
//! GTK asks for activation with timestamp 0 when the request does not come from an input
//! event (a tray menu item, a second launch). Mutter takes that as focus stealing, logs
//! "Buggy client sent a _NET_ACTIVE_WINDOW message with a timestamp of 0" and leaves the
//! window minimized. Docks and panels instead send _NET_ACTIVE_WINDOW as a pager
//! (source 2) with a real server timestamp, which Mutter always honors, so we do the same.

use crate::logln;
use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, PropMode,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;

fn atom(conn: &RustConnection, name: &str) -> Result<u32> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

fn card32(conn: &RustConnection, win: Window, prop: u32, ty: u32) -> Vec<u32> {
    conn.get_property(false, win, prop, ty, 0, 64)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|v| v.collect()))
        .unwrap_or_default()
}

/// Top-level windows of this process, found through _NET_WM_PID.
fn own_windows(conn: &RustConnection, root: Window) -> Result<Vec<Window>> {
    let pid_atom = atom(conn, "_NET_WM_PID")?;
    let me = std::process::id();
    let mut out = Vec::new();
    let mut check = |w: Window| {
        if card32(conn, w, pid_atom, AtomEnum::CARDINAL.into()).first() == Some(&me) {
            out.push(w);
        }
    };
    for w in conn.query_tree(root)?.reply()?.children {
        check(w);
        // Windows with a server-side frame sit one level down.
        if let Ok(t) = conn.query_tree(w).map(|c| c.reply()) {
            if let Ok(t) = t {
                t.children.into_iter().for_each(&mut check);
            }
        }
    }
    Ok(out)
}

/// Our main window: the mapped-or-not top level that carries a title.
fn main_window(conn: &RustConnection, root: Window) -> Result<Window> {
    let name_atom = atom(conn, "_NET_WM_NAME")?;
    let utf8 = atom(conn, "UTF8_STRING")?;
    own_windows(conn, root)?
        .into_iter()
        .find(|&w| {
            conn.get_property(false, w, name_atom, utf8, 0, 64)
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|r| r.value == b"hidpilot")
        })
        .context("main window not found on the X server")
}

/// "Normal"/"Iconic"/"Withdrawn" plus _NET_WM_STATE, for the log.
fn describe(conn: &RustConnection, w: Window) -> String {
    let wm_state = atom(conn, "WM_STATE").unwrap_or(0);
    let net_state = atom(conn, "_NET_WM_STATE").unwrap_or(0);
    let st = match card32(conn, w, wm_state, wm_state).first() {
        Some(0) => "Withdrawn",
        Some(1) => "Normal",
        Some(3) => "Iconic",
        Some(_) => "?",
        None => "none",
    };
    let mapped = conn
        .get_window_attributes(w)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|a| format!("{:?}", a.map_state))
        .unwrap_or_default();
    let net: Vec<String> = card32(conn, w, net_state, AtomEnum::ATOM.into())
        .into_iter()
        .filter_map(|a| conn.get_atom_name(a).ok()?.reply().ok().map(|r| String::from_utf8_lossy(&r.name).into_owned()))
        .collect();
    format!("0x{w:x} WM_STATE={st} map={mapped} _NET_WM_STATE={net:?}")
}

/// Current X server time, read from the PropertyNotify of a throwaway window.
fn server_time(conn: &RustConnection, root: Window) -> Result<u32> {
    let w = conn.generate_id()?;
    conn.create_window(0, w, root, -1, -1, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new())?;
    conn.change_window_attributes(w, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE))?;
    let prop = atom(conn, "_HIDPILOT_TIMESTAMP")?;
    conn.change_property(PropMode::APPEND, w, prop, AtomEnum::STRING, 8, 0, &[])?;
    conn.flush()?;
    let time = loop {
        if let Event::PropertyNotify(e) = conn.wait_for_event()? {
            if e.window == w {
                break e.time;
            }
        }
    };
    conn.destroy_window(w)?;
    conn.flush()?;
    Ok(time)
}

pub fn log_state(context: &str) {
    match x11rb::connect(None) {
        Ok((conn, screen)) => {
            let root = conn.setup().roots[screen].root;
            match main_window(&conn, root) {
                Ok(w) => logln!("{context}: {}", describe(&conn, w)),
                Err(e) => logln!("{context}: {e:#}"),
            }
        }
        Err(e) => logln!("{context}: no X connection ({e})"),
    }
}

/// Asks the window manager, as a pager would, to restore and focus our window.
pub fn activate() -> Result<()> {
    let (conn, screen) = x11rb::connect(None).context("no X connection")?;
    let root = conn.setup().roots[screen].root;
    let w = main_window(&conn, root)?;
    logln!("activate: before {}", describe(&conn, w));
    let time = server_time(&conn, root)?;
    let active = atom(&conn, "_NET_ACTIVE_WINDOW")?;
    // data: source indication 2 = pager, timestamp, requester's active window (none)
    let ev = ClientMessageEvent::new(32, w, active, [2, time, 0, 0, 0]);
    conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev)?;
    conn.flush()?;
    logln!("activate: sent _NET_ACTIVE_WINDOW source=pager time={time}");
    std::thread::sleep(std::time::Duration::from_millis(250));
    logln!("activate: after {}", describe(&conn, w));
    Ok(())
}
