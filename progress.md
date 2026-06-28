# Sticky Timer — Progress

Last updated: 2026-06-28

Linux desktop sticky notes, each a task with a timer + (soon) schedule.
Stack: **Tauri v2** (Rust backend = source of truth) + vanilla TS frontend.
See [plan.md](plan.md) for the design and [docs/](docs/) for per-phase specs.

## Environment

- Ubuntu 24.04, Rust 1.96 (rustup), Node 24, webkit2gtk 4.1 (2.52.3)
- App identifier: `com.stickytimer.app`
- Store dir: `~/.local/share/com.stickytimer.app/` (`tasks.json`)
- Run dev: `WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri dev`
  (add `GDK_BACKEND=x11` if transparent corners render black)

## Decisions (locked)

1. Timer lives in **Rust** (survives window close, no drift).
2. Schedule: **one-shot first**, recurring as secondary extension.
3. Note always visible; schedule only notifies + starts the timer.

## Status by phase

| # | Phase | State | Commit |
|---|-------|-------|--------|
| 0 | Scaffold | ✅ done | `4b43256` |
| 1 | One note | ✅ done | `c854397` |
| 2 | Multi-note + tray | ✅ done | `db5e585` |
| 3 | Timer | ✅ done | `9800379` |
| 4 | Schedule + notifications | ⬜ next | — |
| 5 | Polish (sound/autostart/colors/settings/packaging) | ⬜ todo | — |

Each ✅ phase: `cargo check` + `tsc --noEmit` clean, builds and launches
without panics. Visual/interaction acceptance left to manual run (headless
can't verify drag, transparency, tray clicks, timer counting).

## What works now (phases 0–3)

- Frameless, transparent, always-on-top sticky-note windows; drag strip.
- Multi-note: each note its own OS window (label == uuid task id).
- Rust-owned task state (`Mutex<HashMap<Id,Task>>`) mirrored to `tasks.json`.
- Tray: New note / Show all / Quit. App stays alive in tray after windows close.
- Per-note timer: countdown + stopwatch, single Rust tick loop (drift-free),
  Start/Pause/Reset/configure, countdown done → flash + `timer-done` event.
- Persistence: text, color, geometry, timer numbers survive restart;
  running timers boot **paused**.

## Backend layout (`src-tauri/src/`)

- `task.rs` — `Task` + `Geometry`; null-tolerant `timer` deserialize
- `state.rs` — `AppState`, `persist()`, `load_into_state()`
- `window.rs` — `open_task_window()` (idempotent spawn)
- `commands.rs` — create/delete/list/get/save_task + start/pause/reset/configure_timer
- `timer.rs` — `Timer` / `TimerMode` / `TimerState` / `RunAnchor`
- `tick.rs` — single 1s heartbeat for all timers
- `tray.rs` — tray menu
- `lib.rs` — builder wiring, setup (load → boot-paused → restore windows → tray → tick loop)

## Frontend layout (`src/`)

- `main.ts` — per-window bootstrap: resolve id → get_task → render → save on edit/move
- `timer.ts` — timer UI + listens `timer-tick` / `timer-done`
- `types.ts` — `Task` / `Timer` mirrors of Rust
- `styles.css`, `index.html` — sticky-note card + timer section

## Known notes / gotchas

- GNOME hides legacy tray icons — may need an AppIndicator extension to see the tray.
- `-f`/`--force` scaffolders overwrite non-empty dirs (lost plan.md/docs once; restored).
- Any new `Task` field must carry `#[serde(default)]` to keep old stores loading.

## Next: Phase 4 — Schedule

- `Schedule` on `Task` (kind once/recurring, RFC3339 `at`, weekdays, auto_start, last_fired)
- Fold schedule check into the existing 1s tick loop; `next_fire_time` via chrono
- Fire → `tauri-plugin-notification` + bring note front + auto-start timer (or one-click)
- One-shot first; recurring re-arm secondary. See [docs/phase4-schedule.md](docs/phase4-schedule.md).
