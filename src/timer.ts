import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Task, TimerMode, TimerState } from "./types";

type TickPayload = {
  id: string;
  remaining_secs: number;
  elapsed_secs: number;
  state: TimerState;
};

const id = getCurrentWindow().label; // == task id

const fmt = (s: number): string => {
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
};

const parse = (mmss: string): number => {
  const parts = mmss.split(":").map((n) => parseInt(n, 10) || 0);
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  return parts[0] || 0;
};

let mode: TimerMode = "countdown";

const el = <T extends HTMLElement>(elId: string): T =>
  document.getElementById(elId) as T;

function render(p: TickPayload): void {
  const disp = el("timer-display");
  // Display from Rust ONLY. Countdown shows remaining; stopwatch shows elapsed.
  disp.textContent =
    mode === "countdown" ? fmt(p.remaining_secs) : fmt(p.elapsed_secs);
  disp.classList.toggle("done", p.state === "done");

  const running = p.state === "running";
  el<HTMLButtonElement>("btn-start").disabled = running;
  el<HTMLButtonElement>("btn-pause").disabled = !running;
}

function applyModeUi(): void {
  el("timer").setAttribute("data-mode", mode);
  el("mode-countdown").classList.toggle("active", mode === "countdown");
  el("mode-stopwatch").classList.toggle("active", mode === "stopwatch");
}

export async function initTimer(task: Task): Promise<UnlistenFn[]> {
  // Initial state from the task Rust handed us.
  mode = task.timer.mode;
  el<HTMLInputElement>("dur-input").value = fmt(task.timer.duration_secs);
  applyModeUi();
  render({
    id,
    remaining_secs: task.timer.remaining_secs,
    elapsed_secs: task.timer.elapsed_secs,
    state: task.timer.state,
  });

  el("btn-start").addEventListener("click", () => {
    void invoke("start_timer", { id });
  });
  el("btn-pause").addEventListener("click", () => {
    void invoke("pause_timer", { id });
  });
  el("btn-reset").addEventListener("click", () => {
    void invoke("reset_timer", { id });
  });

  const applyConfig = () => {
    const dur = parse(el<HTMLInputElement>("dur-input").value);
    applyModeUi();
    void invoke("configure_timer", {
      id,
      mode,
      durationSecs: mode === "countdown" ? dur : 0,
    });
  };

  el("mode-countdown").addEventListener("click", () => {
    mode = "countdown";
    applyConfig();
  });
  el("mode-stopwatch").addEventListener("click", () => {
    mode = "stopwatch";
    applyConfig();
  });
  el("dur-input").addEventListener("change", applyConfig);

  const unlistenTick = await listen<TickPayload>("timer-tick", (e) => {
    if (e.payload.id !== id) return;
    render(e.payload);
  });
  const unlistenDone = await listen<{ id: string }>("timer-done", (e) => {
    if (e.payload.id !== id) return;
    el("timer-display").classList.add("done");
  });

  return [unlistenTick, unlistenDone];
}
