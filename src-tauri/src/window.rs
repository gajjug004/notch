use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// Label of the single main window (list + detail SPA).
pub const MAIN_LABEL: &str = "main";

/// Open (or focus) the single main window. Idempotent: focuses the existing
/// window instead of creating a duplicate. Keeps the sticky look — frameless,
/// transparent, always-on-top.
pub fn open_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(MAIN_LABEL) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        return Ok(());
    }

    let win = WebviewWindowBuilder::new(app, MAIN_LABEL, WebviewUrl::App("index.html".into()))
        .title("Sticky Timer")
        .inner_size(360.0, 560.0)
        .min_inner_size(280.0, 360.0)
        .decorations(false)
        .always_on_top(true)
        .transparent(true)
        .resizable(true)
        .skip_taskbar(true)
        .build()
        .map_err(|e| e.to_string())?;

    let _ = win.show();
    Ok(())
}

/// Open (or focus) the single Settings window. Unlike the main window it uses
/// normal window chrome and is not always-on-top.
pub fn open_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Sticky Timer — Settings")
        .inner_size(360.0, 440.0)
        .resizable(false)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
