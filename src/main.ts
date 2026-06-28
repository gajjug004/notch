import "./styles.css";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { initTimer } from "./timer";
import { initSchedule } from "./schedule";
import { initSound } from "./sound";
import type { Task } from "./types";

const PALETTE = [
  "#fff7b1", // yellow
  "#ffd2a8", // peach
  "#ffb3ba", // pink
  "#b8e6c1", // green
  "#a8d8ff", // blue
  "#d9c2ff", // purple
  "#e6e6e6", // grey
];

function buildPalette(currentColor: string, onPick: (c: string) => void): void {
  const wrap = document.getElementById("palette") as HTMLDivElement;
  for (const c of PALETTE) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "swatch";
    b.style.background = c;
    if (c.toLowerCase() === currentColor.toLowerCase())
      b.classList.add("active");
    b.addEventListener("click", () => {
      wrap
        .querySelectorAll(".swatch")
        .forEach((s) => s.classList.remove("active"));
      b.classList.add("active");
      onPick(c);
    });
    wrap.appendChild(b);
  }
}

const appWindow = getCurrentWindow();

/** The window label IS the task id; query string is the explicit fallback. */
function resolveTaskId(): string {
  const fromQuery = new URLSearchParams(window.location.search).get("id");
  return fromQuery ?? appWindow.label;
}

let task: Task;
let saveTimer: number | undefined;

function scheduleSave(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    void invoke("save_task", { task });
  }, 400);
}

function applyColor(color: string): void {
  document.documentElement.style.setProperty("--note-bg", color);
}

async function captureGeometry(): Promise<void> {
  const factor = await appWindow.scaleFactor();
  const pos = (await appWindow.outerPosition()).toLogical(factor);
  const size = (await appWindow.innerSize()).toLogical(factor);
  task.window = {
    x: Math.round(pos.x),
    y: Math.round(pos.y),
    w: Math.round(size.width),
    h: Math.round(size.height),
  };
}

window.addEventListener("DOMContentLoaded", async () => {
  const id = resolveTaskId();
  task = await invoke<Task>("get_task", { id });

  const titleEl = document.getElementById("title") as HTMLInputElement;
  const bodyEl = document.getElementById("body") as HTMLDivElement;
  const delEl = document.getElementById("delete") as HTMLButtonElement;

  // Render
  applyColor(task.color);
  titleEl.value = task.title;
  bodyEl.innerText = task.content;

  // Color palette → live apply + persist.
  buildPalette(task.color, (color) => {
    applyColor(color);
    void invoke("set_task_color", { id: task.id, color });
  });

  await initSound();

  // Timer + schedule (Rust-driven). Keep unlisten handles for reload cleanup.
  const unlisteners: UnlistenFn[] = [
    ...(await initTimer(task)),
    ...(await initSchedule(task)),
  ];

  // Global pause indicator.
  unlisteners.push(
    await listen<boolean>("global-pause", (e) => {
      document.body.classList.toggle("paused", e.payload);
    }),
  );

  window.addEventListener("beforeunload", () => {
    unlisteners.forEach((u) => u());
  });

  // Edits → debounced save
  const onEdit = () => {
    task.title = titleEl.value;
    task.content = bodyEl.innerText;
    scheduleSave();
  };
  titleEl.addEventListener("input", onEdit);
  bodyEl.addEventListener("input", onEdit);

  // Move / resize → debounced save (geometry in logical px)
  const onGeom = async () => {
    await captureGeometry();
    scheduleSave();
  };
  await appWindow.onMoved(() => void onGeom());
  await appWindow.onResized(() => void onGeom());

  // Delete this note
  delEl.addEventListener("click", () => {
    void invoke("delete_task", { id: task.id });
  });

  // Flush pending edits before close
  await appWindow.onCloseRequested(async () => {
    if (saveTimer) clearTimeout(saveTimer);
    task.title = titleEl.value;
    task.content = bodyEl.innerText;
    await invoke("save_task", { task });
  });
});
