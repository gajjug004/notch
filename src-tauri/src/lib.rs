mod commands;
mod state;
mod task;
mod tray;
mod window;

use state::{load_into_state, AppState};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();

            // 1) Load persisted tasks into memory.
            load_into_state(&handle).map_err(|e| e.to_string())?;

            // 2) Restore: open a window per saved task. Snapshot under lock,
            //    drop the guard before spawning windows.
            let tasks: Vec<task::Task> = {
                let state = app.state::<AppState>();
                let guard = state.tasks.lock().map_err(|e| e.to_string())?;
                guard.values().cloned().collect()
            };

            if tasks.is_empty() {
                // First run: give the user one note instead of an empty desktop.
                commands::create_task(handle.clone()).map_err(|e| e.to_string())?;
            } else {
                for t in &tasks {
                    window::open_task_window(&handle, t).map_err(|e| e.to_string())?;
                }
            }

            // 3) System tray.
            tray::build_tray(&handle)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_task,
            commands::delete_task,
            commands::list_tasks,
            commands::get_task,
            commands::save_task,
        ])
        // Don't quit when the last note window closes; only the tray Quit exits.
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
