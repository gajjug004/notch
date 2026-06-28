# Sticky Timer — Plan

Desktop sticky notes for Linux. Each note = a task with a timer and an optional schedule.
When the scheduled time hits, the task can auto-start (or one-click start) its timer.

## Stack

- **Tauri v2** (latest) — Rust backend + web frontend.
- Floating sticky notes = each note is its own OS window.
- Minimal UI.

## Core concept

Each note is a task with:
- **Timer** — both modes:
  - Countdown (pomodoro style — set 25min, counts down)
  - Stopwatch (track time spent — counts up)
- **Schedule** — when the task should start (clock time).
- **Trigger** — schedule time hit → notify + auto-start timer (or one-click start).

## Architecture

**Rust core = source of truth.** Timers/schedules must survive a note window closing.
Frontend is thin: display + send commands.

```
Rust backend                          Frontend (web, per-window)
├─ Task store (persist disk)          ├─ Sticky note UI (1 task/window)
├─ Scheduler loop (tick 1s)           │   ├─ editable text
│   ├─ check schedules                │   ├─ timer display
│   ├─ fire notification              │   └─ start/pause/reset btns
│   └─ auto-start timer               ├─ listens: tick events
├─ Timer engine (run/pause/reset)     └─ sends: commands (invoke)
└─ Tray icon (new note, quit)
```

### Data model

```
Task {
  id, title, content, color,
  window: { x, y, w, h },
  timer: { mode: "countdown"|"stopwatch",
           duration_secs, remaining_secs, elapsed_secs,
           state: "idle"|"running"|"paused" },
  schedule: { kind: "none"|"once"|"recurring",
              at: datetime, weekdays: [..], auto_start: bool },
}
```

## Tauri pieces needed

- **Multi-window** — create `WebviewWindow` per note at runtime.
- Window props: `decorations:false` (frameless), `alwaysOnTop:true`, `transparent`,
  custom drag region (`data-tauri-drag-region`).
- **tauri-plugin-store** — persist tasks as JSON (minimal; SQLite overkill).
- **tauri-plugin-notification** — desktop alerts on schedule hit.
- **tray icon** — built-in Tauri; new note / quit.
- **tauri-plugin-autostart** — launch on login (phase 5).

## Phases

| # | Goal | Deliver |
|---|------|---------|
| 0 | Scaffold | Tauri v2 project, runs empty |
| 1 | One note | frameless draggable always-on-top window, edit text, persist |
| 2 | Multi-note | create/delete, tray, restore positions on boot |
| 3 | Timer | both modes, Rust-driven ticks → UI, controls |
| 4 | Schedule | scheduler loop, notify, auto-start timer |
| 5 | Polish | sound alert, autostart, colors, settings |

## Phase details

### Phase 0 — Scaffold

Goal: empty Tauri v2 app builds + runs on Linux, vanilla TypeScript frontend (no UI framework), git initialized.

Tasks:
- `npm create tauri-app@latest` — vanilla TS frontend (no framework; minimal UI).
- Confirm Rust toolchain + Linux deps (`webkit2gtk`, `libappindicator`, `librsvg`).
- `npm run tauri dev` → blank window opens.
- Init git, add `.gitignore` (target/, node_modules/, dist/).

Files:
- `src-tauri/` (Rust), `src/` (web), `src-tauri/tauri.conf.json`.

Done when: dev build opens a window, no errors.

---

### Phase 1 — One note

Goal: single sticky note — frameless, draggable, always-on-top, editable, persists.

Tasks:
- Configure main window: `decorations:false`, `alwaysOnTop:true`, `transparent:true`,
  small default size, no menu.
- Note UI: colored card, `contenteditable` body + title input.
- Drag: top strip with `data-tauri-drag-region`.
- Persist: add `tauri-plugin-store`. Save `{title, content, color, window:{x,y,w,h}}`
  on edit (debounced) + on move/resize.
- Restore: on boot load store, apply text + window geometry.

Rust:
- Store wiring, command `save_task(task)` / `load_task() -> task`.

Done when: type text, move window, quit, relaunch → text + position restored.

---

### Phase 2 — Multi-note

Goal: many notes, create/delete, tray, restore all on boot.

Tasks:
- Task store = `Vec<Task>` keyed by id (uuid). Persist whole list.
- Command `create_task()` → new Task + spawn new `WebviewWindow` (label = task id).
- Each window loads its own task by id (pass id via window URL query or init script).
- Delete: button on note → `delete_task(id)`, close window, drop from store.
- Tray icon: menu → "New note", "Show all", "Quit".
- Boot: load all tasks, spawn one window each at saved geometry.

Rust:
- `create_task`, `delete_task`, `list_tasks`, window spawn helper.
- App state = `Mutex<HashMap<Id, Task>>` + store sync.

Done when: create 3 notes, move/edit each, quit, relaunch → all 3 return in place.
Delete removes window + persists.

---

### Phase 3 — Timer

Goal: per-note timer, both modes, Rust-driven ticks, controls.

Tasks:
- Timer config UI per note: pick mode (countdown/stopwatch), set duration (countdown).
- Controls: Start / Pause / Reset.
- Rust timer engine: per task track `state`, `remaining_secs` (countdown) /
  `elapsed_secs` (stopwatch).
- Single 1s async loop in Rust ticks all running timers, emits event
  `timer-tick {id, remaining, elapsed, state}`.
- Frontend listens, updates display only (no own clock — avoids drift).
- Countdown reaches 0 → state `done`, emit `timer-done {id}` (notify in phase 4).
- Persist timer state in task (survives restart; decide: resume or reset on boot).

Rust:
- `start_timer(id)`, `pause_timer(id)`, `reset_timer(id)`.
- Tick loop owns truth; persists periodically.

Done when: countdown + stopwatch both run, pause/reset work, display matches Rust,
closing/reopening note keeps timer running.

---

### Phase 4 — Schedule

Goal: schedule per note, scheduler fires, notify + auto-start timer.

Tasks:
- Schedule UI: kind (none/once/recurring), time picker, weekdays (if recurring),
  `auto_start` toggle.
- Scheduler in the existing 1s Rust loop: compare now vs each task's next fire time.
- Fire → desktop notification (`tauri-plugin-notification`) + bring note to front.
  If `auto_start` → call `start_timer(id)`; else notification action "Start".
- Recurring: after fire, compute next occurrence (next matching weekday/time).
- Once: after fire, set schedule kind back to none (or mark done).
- Handle missed fires (app was closed): on boot, skip past or fire once — decide.

Rust:
- `next_fire_time(schedule) -> Option<datetime>`.
- Scheduler check folded into tick loop.

Done when: set a note to fire in 1 min → notification + timer auto-starts.
Recurring re-arms for next day.

---

### Phase 5 — Polish

Goal: production feel.

Tasks:
- Sound alert on `timer-done` + on schedule fire (bundled audio or system sound).
- Autostart on login: `tauri-plugin-autostart` + settings toggle.
- Note colors: palette picker, persist per note.
- Settings window: defaults (countdown length, sound on/off, autostart),
  global pause.
- Minimize-to-tray instead of quit on window close.
- Package: `.deb` / AppImage via `tauri build`.

Done when: installable artifact, autostart works, sound + colors + settings persist.

## Decisions (LOCKED 2026-06-28)

1. **Timer location** — **Rust.** Source of truth; survives note window close; no clock drift.
2. **Recurrence scope** — **one-shot first**, recurring (weekdays/daily) as a ready-but-secondary
   extension in phase 4.
3. **Note ↔ schedule** — **note always visible**; schedule only fires notification + starts
   the timer (does not create/spawn the note).
