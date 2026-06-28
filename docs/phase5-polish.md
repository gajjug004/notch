# Phase 5 — Polish (Implementation Spec)

Production feel for **sticky-timer** (Tauri v2, Linux, vanilla TS, Rust = source of truth).
Assumes Phases 0–4 are complete: multi-window sticky notes, `tauri-plugin-store`,
`tauri-plugin-notification`, a single 1s Rust tick loop emitting `timer-tick` /
`timer-done`, a scheduler folded into that loop, and a tray icon.

App identifier (already in `tauri.conf.json`): **`com.stickytimer.app`**.
Product name: **`sticky-timer`**. Keep both stable for the rest of this phase — the
autostart `.desktop` filename is derived from the identifier (see Gotchas).

Phase 5 ships six features:

1. Sound alerts (timer-done + schedule fire)
2. Autostart on login
3. Note colors
4. Settings window
5. Minimize-to-tray (close ≠ quit)
6. Global pause
7. Packaging to `.deb` + AppImage

All settings persist; all features survive restart.

---

## 0. Dependencies & versions

All Tauri v2 packages are pinned to major `2` (npm `^2`, Cargo `"2"`). Verified
package names:

### Cargo (`src-tauri/Cargo.toml`)

```toml
[dependencies]
tauri = { version = "2", features = ["tray-icon", "image-png"] }
tauri-plugin-opener = "2"
tauri-plugin-store = "2"            # phase 1
tauri-plugin-notification = "2"     # phase 4
tauri-plugin-autostart = "2"        # NEW phase 5
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

> `tray-icon` + `image-png` features on `tauri` are needed for the tray (already
> required since Phase 2; listed here for completeness). No `rodio` — see §1.6.

### npm (`package.json`)

```jsonc
"dependencies": {
  "@tauri-apps/api": "^2",
  "@tauri-apps/plugin-opener": "^2",
  "@tauri-apps/plugin-store": "^2",
  "@tauri-apps/plugin-notification": "^2",
  "@tauri-apps/plugin-autostart": "^2"   // NEW phase 5
}
```

Install:

```bash
npm install @tauri-apps/plugin-autostart
cd src-tauri && cargo add tauri-plugin-autostart && cd ..
```

---

## 1. Sound alerts

### 1.1 Decision: frontend HTML5 `Audio` (recommended)

Play the sound in the **frontend** using HTML5 `Audio`, not Rust. Rationale:

- No extra Rust audio stack / ALSA-PulseAudio plumbing, no `rodio` device
  enumeration headaches inside the bundled AppImage.
- The webview already has an audio output path that "just works" with the desktop
  session's PulseAudio/PipeWire.
- Volume / enable-disable is trivial in JS.

The cost: a window must be **open and focused-enough to have run a user gesture
once** (autoplay policy, see §1.5). For an always-on-top sticky-note app the user
has clicked the app, so this is fine. The `rodio` alternative (§1.6) avoids the
gesture requirement but adds a Rust dependency and bundling burden — only switch if
you need sound while *zero* windows are open.

### 1.2 Bundle the audio file

Put a short WAV/OGG/MP3 under `src-tauri/sounds/`:

```
src-tauri/sounds/alert.ogg     # ~1s chime; OGG plays everywhere webkit runs
```

Declare it as a bundled resource in `tauri.conf.json → bundle.resources`:

```jsonc
"bundle": {
  "resources": ["sounds/*"]
}
```

(Full bundle block is in §7.)

### 1.3 Allow the asset protocol to read it

`convertFileSrc` serves the file over the `asset:` protocol, which is gated by an
**FsScope**. In `tauri.conf.json → app.security`:

```jsonc
"app": {
  "security": {
    "csp": null,
    "assetProtocol": {
      "enable": true,
      "scope": ["$RESOURCE/sounds/*"]
    }
  }
}
```

> `$RESOURCE` resolves to the bundled-resources dir at runtime (next to the binary
> in dev, inside the AppImage/`/usr/lib/<app>/` in a `.deb`). The glob must match the
> path you bundled in §1.2. Without this scope `convertFileSrc` returns a URL the
> webview refuses to load (`Not allowed to load local resource`).

If CSP is ever set to non-null, also add `media-src asset: http://asset.localhost`.

### 1.4 Resolve + play in the frontend

`resolveResource` gives the absolute on-disk path; `convertFileSrc` turns it into an
`asset:`-protocol URL the `<audio>`/`Audio` element can fetch.

```ts
// src/sound.ts
import { resolveResource } from "@tauri-apps/api/path";
import { convertFileSrc } from "@tauri-apps/api/core";

let alertUrl: string | null = null;
let unlocked = false;

export async function initSound(): Promise<void> {
  const path = await resolveResource("sounds/alert.ogg");
  alertUrl = convertFileSrc(path); // -> asset://localhost/...
}

export async function playAlert(soundOn: boolean): Promise<void> {
  if (!soundOn || !alertUrl) return;
  const audio = new Audio(alertUrl);
  audio.volume = 1.0;
  try {
    await audio.play();
  } catch {
    // autoplay blocked until first gesture — silently ignore (see §1.5)
  }
}
```

### 1.5 Autoplay policy — unlock on first gesture

WebKit blocks `audio.play()` until the document has had a user gesture. Unlock once
per window on the first interaction (the user always clicks a note to use it):

```ts
function armAudioUnlock() {
  const unlock = () => {
    const a = new Audio();
    a.muted = true;
    a.play().catch(() => {});
    unlocked = true;
    window.removeEventListener("pointerdown", unlock);
    window.removeEventListener("keydown", unlock);
  };
  window.addEventListener("pointerdown", unlock, { once: true });
  window.addEventListener("keydown", unlock, { once: true });
}
```

Call `initSound()` + `armAudioUnlock()` on note-window load. If a sound fires before
any gesture, it's swallowed by the `catch` — acceptable.

### 1.6 Gate to ONE window (avoid the N-window sound storm)

`timer-done` / schedule-fire events are emitted **globally** by the Rust tick loop, so
*every* open note window would receive them and each would call `playAlert` — N
simultaneous chimes. Pick a single "audio host" window.

**Recommended approach — Rust emits sound to one label.** When the tick loop decides a
sound should play, emit a dedicated event to a single, known-always-present window
rather than broadcasting:

```rust
// pick an audio host: the settings window if open, else the first note window,
// else skip. Store the chosen label in app state, or just target the main/first.
if let Some(win) = app.get_webview_window(&audio_host_label) {
    let _ = win.emit_to(EventTarget::WebviewWindow {
        label: win.label().to_string(),
    }, "play-sound", ());
}
```

Frontend listens for `play-sound` only:

```ts
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

const me = getCurrentWebviewWindow();
const settings = await loadSettings();         // §4
if (me.label === audioHostLabel) {
  await listen("play-sound", () => playAlert(settings.soundOn));
}
```

**Simpler alternative (pure frontend gate):** keep broadcasting `timer-done`, but only
the window whose label sorts first (or equals a fixed `"audio-host"` label) actually
plays. Compute the winner in JS from `getAllWebviewWindows()`. This is racy when
windows open/close; prefer the Rust-targeted emit above.

Either way the **source of truth for "should a sound play"** stays in Rust (it already
owns `timer-done` and schedule fire); the frontend only decides *which* window's
speaker is used.

### 1.7 `rodio` alternative (documented, not used)

If you later need sound with no window open:

```toml
# Cargo.toml
rodio = "0.19"
```

```rust
use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;

// embed at compile time so no resource resolution is needed:
static ALERT: &[u8] = include_bytes!("../sounds/alert.ogg");

fn play_alert() {
    std::thread::spawn(|| {
        if let Ok((_stream, handle)) = OutputStream::try_default() {
            if let Ok(sink) = Sink::try_new(&handle) {
                if let Ok(src) = Decoder::new(Cursor::new(ALERT)) {
                    sink.append(src);
                    sink.sleep_until_end();
                }
            }
        }
    });
}
```

Trade-offs: pulls ALSA dev headers at build time (`libasound2-dev`), adds runtime
audio-device discovery that can fail headless, and you must `include_bytes!` or resolve
the resource yourself. Keep `bundleMediaFramework` off (it only helps the webview path).
**Recommendation stays: frontend `Audio`.**

---

## 2. Autostart on login

### 2.1 Register the plugin (Rust)

```rust
// src-tauri/src/lib.rs
#[cfg(desktop)]
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                app.handle().plugin(tauri_plugin_autostart::init(
                    MacosLauncher::LaunchAgent,
                    Some(vec!["--autostarted"]), // launch args passed on autostart
                ))?;
            }
            Ok(())
        })
        // ...commands, run()
}
```

- `MacosLauncher::LaunchAgent` — macOS uses a LaunchAgent plist (harmless on Linux;
  required arg).
- `Some(vec!["--autostarted"])` — arguments handed to the app when launched at login.
  Read them (`std::env::args`) to e.g. start hidden / skip showing windows on a login
  boot if desired.
- On **Linux** the plugin writes `~/.config/autostart/<identifier>.desktop`, i.e.
  `~/.config/autostart/com.stickytimer.app.desktop`. It is created/removed by
  `enable()`/`disable()`.

### 2.2 Capabilities

Add to `src-tauri/capabilities/default.json → permissions`:

```jsonc
"permissions": [
  "core:default",
  "opener:default",
  "store:default",
  "notification:default",
  "autostart:allow-enable",
  "autostart:allow-disable",
  "autostart:allow-is-enabled"
]
```

(or the umbrella `"autostart:default"`). These must be on a capability whose
`windows` list includes the **settings** window — see §4.3.

### 2.3 JS toggle (in the Settings window)

```ts
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

// reflect real OS state, not the stored flag:
async function syncAutostartUI() {
  const on = await isEnabled();
  autostartCheckbox.checked = on;
}

async function setAutostart(on: boolean) {
  if (on) await enable();
  else await disable();
  // persist the *intent* too, but treat isEnabled() as truth (see Gotchas)
  await writeSetting("autostart", await isEnabled());
}
```

### 2.4 Reconcile on boot

In `setup`, after registering the plugin, optionally re-apply the stored intent only if
it disagrees with `is_enabled()` (covers the case where the `.desktop` file was deleted
externally). Always trust `is_enabled()` for the UI checkbox.

---

## 3. Note colors

### 3.1 Data

`Task.color` already exists in the model (`plan.md` data model). It is a string —
store a hex (e.g. `"#ffd96b"`). Default `"#fff7a8"` (sticky-yellow) for new tasks.

### 3.2 Palette + picker UI (per note)

A small fixed palette (no arbitrary color wheel needed):

```ts
const PALETTE = [
  "#fff7a8", // yellow
  "#ffd2a8", // peach
  "#ffb3ba", // pink
  "#b8e6c1", // green
  "#a8d8ff", // blue
  "#d9c2ff", // purple
  "#e6e6e6", // grey
];
```

Render swatch buttons in the note header (or a popover). On click:

```ts
async function setColor(taskId: string, color: string) {
  document.documentElement.style.setProperty("--note-bg", color);
  await invoke("set_task_color", { id: taskId, color });
}
```

### 3.3 Apply via CSS variable

In `styles.css`, drive the card background from `--note-bg`:

```css
:root { --note-bg: #fff7a8; }
.note-card { background: var(--note-bg); }
.note-header { background: color-mix(in srgb, var(--note-bg) 85%, black); }
```

On window load, after fetching the task, set `--note-bg` from `task.color`.

### 3.4 Rust command (persist via store)

```rust
#[tauri::command]
fn set_task_color(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
    color: String,
) -> Result<(), String> {
    let mut tasks = state.tasks.lock().unwrap();
    if let Some(t) = tasks.get_mut(&id) {
        t.color = color;
        persist_tasks(&app, &tasks)?; // existing store-write helper from phase 1/2
    }
    Ok(())
}
```

Register in `invoke_handler`. Color is now part of the persisted `Task`, so it restores
on boot like any other field.

---

## 4. Settings window

### 4.1 Separate window + page

A dedicated `WebviewWindow` with its own HTML entry `settings.html`. Add it to the Vite
build (multi-page):

```ts
// vite.config.ts
import { resolve } from "path";
export default defineConfig({
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        settings: resolve(__dirname, "settings.html"),
      },
    },
  },
  // existing tauri dev-server config...
});
```

Open it from the tray (or a note) — created once, focused if already open:

```rust
fn open_settings(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") {
        w.show()?; w.set_focus()?;
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        app, "settings", tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("Sticky Timer — Settings")
    .inner_size(360.0, 420.0)
    .resizable(false)
    .decorations(true)        // normal chrome, unlike notes
    .always_on_top(false)
    .build()?;
    Ok(())
}
```

> Note: settings uses **normal decorations** and is **not** always-on-top, unlike the
> frameless notes. Its `CloseRequested` must really close (see §5).

### 4.2 Settings store schema

Keep settings **separate from tasks** — a dedicated store key. Either a separate store
file `settings.json` (cleanest) or a reserved `"settings"` key in the existing store.
Recommended: a distinct file.

```ts
// shape
interface Settings {
  defaultCountdownSecs: number; // default 1500 (25 min)
  soundOn: boolean;             // default true
  autostart: boolean;          // mirror of isEnabled(); truth = isEnabled()
  globalPause: boolean;        // default false (see §6)
}
```

```ts
import { Store } from "@tauri-apps/plugin-store";
const store = await Store.load("settings.json");

async function loadSettings(): Promise<Settings> {
  return {
    defaultCountdownSecs: (await store.get("defaultCountdownSecs")) ?? 1500,
    soundOn:              (await store.get("soundOn")) ?? true,
    autostart:           (await store.get("autostart")) ?? false,
    globalPause:         (await store.get("globalPause")) ?? false,
  };
}
async function writeSetting<K extends keyof Settings>(k: K, v: Settings[K]) {
  await store.set(k, v);
  await store.save();
}
```

### 4.3 Capabilities for the settings window

The default capability targets `"windows": ["main"]`. Add a capability (or widen the
default) covering `settings` and note windows. Simplest: one capability with
`"windows": ["*"]` granting the shared permissions, plus autostart only where needed.

```jsonc
// capabilities/default.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["*"],
  "permissions": [
    "core:default", "opener:default",
    "store:default", "notification:default",
    "autostart:allow-enable", "autostart:allow-disable", "autostart:allow-is-enabled",
    "core:window:allow-create", "core:window:allow-show", "core:window:allow-hide",
    "core:window:allow-set-focus", "core:window:allow-close",
    "core:event:allow-listen", "core:event:allow-emit"
  ]
}
```

### 4.4 Fields + global apply

| Field | Control | On change |
|-------|---------|-----------|
| Default countdown | number/minutes input | `writeSetting("defaultCountdownSecs", …)` — used by `create_task` for the new note's duration |
| Sound on/off | checkbox | `writeSetting("soundOn", …)` — read by audio host before `playAlert` |
| Autostart on/off | checkbox | `setAutostart()` (§2.3) |
| Global pause | checkbox | `invoke("pause_all")` / `invoke("resume_all")` (§6) |

Settings that the **Rust side** needs (default countdown, global pause) get
read/written by Rust commands so the tick loop / `create_task` see them without a
round-trip:

```rust
#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Settings { /* read settings store */ }

#[tauri::command]
fn set_setting(app: tauri::AppHandle, key: String, value: serde_json::Value)
    -> Result<(), String> { /* write + apply side effects */ }
```

When `create_task` runs, it reads `defaultCountdownSecs` from the settings store for
the new task's `timer.duration_secs`. "Global apply" = the tick loop reads global pause
(via the AtomicBool, §6) and the audio host reads `soundOn` each fire.

---

## 5. Minimize-to-tray (close ≠ quit)

### 5.1 Per-label close handling

Intercept `CloseRequested` in the window event handler. **Note windows** hide;
the **settings** window genuinely closes; only the tray "Quit" exits the process.

```rust
tauri::Builder::default()
    // ...
    .on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            let label = window.label();
            if label == "settings" {
                // let it actually close (do NOT prevent) — frees the WebviewWindow
                return;
            }
            // note windows: hide to tray instead of closing
            api.prevent_close();
            let _ = window.hide();
        }
    })
```

> Why the per-label branch: if you blanket-`prevent_close()` every window, the settings
> window can never be destroyed and `get_webview_window("settings")` keeps returning a
> stale handle. Letting it close means §4.1's "create or focus" works correctly next
> time.

### 5.2 Tray menu

Tray items: **Show all**, **New note**, **Settings**, **Quit**.

```rust
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;

let show  = MenuItemBuilder::with_id("show",  "Show all").build(app)?;
let new   = MenuItemBuilder::with_id("new",   "New note").build(app)?;
let prefs = MenuItemBuilder::with_id("prefs", "Settings").build(app)?;
let quit  = MenuItemBuilder::with_id("quit",  "Quit").build(app)?;
let menu  = MenuBuilder::new(app).items(&[&show, &new, &prefs, &quit]).build()?;

TrayIconBuilder::new()
    .icon(app.default_window_icon().unwrap().clone())
    .menu(&menu)
    .on_menu_event(|app, event| match event.id().as_ref() {
        "show" => {
            for w in app.webview_windows().values() {
                if w.label() != "settings" { let _ = w.show(); let _ = w.set_focus(); }
            }
        }
        "new"   => { let _ = create_note_window(app); }      // existing phase-2 helper
        "prefs" => { let _ = open_settings(app); }           // §4.1
        "quit"  => { app.exit(0); }                          // ONLY real quit
        _ => {}
    })
    .build(app)?;
```

> `app.exit(0)` is the **only** path that terminates the app. Window closes never do.
> This makes the app a true tray-resident app: closing the last note leaves it running
> in the tray (needed for scheduler + autostart to be meaningful).

### 5.3 Prevent exit-on-last-window-closed

By default Tauri may exit when the last window closes. Since notes hide rather than
close, this won't trigger normally, but to be safe handle `RunEvent::ExitRequested`:

```rust
.build(tauri::generate_context!())?
.run(|_app, event| {
    if let tauri::RunEvent::ExitRequested { api, .. } = event {
        api.prevent_exit(); // stay alive in tray; only app.exit(0) gets through
    }
});
```

---

## 6. Global pause

A process-wide pause that the tick loop honors **without losing each task's individual
state** (a task that was already `paused` or `idle` stays that way on resume; only
`running` tasks are frozen).

### 6.1 State

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct AppState {
    tasks: Mutex<HashMap<String, Task>>,
    paused: Arc<AtomicBool>,   // global pause flag
}
```

### 6.2 Tick loop checks the flag

The existing 1s loop short-circuits when paused — it does **not** mutate any task's
`state`, so individual running/paused/idle status is preserved:

```rust
loop {
    interval.tick().await;                       // 1s
    if state.paused.load(Ordering::Relaxed) {
        continue;                                // skip decrement/schedule eval entirely
    }
    // ... normal per-task tick: decrement running countdowns, advance stopwatches,
    //     evaluate schedules, emit timer-tick / timer-done ...
}
```

> Because we `continue` instead of pausing each timer, no per-task `state` is touched.
> On resume, every previously-`running` timer simply continues from where it was.
> (Optionally also pause schedule *firing* while paused — recommended, so a schedule
> that comes due during a global pause fires on resume rather than being missed; decide
> per the Phase 4 missed-fire policy.)

### 6.3 Commands

```rust
#[tauri::command]
fn pause_all(state: tauri::State<AppState>, app: tauri::AppHandle) {
    state.paused.store(true, Ordering::Relaxed);
    persist_setting(&app, "globalPause", true);
    let _ = app.emit("global-pause", true);  // notes can show a "paused" badge
}

#[tauri::command]
fn resume_all(state: tauri::State<AppState>, app: tauri::AppHandle) {
    state.paused.store(false, Ordering::Relaxed);
    persist_setting(&app, "globalPause", false);
    let _ = app.emit("global-pause", false);
}
```

On boot, restore `paused` from the `globalPause` setting so a quit-while-paused stays
paused. Note windows listen for `global-pause` to render a paused indicator.

---

## 7. Packaging (`.deb` + AppImage)

### 7.1 Build command

```bash
npm run tauri build
```

Produces (under `src-tauri/target/release/bundle/`):

- `deb/sticky-timer_0.1.0_amd64.deb`
- `appimage/sticky-timer_0.1.0_amd64.AppImage`

### 7.2 Full `tauri.conf.json` bundle config

```jsonc
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "sticky-timer",
  "version": "0.1.0",
  "identifier": "com.stickytimer.app",     // reverse-DNS; keep stable (autostart!)
  "app": {
    "withGlobalTauri": true,
    "security": {
      "csp": null,
      "assetProtocol": {
        "enable": true,
        "scope": ["$RESOURCE/sounds/*"]
      }
    },
    "windows": [
      { "label": "main", "title": "sticky-timer", "width": 280, "height": 280,
        "decorations": false, "alwaysOnTop": true, "transparent": true }
    ]
  },
  "bundle": {
    "active": true,
    "targets": ["deb", "appimage"],        // narrow from "all" for Linux-only
    "category": "Utility",
    "shortDescription": "Sticky notes with timers and schedules",
    "longDescription": "Desktop sticky notes for Linux. Each note is a task with a countdown/stopwatch timer and an optional schedule that fires a notification and auto-starts the timer.",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "resources": ["sounds/*"],
    "linux": {
      "deb": {
        "depends": [
          "libwebkit2gtk-4.1-0",
          "libgtk-3-0",
          "libayatana-appindicator3-1"
        ]
      },
      "appimage": {
        "bundleMediaFramework": true       // gstreamer for webview <audio> in AppImage
      }
    }
  }
}
```

Notes:
- `category: "Utility"` — valid freedesktop category; shows the app in the right menu
  section and the `.desktop` `Categories=`.
- `deb.depends` are the **runtime** shared libs the user's machine needs. `4.1` (not
  `4.0`) matches the build deps below. `libayatana-appindicator3-1` is what makes the
  tray icon appear on most Linux DEs.
- `bundleMediaFramework: true` bundles GStreamer so the webview's `<audio>` actually
  produces sound inside the AppImage sandbox (the most common "sound works in dev,
  silent in AppImage" cause). Not needed for `.deb` (uses system GStreamer).

### 7.3 Host build dependencies (build machine)

```bash
sudo apt install \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  build-essential curl wget file libssl-dev \
  libgtk-3-dev patchelf
```

`patchelf` + `file` + `wget` are required by the AppImage bundler. `libwebkit2gtk-4.1-dev`
is the Tauri v2 webkit; do **not** install the `4.0` dev package for v2.

### 7.4 AppImage gotchas

- **webkit 4.0 vs 4.1 split.** Tauri v2 builds against `libwebkit2gtk-4.1`. Building on
  a box that only has `4.0` (e.g. older Ubuntu 20.04) fails or produces an AppImage that
  won't start on machines with 4.1. Build on Ubuntu 22.04+ / a distro with 4.1.
- **Tray / appindicator.** The AppImage must find an appindicator lib at runtime.
  Install `libayatana-appindicator3-1` on the build host so it gets bundled; on the
  target, GNOME users need an AppIndicator extension for the tray to be visible.
- **FUSE.** AppImages need FUSE to self-mount. On systems without it:
  `./sticky-timer.AppImage --appimage-extract-and-run`, or install `libfuse2`
  (Ubuntu 22.04: `sudo apt install libfuse2`). Document this for end users.
- **glibc forward-incompatibility.** An AppImage built on a newer glibc will **not**
  run on older distros (`GLIBC_2.xx not found`). Build on the **oldest** glibc you want
  to support (e.g. Ubuntu 22.04) for maximum reach. AppImage does not bundle glibc.
- **Resource path differs from dev.** `$RESOURCE` points inside the mounted AppImage
  (`$APPDIR/usr/lib/sticky-timer/...`); `resolveResource("sounds/alert.ogg")` handles
  this — never hard-code paths. Verify the asset scope (§1.3) matches.
- **`bundleMediaFramework`** (above) — without it, `<audio>` is silent inside the
  AppImage even though it works under `tauri dev` and in the `.deb`.

---

## 8. "Done when" checklist

- [ ] **Sound:** countdown reaching 0 plays exactly **one** chime (not N); a schedule
      fire plays one chime; toggling Sound off in Settings silences it; survives restart.
- [ ] Sound works in `tauri dev`, in the installed `.deb`, **and** in the AppImage
      (`bundleMediaFramework` confirmed).
- [ ] **Autostart:** toggling on creates `~/.config/autostart/com.stickytimer.app.desktop`;
      logging out/in launches the app (to tray); toggling off removes the file; the
      Settings checkbox reflects `isEnabled()` not the stored flag.
- [ ] **Colors:** picking a swatch recolors the note immediately (`--note-bg`), persists,
      and restores on relaunch per-note.
- [ ] **Settings window:** opens from tray, single instance (re-focus if open), has
      normal window chrome, and genuinely **closes** (not hides). Default countdown,
      sound, autostart, global pause all read/write and apply.
- [ ] **Minimize-to-tray:** closing a note hides it (tray "Show all" brings it back);
      closing the last note does **not** quit; only tray "Quit" exits.
- [ ] **Global pause:** toggling pause freezes all running timers; resume continues each
      from where it stopped; tasks that were idle/paused are unchanged; persists across
      restart.
- [ ] **Packaging:** `npm run tauri build` emits a `.deb` and an `.AppImage`; both
      install/run on a clean target; tray icon visible; notifications fire.

---

## 9. Cross-cutting gotchas

1. **Autostart identifier / path drift.** The `.desktop` filename is derived from
   `identifier` (`com.stickytimer.app.desktop`) and embeds the executable path. If you
   change the identifier, rename the binary, or move the install location *after*
   enabling autostart, the stale `.desktop` points at the old path and silently fails.
   Re-toggle autostart after any such change. Keep `productName`/`identifier` frozen.
2. **`isEnabled()` is the truth.** Never trust the persisted `autostart` boolean for the
   checkbox — the `.desktop` file can be deleted by the user / another tool. Always read
   `await isEnabled()` to render UI, and reconcile stored intent against it on boot.
3. **Resource resolution.** Always use `resolveResource()` + `convertFileSrc()`; never
   hard-code paths. The on-disk location differs between dev, `.deb`, and AppImage. Match
   the bundled path (`sounds/*`) to the asset scope (`$RESOURCE/sounds/*`) exactly.
4. **Autoplay policy.** `Audio.play()` is blocked until a user gesture in the document.
   Arm a one-time `pointerdown`/`keydown` unlock per window (§1.5); swallow the rejected
   promise so a pre-gesture fire fails silently rather than throwing.
5. **Asset protocol scope.** `assetProtocol.enable: true` **and** a matching `scope`
   are both required; missing scope → `Not allowed to load local resource`. If `csp` is
   non-null, also allow `media-src asset:`.
6. **Capabilities cover every window.** The default capability only targets `["main"]`.
   The settings window and dynamically-created note windows need their permissions too —
   use `"windows": ["*"]` or add a capability listing the right labels, or `invoke`/
   `listen`/autostart calls from those windows will be denied at runtime.
7. **Sound storm.** `timer-done` / schedule-fire are global; gate playback to one window
   (Rust `emit_to` a single audio-host label, §1.6) or you get one chime per open note.
8. **Settings window must really close.** The per-label `CloseRequested` branch (§5.1)
   must let `settings` close; otherwise the "create-or-focus" logic returns a dead
   handle and the window appears broken on second open.
9. **Only `app.exit(0)` quits.** Combined with `RunEvent::ExitRequested →
   prevent_exit()`, this keeps the scheduler alive in the tray — the whole point of a
   sticky-timer app that fires alerts when no note is visible.
