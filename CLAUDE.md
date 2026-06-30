# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Sticky Timer: a Linux desktop app — a single sticky-note window with a task list; each task has a timer/stopwatch and an optional schedule. Click a task to open its full-window detail editor. **Tauri v2** (Rust backend = source of truth) + vanilla TypeScript frontend (no framework). App id `com.stickytimer.app`.

See `plan.md` (design), `progress.md` (phase status / what works), `docs/` (per-phase specs).

## Commands

```bash
# Dev (Linux quirks baked in):
WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri dev
#   add GDK_BACKEND=x11 if transparent corners render black

npm run dev          # vite only (frontend, no Rust)
npm run build        # tsc typecheck + vite build → dist/
npm run tauri build  # package (.deb builds clean)

# Typecheck without emit:
npx tsc --noEmit          # frontend
cd src-tauri && cargo check   # backend
```

No test suite exists. "Clean" = `cargo check` + `tsc --noEmit` pass and app launches without panics. Drag, transparency, tray clicks, timer counting need a real (non-headless) host to verify.

**Notifications in dev (GNOME):** GNOME drops notification banners from any app with no installed `.desktop` entry, so timer/schedule notifications are silently suppressed under `npm run tauri dev` (the DBus call still fires; the shell ignores it). The notify app-name is the binary name `sticky-timer`, so install a matching entry once:

```bash
cat > ~/.local/share/applications/sticky-timer.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=Sticky Timer
Exec=sticky-timer
Icon=sticky-timer
Terminal=false
Categories=Utility;
X-GNOME-UsesNotifications=true
EOF
update-desktop-database ~/.local/share/applications 2>/dev/null
```

The packaged `.deb` already ships `sticky-timer.desktop` (matches `productName=sticky-timer`), so installed builds show notifications without this step.

Packaging: `.deb` works. AppImage config present but its bundler (`linuxdeploy`) needs FUSE — fails in sandbox, builds on a real host.

## Architecture

**Rust owns all state.** Timers and schedules run in Rust so they survive window close and don't drift. The frontend renders state and sends edits; it never holds authoritative timer/schedule values.

- **State**: `Mutex<HashMap<Id, Task>>` in `AppState` (`state.rs`), mirrored to `~/.local/share/com.stickytimer.app/tasks.json` via `persist()`.
- **Single window, list/detail SPA**: one frameless/transparent/always-on-top window labeled `"main"` (`window.rs:MAIN_LABEL`, `open_main_window`). The frontend (`main.ts`) swaps between a **list view** (one row per task, live timer + `⏰` schedule badge) and a **detail view** in the same window — no per-task OS windows.
- **Minimal detail view** (fixed yellow, no color palette): hero clock that's **click-to-edit** when idle+countdown (`#timer-display` button ⇄ `#dur-input`), a `start`⇄`pause` toggle + `reset`, and a **collapsed schedule**: a thin trigger row opens a floating popover (`#sched-pop`, absolute over the notes) with quick presets (15m/30m/1h/3h/tonight/tmrw 10a) and a custom date+time; presets apply instantly, custom/recurring use **Set schedule**.
- **Event routing**: with one window, Rust `emit`s events **globally** and the frontend routes by the `id` in each payload (`timer-tick`, `timer-done`, `play-sound`, `schedule-fired`). `tasks-changed` (create/delete/edit) tells the list to refresh.
- **Single tick loop** (`tick.rs`): one 1s heartbeat drives *all* timers and schedules. On schedule fire → desktop notification + bring the `"main"` window front + auto-start timer. On countdown done → `timer-done` event + flash.
- **Boot sequence** (`lib.rs` setup): load tasks → running timers come back **paused** → open the `"main"` window → tray → `reconcile_on_boot` (one-shot schedules overdue ≤5min still fire, stale ones skipped) → start tick loop.
- **Tray keeps app alive**: closing/hiding the `"main"` window *hides* it (the in-app × calls `appWindow.hide()`); only tray Quit exits. Tray: New task / Show window / Settings / Quit.
- **Settings window**: separate page (`settings.html` / `settings.ts`), label `"settings"` — default countdown, **schedule quick presets** (`schedulePresetMins` minute chips + `tonightTime` / `tomorrowTime`), sound, autostart, global pause. The schedule popover reads these from the store on open (`schedule.ts:refreshPresets`). Global pause freezes all timers, persisted.

Frontend↔backend contract: TS calls `invoke(...)` for commands; Rust pushes `listen`-able events (above + `global-pause`). Keep `src/types.ts` in sync with `task.rs` / `timer.rs` / `schedule.rs` serde shapes — they are mirrored by hand. **Rust owns the timer and schedule**: `save_task` (the detail view's debounced text/geometry save) preserves the existing `timer` *and* `schedule` from state — they're only mutated via `configure_timer`/timer cmds and `set_schedule`, never by a `save_task` payload (else a stale payload clobbers them). `set_task_color` exists but the palette is gone, so the frontend no longer calls it; `Task.color` stays the default yellow.

### Backend (`src-tauri/src/`)
- `lib.rs` — builder wiring + setup sequence; `invoke_handler` lists all commands
- `commands.rs` — create/delete/list/get/save_task, start/pause/reset/configure_timer, set_schedule, set_task_color, open_settings, pause_all/resume_all
- `task.rs` — `Task` + `Geometry`; null-tolerant `timer` deserialize
- `state.rs` — `AppState`, `persist()`, `load_into_state()`
- `timer.rs` — `Timer`/`TimerMode`/`TimerState`/`RunAnchor`
- `schedule.rs` — `Schedule`/`ScheduleKind` + `next_fire_time`/`parse_local` (chrono Local)
- `tick.rs` — the heartbeat; `fire_schedule`, `reconcile_on_boot`
- `window.rs` — `open_main_window()` / `open_settings()` (idempotent) + `MAIN_LABEL`
- `tray.rs` — tray menu

### Frontend (`src/`)
- `main.ts` — app shell/router: `renderList()` (list view), `openDetail(id)` / `showList()`, `+ New` / delete, and all global event listeners (registered once)
- `timer.ts` / `schedule.ts` — controllers wired **once** (`setupTimer`/`setupSchedule`) then re-pointed per task (`loadTimer`/`loadSchedule`, `unload*`); export render/helper fns (`renderTimerTick`, `fmt`, `scheduleBadge`, `onScheduleFired`). The single window lives the whole app life, so no `UnlistenFn` teardown.
- `sound.ts` — `initSound()` + `playAlert()` (bundled chime via asset protocol, respects `soundOn`)
- `settings.ts` / `settings.css` — settings window
- `types.ts` — TS mirror of Rust serde types

Multi-page Vite build: `index.html` (list + detail) + `settings.html` are both rollup inputs (`vite.config.ts`). Vite fixed port 1420, strict.

## Conventions

- TS is `strict` with `noUnusedLocals`/`noUnusedParameters` — unused bindings fail the build.
- Saves are debounced (~400ms) and flushed on window close; geometry stored in logical px.
- When changing a serialized field on either side, update both the Rust struct and `types.ts`.
