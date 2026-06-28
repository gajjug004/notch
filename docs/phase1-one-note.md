# Phase 1 — One Note (Implementation Spec)

> **Goal:** A single sticky-note window that is frameless, draggable, always-on-top,
> transparent, with an editable title + body. Its text **and** window geometry
> persist across app restarts.
>
> **Stack:** Tauri v2, vanilla TypeScript frontend, Rust backend as the eventual
> source of truth. Identifier `com.stickytimer.app`. Scaffolded with `vanilla-ts`.
>
> **Verified versions (June 2026):** `@tauri-apps/plugin-store` 2.4.x (npm),
> `tauri-plugin-store` 2.4.x (crate), `@tauri-apps/api` 2.x.

---

## 0. Scope & non-goals

In scope for Phase 1:

- One window only (the default `main` window). No multi-window, no tray.
- Frameless / transparent / always-on-top / draggable.
- Editable title (`<input>`) and body (`contenteditable`).
- Persist `{title, content, color, window:{x,y,w,h}}` to a single JSON store file.
- Restore text, color, and geometry on boot.

Explicitly **not** in scope (deferred):

- Timer, schedule, notifications, multiple notes, color picker UI (a fixed default
  color is fine; the field is persisted so Phase 5 can wire a picker).
- Rust-owned task state. Phase 1 does persistence **entirely in JS** via the store
  plugin (justified in §8). We only add forward-compatible Rust command stubs.

---

## 1. `tauri.conf.json` window settings

In Tauri v2 the window array lives under the top-level **`app.windows`** key (it was
`tauri.windows` in v1). Edit `src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "sticky-timer",
  "version": "0.1.0",
  "identifier": "com.stickytimer.app",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "Sticky Timer",
        "width": 260,
        "height": 260,
        "minWidth": 160,
        "minHeight": 120,
        "decorations": false,
        "alwaysOnTop": true,
        "transparent": true,
        "shadow": false,
        "skipTaskbar": false,
        "resizable": true,
        "focus": true,
        "visible": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

What each setting buys us:

| Key | Value | Why |
|-----|-------|-----|
| `decorations` | `false` | Frameless — no OS title bar/borders. We draw our own card + drag strip. |
| `alwaysOnTop` | `true` | Note floats above other windows (sticky-note behavior). |
| `transparent` | `true` | Lets the card have rounded corners / padding with a see-through background. **Requires** `macOSPrivateApi` on macOS, but on Linux it works as long as the compositor supports it (see Gotchas §9). |
| `shadow` | `false` | Avoid an OS drop-shadow drawn as a rectangle around a rounded/transparent window (looks wrong on Linux). We can add a CSS `box-shadow` on the card instead. |
| `skipTaskbar` | `false` | Phase 1 keeps it in the taskbar so it's easy to find. Phase 2/5 may flip this to `true` once the tray exists. |
| `resizable` | `true` | User can resize; we persist the new size. |
| `width`/`height` | `260` | Small default sticky size. Overridden on boot if a saved geometry exists. |

> **Note:** `transparent: true` also generally requires the Cargo feature on the
> Tauri crate. With Tauri v2 the `tauri` crate enables it via the default features
> for desktop; if a transparent window shows an opaque background, ensure
> `tauri = { version = "2", features = [...] }` does not exclude it. No extra
> feature flag is normally needed on Linux.

---

## 2. HTML / CSS — the sticky-note card

### 2.1 `index.html`

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Sticky Timer</title>
  </head>
  <body>
    <main class="note" id="note">
      <!-- Drag strip: the ONLY element that moves the window -->
      <div class="note__drag" data-tauri-drag-region>
        <span class="note__dot" aria-hidden="true"></span>
      </div>

      <input
        class="note__title"
        id="title"
        type="text"
        placeholder="Title"
        spellcheck="false"
      />

      <div
        class="note__body"
        id="body"
        contenteditable="true"
        role="textbox"
        aria-multiline="true"
        data-placeholder="Write something…"
      ></div>
    </main>

    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

### 2.2 `src/styles.css`

```css
:root {
  /* default note color; persisted in store as `color` */
  --note-bg: #fff7b1;       /* classic yellow sticky */
  --note-bg-strip: #ffe66b; /* slightly darker drag strip */
  --note-ink: #2a2a22;
}

* { box-sizing: border-box; }

html,
body {
  margin: 0;
  padding: 0;
  /* CRITICAL: window is transparent, so the document must be too,
     otherwise you see an opaque rectangle behind the rounded card */
  background: transparent;
  width: 100%;
  height: 100%;
  overflow: hidden;
  font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
  color: var(--note-ink);
  /* Prevent the whole UI from being text-selected while dragging */
  user-select: none;
}

.note {
  position: fixed;
  inset: 6px;                /* gap = room for CSS shadow, see Gotchas */
  display: flex;
  flex-direction: column;
  background: var(--note-bg);
  border-radius: 10px;
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.28);
  overflow: hidden;
}

.note__drag {
  flex: 0 0 22px;
  display: flex;
  align-items: center;
  padding: 0 8px;
  background: var(--note-bg-strip);
  cursor: grab;             /* hint that this strip moves the window */
}
.note__drag:active { cursor: grabbing; }

.note__dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.25);
}

.note__title {
  border: none;
  outline: none;
  background: transparent;
  font-size: 14px;
  font-weight: 600;
  padding: 8px 10px 2px;
  color: inherit;
  /* inputs must be selectable/typeable — re-enable selection */
  user-select: text;
}

.note__body {
  flex: 1 1 auto;
  outline: none;
  padding: 4px 10px 10px;
  font-size: 13px;
  line-height: 1.4;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-word;
  user-select: text;
}

/* contenteditable placeholder */
.note__body:empty::before {
  content: attr(data-placeholder);
  color: rgba(0, 0, 0, 0.35);
  pointer-events: none;
}
```

Key layout intent: the card fills the window with a small `inset` so the CSS
`box-shadow` has room to render (a transparent window clips at its own edges). The
**drag strip is a separate fixed-height bar** so dragging never starts on the title
or body.

---

## 3. Drag region — how it works in Tauri v2

`data-tauri-drag-region` is an HTML attribute Tauri's webview injection looks for.
On `mousedown` over an element carrying that attribute, Tauri calls the native
`window.start_dragging` command, handing the move to the OS window manager.

Rules that drive our markup choices:

1. **It only applies to the exact element it's on**, not children automatically.
   Tauri does bubble from a child up to a `data-tauri-drag-region` ancestor *as long
   as the child isn't interactive*, but the safe pattern is a dedicated empty strip.
2. **Do not put the attribute on (or wrapping) the title input or contenteditable
   body** — otherwise clicks meant to place the caret start a window drag instead.
   Our `.note__drag` strip is isolated above both fields.
3. Buttons/inputs inside a drag region still work, but a click that misses them
   drags — another reason to keep the strip childless except the decorative dot.

### Required capability

Dragging needs the core permission **`core:window:allow-start-dragging`**. Add it to
`src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main sticky note window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-start-dragging",
    "core:window:allow-set-position",
    "core:window:allow-set-size",
    "core:window:allow-outer-position",
    "core:window:allow-inner-size",
    "core:window:allow-set-always-on-top",
    "store:default"
  ]
}
```

The window-geometry permissions (`set-position`, `set-size`, `outer-position`,
`inner-size`) are needed because we read and write geometry from JS (§5–§6).
`store:default` is added in §4.

> Without `core:window:allow-start-dragging`, the drag strip silently does nothing
> (no error dialog — just no movement). This is the #1 "drag isn't working" cause.

---

## 4. Persistence — `tauri-plugin-store` v2

### 4.1 Install

Run from the project root:

```bash
npm run tauri add store
```

This single command:

- adds the Rust crate **`tauri-plugin-store`** to `src-tauri/Cargo.toml`,
- adds the npm package **`@tauri-apps/plugin-store`** to `package.json`,
- registers the plugin in `src-tauri/src/lib.rs` (verify it did — see §4.2).

Exact package names (for reference / manual install):

| Side | Name | Manual install |
|------|------|----------------|
| Rust | `tauri-plugin-store` | `cargo add tauri-plugin-store` (in `src-tauri/`) |
| JS   | `@tauri-apps/plugin-store` | `npm install @tauri-apps/plugin-store` |

### 4.2 Register the plugin in `lib.rs`

`src-tauri/src/lib.rs`:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            save_task, // Phase 2/3 stubs, see §8
            load_task
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### 4.3 Capability JSON

Already shown in §3 — the relevant line is `"store:default"`. That grants the load /
get / set / save / etc. commands. If you prefer least-privilege, replace it with the
specific permissions actually used:

```json
"store:allow-load",
"store:allow-get",
"store:allow-set",
"store:allow-save"
```

### 4.4 Data shape

A single store file `note.json` with one logical record (Phase 2 will turn this into
a keyed collection):

```ts
type NoteColor = { bg: string; strip: string; ink: string };

interface WindowGeom {
  x: number; // physical px, outer position
  y: number;
  w: number; // physical px, inner size
  h: number;
}

interface NoteData {
  title: string;
  content: string; // plain text extracted from contenteditable
  color: NoteColor;
  window: WindowGeom;
}
```

Stored as flat keys in the store (simplest with the KV API):
`title`, `content`, `color`, `window`.

### 4.5 JS store helper (`src/store.ts`)

```ts
import { load, type Store } from "@tauri-apps/plugin-store";

const STORE_FILE = "note.json";

let _store: Store | null = null;

/** Memoized store handle — load() is async and must only run once. */
export async function getStore(): Promise<Store> {
  if (!_store) {
    // autoSave: 400  → plugin debounces writes to disk by 400ms after a set().
    // We still call save() explicitly on close to guarantee a flush (§9).
    _store = await load(STORE_FILE, { autoSave: 400 });
  }
  return _store;
}

export async function readNote(): Promise<Partial<NoteData>> {
  const s = await getStore();
  return {
    title: (await s.get<string>("title")) ?? "",
    content: (await s.get<string>("content")) ?? "",
    color: (await s.get<NoteColor>("color")) ?? undefined,
    window: (await s.get<WindowGeom>("window")) ?? undefined,
  };
}

export async function writeText(title: string, content: string): Promise<void> {
  const s = await getStore();
  await s.set("title", title);
  await s.set("content", content);
  // no explicit save(): autoSave debounce handles disk write
}

export async function writeWindow(window: WindowGeom): Promise<void> {
  const s = await getStore();
  await s.set("window", window);
}

export async function flush(): Promise<void> {
  const s = await getStore();
  await s.save(); // force write now
}
```

> **API note:** in plugin-store v2 the entry point is the top-level
> `load(path, options)` function (returns a `Promise<Store>`). The old
> `new Store(path)` constructor is removed; `Store.load` / `new LazyStore` also
> exist but `load()` is the documented v2 path. `autoSave` accepts `true`/`false`
> or a number of milliseconds to debounce.

---

## 5. Saving window geometry

Use the window API from `@tauri-apps/api/window`. Geometry is read in **physical
pixels** (`outerPosition` / `innerSize` return `PhysicalPosition` / `PhysicalSize`).

`src/geometry.ts`:

```ts
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeWindow } from "./store";

const appWindow = getCurrentWindow();

let geomTimer: number | undefined;

/** Debounce geometry writes — onMoved/onResized fire rapidly during a drag. */
function scheduleGeomSave(): void {
  if (geomTimer) clearTimeout(geomTimer);
  geomTimer = window.setTimeout(saveGeomNow, 300);
}

async function saveGeomNow(): Promise<void> {
  const pos = await appWindow.outerPosition(); // PhysicalPosition {x,y}
  const size = await appWindow.innerSize();    // PhysicalSize {width,height}
  await writeWindow({ x: pos.x, y: pos.y, w: size.width, h: size.height });
}

export async function trackGeometry(): Promise<void> {
  await appWindow.onMoved(() => scheduleGeomSave());
  await appWindow.onResized(() => scheduleGeomSave());
}
```

- `onMoved` / `onResized` return an unlisten function (ignored here since the window
  lives for the whole app lifetime).
- We store **outer position** (where the window's top-left actually is) but **inner
  size** (the content area we control). Mixing them is a deliberate, documented
  choice — see the physical/logical drift gotcha (§9).

---

## 6. Restore on boot

`src/restore.ts`:

```ts
import { getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import { readNote } from "./store";

const appWindow = getCurrentWindow();

export async function restore(): Promise<void> {
  const note = await readNote();

  // 1. Geometry — apply BEFORE showing to avoid a visible jump.
  if (note.window) {
    const { x, y, w, h } = note.window;
    await appWindow.setSize(new PhysicalSize(w, h));
    await appWindow.setPosition(new PhysicalPosition(x, y));
  }

  // 2. Color
  if (note.color) {
    const root = document.documentElement.style;
    root.setProperty("--note-bg", note.color.bg);
    root.setProperty("--note-bg-strip", note.color.strip);
    root.setProperty("--note-ink", note.color.ink);
  }

  // 3. Text
  const titleEl = document.getElementById("title") as HTMLInputElement;
  const bodyEl = document.getElementById("body") as HTMLDivElement;
  titleEl.value = note.title ?? "";
  bodyEl.innerText = note.content ?? "";
}
```

- `setSize` / `setPosition` accept `PhysicalSize` / `PhysicalPosition` instances
  (matching what we saved). You can pass logical variants, but keep one unit system
  end-to-end.
- Apply geometry first so the window doesn't flash at the default 260×260 before
  snapping to the saved spot. If you see a flash anyway, set `"visible": false` in
  the config and call `appWindow.show()` at the end of `restore()`.

---

## 7. Wiring it together (`src/main.ts`)

```ts
import "./styles.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { restore } from "./restore";
import { trackGeometry } from "./geometry";
import { writeText, flush } from "./store";

const appWindow = getCurrentWindow();

let textTimer: number | undefined;

function scheduleTextSave(title: string, content: string): void {
  if (textTimer) clearTimeout(textTimer);
  textTimer = window.setTimeout(() => writeText(title, content), 400);
}

window.addEventListener("DOMContentLoaded", async () => {
  await restore();
  await trackGeometry();

  const titleEl = document.getElementById("title") as HTMLInputElement;
  const bodyEl = document.getElementById("body") as HTMLDivElement;

  const onEdit = () => scheduleTextSave(titleEl.value, bodyEl.innerText);
  titleEl.addEventListener("input", onEdit);
  bodyEl.addEventListener("input", onEdit);

  // Flush pending debounced writes before the window actually closes (§9).
  await appWindow.onCloseRequested(async () => {
    if (textTimer) clearTimeout(textTimer);
    await writeText(titleEl.value, bodyEl.innerText);
    await flush();
    // not preventing default: let the close proceed after flush resolves
  });
});
```

- `bodyEl.innerText` (not `innerHTML`) gives clean line-broken plain text — see the
  `innerText` gotcha (§9).
- Text saves are **debounced 400ms** so we don't hit disk on every keystroke.

---

## 8. Rust side — why Phase 1 is JS-only, with forward-compatible stubs

**Decision:** do Phase 1 persistence entirely through the store plugin from JS.

Justification:

- The store plugin already exposes a transactional, debounced JSON file from the
  webview. For a single note there is no shared state, no scheduler, and nothing that
  must outlive the window — so a Rust round-trip would be pure overhead.
- It keeps Phase 1 small and lets us validate the window behavior (drag, transparency,
  always-on-top, geometry persistence) without backend plumbing.
- The architecture (plan §Architecture) wants **Rust as source of truth** once timers
  exist (Phase 3), because a timer must keep running after its window closes. That is
  the right moment to move ownership into Rust — not before.

To avoid a disruptive rewrite later, add **no-op command stubs now** that match the
Phase 2/3 signatures. The frontend does *not* call them yet, but the contract is
fixed and `invoke_handler` is already wired (§4.2).

`src-tauri/src/lib.rs` (stubs):

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WindowGeom {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub title: String,
    pub content: String,
    pub color: serde_json::Value, // tighten in Phase 5
    pub window: WindowGeom,
    // timer / schedule fields land in Phase 3 / 4
}

/// Phase 2/3: Rust becomes source of truth. For now this is a stub the JS
/// layer does not call — persistence happens via the store plugin in JS.
#[tauri::command]
async fn save_task(_task: Task) -> Result<(), String> {
    // TODO(phase2): write into Rust-owned state + store
    Ok(())
}

#[tauri::command]
async fn load_task() -> Result<Option<Task>, String> {
    // TODO(phase2): read from Rust-owned state
    Ok(None)
}
```

When Phase 2/3 lands, fill these in (backed by `Mutex<HashMap<Id, Task>>` + store
sync per plan §Phase 2) and switch the JS `store.ts` calls to `invoke("save_task", …)`
/ `invoke("load_task")`. The data shape (`Task`) already matches the JS `NoteData`.

---

## 9. Gotchas

1. **Transparent window + rounded corners on Linux.** The window itself is a plain
   rectangle; the rounding is pure CSS (`border-radius` on `.note`). For the corners
   to show as transparent you need: (a) `transparent: true` in config, (b)
   `html, body { background: transparent }`, and (c) a running **compositor** (most
   GNOME/KDE setups have one; bare tiling WMs without a compositor like `picom` will
   render the "transparent" area black). Set `shadow: false` so the OS doesn't draw a
   square shadow around the rounded card; use a CSS `box-shadow` instead, and leave an
   `inset` gap on `.note` so the shadow isn't clipped at the window edge.

2. **Drag region must not cover the inputs.** Keep `data-tauri-drag-region` on the
   isolated `.note__drag` strip only. If you put it on a parent that contains the
   title/body, clicking to position the caret will instead start a window drag and the
   fields feel "dead." Also set `user-select: text` back on the editable elements
   (the body/html disable selection globally to make dragging cleaner).

3. **Autosave vs quit flush.** `autoSave` debounces disk writes (we use 400ms). If the
   user types and immediately quits, the last edit can be lost because the debounce
   timer never fires. Guard with `onCloseRequested` (§7): cancel the JS debounce,
   write the latest text synchronously, and `await store.save()` to force a flush
   before the window closes.

4. **Physical vs logical pixel drift.** `outerPosition()` / `innerSize()` return
   **physical** pixels (already multiplied by the monitor scale factor). If you save
   physical px and later restore with `LogicalPosition`/`LogicalSize`, the window
   drifts on HiDPI displays (scale ≠ 1). Stay consistent: save physical, restore with
   `PhysicalPosition` / `PhysicalSize` (as in §5–§6). Also note we save **outer**
   position but **inner** size — restoring inner size sets the content area, while the
   frameless window has effectively no decoration offset, so they line up; just don't
   mix `outerSize` into the save path.

5. **`innerText` vs `innerHTML` / `textContent`.** Use `bodyEl.innerText` to extract
   the body: it preserves visible line breaks as `\n` and ignores hidden markup,
   giving clean storable plain text. `innerHTML` would persist `<div>`/`<br>` soup the
   browser injects on Enter; `textContent` collapses line breaks. On restore, write
   back with `bodyEl.innerText = saved` so newlines round-trip.

6. **Empty-store first run.** On first launch the store file doesn't exist; `get`
   returns `null`/`undefined`. All readers default (`?? ""` / `?? undefined`) so the
   note opens at the configured 260×260 with placeholder text — no crash.

---

## 10. Done when…

Manual acceptance test:

1. `npm run tauri dev` → a small frameless yellow card appears, floating above other
   windows (always-on-top), with a transparent rounded corner.
2. Drag the top strip → the window moves. Dragging the title/body does **not** move it
   and instead edits text.
3. Type a **title** and some **body text** (with line breaks).
4. Move the window to a new spot and resize it.
5. Quit the app (close the window).
6. Relaunch (`npm run tauri dev` again, or the built binary).
7. ✅ The window reopens at the **same position and size**, with the **same title and
   body text** (line breaks intact) and color.

If any of those fail, check in order: capability permissions (drag / set-position /
store), `transparent`+compositor (black corners), and the `onCloseRequested` flush
(lost last keystroke).
