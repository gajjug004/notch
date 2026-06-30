import "./styles.css";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { load as loadStore } from "@tauri-apps/plugin-store";
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

/** Active tasks whose schedule fired without auto-start: awaiting a Start/snooze.
 *  Frontend-only — the backend clears one-shots on fire. */
const firedPending = new Set<string>();
/** Whether the list currently reveals done tasks. */
let showDone = false;

// ---- list view ------------------------------------------------------------

function timerText(t: Timer): string {
  return t.mode === "stopwatch" ? fmt(t.elapsed_secs) : fmt(t.remaining_secs);
}

/** Urgency rank for list ordering: lower = nearer the top (needs attention). */
function urgencyRank(task: Task): number {
  if (task.status === "done") return 5;
  if (firedPending.has(task.id)) return 0; // schedule fired, awaiting start
  if (task.timer.state === "done") return 1; // countdown finished
  if (task.timer.state === "running") return 2;
  if (task.schedule.kind !== "none") return 3; // upcoming schedule
  return 4; // unscheduled active
}

/** Small status badges for a row, in priority order. */
function rowBadges(task: Task): { text: string; cls: string }[] {
  // Done is terminal: a single badge, no timer/schedule noise.
  if (task.status === "done") return [{ text: "done", cls: "done" }];
  const out: { text: string; cls: string }[] = [];
  if (firedPending.has(task.id))
    out.push({ text: "needs start", cls: "fired" });
  if (task.timer.state === "done") out.push({ text: "finished", cls: "finished" });
  if (task.timer.state === "running") out.push({ text: "running", cls: "running" });
  if (task.schedule.kind !== "none")
    out.push({ text: "scheduled", cls: "scheduled" });
  return out;
}

async function renderList(): Promise<void> {
  const tasks = await invoke<Task[]>("list_tasks");
  tasks.sort((a, b) => {
    const r = urgencyRank(a) - urgencyRank(b);
    return r !== 0 ? r : a.title.localeCompare(b.title);
  });

  const active = tasks.filter((t) => t.status !== "done");
  const done = tasks.filter((t) => t.status === "done");
  const visible = showDone ? [...active, ...done] : active;

  const listEl = el("task-list");
  listEl.replaceChildren();
  rows.clear();

  // Empty hint shows only when there are no active tasks. The top bar keeps
  // "+ New task", so the empty hint just adds "Open settings".
  el("list-empty").hidden = active.length > 0;

  // Done toggle reflects count + state; hidden when nothing is done.
  const toggle = el<HTMLButtonElement>("done-toggle");
  toggle.hidden = done.length === 0;
  toggle.textContent = showDone ? `Hide done (${done.length})` : `Done (${done.length})`;
  toggle.classList.toggle("active", showDone);

  for (const task of visible) {
    const root = document.createElement("button");
    root.type = "button";
    root.className = "task-row";
    if (task.timer.state === "running") root.classList.add("running");
    if (task.status === "done") root.classList.add("is-done");

    const main = document.createElement("span");
    main.className = "task-row__main";
    const title = document.createElement("span");
    title.className = "task-row__title";
    title.textContent = task.title || "(untitled)";

    const meta = document.createElement("span");
    meta.className = "task-row__meta";
    for (const b of rowBadges(task)) {
      const badge = document.createElement("span");
      badge.className = `task-row__badge task-row__badge--${b.cls}`;
      badge.textContent = b.text;
      meta.appendChild(badge);
    }
    const sched = document.createElement("span");
    sched.className = "task-row__sched";
    sched.textContent = scheduleBadge(task.schedule);
    meta.appendChild(sched);

    main.append(title, meta);

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
  showDeleteConfirm(false);
  unloadTimer();
  unloadSchedule();
  el("detail-view").hidden = true;
  el("list-view").hidden = false;
  void renderList();
}

// ---- detail view ----------------------------------------------------------

function setDoneButton(status: Task["status"]): void {
  el("btn-done").textContent = status === "done" ? "reopen" : "done";
}

function showDeleteConfirm(show: boolean): void {
  el("delete").hidden = show;
  el("del-confirm").hidden = !show;
}

function scheduleSave(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    if (detailTask) void invoke("save_task", { task: detailTask });
  }, 400);
}

async function openDetail(id: string): Promise<void> {
  const task = await invoke<Task>("get_task", { id });
  detailTask = task;
  firedPending.delete(id); // viewing it resolves the "needs start" prompt

  el<HTMLInputElement>("title").value = task.title;
  el<HTMLDivElement>("body").innerText = task.content;

  loadTimer(task);
  loadSchedule(task);
  setDoneButton(task.status);
  showDeleteConfirm(false);

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
  setupSchedule((id) => {
    firedPending.delete(id);
    if (!el("list-view").hidden) void renderList();
  });

  // List view actions.
  const createTask = async () => {
    const task = await invoke<Task>("create_task");
    await openDetail(task.id);
  };
  el("new-task").addEventListener("click", () => void createTask());
  el("empty-settings").addEventListener("click", () => void invoke("open_settings"));
  el("done-toggle").addEventListener("click", () => {
    showDone = !showDone;
    void renderList();
  });
  el("list-settings").addEventListener("click", () => void invoke("open_settings"));
  el("list-hide").addEventListener("click", () => void appWindow.hide());

  // Detail view actions.
  el("back").addEventListener("click", () => {
    flushSave();
    showList();
  });
  el("detail-hide").addEventListener("click", () => void appWindow.hide());

  // Done / Reopen toggle.
  el("btn-done").addEventListener("click", () => {
    if (!detailTask) return;
    if (detailTask.status === "done") {
      void invoke("reopen_task", { id: detailTask.id });
      detailTask.status = "active";
      setDoneButton("active");
    } else {
      void invoke("complete_task", { id: detailTask.id });
      showList();
    }
  });

  // Inline delete confirm: 🗑 → delete / ✕.
  el("delete").addEventListener("click", () => showDeleteConfirm(true));
  el("del-no").addEventListener("click", () => showDeleteConfirm(false));
  el("del-yes").addEventListener("click", () => {
    if (!detailTask) return;
    void invoke("delete_task", { id: detailTask.id });
    showList();
  });

  // Finished-countdown actions.
  el("fin-reset").addEventListener("click", () => {
    if (detailTask) void invoke("reset_timer", { id: detailTask.id });
  });
  el("fin-restart").addEventListener("click", async () => {
    if (!detailTask) return;
    await invoke("reset_timer", { id: detailTask.id });
    void invoke("start_timer", { id: detailTask.id });
  });
  el("fin-done").addEventListener("click", () => {
    if (!detailTask) return;
    void invoke("complete_task", { id: detailTask.id });
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
  await listen<string>("schedule-fired", (e) => {
    firedPending.add(e.payload);
    onScheduleFired(e.payload);
    if (!el("list-view").hidden) void renderList();
  });

  // Task set changed (create/delete/edit) → refresh the list if it's showing.
  await listen<unknown>("tasks-changed", () => {
    if (!el("list-view").hidden) void renderList();
  });

  // Global pause indicator: body class + persistent banner in both views.
  const setPaused = (paused: boolean) => {
    document.body.classList.toggle("paused", paused);
    for (const b of document.querySelectorAll<HTMLElement>(".pause-banner")) {
      b.hidden = !paused;
    }
  };
  await listen<boolean>("global-pause", (e) => setPaused(e.payload));
  // Reflect the persisted pause state on boot.
  try {
    const s = await loadStore("settings.json");
    setPaused((await s.get<boolean>("globalPause")) ?? false);
  } catch {
    /* store missing → unpaused */
  }

  // Flush a pending edit if the window is hidden/closed mid-edit.
  await appWindow.onCloseRequested(() => flushSave());

  // Start on the list.
  showList();
});
