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
use std::sync::Mutex;

const TRAY_ID: &str = "hidpilot";
const STATUS_ID: &str = "status";

/// The few strings the backend shows by itself, in the UI languages of gui/ui/i18n.js:
/// searching, open, quit, no devices, could not turn a service on ({unit}).
const TEXTS: &[(&str, [&str; 5])] = &[
    ("pt-BR", ["Procurando dispositivos…", "Abrir Orbit", "Sair", "Nenhum dispositivo", "não foi possível ativar {unit}"]),
    ("en", ["Looking for devices…", "Open Orbit", "Quit", "No devices", "could not turn on {unit}"]),
    ("es", ["Buscando dispositivos…", "Abrir Orbit", "Salir", "Ningún dispositivo", "no se pudo activar {unit}"]),
    ("fr", ["Recherche des appareils…", "Ouvrir Orbit", "Quitter", "Aucun appareil", "impossible d'activer {unit}"]),
    ("de", ["Suche nach Geräten…", "Orbit öffnen", "Beenden", "Keine Geräte", "{unit} konnte nicht aktiviert werden"]),
    ("it", ["Ricerca dispositivi…", "Apri Orbit", "Esci", "Nessun dispositivo", "impossibile attivare {unit}"]),
    ("nl", ["Apparaten zoeken…", "Orbit openen", "Afsluiten", "Geen apparaten", "kan {unit} niet inschakelen"]),
    ("pl", ["Wyszukiwanie urządzeń…", "Otwórz Orbit", "Zakończ", "Brak urządzeń", "nie można włączyć {unit}"]),
    ("cs", ["Hledání zařízení…", "Otevřít Orbit", "Ukončit", "Žádná zařízení", "{unit} nelze zapnout"]),
    ("sv", ["Söker efter enheter…", "Öppna Orbit", "Avsluta", "Inga enheter", "kunde inte aktivera {unit}"]),
    ("ru", ["Поиск устройств…", "Открыть Orbit", "Выход", "Нет устройств", "не удалось включить {unit}"]),
    ("uk", ["Пошук пристроїв…", "Відкрити Orbit", "Вийти", "Немає пристроїв", "не вдалося увімкнути {unit}"]),
    ("tr", ["Aygıtlar aranıyor…", "Orbit'i aç", "Çık", "Aygıt yok", "{unit} etkinleştirilemedi"]),
    ("id", ["Mencari perangkat…", "Buka Orbit", "Keluar", "Tidak ada perangkat", "tidak dapat mengaktifkan {unit}"]),
    ("vi", ["Đang tìm thiết bị…", "Mở Orbit", "Thoát", "Không có thiết bị", "không thể bật {unit}"]),
    ("hi", ["डिवाइस खोजे जा रहे हैं…", "Orbit खोलें", "बाहर निकलें", "कोई डिवाइस नहीं", "{unit} चालू नहीं हो सका"]),
    ("ja", ["デバイスを検索中…", "Orbit を開く", "終了", "デバイスなし", "{unit} を有効にできませんでした"]),
    ("ko", ["장치 검색 중…", "Orbit 열기", "종료", "장치 없음", "{unit}을(를) 켤 수 없습니다"]),
    ("zh-CN", ["正在查找设备…", "打开 Orbit", "退出", "无设备", "无法启用 {unit}"]),
    ("zh-TW", ["正在尋找裝置…", "開啟 Orbit", "結束", "沒有裝置", "無法啟用 {unit}"]),
];

#[derive(Clone, Copy)]
pub enum Text {
    Searching,
    Open,
    Quit,
    NoDevices,
    EnableFailed,
}

/// Index into TEXTS of the language in use; the page sets it, see `set_language`.
static LANG: Mutex<usize> = Mutex::new(0);
/// Battery summary for the status line: None until the first scan, "" for no devices.
static SUMMARY: Mutex<Option<String>> = Mutex::new(None);

pub fn tr(t: Text) -> &'static str {
    TEXTS[*LANG.lock().unwrap()].1[t as usize]
}

/// Same rules as matchLanguage in i18n.js: exact code, then base language, pt-* -> pt-BR,
/// zh-TW/HK/Hant -> zh-TW. Accepts "de_DE.UTF-8" as well as "de-DE".
pub fn match_language(tag: &str) -> Option<usize> {
    let tag = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    if let Some(i) = TEXTS.iter().position(|(c, _)| c.eq_ignore_ascii_case(&tag)) {
        return Some(i);
    }
    let lower = tag.to_ascii_lowercase();
    let base = match lower.split('-').next().unwrap_or("") {
        "pt" => "pt-BR",
        "zh" if ["-tw", "-hk", "-mo", "-hant"].iter().any(|s| lower.contains(s)) => "zh-TW",
        "zh" => "zh-CN",
        b => b,
    };
    TEXTS.iter().position(|(c, _)| *c == base)
}

/// Language before the page reports one: the saved choice, else the session locale.
fn initial_language(pref: &str) -> usize {
    let env = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).find(|v| !v.is_empty());
    match_language(pref)
        .or_else(|| env.as_deref().and_then(match_language))
        .unwrap_or(1)
}

/// The page tells which language it shows; the menu follows it.
pub fn set_language<R: Runtime>(app: &AppHandle<R>, tag: &str) {
    let Some(i) = match_language(tag) else { return };
    *LANG.lock().unwrap() = i;
    if let Some(items) = app.try_state::<MenuItems<R>>() {
        let _ = items.open.set_text(tr(Text::Open));
        let _ = items.quit.set_text(tr(Text::Quit));
    }
    let summary = SUMMARY.lock().unwrap().clone();
    if let Some(s) = summary {
        set_status(app, &s);
    } else if let Some(items) = app.try_state::<MenuItems<R>>() {
        let _ = items.status.set_text(tr(Text::Searching));
    }
}

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

/// `language` is the saved setting ("auto" or a code from TEXTS).
pub fn install<R: Runtime>(app: &AppHandle<R>, language: &str) -> tauri::Result<()> {
    *LANG.lock().unwrap() = initial_language(language);
    // Disabled item used as a read-only line with the battery summary.
    let status = MenuItem::with_id(app, STATUS_ID, tr(Text::Searching), false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", tr(Text::Open), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr(Text::Quit), true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    // On GNOME a click on the icon only opens this menu, so "open" comes first.
    let menu = Menu::with_items(app, &[&open, &separator, &status, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Orbit")
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

    // Keep handles to the items so set_status and set_language can rewrite them.
    app.manage(MenuItems { status, open, quit });
    Ok(())
}

struct MenuItems<R: Runtime> {
    status: MenuItem<R>,
    open: MenuItem<R>,
    quit: MenuItem<R>,
}

/// Rewrites the status line and the tooltip with the battery summary ("" = no devices).
pub fn set_status<R: Runtime>(app: &AppHandle<R>, summary: &str) {
    *SUMMARY.lock().unwrap() = Some(summary.to_string());
    let text = if summary.is_empty() { tr(Text::NoDevices) } else { summary };
    if let Some(items) = app.try_state::<MenuItems<R>>() {
        let _ = items.status.set_text(text);
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
