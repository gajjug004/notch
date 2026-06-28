mod commands;
mod schedule;
mod settings;
mod state;
mod task;
mod tick;
mod timer;
mod tray;
mod window;

use std::sync::atomic::Ordering;

use state::{load_into_state, AppState};
use tauri::Manager;
use timer::TimerState;

#[cfg(desktop)]
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();

            // Autostart-on-login plugin (writes ~/.config/autostart/<id>.desktop).
            #[cfg(desktop)]
            handle.plugin(tauri_plugin_autostart::init(
                MacosLauncher::LaunchAgent,
                Some(vec!["--autostarted"]),
            ))?;

            // 1) Load persisted tasks into memory.
            load_into_state(&handle).map_err(|e| e.to_string())?;

            // Restore global pause from settings (quit-while-paused stays paused).
            if settings::get_bool(&handle, "globalPause", false) {
                app.state::<AppState>().paused.store(true, Ordering::Relaxed);
            }

            // 2) Boot timers PAUSED: a monotonic anchor can't survive a restart,
            //    and silently fast-forwarding while the app was closed is wrong.
            //    Keep the folded numbers; the user resumes with one click.
            {
                let state = app.state::<AppState>();
                let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
                for task in guard.values_mut() {
                    if task.timer.state == TimerState::Running {
                        task.timer.state = TimerState::Paused;
                        task.timer.anchor = None;
                    }
                }
            }

            // 3) Restore: open a window per saved task. Snapshot under lock,
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

            // 4) System tray.
            tray::build_tray(&handle)?;

            // 5) Reconcile schedules missed while the app was closed.
            tick::reconcile_on_boot(&handle);

            // 6) Single shared heartbeat for all timers + schedules.
            tick::spawn_tick_loop(handle.clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_task,
            commands::delete_task,
            commands::list_tasks,
            commands::get_task,
            commands::save_task,
            commands::start_timer,
            commands::pause_timer,
            commands::reset_timer,
            commands::configure_timer,
            commands::set_schedule,
            commands::set_task_color,
            commands::open_settings,
            commands::pause_all,
            commands::resume_all,
        ])
        // Minimize-to-tray: closing a note hides it; settings really closes.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "settings" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        // Don't quit when the last note window closes; only the tray Quit exits.
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
