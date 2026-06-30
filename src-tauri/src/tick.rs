use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use chrono::Local;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::time::{interval, MissedTickBehavior};

use crate::schedule::{next_fire_time, ScheduleKind};
use crate::state::{persist, AppState};
use crate::timer::{TimerMode, TimerState};

#[derive(Clone, serde::Serialize)]
struct TickPayload {
    id: String,
    remaining_secs: u64,
    elapsed_secs: u64,
    state: TimerState,
}

#[derive(Clone, serde::Serialize)]
struct DonePayload {
    id: String,
    title: String,
    #[serde(skip)]
    content: String,
}

/// Built under the lock, consumed after release (side effects must not run under
/// the state mutex — notifications block and start_timer re-locks).
struct FirePlan {
    id: String,
    auto_start: bool,
    title: String,
    content: String,
}

/// Grace window: a one-shot overdue by no more than this on boot still fires.
const BOOT_GRACE_SECS: i64 = 5 * 60;

/// Spawned once from setup(). Owns the heartbeat for ALL timers + schedules.
pub fn spawn_tick_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut ticks_since_persist: u32 = 0;

        loop {
            ticker.tick().await;

            // Global pause: skip timers + schedules entirely, without mutating
            // any task's individual state.
            if app.state::<AppState>().paused.load(Ordering::Relaxed) {
                continue;
            }

            let now_wall = Local::now();

            // ---- critical section: short, std Mutex, NO .await inside ----
            let mut ticks: Vec<TickPayload> = Vec::new();
            let mut dones: Vec<DonePayload> = Vec::new();
            let mut to_fire: Vec<FirePlan> = Vec::new();
            {
                let state = app.state::<AppState>();
                let mut tasks = match state.tasks.lock() {
                    Ok(g) => g,
                    Err(_) => continue, // poisoned: skip this tick
                };
                let now = Instant::now();

                for task in tasks.values_mut() {
                    // ---- timers ----
                    let t = &mut task.timer;
                    if t.state == TimerState::Running {
                        if let Some(anchor) = t.anchor {
                            let run = now.saturating_duration_since(anchor.started_at).as_secs();
                            match t.mode {
                                TimerMode::Stopwatch => {
                                    t.elapsed_secs = anchor.base_secs + run;
                                }
                                TimerMode::Countdown => {
                                    let spent = anchor.base_secs + run;
                                    if spent >= t.duration_secs {
                                        t.remaining_secs = 0;
                                        t.elapsed_secs = t.duration_secs;
                                        t.state = TimerState::Done;
                                        t.anchor = None;
                                        dones.push(DonePayload {
                                            id: task.id.clone(),
                                            title: task.title.clone(),
                                            content: task.content.clone(),
                                        });
                                    } else {
                                        t.elapsed_secs = spent;
                                        t.remaining_secs = t.duration_secs - spent;
                                    }
                                }
                            }
                            ticks.push(TickPayload {
                                id: task.id.clone(),
                                remaining_secs: t.remaining_secs,
                                elapsed_secs: t.elapsed_secs,
                                state: t.state,
                            });
                        }
                    }

                    // ---- schedules ----
                    if task.schedule.kind == ScheduleKind::None {
                        continue;
                    }
                    if let Some(fire_at) = next_fire_time(&task.schedule, now_wall) {
                        if fire_at <= now_wall {
                            let fire_key = fire_at.to_rfc3339();
                            if task.schedule.last_fired.as_deref() != Some(fire_key.as_str()) {
                                task.schedule.last_fired = Some(fire_key);
                                to_fire.push(FirePlan {
                                    id: task.id.clone(),
                                    auto_start: task.schedule.auto_start,
                                    title: task.title.clone(),
                                    content: task.content.clone(),
                                });
                                // one-shot is spent; recurring re-arms via next_fire_time
                                if task.schedule.kind == ScheduleKind::Once {
                                    task.schedule.kind = ScheduleKind::None;
                                }
                            }
                        }
                    }
                }

                ticks_since_persist += 1;
            } // <-- lock released HERE

            // ---- emit + side effects AFTER releasing the lock ----
            // One window: emit globally; the frontend routes by payload.id.
            for p in &ticks {
                let _ = app.emit("timer-tick", p);
            }
            for d in &dones {
                let _ = app.emit("timer-done", d);
                // Desktop notification when a countdown finishes (mirrors schedules).
                let title = if d.title.is_empty() {
                    "Notch"
                } else {
                    d.title.as_str()
                };
                let _ = app
                    .notification()
                    .builder()
                    .title(title)
                    .body("Timer finished.")
                    .show();
                // Telegram push (best-effort; no-op if disabled/unconfigured).
                crate::telegram::send(
                    &app,
                    crate::telegram::format_timer_done(&d.title, &d.content),
                );
            }
            let fired = !to_fire.is_empty();
            for plan in to_fire {
                fire_schedule(&app, plan);
            }

            // ---- coarse persistence, outside the lock ----
            if fired || !dones.is_empty() || (!ticks.is_empty() && ticks_since_persist >= 5) {
                ticks_since_persist = 0;
                let _ = persist(&app);
            }
        }
    });
}

/// Notify, bring the note to the front, and auto-start or offer a Start button.
fn fire_schedule(app: &AppHandle, plan: FirePlan) {
    let body = if plan.auto_start {
        "Timer started."
    } else {
        "Tap the note to start the timer."
    };
    let title = if plan.title.is_empty() {
        "Notch"
    } else {
        plan.title.as_str()
    };
    // Swallow errors: a missing notification daemon must not crash the loop.
    let _ = app.notification().builder().title(title).body(body).show();

    // Telegram push (best-effort; no-op if disabled/unconfigured).
    crate::telegram::send(
        app,
        crate::telegram::format_schedule_fired(&plan.title, &plan.content, plan.auto_start),
    );

    // Bring the single main window to the front.
    if let Some(win) = app.get_webview_window(crate::window::MAIN_LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }

    // Chime (frontend respects the soundOn setting).
    let _ = app.emit("play-sound", ());

    if plan.auto_start {
        let _ = crate::commands::start_timer(app.clone(), plan.id.clone());
    } else {
        // Linux notification daemons can't be relied on for action buttons, so
        // the in-app UI surfaces Start when it receives this event (carries the id).
        let _ = app.emit("schedule-fired", plan.id.clone());
    }
}

/// Run once at boot (before the loop): honor one-shots overdue within the grace
/// window, drop stale ones, and suppress recurring slots already past today.
pub fn reconcile_on_boot(app: &AppHandle) {
    let now = Local::now();
    let mut plans: Vec<FirePlan> = Vec::new();
    {
        let state = app.state::<AppState>();
        let mut tasks = match state.tasks.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        for task in tasks.values_mut() {
            match task.schedule.kind {
                ScheduleKind::Once => {
                    if let Some(at) = next_fire_time(&task.schedule, now) {
                        if at <= now {
                            let overdue = (now - at).num_seconds();
                            task.schedule.kind = ScheduleKind::None;
                            if overdue <= BOOT_GRACE_SECS {
                                task.schedule.last_fired = Some(at.to_rfc3339());
                                plans.push(FirePlan {
                                    id: task.id.clone(),
                                    auto_start: task.schedule.auto_start,
                                    title: task.title.clone(),
                                    content: task.content.clone(),
                                });
                            }
                        }
                    }
                }
                ScheduleKind::Recurring => {
                    if let Some(slot) = next_fire_time(&task.schedule, now) {
                        if slot <= now {
                            task.schedule.last_fired = Some(slot.to_rfc3339());
                        }
                    }
                }
                ScheduleKind::None => {}
            }
        }
    } // lock dropped
    for plan in plans {
        fire_schedule(app, plan);
    }
    let _ = persist(app);
}
