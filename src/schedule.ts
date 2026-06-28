import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import type { ScheduleKind, Task } from "./types";

const id = getCurrentWindow().label; // == task id

const el = <T extends HTMLElement>(elId: string): T =>
  document.getElementById(elId) as T;

async function ensureNotificationPermission(): Promise<void> {
  // No-op on most Linux setups, but initializes state + works if repackaged.
  if (!(await isPermissionGranted())) {
    await requestPermission();
  }
}

export async function initSchedule(task: Task): Promise<UnlistenFn[]> {
  const kindEl = el<HTMLSelectElement>("sched-kind");
  const atEl = el<HTMLInputElement>("sched-at");
  const recEl = el<HTMLDivElement>("sched-recurring");
  const timeEl = el<HTMLInputElement>("sched-time");
  const autoEl = el<HTMLInputElement>("sched-autostart");
  const firedBtn = el<HTMLButtonElement>("sched-fired");

  const syncVisibility = () => {
    atEl.hidden = kindEl.value !== "once";
    recEl.hidden = kindEl.value !== "recurring";
  };

  // Hydrate from the stored schedule.
  const s = task.schedule;
  kindEl.value = s.kind;
  autoEl.checked = s.auto_start;
  if (s.at) {
    if (s.kind === "once") atEl.value = s.at.slice(0, 16);
    else if (s.kind === "recurring") timeEl.value = s.at.slice(0, 5);
  }
  for (const wd of s.weekdays) {
    const cb = recEl.querySelector<HTMLInputElement>(`input[data-wd="${wd}"]`);
    if (cb) cb.checked = true;
  }
  syncVisibility();

  kindEl.addEventListener("change", syncVisibility);

  el("sched-save").addEventListener("click", () => {
    const kind = kindEl.value as ScheduleKind;
    const weekdays = [
      ...recEl.querySelectorAll<HTMLInputElement>("input[data-wd]"),
    ]
      .filter((c) => c.checked)
      .map((c) => Number(c.dataset.wd));

    const schedule = {
      kind,
      at:
        kind === "once"
          ? atEl.value
          : kind === "recurring"
            ? timeEl.value
            : null,
      weekdays,
      auto_start: autoEl.checked,
      last_fired: null,
    };
    void invoke("set_schedule", { id, schedule });
    firedBtn.hidden = true;
  });

  // No auto-start: surface a Start button in the (always-visible) note.
  firedBtn.addEventListener("click", () => {
    void invoke("start_timer", { id });
    firedBtn.hidden = true;
  });

  await ensureNotificationPermission();

  const unlisten = await listen<string>("schedule-fired", (e) => {
    if (e.payload !== id) return;
    firedBtn.hidden = false;
    // Reflect that a one-shot is now spent.
    if (kindEl.value === "once") {
      kindEl.value = "none";
      syncVisibility();
    }
  });

  return [unlisten];
}
