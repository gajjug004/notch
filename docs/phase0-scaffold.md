# Phase 0 — Scaffold

Status: **Done** (this document records what was done and how to verify it).

## Goal

An empty **Tauri v2** application that **builds and runs on Linux** with a
**vanilla TypeScript** frontend (no UI framework, minimal HTML/CSS/TS), and a
**git** repository initialized.

Done when:
- `npm run tauri dev` opens a blank window with no errors.
- Rust + Node toolchains and Linux system deps are present and verifiable.
- `.gitignore` excludes build artifacts; repo is initialized.

This is the foundation for Phase 1 (frameless / transparent / always-on-top
sticky note window), so this phase intentionally leaves the default window
config in place and flags the fields Phase 1 will change.

---

## 1. Environment (verified on this machine)

| Component | Required | Installed |
|-----------|----------|-----------|
| OS | Linux (Ubuntu 24.04) | Ubuntu 24.04.4 LTS |
| Rust (rustc/cargo) | stable | `rustc 1.96.0`, `cargo 1.96.0` |
| rustup | any | `1.29.0` |
| Node.js | 18+ (LTS) | `v24.12.0` |
| npm | 9+ | `11.11.0` |
| webkit2gtk | **4.1** | `2.52.3` (via `libwebkit2gtk-4.1-dev`) |
| ayatana-appindicator3 | 0.1 | `0.5.90` |
| librsvg-2.0 | any | `2.58.0` |

Tauri v2 stack: `@tauri-apps/api ^2`, `@tauri-apps/cli ^2`,
`tauri = "2"`, `tauri-build = "2"`, plus the default `tauri-plugin-opener "2"`.

---

## 2. Linux system dependencies

Tauri v2 on Linux builds against **webkit2gtk 4.1** (not 4.0 — see Gotchas).
Install the full dev toolchain before scaffolding:

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

What each provides:
- `libwebkit2gtk-4.1-dev` — the WebView Tauri renders the frontend in (required).
- `build-essential` — gcc/g++/make, needed to compile native code.
- `curl`, `wget`, `file` — used by rustup install and the Tauri bundler.
- `libxdo-dev` — X11 automation, required by Tauri's input handling on Linux.
- `libssl-dev` — OpenSSL headers for Rust TLS crates.
- `libayatana-appindicator3-dev` — **system tray icon** support (needed Phase 2).
- `librsvg2-dev` — SVG rendering, used for icon generation/bundling.

---

## 3. Rust toolchain (rustup)

Installed via the official rustup script (stable channel):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# then load cargo into the current shell:
source "$HOME/.cargo/env"
```

rustup installs the toolchain under `~/.cargo/bin` (rustc, cargo, rustup, …).
If `rustc`/`cargo` are "command not found" in a fresh shell, the PATH line was
not sourced — add `source "$HOME/.cargo/env"` to your shell rc, or run
`~/.cargo/bin/rustc --version` directly.

Verify:

```bash
rustc --version     # rustc 1.96.0 (...)
cargo --version     # cargo 1.96.0 (...)
rustup show         # active toolchain = stable
```

---

## 4. Scaffold command

> **WARNING — `-f` / `--force` overwrites existing files.**
> The project directory was **not empty** when scaffolded (it already held
> `plan.md`, `docs/`, etc.), so `-f` was required. `-f` will **overwrite** any
> colliding files without asking (`package.json`, `index.html`,
> `tsconfig.json`, `src/`, `src-tauri/`, `.gitignore`, …). **Back up the
> directory first** (`git stash`, a copy, or commit) before re-running this.

Exact command used:

```bash
npm create tauri-app@latest sticky-timer -- \
  --template vanilla-ts \
  --manager npm \
  --identifier com.stickytimer.app \
  -y -f
```

Flag breakdown:
- `--template vanilla-ts` — vanilla TypeScript frontend, **no UI framework**.
- `--manager npm` — use npm (writes `package-lock.json`, npm scripts).
- `--identifier com.stickytimer.app` — reverse-DNS app identifier.
- `-y` — accept all defaults non-interactively.
- `-f` — force into a non-empty directory (overwrites — see warning above).

Install JS deps and (optionally) trigger the first Rust build:

```bash
cd sticky-timer
npm install
```

---

## 5. Resulting directory tree

```
sticky-timer/
├── index.html                 # frontend entry (Vite)
├── package.json               # npm scripts + Tauri JS deps
├── package-lock.json
├── tsconfig.json              # TypeScript config (vanilla-ts)
├── vite.config.ts             # Vite dev server (port 1420, HMR)
├── .gitignore                 # node_modules, dist, *.log, editor files
├── README.md
├── plan.md                    # project plan (pre-existing)
├── docs/
│   └── phase0-scaffold.md     # this file
├── .vscode/
│   └── extensions.json        # recommends tauri-vscode + rust-analyzer
├── src/                       # === FRONTEND (web) ===
│   ├── main.ts                # TS entry (demo greet wiring)
│   ├── styles.css
│   └── assets/                # tauri.svg, typescript.svg, vite.svg
└── src-tauri/                 # === BACKEND (Rust) ===
    ├── Cargo.toml             # Rust deps (tauri 2, opener, serde)
    ├── Cargo.lock
    ├── build.rs               # calls tauri_build::build()
    ├── tauri.conf.json        # Tauri app config (see §6)
    ├── .gitignore             # /target/, /gen/schemas
    ├── src/
    │   ├── main.rs            # bin entry → sticky_timer_lib::run()
    │   └── lib.rs             # app builder + demo `greet` command
    ├── capabilities/
    │   └── default.json       # permissions for the "main" window
    ├── icons/                 # generated app icons (png/icns/ico/...)
    └── target/                # Rust build output (gitignored)
```

### Key files (as scaffolded)

`src-tauri/src/main.rs`:
```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    sticky_timer_lib::run()
}
```

`src-tauri/src/lib.rs` (the real entry point — Phase 1+ wiring goes here):
```rust
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`src-tauri/capabilities/default.json` — capability granting permissions to the
`main` window. New windows created at runtime (Phase 2) will need their labels
added here or a new capability file.
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": ["core:default", "opener:default"]
}
```

`src-tauri/Cargo.toml` (dependencies section):
```toml
[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-opener = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

`package.json` scripts:
```json
"scripts": {
  "dev": "vite",
  "build": "tsc && vite build",
  "preview": "vite preview",
  "tauri": "tauri"
}
```

---

## 6. Key `tauri.conf.json` fields

Current scaffolded config (`src-tauri/tauri.conf.json`):

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "sticky-timer",
  "version": "0.1.0",
  "identifier": "com.stickytimer.app",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "withGlobalTauri": true,
    "windows": [
      {
        "title": "sticky-timer",
        "width": 800,
        "height": 600
      }
    ],
    "security": { "csp": null }
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

Field notes:
- `identifier` = `com.stickytimer.app` (matches the scaffold flag; must be
  reverse-DNS and unique — it drives the bundle id and the app data dir).
- `build.devUrl` = `http://localhost:1420` — must match the Vite port in
  `vite.config.ts`. `beforeDevCommand`/`beforeBuildCommand` chain the Vite
  dev/build steps automatically when you run `npm run tauri dev|build`.
- `app.withGlobalTauri: true` — exposes `window.__TAURI__` in the frontend
  (handy for vanilla TS without importing modules).
- `app.windows[0]` — the single placeholder window. **Phase 1 will turn this
  sticky.** The fields to add then (kept here as a commented reference — JSON
  has no comments, so these are documentation only):

  ```jsonc
  {
    "title": "sticky-note",
    "width": 280,
    "height": 280,
    // --- Phase 1 sticky-note window props ---
    // "decorations": false,    // frameless (no titlebar/border)
    // "transparent": true,     // transparent bg for rounded card
    // "alwaysOnTop": true,     // float above other windows
    // "resizable": true,
    // "skipTaskbar": true,     // optional: keep notes out of taskbar
    // "shadow": false
  }
  ```
  Note: `transparent: true` also requires the macOS-only
  `macOSPrivateApi` flag on macOS; on Linux it depends on the compositor
  (see Gotchas). For multi-window (Phase 2) windows are created at runtime via
  the `WebviewWindowBuilder` rather than declared here.

---

## 7. `.gitignore`

Two `.gitignore` files are present.

Project root `/.gitignore` (frontend + general) — relevant excludes:
```gitignore
# Logs
logs
*.log
npm-debug.log*
...

node_modules
dist
dist-ssr
*.local

# Editor directories and files
.vscode/*
!.vscode/extensions.json
.idea
.DS_Store
*.sw?
```

`src-tauri/.gitignore` (Rust build output) — **this is where `target/` is
ignored**:
```gitignore
# Generated by Cargo
/target/

# Generated by Tauri (capability autocompletion schemas)
/gen/schemas
```

Together these cover the plan's required excludes: **`node_modules/`**,
**`dist/`**, and **`src-tauri/target/`**.

Git init (run at repo root if not already a repo):
```bash
git init
git add .
git commit -m "Phase 0: scaffold Tauri v2 vanilla-ts app"
```

---

## 8. Done-when verification

Run each and confirm the expected result:

```bash
# 1. Toolchains
rustc --version          # -> rustc 1.96.0 (...)
cargo --version          # -> cargo 1.96.0 (...)
node --version           # -> v24.12.0 (>= 18)
npm --version            # -> 11.11.0

# 2. Linux WebView dependency (MUST be 4.1, not 4.0)
pkg-config --modversion webkit2gtk-4.1     # -> 2.52.3 (any 2.x; key is the 4.1 API)
pkg-config --modversion librsvg-2.0        # -> 2.58.0
pkg-config --modversion ayatana-appindicator3-0.1   # -> 0.5.90

# 3. Frontend deps installed
npm install

# 4. Run it — first run compiles the Rust crate (slow), then opens a window
npm run tauri dev
```

Success criteria:
- `npm run tauri dev` compiles `sticky-timer` and **opens a window** showing the
  default Tauri/vanilla-ts page.
- No Rust compile errors, no WebKit/GTK runtime errors in the terminal.
- Window renders content (not blank — if blank, see Gotchas).
- `git status` is clean / artifacts (`target/`, `node_modules/`, `dist/`) are
  ignored.

Optional release build sanity check (slower, produces a `.deb`/AppImage):
```bash
npm run tauri build
```

---

## 9. Linux gotchas

- **Blank / white window on launch.** Common with newer WebKitGTK + some GPU
  drivers (DMABUF renderer bug). Workaround — disable the DMABUF renderer:
  ```bash
  WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri dev
  ```
  If that fixes it, export the var in your shell or a `.env` for dev.

- **webkit2gtk 4.1 vs 4.0.** Tauri **v2** links against the **4.1** API.
  Ubuntu also ships `libwebkit2gtk-4.0-dev` (used by Tauri v1) — installing the
  4.0 package does **not** satisfy v2. Build errors mentioning
  `javascriptcoregtk-4.1` / `webkit2gtk-4.1` not found mean the **4.1** `-dev`
  package is missing. Verify with `pkg-config --modversion webkit2gtk-4.1`.

- **Transparency on Wayland vs X11.** `transparent: true` (needed Phase 1)
  behaves inconsistently under Wayland and some compositors — you may see a
  black/opaque background instead of true transparency. Forcing the X11 backend
  is the reliable workaround:
  ```bash
  GDK_BACKEND=x11 npm run tauri dev
  ```
  Also relevant for frameless-window dragging and always-on-top edge cases.

- **System tray needs libayatana-appindicator.** The tray icon (Phase 2) won't
  appear without `libayatana-appindicator3-dev` installed. On some desktops
  (e.g. GNOME) a tray/appindicator extension must also be enabled for the icon
  to be visible.

- **First `tauri dev` is slow.** It compiles the entire Rust dependency tree
  once; subsequent runs are incremental and fast. Don't mistake the long first
  compile for a hang.

- **`cargo`/`rustc` not found in new shells.** rustup writes PATH to
  `~/.cargo/env`; if your shell rc doesn't source it, commands fail. Fix:
  `source "$HOME/.cargo/env"` (or use `~/.cargo/bin/...`).
