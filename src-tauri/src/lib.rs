mod commands;
mod schedule;
mod settings;
mod state;
mod task;
mod telegram;
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

            // 3) Open the single main window (list + detail SPA). An empty task
            //    list is fine on first run — the list view offers "+ New".
            window::open_main_window(&handle).map_err(|e| e.to_string())?;

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
            commands::telegram_test,
        ])
        // Minimize-to-tray: closing the main window hides it; settings really closes.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == window::MAIN_LABEL {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        // Don't quit when the main window hides; only the tray Quit exits.
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                // code == None  -> implicit exit (last window closed): keep alive in tray.
                // code == Some  -> explicit app.exit(n) from tray Quit: let it through.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
