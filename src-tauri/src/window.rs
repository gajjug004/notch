use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::task::Task;

/// Open (or focus) the window for a task, positioned at its saved geometry.
/// Idempotent: if a window with this id-label already exists, focus it instead
/// of creating a duplicate.
pub fn open_task_window<R: Runtime>(app: &AppHandle<R>, task: &Task) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(&task.id) {
        let _ = win.set_focus();
        return Ok(());
    }

    // index.html is the per-note view; the id is passed via query string.
    let url = WebviewUrl::App(format!("index.html?id={}", task.id).into());

    let win = WebviewWindowBuilder::new(app, &task.id /* label == id */, url)
        .title("note")
        .inner_size(task.window.w as f64, task.window.h as f64)
        .position(task.window.x as f64, task.window.y as f64)
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

/// Open (or focus) the single Settings window. Unlike notes it uses normal
/// window chrome and is not always-on-top.
pub fn open_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Sticky Timer — Settings")
        .inner_size(340.0, 300.0)
        .resizable(false)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
