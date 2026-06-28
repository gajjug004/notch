use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{interval, MissedTickBehavior};

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
}

/// Spawned once from setup(). Owns the heartbeat for ALL timers.
pub fn spawn_tick_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        // Recompute absolute position from the anchor each tick, so a missed tick
        // costs at most one second of visual lag, never a counting error.
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut ticks_since_persist: u32 = 0;

        loop {
            ticker.tick().await;

            // ---- critical section: short, std Mutex, NO .await inside ----
            let mut ticks: Vec<TickPayload> = Vec::new();
            let mut dones: Vec<DonePayload> = Vec::new();
            {
                let state = app.state::<AppState>();
                let mut tasks = match state.tasks.lock() {
                    Ok(g) => g,
                    Err(_) => continue, // poisoned: skip this tick
                };
                let now = Instant::now();

                for task in tasks.values_mut() {
                    let t = &mut task.timer;
                    if t.state != TimerState::Running {
                        continue;
                    }
                    let Some(anchor) = t.anchor else { continue };

                    // Drift-free: position = base + (now - started_at).
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
                                t.anchor = None; // stop counting
                                dones.push(DonePayload {
                                    id: task.id.clone(),
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

                ticks_since_persist += 1;
            } // <-- lock released HERE, before any emit / disk I/O

            // ---- emit AFTER releasing the lock ----
            for p in &ticks {
                let _ = app.emit_to(p.id.as_str(), "timer-tick", p);
            }
            for d in &dones {
                let _ = app.emit_to(d.id.as_str(), "timer-done", d);
            }

            // ---- coarse persistence, outside the lock ----
            if !dones.is_empty() || (!ticks.is_empty() && ticks_since_persist >= 5) {
                ticks_since_persist = 0;
                let _ = persist(&app);
            }
        }
    });
}
