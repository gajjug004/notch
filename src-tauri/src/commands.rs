use std::sync::atomic::Ordering;
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::schedule::{next_fire_time, Schedule, ScheduleKind};
use crate::settings;
use crate::telegram;
use crate::state::{persist, AppState};
use crate::task::{Task, TaskStatus};
use crate::timer::{RunAnchor, Timer, TimerMode, TimerState};

/// Create a new task and store it. The single main window's list refreshes via
/// the `tasks-changed` event.
#[tauri::command]
pub fn create_task<R: Runtime>(app: AppHandle<R>) -> Result<Task, String> {
    let mut task = Task::new();

    // New tasks start with the user's configured default countdown length.
    let default_secs = settings::get_u64(&app, "defaultCountdownSecs", 25 * 60);
    task.timer.duration_secs = default_secs;
    task.timer.remaining_secs = default_secs;

    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.insert(task.id.clone(), task.clone());
    }

    persist(&app)?; // no lock held
    let _ = app.emit("tasks-changed", ());
    Ok(task)
}

/// Delete a task: drop from map, persist, refresh the list.
#[tauri::command]
pub fn delete_task<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.remove(&id);
    }

    persist(&app)?;
    let _ = app.emit("tasks-changed", ());
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
            // The detail view owns text/color/geometry; Rust owns the timer and
            // the schedule (edited via configure_timer / set_schedule). Don't let
            // a stale save_task payload clobber them.
            task.timer = existing.timer.clone();
            task.schedule = existing.schedule.clone();
            // Lifecycle is owned by complete_task/reopen_task, not text saves.
            task.status = existing.status;
            task.completed_at = existing.completed_at.clone();
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
        let _ = app.emit(
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

/// Fold a running timer's in-flight segment into the stored numbers, drop the
/// anchor, and mark it Paused. No-op unless currently Running.
fn fold_and_pause(t: &mut Timer) {
    if t.state != TimerState::Running {
        return;
    }
    if let Some(anchor) = t.anchor.take() {
        let run = Instant::now()
            .saturating_duration_since(anchor.started_at)
            .as_secs();
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

/// Pause: fold the in-flight segment into the stored numbers, drop the anchor.
#[tauri::command]
pub fn pause_timer<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        if task.timer.state != TimerState::Running {
            return Ok(());
        }
        fold_and_pause(&mut task.timer);
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
    let _ = app.emit("tasks-changed", ());
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
    let notice = {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        let mut schedule = schedule;
        schedule.last_fired = None;
        task.schedule = schedule;

        // Build a "task scheduled" Telegram notice for a future fire time.
        if task.schedule.kind != ScheduleKind::None {
            next_fire_time(&task.schedule, chrono::Local::now()).map(|at| {
                let when = at.format("%a %d %b, %H:%M").to_string();
                telegram::format_task_scheduled(&task.title, &task.content, &when)
            })
        } else {
            None
        }
    };
    persist(&app)?;
    if let Some(text) = notice {
        telegram::send(&app, text);
    }
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

/// Settings "Send test" — push a probe message, surfacing any API error.
#[tauri::command]
pub async fn telegram_test(token: String, chat_id: String) -> Result<(), String> {
    let text = telegram::format_test_message();
    telegram::post(token.trim(), chat_id.trim(), &text).await
}

// ---- Phase 5: polish commands --------------------------------------------

/// Persist a note's color (applied live in the frontend via a CSS var).
#[tauri::command]
pub fn set_task_color<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    color: String,
) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        task.color = color;
    }
    persist(&app)?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

/// Open (or focus) the Settings window.
#[tauri::command]
pub fn open_settings<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::window::open_settings(&app)
}

/// Global pause: freeze all running timers without touching their individual state.
#[tauri::command]
pub fn pause_all<R: Runtime>(app: AppHandle<R>) {
    app.state::<AppState>().paused.store(true, Ordering::Relaxed);
    settings::set_value(&app, "globalPause", true.into());
    let _ = app.emit("global-pause", true);
}

#[tauri::command]
pub fn resume_all<R: Runtime>(app: AppHandle<R>) {
    app.state::<AppState>()
        .paused
        .store(false, Ordering::Relaxed);
    settings::set_value(&app, "globalPause", false.into());
    let _ = app.emit("global-pause", false);
}

// ---- lifecycle: done / reopen / snooze / dismiss --------------------------

/// Mark a task Done. Folds a running timer so a finished task stops counting.
#[tauri::command]
pub fn complete_task<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        fold_and_pause(&mut task.timer); // no-op unless running
        task.status = TaskStatus::Done;
        task.completed_at = Some(chrono::Local::now().to_rfc3339());
    }
    persist(&app)?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

/// Reopen a Done task back to Active.
#[tauri::command]
pub fn reopen_task<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        task.status = TaskStatus::Active;
        task.completed_at = None;
    }
    persist(&app)?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

/// Snooze a fired schedule: a fresh one-shot for the same task, `minutes` out,
/// keeping the prior auto-start preference. Reuses set_schedule (persist +
/// "task scheduled" Telegram notice + tasks-changed).
#[tauri::command]
pub fn snooze_task<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    minutes: i64,
) -> Result<(), String> {
    let auto_start = {
        let state = app.state::<AppState>();
        let guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.get(&id).ok_or("no such task")?.schedule.auto_start
    };
    let at = (chrono::Local::now() + chrono::Duration::minutes(minutes.max(1)))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let schedule = Schedule {
        kind: ScheduleKind::Once,
        at: Some(at),
        weekdays: Vec::new(),
        auto_start,
        last_fired: None,
    };
    set_schedule(app, id, schedule)
}

/// Clear a recurring task's just-fired slot so it won't immediately re-surface
/// the in-app "needs start" state. One-shots already self-clear on fire, so for
/// them this is a no-op beyond a refresh.
#[tauri::command]
pub fn dismiss_fired_schedule<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let task = guard.get_mut(&id).ok_or("no such task")?;
        if task.schedule.kind == ScheduleKind::Recurring {
            if let Some(slot) = next_fire_time(&task.schedule, chrono::Local::now()) {
                if slot <= chrono::Local::now() {
                    task.schedule.last_fired = Some(slot.to_rfc3339());
                }
            }
        }
    }
    persist(&app)?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}
