use tauri::{AppHandle, Manager, Runtime};

use crate::state::{persist, AppState};
use crate::task::Task;
use crate::window::open_task_window;

/// Create a new task, store it, and spawn its window.
#[tauri::command]
pub fn create_task<R: Runtime>(app: AppHandle<R>) -> Result<Task, String> {
    let mut task = Task::new();

    // Cascade new windows so they don't stack exactly on top of each other.
    {
        let state = app.state::<AppState>();
        let guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let n = guard.len() as i32;
        task.window.x = 120 + (n % 8) * 28;
        task.window.y = 120 + (n % 8) * 28;
    } // drop guard before further work

    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.insert(task.id.clone(), task.clone());
    }

    persist(&app)?; // no lock held
    open_task_window(&app, &task)?;
    Ok(task)
}

/// Delete a task: destroy its window, drop from map, persist.
#[tauri::command]
pub fn delete_task<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(&id) {
        let _ = win.destroy();
    }

    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.remove(&id);
    }

    persist(&app)?;
    Ok(())
}

/// All tasks (used by tray "Show all" and any future list UI).
#[tauri::command]
pub fn list_tasks<R: Runtime>(app: AppHandle<R>) -> Result<Vec<Task>, String> {
    let state = app.state::<AppState>();
    let guard = state.tasks.lock().map_err(|e| e.to_string())?;
    Ok(guard.values().cloned().collect())
}

/// One task by id (called by each window on load).
#[tauri::command]
pub fn get_task<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Task, String> {
    let state = app.state::<AppState>();
    let guard = state.tasks.lock().map_err(|e| e.to_string())?;
    guard.get(&id).cloned().ok_or_else(|| format!("no task {id}"))
}

/// Save edits (text/color/geometry) from a window. Whole-task upsert.
#[tauri::command]
pub fn save_task<R: Runtime>(app: AppHandle<R>, task: Task) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.insert(task.id.clone(), task);
    }
    persist(&app)?;
    Ok(())
}
