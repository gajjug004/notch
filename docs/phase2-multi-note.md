# Phase 2 — Multi-note (implementation spec)

> Status target: many notes, each its own OS window; create/delete; system tray;
> restore **all** notes on boot at their saved geometry.
> Assumes **Phase 1 is done** (single frameless, draggable, always-on-top note that
> persists its text + geometry through `tauri-plugin-store`).

All APIs below are **Tauri v2** (verified against the v2 docs). Treat **Rust as the
source of truth**; the frontend is a thin per-window view of one `Task`.

---

## 0. Outcome / "Done when"

- Click tray → **New note** three times → 3 separate windows appear.
- Edit text + move/resize each window.
- **Quit** (tray → Quit).
- **Relaunch** → all 3 notes return, with their text **and** at their saved
  `{x, y, w, h}`.
- Click the delete button on a note → its window closes **and** it is gone from the
  store on next launch (other notes untouched).

---

## 1. Dependencies & features

### `src-tauri/Cargo.toml`

```toml
[dependencies]
tauri = { version = "2", features = ["tray-icon"] }   # tray-icon REQUIRED for system tray
tauri-plugin-opener = "2"                              # already present from scaffold
tauri-plugin-store = "2"                               # task persistence
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }            # note ids
```

Notes:
- The system tray is **gated behind the `tray-icon` Cargo feature** on the `tauri`
  crate. Without it, `tauri::tray::*` will not compile.
- `uuid` v4 gives random ids; `Uuid::new_v4().to_string()` is hyphenated lowercase
  (e.g. `9b2c...-...`), which is a **legal Tauri window label** (see §8 gotchas).

### Frontend (`package.json`)

```bash
npm add @tauri-apps/plugin-store
```

(`@tauri-apps/api` is already present from the scaffold; we use `core`, `window`,
and `event` from it.)

---

## 2. Data model (Rust)

Define the `Task` once, in Rust, and serialize it straight to the store. Add the
timer/schedule fields **now** behind `#[serde(default)]` so Phases 3–4 can populate
them without a store migration (forward-compat).

`src-tauri/src/task.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Default for Geometry {
    fn default() -> Self {
        // Cascade is applied at create time; this is just a safe fallback.
        Geometry { x: 120, y: 120, w: 260, h: 260 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,                 // uuid v4, hyphenated; == window label
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub window: Geometry,

    // ---- Forward-compat for Phases 3 (timer) & 4 (schedule). ----
    // Present in the struct so old store files (no these keys) still
    // deserialize, and new writes start carrying them.
    #[serde(default)]
    pub timer: Option<serde_json::Value>,    // becomes a typed Timer in Phase 3
    #[serde(default)]
    pub schedule: Option<serde_json::Value>, // becomes a typed Schedule in Phase 4
}

fn default_color() -> String {
    "#fff7b0".to_string() // sticky yellow
}

impl Task {
    pub fn new() -> Self {
        Task {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            content: String::new(),
            color: default_color(),
            window: Geometry::default(),
            timer: None,
            schedule: None,
        }
    }
}
```

> Why `Option<serde_json::Value>` and not the real types yet: it keeps Phase 2
> self-contained while guaranteeing that adding the real `Timer`/`Schedule` structs
> in later phases is a non-breaking change. If you prefer, define the real structs
> now and keep `#[serde(default)]` — the forward-compat property is the same. The
> rule that matters: **every field added later must carry `#[serde(default)]`.**

---

## 3. App state: `Mutex<HashMap<Id, Task>>` synced to the store

State lives in memory as the working copy; the store is the durable mirror. Every
mutation: lock → mutate map → `persist()` (writes the whole map to the store) →
unlock.

`src-tauri/src/state.rs`:

```rust
use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Runtime, Manager};
use tauri_plugin_store::StoreExt;

use crate::task::Task;

pub const STORE_FILE: &str = "tasks.json";
pub const STORE_KEY: &str = "tasks"; // single key holding the whole map

#[derive(Default)]
pub struct AppState {
    // Mutex (not async): held only for tiny, synchronous critical sections.
    pub tasks: Mutex<HashMap<String, Task>>,
}

/// Write the entire in-memory map to the store and flush to disk.
/// Call this while you HOLD the lock (pass the guard's contents), or re-lock
/// briefly inside. Here we take a snapshot to keep the lock scope tiny.
pub fn persist<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    // 1) snapshot under lock, then drop the guard before touching the store.
    let snapshot: HashMap<String, Task> = {
        let state = app.state::<AppState>();
        let guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.clone()
    }; // guard dropped here

    // 2) store I/O happens with NO lock held.
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(STORE_KEY, serde_json::to_value(&snapshot).map_err(|e| e.to_string())?);
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// Load the map from the store into memory at boot.
pub fn load_into_state<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let map: HashMap<String, Task> = match store.get(STORE_KEY) {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string())?,
        None => HashMap::new(),
    };
    let state = app.state::<AppState>();
    *state.tasks.lock().map_err(|e| e.to_string())? = map;
    Ok(())
}
```

`.manage()` the state in the builder (see §7).

**Mutex discipline (critical — see §8):** never `.await` while holding the guard,
and never do store/disk I/O while holding it. `persist()` above deliberately clones
a snapshot under the lock, drops the guard, then writes. This avoids both deadlocks
and holding the lock across slow I/O.

---

## 4. Multi-window: spawn one `WebviewWindow` per task

A note window's **label is the task id**. Spawning is **idempotent**: if a window
with that label already exists, focus it instead of creating a duplicate.

`src-tauri/src/window.rs`:

```rust
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::task::Task;

/// Open (or focus) the window for a task, positioned at its saved geometry.
pub fn open_task_window<R: Runtime>(app: &AppHandle<R>, task: &Task) -> Result<(), String> {
    // Idempotent: focus an existing window for this id.
    if let Some(win) = app.get_webview_window(&task.id) {
        let _ = win.set_focus();
        return Ok(());
    }

    // index.html is the per-note view; the id is passed via query string.
    let url = WebviewUrl::App(format!("index.html?id={}", task.id).into());

    let win = WebviewWindowBuilder::new(app, &task.id /* label == id */, url)
        .title("note")
        .inner_size(task.window.w as f64, task.window.h as f64)
        .position(task.window.x as f64, task.window.y as f64)
        .decorations(false)     // frameless (Phase 1 style)
        .always_on_top(true)
        .transparent(true)
        .resizable(true)
        .skip_taskbar(true)     // sticky notes shouldn't clutter the taskbar
        .build()
        .map_err(|e| e.to_string())?;

    let _ = win.show();
    Ok(())
}
```

Notes:
- `WebviewWindowBuilder::new(app, label, url)` — `app` is anything implementing
  `Manager` (the `AppHandle` works). The **label is the id**; it must be unique and
  stable.
- `get_webview_window(label)` is the idempotency check. Use it before every spawn.
- Geometry uses **logical** units via the `f64` builder methods. If you stored
  physical pixels in Phase 1, stay consistent; logical is recommended so it behaves
  across HiDPI.
- `transparent(true)` on Linux requires the window to be created transparent at
  build time (it is, here) — you cannot toggle it later.

---

## 5. Passing the task id to each window

Two complementary mechanisms; use both.

### 5a. Primary: URL query string (`?id=...`)

Set in `open_task_window` above (`index.html?id=<uuid>`). The frontend reads it:

```ts
// src/main.ts (per-window bootstrap)
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

function resolveTaskId(): string {
  // Preferred: query string set by the Rust spawner.
  const fromQuery = new URLSearchParams(window.location.search).get("id");
  if (fromQuery) return fromQuery;
  // Fallback / shortcut: the window label IS the id (see 5b).
  return getCurrentWindow().label;
}

const taskId = resolveTaskId();
const task = await invoke<Task>("get_task", { id: taskId });
// ...render task into the note UI...
```

### 5b. Shortcut: `label == id`

Because we set the window **label to the task id**, the frontend can always recover
its id with `getCurrentWindow().label` even if the URL is missing the query (e.g.
after an in-app navigation). Keep the query string as the primary path because it is
explicit and survives a dev-server reload at the document level; use the label as a
robust fallback.

`Task` TS type (mirror of the Rust struct):

```ts
interface Geometry { x: number; y: number; w: number; h: number; }
interface Task {
  id: string;
  title: string;
  content: string;
  color: string;
  window: Geometry;
  timer?: unknown;     // Phase 3
  schedule?: unknown;  // Phase 4
}
```

---

## 6. Rust commands

`src-tauri/src/commands.rs`:

```rust
use tauri::{AppHandle, Manager, Runtime};

use crate::state::{persist, AppState};
use crate::task::Task;
use crate::window::open_task_window;

/// Create a new task, store it, and spawn its window.
#[tauri::command]
pub fn create_task<R: Runtime>(app: AppHandle<R>) -> Result<Task, String> {
    let mut task = Task::new();

    // Optional: cascade new windows so they don't stack exactly.
    {
        let state = app.state::<AppState>();
        let guard = state.tasks.lock().map_err(|e| e.to_string())?;
        let n = guard.len() as i32;
        task.window.x = 120 + (n % 8) * 28;
        task.window.y = 120 + (n % 8) * 28;
    } // drop guard before any further work

    // Insert under lock, then drop guard before I/O / window work.
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.insert(task.id.clone(), task.clone());
    }

    persist(&app)?;            // no lock held
    open_task_window(&app, &task)?;
    Ok(task)
}

/// Delete a task: destroy its window, drop from map, persist.
#[tauri::command]
pub fn delete_task<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    // Destroy the window first (ignore if already gone).
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

/// All tasks (e.g. for a future "list" UI; also used by tray "Show all").
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
    guard.get(&id).cloned().ok_or_else(|| format!("no task {id}"))
}

/// Save edits (text/color/geometry) from a window. Whole-task upsert.
#[tauri::command]
pub fn save_task<R: Runtime>(app: AppHandle<R>, task: Task) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.insert(task.id.clone(), task);
    }
    persist(&app)?;
    Ok(())
}
```

Frontend call sites:

```ts
import { invoke } from "@tauri-apps/api/core";

await invoke("create_task");                 // tray "New note" or in-app button
await invoke("delete_task", { id: taskId }); // note's delete button
await invoke("save_task",   { task });       // debounced on edit + on move/resize
```

> **Geometry persistence:** keep Phase 1's behavior — each window listens to its own
> move/resize events (`getCurrentWindow().onMoved` / `.onResized`), updates
> `task.window`, and calls `save_task` (debounced). This is what makes "restore at
> saved geometry" work in §9.

---

## 7. Builder wiring (`lib.rs`)

`src-tauri/src/lib.rs`:

```rust
mod commands;
mod state;
mod task;
mod tray;
mod window;

use state::{load_into_state, AppState};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle();

            // 1) Load persisted tasks into memory.
            load_into_state(handle).map_err(|e| e.to_string())?;

            // 2) Restore: open a window per task at its saved geometry.
            {
                let state = app.state::<AppState>();
                let tasks: Vec<task::Task> = state
                    .tasks
                    .lock()
                    .map_err(|e| e.to_string())?
                    .values()
                    .cloned()
                    .collect(); // snapshot, drop lock before spawning windows
                for t in &tasks {
                    window::open_task_window(handle, t).map_err(|e| e.to_string())?;
                }
            }

            // 3) System tray.
            tray::build_tray(handle)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_task,
            commands::delete_task,
            commands::list_tasks,
            commands::get_task,
            commands::save_task,
        ])
        // 4) Don't quit when the last note window closes; only the tray Quit exits.
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                // Keep the app alive in the tray after windows close.
                api.prevent_exit();
            }
        });
}
```

> Note the switch from `.run(generate_context!())` to
> `.build(...)?.run(|app, event| ...)`. The closure gives access to `RunEvent`,
> which is how we intercept `ExitRequested` (§8).

---

## 8. System tray (Tauri v2)

`src-tauri/src/tray.rs`:

```rust
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

use crate::commands;
use crate::state::AppState;

pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let new_i = MenuItem::with_id(app, "new", "New note", true, None::<&str>)?;
    let show_i = MenuItem::with_id(app, "show_all", "Show all", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&new_i, &show_i, &quit_i])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(true) // Linux: rely on the menu, not click events
        .on_menu_event(|app, event| match event.id.as_ref() {
            "new" => {
                let _ = commands::create_task(app.clone());
            }
            "show_all" => {
                // Bring every existing note window to the front.
                for (_label, win) in app.webview_windows() {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
                // (Optional) re-open any task whose window was destroyed.
                let _ = show_all_missing(app);
            }
            "quit" => {
                app.exit(0); // bypasses prevent_exit; this is the real quit
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn show_all_missing<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let tasks: Vec<crate::task::Task> = {
        let state = app.state::<AppState>();
        let guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.values().cloned().collect()
    };
    for t in &tasks {
        crate::window::open_task_window(app, t)?;
    }
    Ok(())
}
```

Tauri v2 tray facts (verified):
- API lives in `tauri::tray::TrayIconBuilder`; menu types in `tauri::menu::{Menu, MenuItem}`.
- Menu clicks handled by `.on_menu_event(|app, event| ...)`, matched on
  `event.id.as_ref()` against the ids you gave `MenuItem::with_id`.
- **Requires the `tray-icon` Cargo feature** (§1).
- **Linux caveat (AppIndicator):** on Linux the tray is rendered through
  **libappindicator** (system dep `libayatana-appindicator3` /
  `libappindicator3`). It must be installed on the dev/build machine and declared
  as a runtime dependency in the bundle. Also, **`on_tray_icon_event` clicks are
  not emitted on Linux** ("the event is not emitted even though the icon is
  shown") — the **menu still works**, so put all actions in the menu rather than
  relying on left/right-click events. We set `show_menu_on_left_click(true)` for
  that reason.

---

## 9. Boot / restore flow (summary)

Order, in `setup` (§7):

1. `load_into_state` — read `tasks.json` → `Mutex<HashMap<…>>`. Empty map if the
   file/key is absent (first run).
2. For each task, `open_task_window` at its saved `{x,y,w,h}`.
3. Build the tray.
4. Install `ExitRequested → prevent_exit` so closing windows leaves the app in the
   tray.

Per-window runtime (frontend, unchanged from Phase 1 but now id-aware):
- On load: resolve id (§5) → `get_task` → render.
- On edit (debounced) and on move/resize: update local `task` → `save_task`.

This is exactly what satisfies the relaunch test: geometry and text were persisted
on every change, and boot replays them.

---

## 10. Capabilities

Per-window capabilities must apply to the **dynamic note windows** whose labels are
uuids. The Phase 1 capability targeted `"windows": ["main"]`; widen it to all note
windows. Tauri capability `windows` supports glob patterns, so a permissive pattern
covers uuid labels.

`src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for note windows + store + tray",
  "windows": ["*"],
  "permissions": [
    "core:default",
    "opener:default",
    "core:window:allow-set-focus",
    "core:window:allow-show",
    "core:window:allow-set-position",
    "core:window:allow-set-size",
    "core:window:allow-start-dragging",
    "core:event:default",
    "store:default"
  ]
}
```

Notes:
- `"windows": ["*"]` matches every label, including the hyphenated uuids. If you
  prefer scoping, note labels are uuids with no shared prefix, so a glob like
  `"note-*"` would require prefixing labels (e.g. label = `note-<uuid>`); plain
  `"*"` is simplest here.
- `store:default` is the store plugin's permission set (needed even though most
  store calls are Rust-side, in case the frontend uses `@tauri-apps/plugin-store`).
- `core:window:allow-start-dragging` keeps the Phase 1 `data-tauri-drag-region`
  working on the new windows.
- Tray/menu are created in Rust and need no frontend capability.

---

## 11. Removing the static main window

Phase 1 declared a static `"main"` window in `tauri.conf.json`. In Phase 2 windows
are created **at runtime**, so:

- **Delete the `windows` array** (or set it to `[]`) in `tauri.conf.json` so no
  empty `main` window opens on launch.

```jsonc
// src-tauri/tauri.conf.json  (app section)
"app": {
  "withGlobalTauri": true,
  "windows": [],            // was: a single "main" window
  "security": { "csp": null }
}
```

- First run then has **zero** notes and **zero** windows — only the tray. That is
  intentional; the user creates the first note via tray → **New note**. (Optional
  nicety: if `load_into_state` yields an empty map on first launch, call
  `create_task` once in `setup` so the user isn't staring at an empty desktop.)
- Drop the old `"main"`-scoped capability assumption (handled in §10).

---

## 12. Gotchas checklist

- **Window label charset:** labels may only contain alphanumerics, `-`, `/`, `:`,
  `_`. `Uuid::new_v4().to_string()` is hyphenated lowercase hex → **valid**. Do not
  wrap ids in arbitrary characters when used as labels.
- **Idempotent spawn:** always `get_webview_window(&id)` first; on hit, `set_focus`
  and return. Prevents duplicate windows for one task (e.g. tray "Show all" while a
  note is already open).
- **Last-window-vs-quit:** by default Tauri exits when the last window closes.
  Intercept `RunEvent::ExitRequested { api, .. }` and call `api.prevent_exit()` so
  the app survives in the tray. The tray **Quit** uses `app.exit(0)`, which is a
  hard exit and is **not** blocked by `prevent_exit`. Closing a single note window
  must **destroy** that window (`win.destroy()`), not hide it, when the user deletes
  the task — but note that the *window close button* is disabled here (frameless),
  so deletion goes exclusively through `delete_task`.
- **Mutex discipline:**
  - Never hold the `tasks` guard across `.await` (the lock is a std `Mutex`, not
    async; awaiting under it can deadlock the runtime and is a logic hazard).
  - Never do store/disk I/O while the guard is held — snapshot (`clone`) under the
    lock, drop the guard, then persist (see `persist()` in §3).
  - **Poisoning:** if a thread panics while holding the lock, `.lock()` returns
    `Err(PoisonError)`. We map it to a `String` error (`map_err(|e| e.to_string())`)
    so a single panic doesn't cascade into `unwrap()` crashes. Don't `.unwrap()` the
    lock in command handlers.
- **libappindicator on Linux:** install `libayatana-appindicator3-dev` (or distro
  equivalent) for dev; ensure it is in the bundle's Debian `depends`. Without it the
  tray silently fails to appear.
- **Tray click events on Linux are not emitted** — wire all actions through the
  **menu** (`on_menu_event`), not `on_tray_icon_event`.
- **Forward-compat fields:** `timer` and `schedule` (and any field added in Phases
  3–5) **must** carry `#[serde(default)]` so older `tasks.json` files keep
  deserializing after upgrades. Adding a non-default field is a breaking store
  change.
- **Geometry units:** stay consistent (logical recommended). Mixing logical and
  physical between save and restore causes notes to drift/resize on HiDPI.
- **`transparent` on Linux** must be set at build time; it cannot be toggled after
  the window exists.
- **Store path consistency:** always open the store with the **same file name**
  (`tasks.json`) everywhere; `app.store("tasks.json")` returns the shared
  `Arc<Store>` instance, so reads and writes hit the same resource.

---

## 13. File map (new/changed in Phase 2)

```
src-tauri/Cargo.toml          # + tray-icon feature, tauri-plugin-store, uuid
src-tauri/src/lib.rs          # builder wiring, setup restore, RunEvent hook
src-tauri/src/task.rs         # Task + Geometry (forward-compat fields)
src-tauri/src/state.rs        # AppState, persist(), load_into_state()
src-tauri/src/window.rs       # open_task_window() (idempotent spawn)
src-tauri/src/commands.rs     # create/delete/list/get/save_task
src-tauri/src/tray.rs         # build_tray()
src-tauri/tauri.conf.json     # windows: [] (remove static main)
src-tauri/capabilities/default.json   # windows:["*"] + store/window perms
src/main.ts                   # resolve id, get_task, render, save_task on change
index.html                    # per-note view (1 task/window)
package.json                  # + @tauri-apps/plugin-store
```

---

## 14. Manual test script (the "Done when")

1. `npm run tauri dev`. Expect: tray icon appears, no windows (fresh store).
2. Tray → **New note** ×3 → three cascaded notes appear.
3. Type distinct text in each; drag each to a different spot; resize one.
4. Tray → **Quit**.
5. `npm run tauri dev` again → all three notes reappear with the same text and at
   their saved positions/sizes.
6. On one note, click its delete button (`delete_task`) → window vanishes
   immediately. Quit + relaunch → that note does **not** return; the other two do.
7. Close a note's window via the system (if reachable) → app stays alive in tray
   (prevent_exit); tray → **Show all** re-focuses/re-opens it.
