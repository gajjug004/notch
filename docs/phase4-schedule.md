# Phase 4 — Schedule (Implementation Spec)

Status: ready to build. Assumes Phases 0–3 are done: multi-window notes, a
`Mutex<HashMap<Id, Task>>` app state synced to `tauri-plugin-store`, a Rust
timer engine with `start_timer/pause_timer/reset_timer`, and a single 1-second
async tick loop that already iterates all tasks and emits `timer-tick`.

## Goal

Each note carries an optional schedule. A scheduler — folded into the **existing
1 s tick loop** — compares "now" against each task's next fire time. When a
schedule fires it:

1. Shows a desktop notification (`tauri-plugin-notification`).
2. Brings the note window to the front (unminimize + show + focus).
3. If `auto_start` is true, calls `start_timer(id)`; otherwise emits
   `schedule-fired { id }` so the note shows a one-click **Start** button.

The note is **always visible** (created in Phase 2). A schedule never creates or
destroys a note — it only notifies and (optionally) starts the timer.

### Scope decision (locked)

- **One-shot (`once`) is the primary deliverable.** Build and verify it fully.
- **Recurring (`recurring`, weekdays/daily) is designed in but secondary** —
  the data model, `next_fire_time`, and re-arm logic all handle it; ship it
  behind the same UI but treat one-shot as the acceptance path.

---

## 1. Data model — schedule fields on `Task`

Extend the existing `Task` struct. The schedule is a sub-struct so it serializes
as a nested object in the store and is easy to replace wholesale via the
`set_schedule` command.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleKind {
    None,
    Once,
    Recurring,
}

impl Default for ScheduleKind {
    fn default() -> Self {
        ScheduleKind::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Schedule {
    /// none | once | recurring
    pub kind: ScheduleKind,

    /// RFC3339 string, e.g. "2026-06-28T14:30:00+05:30".
    /// For `once`: the absolute instant to fire.
    /// For `recurring`: only the *time-of-day* component is used (date ignored).
    /// May also be a bare "datetime-local" value ("2026-06-28T14:30") coming
    /// straight from the HTML input — `parse_local` accepts both.
    #[serde(default)]
    pub at: Option<String>,

    /// Recurring only. Days the schedule fires, as chrono weekday numbers
    /// Mon=0 .. Sun=6 (matches `Weekday::num_days_from_monday()`).
    /// Empty + kind=recurring is treated as "no days" => never fires.
    #[serde(default)]
    pub weekdays: Vec<u8>,

    /// Auto-start the timer on fire. If false, only notify + emit schedule-fired.
    #[serde(default)]
    pub auto_start: bool,

    /// Duplicate-fire guard. RFC3339 of the last instant we actually fired for.
    /// Set to the *scheduled* fire instant (truncated to the second), not
    /// `Instant::now`, so we can compare deterministically.
    #[serde(default)]
    pub last_fired: Option<String>,
}
```

### Why RFC3339 string over a unix timestamp

- **Human-debuggable store.** The JSON store is hand-inspectable; an ISO string
  ("2026-06-28T14:30:00+05:30") is readable, a `1750000000` is not.
- **Carries the offset.** RFC3339 records the UTC offset, so a `once` schedule
  fires at the wall-clock instant the user picked even if the machine's tz
  config is re-read. A bare unix ts loses the "what local time did they mean".
- **Matches the frontend.** `<input type="datetime-local">` produces
  `"2026-06-28T14:30"`; `<input type="time">` produces `"14:30"`. Keeping `at`
  as a string lets the frontend round-trip its own value with no numeric
  conversion, and we parse leniently in Rust (see `parse_local`).
- **chrono-native.** `DateTime::parse_from_rfc3339` and `.to_rfc3339()` are
  first-class, so serialization is trivial and lossless.

`last_fired` is also RFC3339 for the same reason and so it directly compares to
the computed fire instant.

---

## 2. Scheduler folded into the 1 s tick loop

The Phase 3 loop already locks state once per second to tick timers. Add the
schedule check in the **same lock**, collect a `to_fire` list, then **drop the
lock before any side effects** (notification, window focus, `start_timer`,
event emit). Side effects must never run under the state mutex — notifications
and window ops can block, and `start_timer` re-locks state (deadlock otherwise).

```rust
use chrono::Local;
use tauri::{AppHandle, Manager};

// A small plan describing one fire, built under the lock, consumed after.
struct FirePlan {
    id: String,
    auto_start: bool,
    title: String,
}

pub async fn tick_loop(app: AppHandle) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        let now = Local::now();

        // ---- phase A: under lock, mutate timers + collect schedule fires ----
        let mut to_fire: Vec<FirePlan> = Vec::new();
        {
            let state = app.state::<AppState>();
            let mut tasks = state.tasks.lock().unwrap();

            for task in tasks.values_mut() {
                // (existing) advance running timers here, build timer-tick payloads...

                // schedule check
                if task.schedule.kind == ScheduleKind::None {
                    continue;
                }
                if let Some(fire_at) = next_fire_time(&task.schedule, now) {
                    // due if scheduled instant has arrived (second granularity)
                    if fire_at <= now {
                        let fire_key = fire_at.to_rfc3339();
                        // duplicate-fire guard: never fire the same instant twice
                        if task.schedule.last_fired.as_deref() != Some(fire_key.as_str()) {
                            task.schedule.last_fired = Some(fire_key);
                            to_fire.push(FirePlan {
                                id: task.id.clone(),
                                auto_start: task.schedule.auto_start,
                                title: task.title.clone(),
                            });

                            // re-arm immediately, still under lock
                            match task.schedule.kind {
                                ScheduleKind::Once => {
                                    task.schedule.kind = ScheduleKind::None;
                                }
                                ScheduleKind::Recurring => {
                                    // next_fire_time recomputes from `now`; nothing
                                    // else to mutate — last_fired prevents re-fire
                                    // within the same second.
                                }
                                ScheduleKind::None => {}
                            }
                        }
                    }
                }
            }
            // (existing) persist timer state periodically here while locked
        } // <-- lock dropped HERE

        // ---- phase B: no lock held — do side effects ----
        // (existing) emit timer-tick events...
        for plan in to_fire {
            fire_schedule(&app, plan);
        }

        // persist after fires so kind=none / last_fired survive a crash
        if /* anything fired or timer state changed */ true {
            persist(&app); // existing helper; takes its own short-lived lock
        }
    }
}
```

Key invariants:

- **Collect under lock, act after.** `to_fire` is the hand-off.
- **Guard set under lock** (`last_fired = fire_key`) before pushing, so even if
  two loop iterations race (they can't — single loop — but defensive) the same
  instant is fired once.
- **Side effects re-lock freely.** `fire_schedule` calls `start_timer(id)`,
  which locks state itself; safe because we already released.

---

## 3. `next_fire_time` + `parse_local`

```rust
use chrono::{DateTime, Datelike, Duration, Local, NaiveDateTime, NaiveTime, TimeZone, Weekday};

/// Returns the next instant this schedule should fire, or None.
/// `once`: the `at` instant if it is in the future (or now); None once past
///         (the loop's `fire_at <= now` + last_fired handles the actual firing,
///         and re-arm flips kind to none).
/// `recurring`: the soonest matching weekday at `at`'s time-of-day, scanning
///         today + the next 7 days. Returns the matching instant even if it is
///         a few seconds in the past today (so a fire that's due "now" is seen).
pub fn next_fire_time(sched: &Schedule, now: DateTime<Local>) -> Option<DateTime<Local>> {
    match sched.kind {
        ScheduleKind::None => None,

        ScheduleKind::Once => {
            let at = parse_local(sched.at.as_deref()?)?;
            // Keep returning the instant; the loop fires when at <= now and
            // last_fired != at. Returning Some(past) is fine — guarded by
            // last_fired so it fires exactly once, then kind->none.
            Some(at)
        }

        ScheduleKind::Recurring => {
            if sched.weekdays.is_empty() {
                return None;
            }
            let time = parse_local(sched.at.as_deref()?)?.time();
            // Scan today..today+7. For each candidate day whose weekday is in
            // the set, build the local datetime at `time`. Pick the soonest
            // candidate that is today-and-due-or-future. We accept a candidate
            // that is slightly in the past *today* so the loop can fire it.
            let today = now.date_naive();
            let mut best: Option<DateTime<Local>> = None;
            for offset in 0..=7 {
                let day = today + Duration::days(offset);
                let wd = day.weekday().num_days_from_monday() as u8;
                if !sched.weekdays.contains(&wd) {
                    continue;
                }
                let naive = NaiveDateTime::new(day, time);
                // DST-safe construction (see Gotchas):
                let dt = match Local.from_local_datetime(&naive) {
                    chrono::LocalResult::Single(dt) => dt,
                    chrono::LocalResult::Ambiguous(dt, _) => dt, // earlier of the two
                    chrono::LocalResult::None => continue,       // skipped hour: skip day
                };
                // Today's already-fired occurrence: skip if last_fired matches it.
                if offset == 0 {
                    if sched.last_fired.as_deref() == Some(dt.to_rfc3339().as_str()) {
                        continue; // fired today already; look at later days
                    }
                }
                if best.map_or(true, |b| dt < b) {
                    best = Some(dt);
                }
                if offset == 0 && dt <= now {
                    // today's slot is due right now — return it immediately
                    return Some(dt);
                }
            }
            best
        }
    }
}

/// Parse either a full RFC3339 ("...T14:30:00+05:30") or a bare
/// datetime-local ("2026-06-28T14:30" / "...:30:00") or a bare time
/// ("14:30") into a Local DateTime. Bare values are interpreted in Local tz.
pub fn parse_local(s: &str) -> Option<DateTime<Local>> {
    // 1) Full RFC3339 with offset.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Local));
    }
    // 2) Bare datetime-local, with or without seconds.
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            if let chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) =
                Local.from_local_datetime(&naive)
            {
                return Some(dt);
            }
        }
    }
    // 3) Bare time-of-day -> today at that time (used for recurring `at`).
    for fmt in ["%H:%M:%S", "%H:%M"] {
        if let Ok(t) = NaiveTime::parse_from_str(s, fmt) {
            let naive = NaiveDateTime::new(Local::now().date_naive(), t);
            if let chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) =
                Local.from_local_datetime(&naive)
            {
                return Some(dt);
            }
        }
    }
    None
}
```

Add to `Cargo.toml`:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

---

## 4. Firing — notification + bring to front + start

### 4.1 Plugin install

Cargo (`src-tauri/Cargo.toml`):

```toml
tauri-plugin-notification = "2"
```

npm:

```bash
npm add @tauri-apps/plugin-notification
```

Register in `lib.rs`:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_notification::init())
    // ...existing plugins
```

Capability (`src-tauri/capabilities/default.json`) — add to `permissions`:

```json
"notification:default"
```

`notification:default` bundles `allow-notify`, `allow-is-permission-granted`,
and `allow-request-permission` — enough to request permission and send.

### 4.2 Permission request

On Linux, native notifications generally need no per-app prompt, but call the
request once anyway so the permission state is initialized (and the same code
works if later packaged for macOS/Windows). Do it from the frontend at boot:

```ts
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

export async function ensureNotificationPermission() {
  let granted = await isPermissionGranted();
  if (!granted) {
    granted = (await requestPermission()) === "granted";
  }
  return granted;
}
```

### 4.3 `fire_schedule` (Rust)

```rust
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

fn fire_schedule(app: &AppHandle, plan: FirePlan) {
    // 1) desktop notification
    let body = if plan.auto_start {
        "Timer started."
    } else {
        "Tap the note to start the timer."
    };
    let _ = app
        .notification()
        .builder()
        .title(if plan.title.is_empty() { "Sticky Timer" } else { &plan.title })
        .body(body)
        .show(); // swallow error: a missing notif daemon must not crash the loop

    // 2) bring the note window to the front
    if let Some(win) = app.get_webview_window(&plan.id) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }

    // 3) auto-start or offer one-click start
    if plan.auto_start {
        let _ = start_timer(app.clone(), plan.id.clone()); // existing command/fn
    } else {
        // Linux libnotify has no reliable action buttons across daemons, so we
        // do NOT use notification action buttons. Instead the in-note UI shows
        // a Start button when it receives this event.
        let _ = app.emit_to(&plan.id, "schedule-fired", &plan.id);
    }
}
```

> **Why no notification action buttons:** action buttons (`.action(...)`) are
> unsupported / inconsistent across Linux notification daemons (GNOME, dunst,
> mako behave differently and many drop actions). The reliable path is to
> surface the Start affordance inside the always-visible note window via the
> `schedule-fired` event.

### 4.4 Frontend: handle `schedule-fired`

```ts
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

// in the note window, knowing its own taskId:
listen<string>("schedule-fired", (e) => {
  if (e.payload !== taskId) return;
  showStartButton(); // reveal a prominent "Start" button
});

startButton.onclick = () => invoke("start_timer", { id: taskId });
```

---

## 5. After-fire bookkeeping

Handled inside the loop (Section 2) and persisted in phase B:

- **`once`** → set `kind = ScheduleKind::None`. The schedule is spent. `at` and
  `last_fired` are left as-is (harmless; UI shows kind=none). The note remains.
- **`recurring`** → leave `kind = Recurring`. `last_fired` is set to the fired
  instant's RFC3339. On the next loop iteration `next_fire_time` recomputes:
  today's occurrence is skipped (because `last_fired` matches it) and the next
  matching weekday is returned. No explicit "compute next and store" field is
  needed — `next_fire_time` is pure over (`schedule`, `now`).

Persistence: call the existing `persist(app)` after firing so `kind=none` /
`last_fired` survive a crash or quit immediately after a fire.

---

## 6. Missed fires while the app was closed (boot reconcile)

Run once in `setup()` after tasks are loaded, before the tick loop starts.

Policy:

- **`once`, overdue ≤ 5 min** (grace window): fire once now, then set
  `kind=none`. The user just missed it by a moment; honor it.
- **`once`, overdue > 5 min**: skip — set `kind=none` without firing. Stale.
- **`recurring`: never replay.** Do not fire missed past occurrences. Just let
  `next_fire_time` arm the next future occurrence. To prevent the loop from
  immediately firing "today's already-past slot", set `last_fired` to today's
  computed occurrence if that occurrence is in the past.

```rust
use chrono::{Duration, Local};

const GRACE: i64 = 5 * 60; // seconds

fn reconcile_on_boot(app: &AppHandle) {
    let now = Local::now();
    let mut plans: Vec<FirePlan> = Vec::new();
    {
        let state = app.state::<AppState>();
        let mut tasks = state.tasks.lock().unwrap();
        for task in tasks.values_mut() {
            match task.schedule.kind {
                ScheduleKind::Once => {
                    if let Some(at) = next_fire_time(&task.schedule, now) {
                        if at <= now {
                            let overdue = (now - at).num_seconds();
                            if overdue <= GRACE {
                                task.schedule.last_fired = Some(at.to_rfc3339());
                                task.schedule.kind = ScheduleKind::None;
                                plans.push(FirePlan {
                                    id: task.id.clone(),
                                    auto_start: task.schedule.auto_start,
                                    title: task.title.clone(),
                                });
                            } else {
                                task.schedule.kind = ScheduleKind::None; // stale, skip
                            }
                        }
                    }
                }
                ScheduleKind::Recurring => {
                    // Suppress today's already-past slot so the loop won't replay it.
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
    persist(app);
}
```

---

## 7. Schedule UI + wiring

Minimal markup added to the note window. Keep it collapsed behind a small
"Schedule" toggle so it doesn't crowd the note.

```html
<section class="schedule">
  <label>
    Schedule
    <select id="sched-kind">
      <option value="none">Off</option>
      <option value="once">Once</option>
      <option value="recurring">Recurring</option>
    </select>
  </label>

  <!-- shown when kind=once -->
  <input type="datetime-local" id="sched-at" hidden />

  <!-- shown when kind=recurring -->
  <div id="sched-recurring" hidden>
    <input type="time" id="sched-time" />
    <span class="weekdays">
      <label><input type="checkbox" data-wd="0" />Mon</label>
      <label><input type="checkbox" data-wd="1" />Tue</label>
      <label><input type="checkbox" data-wd="2" />Wed</label>
      <label><input type="checkbox" data-wd="3" />Thu</label>
      <label><input type="checkbox" data-wd="4" />Fri</label>
      <label><input type="checkbox" data-wd="5" />Sat</label>
      <label><input type="checkbox" data-wd="6" />Sun</label>
    </span>
  </div>

  <label><input type="checkbox" id="sched-autostart" /> Auto-start timer</label>
  <button id="sched-save">Set schedule</button>
</section>
```

```ts
import { invoke } from "@tauri-apps/api/core";

const kindEl = document.querySelector<HTMLSelectElement>("#sched-kind")!;
const atEl = document.querySelector<HTMLInputElement>("#sched-at")!;
const recEl = document.querySelector<HTMLDivElement>("#sched-recurring")!;
const timeEl = document.querySelector<HTMLInputElement>("#sched-time")!;
const autoEl = document.querySelector<HTMLInputElement>("#sched-autostart")!;

function syncVisibility() {
  atEl.hidden = kindEl.value !== "once";
  recEl.hidden = kindEl.value !== "recurring";
}
kindEl.onchange = syncVisibility;

document.querySelector("#sched-save")!.addEventListener("click", async () => {
  const kind = kindEl.value as "none" | "once" | "recurring";
  const weekdays = [...recEl.querySelectorAll<HTMLInputElement>("input[data-wd]")]
    .filter((c) => c.checked)
    .map((c) => Number(c.dataset.wd));

  const schedule = {
    kind,
    // once -> the datetime-local value; recurring -> the time value;
    // none -> null. parse_local in Rust accepts both shapes.
    at: kind === "once" ? atEl.value : kind === "recurring" ? timeEl.value : null,
    weekdays,
    auto_start: autoEl.checked,
    last_fired: null, // resetting clears the guard so the new schedule can fire
  };

  await invoke("set_schedule", { id: taskId, schedule });
});
```

Rust command:

```rust
#[tauri::command]
fn set_schedule(app: AppHandle, id: String, schedule: Schedule) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut tasks = state.tasks.lock().unwrap();
        let task = tasks.get_mut(&id).ok_or("no such task")?;
        // clear the guard on (re)set so the new time can fire
        let mut schedule = schedule;
        schedule.last_fired = None;
        task.schedule = schedule;
    }
    persist(&app);
    Ok(())
}
```

Register `set_schedule` in the `invoke_handler` generate list alongside the
Phase 3 timer commands. On note load, hydrate the controls from the task's
stored `schedule` and call `syncVisibility()`.

---

## 8. Timezone

All comparisons use **`chrono::Local`** (the machine's configured timezone).
`Local::now()` is the single clock source for the loop and reconcile. `at` is
stored with offset (RFC3339) when it comes from a full timestamp, or interpreted
in Local when it's a bare datetime-local/time value via `parse_local`. No UTC
conversion is exposed to the user — they think and see in local wall-clock time.

---

## 9. "Done when" (acceptance)

1. Create a note, set **Once** to fire ~1 minute out, **Auto-start ON**, Save.
2. At the target second: a desktop notification appears, the note window comes
   to the front, and its timer starts counting (verify via `timer-tick`).
3. The schedule flips to `kind=none` (re-opening the schedule UI shows "Off").
   It does not fire again.
4. Set a note to **Recurring**, check today's weekday, time ~1 min out,
   Auto-start OFF, Save. It fires: notification + front + a **Start** button
   appears in the note (no auto-start). Clicking Start runs the timer.
5. After firing, the recurring schedule re-arms: `next_fire_time` now points at
   the next checked weekday (e.g. tomorrow/next week), and `last_fired` is set
   so it does not re-fire today.
6. Quit the app, change the schedule's `at` in the store to 2 min in the past,
   relaunch → it fires once on boot (within the 5 min grace). Set it 10 min in
   the past → it does not fire (stale), kind becomes none.

---

## 10. Gotchas

- **Linux notification daemon / permission.** Notifications need a running
  notification daemon (GNOME Shell, dunst, mako, etc.). On a bare WM with none,
  `.show()` returns an error — swallow it (don't crash the loop) but log it. The
  `requestPermission()` call is effectively a no-op on most Linux setups but
  keep it for portability and to initialize state.
- **Duplicate fires in the same second.** The loop runs every ~1 s and second
  comparisons (`at <= now`) stay true for a whole second. The `last_fired`
  guard (set to the *scheduled instant's* RFC3339, compared before firing) is
  what prevents firing 1–2 times within that window. Always set the guard
  **under the lock, before pushing to `to_fire`**.
- **Second-granularity comparison.** Truncate to whole seconds. `Local::now()`
  has sub-second precision; the picker gives minute/second precision. Compare
  `at <= now` (firing when due is fine) and key `last_fired` off the *scheduled*
  instant, not `now`, so the key is stable across the firing second.
- **DST `None` / `Ambiguous` for recurring.** Building a local datetime from a
  naive (date + time) can be:
  - `LocalResult::None` — the wall-clock time was skipped (spring-forward gap):
    **skip that day's occurrence** (`continue`).
  - `LocalResult::Ambiguous(a, b)` — time occurred twice (fall-back): **pick the
    earlier** (`a`) deterministically.
  Both are handled in `next_fire_time` and `parse_local`. One-shot `once` uses an
  absolute RFC3339 with offset, so it is immune to DST ambiguity.
- **Recurring with empty `weekdays`** → `next_fire_time` returns `None` (never
  fires). The UI should warn, but Rust must not panic.
- **Re-set clears the guard.** `set_schedule` nulls `last_fired` so editing a
  schedule to the same minute can fire again.
```
