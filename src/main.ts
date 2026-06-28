import "./styles.css";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { initTimer } from "./timer";
import { initSchedule } from "./schedule";
import type { Task } from "./types";

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

  // Timer + schedule (Rust-driven). Keep unlisten handles for reload cleanup.
  const unlisteners: UnlistenFn[] = [
    ...(await initTimer(task)),
    ...(await initSchedule(task)),
  ];
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
