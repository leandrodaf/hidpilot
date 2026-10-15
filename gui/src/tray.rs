//! Tray icon (top bar, through the AppIndicator extension Ubuntu enables by default).
//!
//! Closing or minimizing the window sends the app to the tray; the menu brings it back.
//!
//! On Linux an AppIndicator only shows a menu: left clicks never reach the app, so every
//! action lives in the menu. Needs libayatana-appindicator3-1 at runtime.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use crate::logln;
use tauri::{AppHandle, Manager, Runtime, Window, WindowEvent};

const TRAY_ID: &str = "hidpilot";
const STATUS_ID: &str = "status";

/// Brings the window back from the tray: shown, restored and focused, in that order.
pub fn show_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_webview_window("main") else {
        logln!("show_window: no main window");
        return;
    };
    logln!("show_window: visible={:?} minimized={:?}", w.is_visible(), w.is_minimized());
    let _ = w.set_skip_taskbar(false);
    let _ = w.show();
    let _ = w.unminimize();
    // GTK's own focus request carries timestamp 0, which GNOME refuses; ask as a pager.
    #[cfg(target_os = "linux")]
    if let Err(e) = crate::x11::activate() {
        logln!("show_window: activation failed: {e:#}");
        let _ = w.set_focus();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = w.set_focus();
}

pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    // Disabled item used as a read-only line with the battery summary.
    let status = MenuItem::with_id(app, STATUS_ID, "Procurando dispositivos…", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Abrir hidpilot", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    // On GNOME a click on the icon only opens this menu, so "open" comes first.
    let menu = Menu::with_items(app, &[&open, &separator, &status, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("hidpilot")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            logln!("tray: menu item {:?}", event.id.as_ref());
            match event.id.as_ref() {
                "open" => show_window(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        // Windows and macOS deliver clicks; open the window on a left click there.
        .on_tray_icon_event(|tray, event| {
            logln!("tray: icon event {event:?}");
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    logln!("tray: installed");

    // Keep a handle to the status line so set_status can rewrite it.
    app.manage(StatusLine(status));
    Ok(())
}

struct StatusLine<R: Runtime>(MenuItem<R>);

/// Rewrites the first menu line and the tooltip, e.g. with battery levels.
pub fn set_status<R: Runtime>(app: &AppHandle<R>, text: &str) {
    if let Some(line) = app.try_state::<StatusLine<R>>() {
        let _ = line.0.set_text(text);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Closing or minimizing the window hides it to the tray instead.
pub fn hide_on_close<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            logln!("window: close requested, hiding to the tray");
            let _ = window.hide();
            api.prevent_close();
        }
        // Minimizing shows up as a resize. The window stays minimized, as the window
        // manager left it, and only leaves the dock and Alt+Tab, so the tray icon is what
        // brings it back. Never hide it here: unmapping a window while the window
        // manager is minimizing it leaves Mutter with a window it will not show again.
        WindowEvent::Resized(_) if window.is_minimized().unwrap_or(false) => {
            if window.set_skip_taskbar(true).is_ok() {
                logln!("window: minimized, now only in the tray");
            }
        }
        WindowEvent::Focused(f) => logln!("window: focused={f}"),
        _ => {}
    }
}
