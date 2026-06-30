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
| 4 | Schedule + notifications | ✅ done | `fd914ce` |
| 5 | Polish (sound/autostart/colors/settings/packaging) | ✅ done | (this commit) |
| 6 | Single-window list/detail refactor | ✅ done | (pending) |

**Phases 0–6 complete.**

## Phase 6 — single-window list/detail (UX overhaul)

Dropped the one-OS-window-per-task model. Now **one** frameless/transparent/
always-on-top window (label `"main"`) that swaps between:
- **List view**: a row per task with live timer/stopwatch + schedule badge,
  a `+ New task` button, hide-to-tray ×.
- **Detail view**: the former note layout (title, palette, timer, schedule,
  body) + `← Back` and a 🗑 delete button.

Backend: `window.rs` → `open_main_window` (+ `MAIN_LABEL`); tick/commands emit
events **globally** (frontend routes by payload `id`) instead of `emit_to(label)`;
new `tasks-changed` event refreshes the list; `lib.rs` opens one window and
hides-to-tray on the `"main"` label; tray = New task / Show window / Settings /
Quit. `Task.window` geometry field retained but unused.

Frontend: `main.ts` is now the shell/router; `timer.ts`/`schedule.ts` became
setup-once + `load(task)` controllers (no per-task re-init / unlisten juggling).

`cargo check` + `tsc --noEmit` + `npm run build` clean. Visual/interaction
acceptance still needs a manual run.

## Phase 7 — minimal task view + drop color palette

- Removed the color/theme palette entirely (HTML + `buildPalette`/`applyColor`
  in `main.ts` + `.palette`/`.swatch` CSS + list row color accent). Window stays
  the fixed `:root` yellow. Backend `set_task_color` kept but now unused;
  `Task.color` retained, defaults yellow.
- Click-to-edit clock: dropped the separate duration field; `#timer-display` is a
  button — clicking it while idle+countdown swaps in `#dur-input` to retype the
  time (`timer.ts` open/closeEdit + `applyConfig`).
- Start ⇄ Pause is now one `#btn-toggle` (+ `#btn-reset`); `renderTimerTick`
  sets its label/`running` class.
- Schedule collapses: everything but the kind selector lives in `#sched-body`,
  hidden until a kind is picked (`schedule.ts syncVisibility`).
- Restyled minimal: hero clock, hairline rules, ghost buttons, more whitespace,
  lowercase captions. `tsc` + `npm run build` clean.

Each ✅ phase: `cargo check` + `tsc --noEmit` clean, builds and launches
without panics. Visual/interaction acceptance left to manual run (headless
can't verify drag, transparency, tray clicks, timer counting).

## What works now (phases 0–5)

- Sound chime on timer-done + schedule fire (frontend Audio, gated per note,
  respects soundOn). Bundled `src-tauri/sounds/alert.ogg` via asset protocol.
- Autostart on login (`tauri-plugin-autostart`), toggled in Settings.
- Note color palette (7 swatches), persisted per note.
- Settings window (default countdown, sound, autostart, global pause).
- Minimize-to-tray: closing a note hides it; only tray Quit exits.
- Global pause: freezes all running timers, preserved across restart.
- Packaging: `.deb` builds clean. AppImage config present but its bundler
  (`linuxdeploy`) needs FUSE — fails in this sandbox, builds on a real host.

## What works now (phases 0–4)

- Frameless, transparent, always-on-top sticky-note windows; drag strip.
- Multi-note: each note its own OS window (label == uuid task id).
- Rust-owned task state (`Mutex<HashMap<Id,Task>>`) mirrored to `tasks.json`.
- Tray: New note / Show all / Quit. App stays alive in tray after windows close.
- Per-note schedule (once + recurring): folded into the tick loop; on fire →
  desktop notification + bring note front + auto-start timer (or one-click
  Start button). Boot reconcile honors one-shots overdue ≤5 min, skips stale.
- Per-note timer: countdown + stopwatch, single Rust tick loop (drift-free),
  Start/Pause/Reset/configure, countdown done → flash + `timer-done` event.
- Persistence: text, color, geometry, timer numbers survive restart;
  running timers boot **paused**.

## Backend layout (`src-tauri/src/`)

- `task.rs` — `Task` + `Geometry`; null-tolerant `timer` deserialize
- `state.rs` — `AppState`, `persist()`, `load_into_state()`
- `window.rs` — `open_task_window()` (idempotent spawn)
- `commands.rs` — create/delete/list/get/save_task + start/pause/reset/configure_timer + set_schedule
- `timer.rs` — `Timer` / `TimerMode` / `TimerState` / `RunAnchor`
- `schedule.rs` — `Schedule` / `ScheduleKind` + `next_fire_time` / `parse_local` (chrono Local)
- `tick.rs` — single 1s heartbeat for all timers + schedules; `fire_schedule`, `reconcile_on_boot`
- `tray.rs` — tray menu
- `lib.rs` — builder wiring, setup (load → boot-paused → restore windows → tray → reconcile → tick loop)

## Frontend layout (`src/`)

- `main.ts` — per-window bootstrap: resolve id → get_task → render → save on edit/move
- `timer.ts` — timer UI + listens `timer-tick` / `timer-done`
- `schedule.ts` — schedule UI + `set_schedule`, notification permission, listens `schedule-fired`
- `types.ts` — `Task` / `Timer` / `Schedule` mirrors of Rust
- `styles.css`, `index.html` — sticky-note card + timer + schedule sections

## Known notes / gotchas

- GNOME hides legacy tray icons — may need an AppIndicator extension to see the tray.
- `-f`/`--force` scaffolders overwrite non-empty dirs (lost plan.md/docs once; restored).
- Any new `Task` field must carry `#[serde(default)]` to keep old stores loading.

## Build

```
WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri build
```
→ `src-tauri/target/release/bundle/deb/sticky-timer_0.1.0_amd64.deb`.
AppImage needs FUSE on the build host (`sudo apt install libfuse2`), else
run the linuxdeploy step on a machine with FUSE.

## Possible follow-ups (not in original plan)

- App icon (still the Tauri default).
- Schedule UI polish; per-note sound choice; pause indicator styling.
- CI to build artifacts on a FUSE-enabled runner.
