import "./styles.css";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  fmt,
  loadTimer,
  renderTimerTick,
  setupTimer,
  unloadTimer,
  type TickPayload,
} from "./timer";
import {
  loadSchedule,
  onScheduleFired,
  scheduleBadge,
  setupSchedule,
  unloadSchedule,
} from "./schedule";
import { initSound, playAlert } from "./sound";
import type { Task, Timer } from "./types";

const appWindow = getCurrentWindow();
const el = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

// ---- view state -----------------------------------------------------------

/** The task currently open in the detail view, or null when showing the list. */
let detailTask: Task | null = null;
/** Live row handles for the list view, keyed by task id. */
const rows = new Map<
  string,
  { time: HTMLElement; root: HTMLElement; mode: Timer["mode"] }
>();
let saveTimer: number | undefined;

// ---- list view ------------------------------------------------------------

function timerText(t: Timer): string {
  return t.mode === "stopwatch" ? fmt(t.elapsed_secs) : fmt(t.remaining_secs);
}

async function renderList(): Promise<void> {
  const tasks = await invoke<Task[]>("list_tasks");
  tasks.sort((a, b) => a.title.localeCompare(b.title));

  const listEl = el("task-list");
  listEl.replaceChildren();
  rows.clear();

  el("list-empty").hidden = tasks.length > 0;

  for (const task of tasks) {
    const root = document.createElement("button");
    root.type = "button";
    root.className = "task-row";
    if (task.timer.state === "running") root.classList.add("running");

    const main = document.createElement("span");
    main.className = "task-row__main";
    const title = document.createElement("span");
    title.className = "task-row__title";
    title.textContent = task.title || "(untitled)";
    const sched = document.createElement("span");
    sched.className = "task-row__sched";
    sched.textContent = scheduleBadge(task.schedule);
    main.append(title, sched);

    const time = document.createElement("span");
    time.className = "task-row__time tabular";
    time.textContent = timerText(task.timer);

    root.append(main, time);
    root.addEventListener("click", () => void openDetail(task.id));
    listEl.appendChild(root);
    rows.set(task.id, { time, root, mode: task.timer.mode });
  }
}

function showList(): void {
  detailTask = null;
  unloadTimer();
  unloadSchedule();
  el("detail-view").hidden = true;
  el("list-view").hidden = false;
  void renderList();
}

// ---- detail view ----------------------------------------------------------

function scheduleSave(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    if (detailTask) void invoke("save_task", { task: detailTask });
  }, 400);
}

async function openDetail(id: string): Promise<void> {
  const task = await invoke<Task>("get_task", { id });
  detailTask = task;

  el<HTMLInputElement>("title").value = task.title;
  el<HTMLDivElement>("body").innerText = task.content;

  loadTimer(task);
  loadSchedule(task);

  el("list-view").hidden = true;
  el("detail-view").hidden = false;
}

function flushSave(): void {
  if (!detailTask) return;
  if (saveTimer) clearTimeout(saveTimer);
  detailTask.title = el<HTMLInputElement>("title").value;
  detailTask.content = el<HTMLDivElement>("body").innerText;
  void invoke("save_task", { task: detailTask });
}

// ---- wiring (once) --------------------------------------------------------

window.addEventListener("DOMContentLoaded", async () => {
  await initSound();
  setupTimer();
  setupSchedule();

  // List view actions.
  el("new-task").addEventListener("click", async () => {
    const task = await invoke<Task>("create_task");
    await openDetail(task.id);
  });
  el("list-hide").addEventListener("click", () => void appWindow.hide());

  // Detail view actions.
  el("back").addEventListener("click", () => {
    flushSave();
    showList();
  });
  el("detail-hide").addEventListener("click", () => void appWindow.hide());
  el("delete").addEventListener("click", () => {
    if (!detailTask) return;
    void invoke("delete_task", { id: detailTask.id });
    showList();
  });

  const onEdit = () => {
    if (!detailTask) return;
    detailTask.title = el<HTMLInputElement>("title").value;
    detailTask.content = el<HTMLDivElement>("body").innerText;
    scheduleSave();
  };
  el("title").addEventListener("input", onEdit);
  el("body").addEventListener("input", onEdit);

  // Global events from Rust (single window; route by payload.id).
  await listen<TickPayload>("timer-tick", (e) => {
    const p = e.payload;
    const row = rows.get(p.id);
    if (row) {
      row.time.textContent = fmt(
        row.mode === "stopwatch" ? p.elapsed_secs : p.remaining_secs,
      );
      row.root.classList.toggle("running", p.state === "running");
    }
    if (detailTask && detailTask.id === p.id) renderTimerTick(p);
  });

  await listen<{ id: string }>("timer-done", (e) => {
    void playAlert();
    const row = rows.get(e.payload.id);
    if (row) row.root.classList.remove("running");
  });

  // Schedule fired with no auto-start: chime + surface Start in the open task.
  await listen<unknown>("play-sound", () => void playAlert());
  await listen<string>("schedule-fired", (e) => onScheduleFired(e.payload));

  // Task set changed (create/delete/edit) → refresh the list if it's showing.
  await listen<unknown>("tasks-changed", () => {
    if (!el("list-view").hidden) void renderList();
  });

  // Global pause indicator.
  await listen<boolean>("global-pause", (e) => {
    document.body.classList.toggle("paused", e.payload);
  });

  // Flush a pending edit if the window is hidden/closed mid-edit.
  await appWindow.onCloseRequested(() => flushSave());

  // Start on the list.
  showList();
});
