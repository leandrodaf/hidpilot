//! Tray icon (top bar, through the AppIndicator extension Ubuntu enables by default).
//!
//! Not wired in yet. To enable it:
//!   1. gui/Cargo.toml: `tauri = { version = "2", features = ["tray-icon"] }`
//!   2. gui/src/main.rs: `mod tray;` and, in the builder,
//!      `.setup(|app| { tray::install(app.handle())?; Ok(()) })`
//!   3. To keep running in the tray when the window closes, add
//!      `.on_window_event(tray::hide_on_close)`.
//!   4. Call `tray::set_status(&app, "MX Master 3S 80% · Zone Vibe 60%")` whenever
//!      batteries are refreshed.
//!
//! On Linux an AppIndicator only shows a menu: left clicks never reach the app, so every
//! action lives in the menu. Needs libayatana-appindicator3-1 at runtime.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, Window, WindowEvent};

const TRAY_ID: &str = "hidpilot";
const STATUS_ID: &str = "status";

fn show_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    // Disabled item used as a read-only line with the battery summary.
    let status = MenuItem::with_id(app, STATUS_ID, "Procurando dispositivos…", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Abrir hidpilot", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &separator, &open, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("hidpilot")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        // Windows and macOS deliver clicks; open the window on a left click there.
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

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

/// Closing the window hides it instead, so the tray (and Flow status) keeps running.
pub fn hide_on_close<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let _ = window.hide();
        api.prevent_close();
    }
}
