use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Runtime,
};

pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let new_i = MenuItem::with_id(app, "new", "New task", true, None::<&str>)?;
    let show_i = MenuItem::with_id(app, "show_all", "Show window", true, None::<&str>)?;
    let prefs_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&new_i, &show_i, &prefs_i, &quit_i])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(true) // Linux: rely on the menu, not click events
        .on_menu_event(|app, event| match event.id.as_ref() {
            "new" => {
                // Create a task and surface the window so the list shows it.
                let _ = crate::commands::create_task(app.clone());
                let _ = crate::window::open_main_window(app);
            }
            "show_all" => {
                // Show/focus the single main window (re-open if it was destroyed).
                let _ = crate::window::open_main_window(app);
            }
            "settings" => {
                let _ = crate::window::open_settings(app);
            }
            "quit" => {
                app.exit(0); // bypasses prevent_exit; this is the real quit
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
