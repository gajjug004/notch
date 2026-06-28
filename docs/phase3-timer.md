# Phase 3 — Timer (Implementation Spec)

> Per-note timer with **both** modes (countdown + stopwatch), **driven entirely by
> Rust**. Rust is the source of truth: the timer keeps running even after its note
> window is closed. The frontend has **no clock of its own** — it renders only the
> values Rust pushes to it via events.

**Decision locked:** TIMER LIVES IN RUST. The frontend never computes elapsed time,
never runs `setInterval` to count, never decrements a number on its own. It listens
for `timer-tick` events and paints the numbers. This is the single rule that makes
the whole design coherent — every other choice below follows from it.

Assumes Phases 0–2 are complete:

- Multi-note: one OS window per task, window `label == task id`.
- Rust state: `AppState { tasks: Mutex<HashMap<TaskId, Task>> }`, managed via
  `Manager::manage`, read via `Manager::state`.
- Store persistence: tasks serialize to JSON through `tauri-plugin-store`; a
  `persist(&AppState, &AppHandle)` helper writes the whole map (or a single task)
  to disk.

Tauri v2 latest APIs only: `Emitter::emit` / `Emitter::emit_to`, `Manager::state`,
`tauri::async_runtime::spawn`.

---

## 1. Data model — timer fields on `Task`

The timer is a sub-struct of `Task`. Everything except the run anchor is plain
serializable data that rides the existing task persistence.

```rust
// src-tauri/src/timer.rs   (or inline in lib.rs / task.rs)

use std::time::Instant;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerMode {
    Countdown,
    Stopwatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerState {
    Idle,    // never started, or reset
    Running, // tick loop is advancing it
    Paused,  // frozen, segment folded into base
    Done,    // countdown only: reached 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Timer {
    pub mode: TimerMode,
    /// Configured length for countdown. Ignored by stopwatch (kept for mode switch).
    pub duration_secs: u64,
    /// Countdown: seconds left. Updated by the tick loop. == duration when idle.
    pub remaining_secs: u64,
    /// Stopwatch: seconds counted up. Also used to show total time for countdown.
    pub elapsed_secs: u64,
    pub state: TimerState,

    /// Monotonic run anchor. NEVER persisted, NEVER serialized.
    /// `Some` only while `state == Running`. This is what makes counting drift-free.
    #[serde(skip)]
    pub anchor: Option<RunAnchor>,
}

/// Captured at the instant a timer starts/resumes. Lets every tick recompute the
/// *true* position from a single monotonic reference instead of accumulating
/// per-tick rounding error.
#[derive(Clone, Copy, Debug)]
pub struct RunAnchor {
    /// Monotonic clock reading at the moment Running began.
    pub started_at: Instant,
    /// Seconds already accumulated before this run segment (folded on each pause).
    /// Countdown: seconds already spent. Stopwatch: seconds already elapsed.
    pub base_secs: u64,
}

impl Default for Timer {
    fn default() -> Self {
        Timer {
            mode: TimerMode::Countdown,
            duration_secs: 25 * 60, // pomodoro default
            remaining_secs: 25 * 60,
            elapsed_secs: 0,
            state: TimerState::Idle,
            anchor: None,
        }
    }
}
```

Add it to `Task`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub content: String,
    pub color: String,
    pub window: WindowGeom,
    #[serde(default)]            // tolerate tasks persisted before Phase 3
    pub timer: Timer,
}
```

### Why `#[serde(skip)]` on the anchor

`Instant` is a monotonic, process-local value — it is **not** meaningful across a
restart (it is relative to an arbitrary boot/process epoch). Serializing it would
be wrong and, in fact, `Instant` is not `Serialize`. So the anchor is purely
in-memory runtime state. Persistence stores the *folded* numbers
(`remaining_secs` / `elapsed_secs`), and on boot the timer is reconstructed without
an anchor (it boots paused — see §8). `#[serde(default)]` on `anchor` is implicit
through `skip`: skipped fields are filled with `Default` on deserialize, so
`anchor` comes back as `None`. 

---

## 2. The single shared tick loop

There is **ONE** async task for the whole app, not one per timer. It wakes once a
second, walks the task map, advances every `Running` timer from its monotonic
anchor, buffers the resulting event payloads while holding the lock, **releases the
lock**, and only then emits. Persistence of the folded numbers happens on a coarser
cadence (every N ticks) to avoid hammering the disk.

```rust
// src-tauri/src/tick.rs
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{interval, MissedTickBehavior};

use crate::{AppState, TimerMode, TimerState};

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
        // If the executor is busy and we miss a tick, do NOT fire a burst of
        // catch-up ticks. Skip the missed ones — we recompute absolute position
        // from Instant anyway, so a skipped tick costs at most one second of
        // visual lag, never a counting error.
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut ticks_since_persist: u32 = 0;

        loop {
            ticker.tick().await;

            // ---- critical section: short, std Mutex, NO .await inside ----
            let mut ticks: Vec<TickPayload> = Vec::new();
            let mut dones: Vec<DonePayload> = Vec::new();
            {
                let state = app.state::<AppState>();
                let mut tasks = state.tasks.lock().unwrap();
                let now = Instant::now();

                for task in tasks.values_mut() {
                    let t = &mut task.timer;
                    if t.state != TimerState::Running {
                        continue;
                    }
                    let Some(anchor) = t.anchor else { continue };

                    // Drift-free: position = base + (now - started_at).
                    // No per-tick decrement is ever accumulated.
                    let run = now.saturating_duration_since(anchor.started_at).as_secs();

                    match t.mode {
                        TimerMode::Stopwatch => {
                            t.elapsed_secs = anchor.base_secs + run;
                        }
                        TimerMode::Countdown => {
                            let spent = anchor.base_secs + run;
                            t.elapsed_secs = spent;
                            if spent >= t.duration_secs {
                                // Reached zero.
                                t.remaining_secs = 0;
                                t.elapsed_secs = t.duration_secs;
                                t.state = TimerState::Done;
                                t.anchor = None; // stop counting
                                dones.push(DonePayload { id: task.id.clone() });
                            } else {
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
                // Targeted: window label == task id (see §3).
                let _ = app.emit_to(p.id.as_str(), "timer-tick", p);
            }
            for d in &dones {
                let _ = app.emit_to(d.id.as_str(), "timer-done", d);
            }

            // ---- coarse persistence, also outside the lock ----
            // Persist when something finished, or every ~5s while running.
            if !dones.is_empty() || (!ticks.is_empty() && ticks_since_persist >= 5) {
                ticks_since_persist = 0;
                crate::persist::save_all(&app); // re-locks briefly internally
            }
        }
    });
}
```

Spawn it in `setup()` with the app handle:

```rust
// lib.rs
.setup(|app| {
    let handle = app.handle().clone();
    crate::tick::spawn_tick_loop(handle);
    Ok(())
})
```

Required deps in `Cargo.toml`:

```toml
[dependencies]
tauri = { version = "2", features = [] }
tokio = { version = "1", features = ["time", "rt", "macros"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
# tauri-plugin-store (from Phase 1/2)
```

> `tauri::async_runtime` is a re-export of Tokio, so `tokio::time::interval`
> composes correctly with `tauri::async_runtime::spawn`.

**Why one loop, not one-per-timer:** N timers = N tasks contending for the lock,
N timers to cancel/track on pause/reset/delete, and N wakeups per second. A single
loop is O(1) tasks, trivially correct on add/remove (it just reads the current map
each tick), and the per-tick work is a cheap map scan. There is nothing to cancel
when a timer pauses — the loop simply skips non-`Running` entries.

---

## 3. Events

Two events, both per-note. Use **`emit_to(window_label, …)`** so each note window
receives only its own timer's updates. Because Phase 2 set `window label == task
id`, the routing is just `emit_to(&task.id, …)`.

### Payloads

| Event         | Payload                                                        | When |
|---------------|---------------------------------------------------------------|------|
| `timer-tick`  | `{ id, remaining_secs, elapsed_secs, state }`                  | every 1s for each running timer; plus a one-shot after each command (§4) |
| `timer-done`  | `{ id }`                                                       | countdown reaches 0 |

### Rust emit (preferred — targeted)

```rust
app.emit_to(task.id.as_str(), "timer-tick", &payload)?;
```

`emit_to`'s first arg is an `EventTarget` (a `&str` coerces to a window label).
This sends the event only to the matching webview — no wasted IPC, no client-side
filtering.

### JS listen

```ts
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

const myId = getCurrentWindow().label; // == task id

type TickPayload = {
  id: string;
  remaining_secs: number;
  elapsed_secs: number;
  state: "idle" | "running" | "paused" | "done";
};

const unlistenTick = await listen<TickPayload>("timer-tick", (e) => {
  // With emit_to we only ever get our own events, but guard anyway.
  if (e.payload.id !== myId) return;
  renderTimer(e.payload);
});

const unlistenDone = await listen<{ id: string }>("timer-done", (e) => {
  if (e.payload.id !== myId) return;
  flashDone(); // visual cue only in Phase 3 (§6)
});
```

### Broadcast + filter fallback

If you prefer a single broadcast (e.g. an "overview"/settings window that wants
*all* timers), emit globally and filter on the client:

```rust
// Rust: broadcast to every window
app.emit("timer-tick", &payload)?;
```

```ts
// JS: ignore events that aren't mine
await listen<TickPayload>("timer-tick", (e) => {
  if (e.payload.id !== myId) return; // <-- the filter
  renderTimer(e.payload);
});
```

**Recommendation:** use `emit_to` for the per-note windows (less IPC chatter, no
risk of a missed filter leaking another note's numbers); reserve the broadcast
form for an aggregate view if/when one exists.

---

## 4. Commands

All four are thin: take a short `std::sync::Mutex` lock, mutate one task, drop the
lock, then persist and emit a one-shot tick so the UI updates *immediately* without
waiting up to a second for the loop. **No `.await` is ever held across the lock.**

```rust
// src-tauri/src/commands.rs
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppState, Timer, TimerMode, TimerState, RunAnchor};

/// Recompute folded numbers + emit one immediate tick for `id`.
/// Locks briefly, releases, then emits. Returns the fresh snapshot.
fn emit_now(app: &AppHandle, id: &str) {
    let snapshot = {
        let state = app.state::<AppState>();
        let tasks = state.tasks.lock().unwrap();
        tasks.get(id).map(|t| (
            t.id.clone(),
            t.timer.remaining_secs,
            t.timer.elapsed_secs,
            t.timer.state,
        ))
    }; // lock dropped
    if let Some((id, remaining_secs, elapsed_secs, state)) = snapshot {
        let _ = app.emit_to(id.as_str(), "timer-tick", &serde_json::json!({
            "id": id,
            "remaining_secs": remaining_secs,
            "elapsed_secs": elapsed_secs,
            "state": state,
        }));
    }
}

/// Start OR resume (same command). Sets state=Running and arms the anchor with the
/// already-accumulated seconds as base, so the loop continues from where we are.
#[tauri::command]
pub fn start_timer(id: String, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    {
        let mut tasks = state.tasks.lock().unwrap();
        let task = tasks.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;

        match t.state {
            TimerState::Running => return Ok(()), // idempotent
            TimerState::Done => return Ok(()),    // must reset first
            _ => {}
        }

        // base = seconds already consumed in this timer so far.
        let base = match t.mode {
            TimerMode::Countdown => t.duration_secs.saturating_sub(t.remaining_secs),
            TimerMode::Stopwatch => t.elapsed_secs,
        };
        t.anchor = Some(RunAnchor { started_at: Instant::now(), base_secs: base });
        t.state = TimerState::Running;
    } // lock dropped
    crate::persist::save_all(&app);
    emit_now(&app, &id);
    Ok(())
}

/// Pause: FOLD the in-flight segment into the stored numbers, drop the anchor.
/// After this, remaining/elapsed are exact and self-contained (persist-safe).
#[tauri::command]
pub fn pause_timer(id: String, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    {
        let mut tasks = state.tasks.lock().unwrap();
        let task = tasks.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;

        if t.state != TimerState::Running { return Ok(()); }
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
    crate::persist::save_all(&app);
    emit_now(&app, &id);
    Ok(())
}

/// Reset: back to idle at the configured start. Drops the anchor.
#[tauri::command]
pub fn reset_timer(id: String, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    {
        let mut tasks = state.tasks.lock().unwrap();
        let task = tasks.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;
        t.anchor = None;
        t.state = TimerState::Idle;
        t.elapsed_secs = 0;
        t.remaining_secs = t.duration_secs; // meaningful for countdown
    }
    crate::persist::save_all(&app);
    emit_now(&app, &id);
    Ok(())
}

/// Configure mode + duration. Only allowed when not actively running
/// (force a reset-like state so numbers stay consistent).
#[tauri::command]
pub fn configure_timer(
    id: String,
    mode: TimerMode,
    duration_secs: u64,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    {
        let mut tasks = state.tasks.lock().unwrap();
        let task = tasks.get_mut(&id).ok_or("no such task")?;
        let t = &mut task.timer;
        t.anchor = None;
        t.mode = mode;
        t.duration_secs = duration_secs;
        t.elapsed_secs = 0;
        t.remaining_secs = duration_secs;
        t.state = TimerState::Idle;
    }
    crate::persist::save_all(&app);
    emit_now(&app, &id);
    Ok(())
}
```

Register them:

```rust
.invoke_handler(tauri::generate_handler![
    // ...phase 1/2 commands...
    start_timer, pause_timer, reset_timer, configure_timer
])
```

Capability (`capabilities/default.json`) — the command permissions are generated
from `generate_handler!`; ensure the note windows are in the capability's `windows`
list (Phase 2 likely uses `["main", "note-*"]` or a wildcard). Event listening
(`listen`) needs `core:event:default` which is included in `core:default`.

**Why `start_timer` == resume:** there is no behavioral difference between "begin"
and "continue" — both set `state = Running` and arm the anchor with whatever
seconds are already banked (`base_secs`). A fresh start just happens to have
`base = 0` (idle) and a resume has `base > 0` (paused). One command, one code path.

---

## 5. Countdown hitting zero

Handled inside the tick loop (§2). When `base + run >= duration_secs`:

1. `remaining_secs = 0`, `elapsed_secs = duration_secs` (clamp, don't overshoot).
2. `state = Done`.
3. `anchor = None` — counting stops; the loop will skip it next tick.
4. Push a `timer-done { id }` payload, emitted after the lock is released.
5. Persistence is forced on the tick where a `done` occurs (so the final state
   survives even an immediate quit).

**Notifications are deferred to Phase 4** (`tauri-plugin-notification`). Phase 3
provides only a **visual cue** in the note: the display flashes / turns the accent
color and shows `00:00` (see §6). `Done` is terminal until the user hits **Reset**
(or **Start** after a reset) — `start_timer` is a no-op while `Done` to avoid
restarting a finished countdown by accident.

---

## 6. Timer config + display UI

Pure display from Rust. The only client-side computation is `secs -> "mm:ss"`
**formatting** (not counting). Buttons just `invoke` commands; the numbers that
appear come back through `timer-tick`.

### HTML (add to the note `index.html`)

```html
<section class="timer">
  <div id="timer-display" class="timer-display tabular">25:00</div>

  <div class="timer-config">
    <div class="mode-toggle" role="tablist">
      <button id="mode-countdown" class="mode-btn active">Countdown</button>
      <button id="mode-stopwatch" class="mode-btn">Stopwatch</button>
    </div>
    <label class="duration" data-mode="countdown">
      <input id="dur-input" type="text" inputmode="numeric"
             value="25:00" pattern="[0-9]{1,2}:[0-5][0-9]" />
    </label>
  </div>

  <div class="timer-controls">
    <button id="btn-start">Start</button>
    <button id="btn-pause" disabled>Pause</button>
    <button id="btn-reset">Reset</button>
  </div>
</section>
```

### CSS (`styles.css`)

```css
.timer-display {
  font-size: 2.6rem;
  font-weight: 600;
  text-align: center;
  letter-spacing: 0.02em;
}
/* tabular-nums: digits keep constant width so the clock doesn't jitter */
.tabular { font-variant-numeric: tabular-nums; }

.timer-display.done {
  color: #e2483d;
  animation: done-flash 0.6s ease-in-out 3;
}
@keyframes done-flash { 50% { opacity: 0.25; } }

.timer-controls { display: flex; gap: 6px; justify-content: center; }
.mode-toggle { display: flex; gap: 4px; justify-content: center; }
.mode-btn.active { font-weight: 700; text-decoration: underline; }
/* hide the duration input when stopwatch mode is active */
.timer[data-mode="stopwatch"] .duration { display: none; }
```

### TypeScript (`timer.ts`, imported by the note's `main.ts`)

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Mode = "countdown" | "stopwatch";
type TickPayload = {
  id: string;
  remaining_secs: number;
  elapsed_secs: number;
  state: "idle" | "running" | "paused" | "done";
};

const id = getCurrentWindow().label;

const fmt = (s: number) => {
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
};

const parse = (mmss: string): number => {
  const [m, s] = mmss.split(":").map((n) => parseInt(n, 10) || 0);
  return m * 60 + s;
};

let mode: Mode = "countdown";

function render(p: TickPayload) {
  const disp = document.getElementById("timer-display")!;
  // Display from Rust ONLY. Countdown shows remaining; stopwatch shows elapsed.
  disp.textContent = mode === "countdown" ? fmt(p.remaining_secs) : fmt(p.elapsed_secs);

  disp.classList.toggle("done", p.state === "done");
  const running = p.state === "running";
  (document.getElementById("btn-start") as HTMLButtonElement).disabled = running;
  (document.getElementById("btn-pause") as HTMLButtonElement).disabled = !running;
}

export async function initTimer(): Promise<UnlistenFn[]> {
  const section = document.querySelector(".timer")!;
  section.setAttribute("data-mode", mode);

  document.getElementById("btn-start")!
    .addEventListener("click", () => invoke("start_timer", { id }));
  document.getElementById("btn-pause")!
    .addEventListener("click", () => invoke("pause_timer", { id }));
  document.getElementById("btn-reset")!
    .addEventListener("click", () => invoke("reset_timer", { id }));

  const applyConfig = () => {
    const dur = parse((document.getElementById("dur-input") as HTMLInputElement).value);
    section.setAttribute("data-mode", mode);
    invoke("configure_timer", { id, mode, durationSecs: mode === "countdown" ? dur : 0 });
  };
  document.getElementById("mode-countdown")!
    .addEventListener("click", () => { mode = "countdown"; toggleActive(); applyConfig(); });
  document.getElementById("mode-stopwatch")!
    .addEventListener("click", () => { mode = "stopwatch"; toggleActive(); applyConfig(); });
  document.getElementById("dur-input")!
    .addEventListener("change", applyConfig);

  // Subscribe; keep handles for cleanup (§9).
  const unlistenTick = await listen<TickPayload>("timer-tick", (e) => {
    if (e.payload.id !== id) return;
    render(e.payload);
  });
  const unlistenDone = await listen<{ id: string }>("timer-done", (e) => {
    if (e.payload.id !== id) return;
    document.getElementById("timer-display")!.classList.add("done");
  });

  return [unlistenTick, unlistenDone];
}

function toggleActive() {
  document.getElementById("mode-countdown")!
    .classList.toggle("active", mode === "countdown");
  document.getElementById("mode-stopwatch")!
    .classList.toggle("active", mode === "stopwatch");
}
```

> Note on `invoke` arg casing: Tauri auto-converts `camelCase` JS keys to the Rust
> `snake_case` params, so `durationSecs` → `duration_secs`. Keep `id` as-is.

On load, the note should fetch its current task (Phase 2's `get_task(id)` /
`list_tasks`) once to paint the initial display and set the mode toggle, then let
`timer-tick` take over.

---

## 7. Persistence

The timer is a field of `Task`, so it rides the **existing** task persistence — no
separate store. Two write triggers:

1. **Commands** (`start/pause/reset/configure`) persist immediately on each call.
2. **The tick loop** persists coarsely (every ~5s while running, and always on a
   `done`) so a crash loses at most a few seconds of progress.

What is written is always the **folded** state: `remaining_secs` / `elapsed_secs`
/ `state` / `mode` / `duration_secs`. The `RunAnchor` is `#[serde(skip)]` and never
touches disk.

---

## 8. Restart behavior — boot **paused**

On boot, after loading tasks from the store, **force every timer that was `Running`
to `Paused`** (keep its `remaining_secs` / `elapsed_secs`, clear the anchor):

```rust
// during setup(), after loading tasks into AppState
for task in tasks.values_mut() {
    if task.timer.state == TimerState::Running {
        task.timer.state = TimerState::Paused;
        task.timer.anchor = None; // no valid Instant across a restart
    }
}
```

**Justification:**

- An `Instant` anchor cannot survive a process restart (monotonic clock is relative
  to this process). To "resume running" we would have to fall back to wall-clock
  arithmetic against a persisted timestamp — but the app was *closed*, so the user
  was not watching; silently fast-forwarding a pomodoro past its end (or counting
  hours of "elapsed" stopwatch time while the machine was off) is almost never what
  they want.
- Booting **paused** is the honest, least-surprising default: the exact numbers
  are preserved, and one click on **Start** resumes cleanly (the anchor is re-armed
  from the banked seconds via §4). A `Done` timer stays `Done`; an `Idle` timer
  stays `Idle`.
- (If a future phase wants true wall-clock catch-up — e.g. "this countdown should
  have finished while I was away, fire it now" — that belongs with the Phase 4
  scheduler's missed-fire handling, not here.)

---

## 9. Gotchas (must-follow)

- **Never hold the `Mutex` across `.await`.** Use `std::sync::Mutex` (not
  `tokio::sync::Mutex`), take it in a tight scope, drop it before any emit or disk
  I/O. A std Mutex held across an await point can deadlock the single-threaded
  parts of the runtime and stall every other timer.
- **One loop, not one-per-timer.** The heartbeat is global; it reads the live map
  each tick. Adding/removing/pausing a timer requires zero loop bookkeeping.
- **Emit and persist *outside* the lock.** Build a `Vec<payload>` while locked,
  release, then `emit_to` / `save_all`. Emitting under the lock serializes all IPC
  behind the mutex and risks contention with commands.
- **`MissedTickBehavior::Skip`.** Because position is recomputed from the monotonic
  anchor every tick, a skipped tick is cosmetic (≤1s lag), never a counting error.
  Burst/`Delay` behavior would cause visible stutter with no benefit.
- **Frontend listener cleanup.** `listen` returns an `UnlistenFn`; on a webview
  reload the old closures would otherwise leak and double-render. Tear down on
  `beforeunload`:

  ```ts
  let unlisteners: UnlistenFn[] = [];
  window.addEventListener("DOMContentLoaded", async () => {
    unlisteners = await initTimer();
  });
  window.addEventListener("beforeunload", () => {
    unlisteners.forEach((u) => u());
    unlisteners = [];
  });
  ```

- **Idempotent commands.** `start_timer` on an already-running (or `Done`) timer is
  a no-op; `pause_timer` on a non-running timer is a no-op. The UI also disables the
  wrong button, but the backend must not assume the UI is correct.
- **Clamp countdown.** Never let `elapsed > duration` or `remaining` underflow
  (`u64` wraps). Use `saturating_sub` / `.min(duration)` everywhere folding occurs.
- **`emit_to` target == window label == task id.** If Phase 2 chose a different
  labeling scheme, the emit target and the `getCurrentWindow().label` filter must
  use that scheme consistently.

---

## 10. Done criteria (from plan)

- Countdown and stopwatch both run; numbers come only from Rust.
- Start / Pause / Reset all work; mode + duration configurable.
- Display matches Rust exactly (no client drift).
- Close a note window, reopen it → the timer is still counting (state lived in
  Rust the whole time).
- Countdown reaching 0 → `Done` state + visual flash + `timer-done` event
  (notification wired in Phase 4).
- Quit + relaunch → timer numbers preserved; previously-running timers come back
  **paused** at the right value.
