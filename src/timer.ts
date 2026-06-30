import { invoke } from "@tauri-apps/api/core";
import type { Task, TimerMode, TimerState } from "./types";

export type TickPayload = {
  id: string;
  remaining_secs: number;
  elapsed_secs: number;
  state: TimerState;
};

export const fmt = (s: number): string => {
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
};

const parse = (mmss: string): number => {
  const parts = mmss.split(":").map((n) => parseInt(n, 10) || 0);
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  return parts[0] || 0;
};

const el = <T extends HTMLElement>(elId: string): T =>
  document.getElementById(elId) as T;

// The single window's detail view operates on one task at a time.
let currentId: string | null = null;
let mode: TimerMode = "countdown";
let state: TimerState = "idle";
let editing = false;

function applyModeUi(): void {
  el("timer").setAttribute("data-mode", mode);
  el("mode-countdown").classList.toggle("active", mode === "countdown");
  el("mode-stopwatch").classList.toggle("active", mode === "stopwatch");
}

/** Idle countdown is the only state where the clock can be retimed by click. */
function canEdit(): boolean {
  return state === "idle" && mode === "countdown";
}

/** Push mode + duration to Rust. Used by the mode toggle and clock edit. */
function applyConfig(): void {
  if (!currentId) return;
  const dur = parse(el<HTMLInputElement>("dur-input").value);
  applyModeUi();
  void invoke("configure_timer", {
    id: currentId,
    mode,
    durationSecs: mode === "countdown" ? dur : 0,
  });
}

function openEdit(): void {
  if (!canEdit()) return;
  editing = true;
  const disp = el("timer-display");
  const input = el<HTMLInputElement>("dur-input");
  input.value = disp.textContent?.trim() ?? "";
  disp.hidden = true;
  input.hidden = false;
  input.focus();
  input.select();
}

function closeEdit(): void {
  editing = false;
  el<HTMLInputElement>("dur-input").hidden = true;
  el("timer-display").hidden = false;
}

/** Human-readable timer status line, e.g. "countdown · idle", "running". */
function statusText(s: TimerState): string {
  switch (s) {
    case "running":
      return "running";
    case "paused":
      return "paused";
    case "done":
      return "timer finished";
    default:
      return `${mode} · idle`;
  }
}

/** Update the detail view's clock + controls from a Rust snapshot. */
export function renderTimerTick(p: TickPayload): void {
  state = p.state;

  const disp = el("timer-display");
  disp.textContent =
    mode === "countdown" ? fmt(p.remaining_secs) : fmt(p.elapsed_secs);
  disp.classList.toggle("done", p.state === "done");
  const editable = canEdit();
  disp.classList.toggle("editable", editable);
  // Only hint the click-to-edit when it actually does something.
  if (editable) disp.title = "Click to set the time";
  else disp.removeAttribute("title");

  el("timer-status").textContent = statusText(p.state);

  const running = p.state === "running";
  const toggle = el<HTMLButtonElement>("btn-toggle");
  toggle.textContent = running ? "pause" : "start";
  toggle.classList.toggle("running", running);
  toggle.disabled = p.state === "done";

  // On done, swap the normal controls for the finished action block so
  // start/reset/done aren't shown twice.
  const finished = p.state === "done";
  el("timer-controls").hidden = finished;
  el("timer-finished").hidden = !finished;
}

/** Wire the detail timer controls ONCE. Handlers read the current task id. */
export function setupTimer(): void {
  el("btn-toggle").addEventListener("click", () => {
    if (!currentId) return;
    void invoke(state === "running" ? "pause_timer" : "start_timer", {
      id: currentId,
    });
  });
  el("btn-reset").addEventListener("click", () => {
    if (currentId) void invoke("reset_timer", { id: currentId });
  });

  // Click the clock to retime it (idle countdown only).
  el("timer-display").addEventListener("click", openEdit);

  const input = el<HTMLInputElement>("dur-input");
  input.addEventListener("change", () => {
    applyConfig(); // emit_now from Rust re-renders the display text
    closeEdit();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") input.blur();
    else if (e.key === "Escape") closeEdit();
  });
  input.addEventListener("blur", () => {
    if (editing) closeEdit();
  });

  el("mode-countdown").addEventListener("click", () => {
    mode = "countdown";
    applyConfig();
  });
  el("mode-stopwatch").addEventListener("click", () => {
    mode = "stopwatch";
    applyConfig();
  });
}

/** Point the detail timer at a task and render its current state. */
export function loadTimer(task: Task): void {
  currentId = task.id;
  mode = task.timer.mode;
  closeEdit();
  el<HTMLInputElement>("dur-input").value = fmt(task.timer.duration_secs);
  applyModeUi();
  renderTimerTick({
    id: task.id,
    remaining_secs: task.timer.remaining_secs,
    elapsed_secs: task.timer.elapsed_secs,
    state: task.timer.state,
  });
}

/** Called when leaving the detail view, so stray ticks are ignored. */
export function unloadTimer(): void {
  currentId = null;
}

export type { TimerMode };
