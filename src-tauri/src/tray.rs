use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

use crate::state::AppState;

pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let new_i = MenuItem::with_id(app, "new", "New note", true, None::<&str>)?;
    let show_i = MenuItem::with_id(app, "show_all", "Show all", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&new_i, &show_i, &quit_i])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(true) // Linux: rely on the menu, not click events
        .on_menu_event(|app, event| match event.id.as_ref() {
            "new" => {
                let _ = crate::commands::create_task(app.clone());
            }
            "show_all" => {
                // Re-open any task whose window was destroyed, and focus all.
                let _ = show_all(app);
            }
            "quit" => {
                app.exit(0); // bypasses prevent_exit; this is the real quit
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn show_all<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let tasks: Vec<crate::task::Task> = {
        let state = app.state::<AppState>();
        let guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.values().cloned().collect()
    };
    for t in &tasks {
        crate::window::open_task_window(app, t)?;
    }
    Ok(())
}
