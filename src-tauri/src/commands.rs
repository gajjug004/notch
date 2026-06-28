use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::schedule::Schedule;
use crate::state::{persist, AppState};
use crate::task::Task;
use crate::timer::{RunAnchor, TimerMode, TimerState};
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
    guard
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("no task {id}"))
}

/// Save edits (text/color/geometry) from a window. Whole-task upsert.
/// Preserves the existing typed timer (the frontend never sends timer state).
#[tauri::command]
pub fn save_task<R: Runtime>(app: AppHandle<R>, task: Task) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let mut task = task;
        if let Some(existing) = guard.get(&task.id) {
            // The note window owns text/color/geometry; Rust owns the timer.
            task.timer = existing.timer.clone();
        }
        guard.insert(task.id.clone(), task);
    }
    persist(&app)?;
    Ok(())
}

// ---- Phase 3: timer commands ----------------------------------------------

/// Recompute is not needed here — just read the folded numbers and emit one
/// immediate tick so the UI updates without waiting up to a second for the loop.
fn emit_now<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let snapshot = {
        let state = app.state::<AppState>();
        let guard = match state.tasks.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.get(id).map(|t| {
            (
                t.id.clone(),
                t.timer.remaining_secs,
                t.timer.elapsed_secs,
                t.timer.state,
            )
        })
    }; // lock dropped
    if let Some((id, remaining_secs, elapsed_secs, state)) = snapshot {
        let _ = app.emit_to(
            id.as_str(),
            "timer-tick",
            &serde_json::json!({
                "id": id,
                "remaining_secs": remaining_secs,
                "elapsed_secs": elapsed_secs,
                "state": state,
            }),
        );
    }
}

/// Start OR resume. Arms the anchor with already-accumulated seconds as base.
#[tauri::command]
pub fn start_timer<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;

        match t.state {
            TimerState::Running => return Ok(()), // idempotent
            TimerState::Done => return Ok(()),    // must reset first
            _ => {}
        }

        let base = match t.mode {
            TimerMode::Countdown => t.duration_secs.saturating_sub(t.remaining_secs),
            TimerMode::Stopwatch => t.elapsed_secs,
        };
        t.anchor = Some(RunAnchor {
            started_at: Instant::now(),
            base_secs: base,
        });
        t.state = TimerState::Running;
    }
    persist(&app)?;
    emit_now(&app, &id);
    Ok(())
}

/// Pause: fold the in-flight segment into the stored numbers, drop the anchor.
#[tauri::command]
pub fn pause_timer<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;

        if t.state != TimerState::Running {
            return Ok(());
        }
        let now = Instant::now();

        if let Some(anchor) = t.anchor.take() {
            let run = now.saturating_duration_since(anchor.started_at).as_secs();
            let spent = anchor.base_secs + run;
            match t.mode {
                TimerMode::Stopwatch => t.elapsed_secs = spent,
                TimerMode::Countdown => {
                    let spent = spent.min(t.duration_secs);
                    t.elapsed_secs = spent;
                    t.remaining_secs = t.duration_secs - spent;
                }
            }
        }
        t.state = TimerState::Paused;
    }
    persist(&app)?;
    emit_now(&app, &id);
    Ok(())
}

/// Reset: back to idle at the configured start. Drops the anchor.
#[tauri::command]
pub fn reset_timer<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;
        t.anchor = None;
        t.state = TimerState::Idle;
        t.elapsed_secs = 0;
        t.remaining_secs = t.duration_secs; // meaningful for countdown
    }
    persist(&app)?;
    emit_now(&app, &id);
    Ok(())
}

/// Configure mode + duration. Resets the timer to a consistent idle state.
#[tauri::command]
pub fn configure_timer<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    mode: TimerMode,
    duration_secs: u64,
) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;
        t.anchor = None;
        t.mode = mode;
        // Keep a sensible duration for countdown; stopwatch ignores it.
        if duration_secs > 0 {
            t.duration_secs = duration_secs;
        }
        t.elapsed_secs = 0;
        t.remaining_secs = t.duration_secs;
        t.state = TimerState::Idle;
    }
    persist(&app)?;
    emit_now(&app, &id);
    Ok(())
}

// ---- Phase 4: schedule command -------------------------------------------

/// Replace a task's schedule wholesale. Clears the duplicate-fire guard so the
/// (re)set time can fire.
#[tauri::command]
pub fn set_schedule<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    schedule: Schedule,
) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        let mut schedule = schedule;
        schedule.last_fired = None;
        task.schedule = schedule;
    }
    persist(&app)?;
    Ok(())
}
