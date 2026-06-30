import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { load } from "@tauri-apps/plugin-store";
import type { Schedule, ScheduleKind, Task } from "./types";

const el = <T extends HTMLElement>(elId: string): T =>
  document.getElementById(elId) as T;

const WD = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const pad = (n: number): string => String(n).padStart(2, "0");

// The detail view edits one task's schedule at a time.
let currentId: string | null = null;

// Preset config, refreshed from Settings each time the popover opens.
let tonightTime = "20:00";
let tomorrowTime = "10:00";

// Notify the app shell that a fired schedule was acted on, so the list can drop
// its "needs start" state. Set in setupSchedule.
let firedResolved: (id: string) => void = () => {};

function labelMins(m: number): string {
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const mm = m % 60;
  return mm === 0 ? `${h}h` : `${h}h${mm}m`;
}

/** Rebuild the minute chips + cache tonight/tomorrow times from Settings. */
async function refreshPresets(): Promise<void> {
  let minsStr = "15, 30, 60, 180";
  try {
    const s = await load("settings.json");
    minsStr = (await s.get<string>("schedulePresetMins")) ?? minsStr;
    tonightTime = (await s.get<string>("tonightTime")) ?? tonightTime;
    tomorrowTime = (await s.get<string>("tomorrowTime")) ?? tomorrowTime;
  } catch {
    /* store missing → keep defaults */
  }
  const mins = minsStr
    .split(",")
    .map((x) => parseInt(x.trim(), 10))
    .filter((n) => Number.isFinite(n) && n > 0);
  const wrap = el("sched-mins");
  wrap.replaceChildren();
  for (const m of mins) {
    const b = document.createElement("button");
    b.type = "button";
    b.dataset.min = String(m);
    b.textContent = labelMins(m);
    wrap.appendChild(b);
  }
}

// ---- formatting helpers ---------------------------------------------------

/** A Date → the local "YYYY-MM-DDTHH:MM" string the Rust side parses. */
function toLocalInput(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(
    d.getDate(),
  )}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Compact one-line badge for a list row / the trigger (empty when off). */
export function scheduleBadge(s: Schedule): string {
  if (s.kind === "none" || !s.at) return "";
  if (s.kind === "once") {
    const when = new Date(s.at).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
    return `⏰ ${when}`;
  }
  const days =
    s.weekdays.length === 7
      ? "daily"
      : s.weekdays.map((w) => WD[w].slice(0, 2)).join("");
  return `⏰ ${s.at} ${days}`;
}

async function ensureNotificationPermission(): Promise<void> {
  if (!(await isPermissionGranted())) {
    await requestPermission();
  }
}

// ---- status line ----------------------------------------------------------

let statusTimer: number | undefined;
function showStatus(msg: string, ok = true): void {
  const statusEl = el<HTMLSpanElement>("sched-status");
  statusEl.textContent = msg;
  statusEl.classList.toggle("sched-status--err", !ok);
  window.clearTimeout(statusTimer);
  if (ok && msg)
    statusTimer = window.setTimeout(() => (statusEl.textContent = ""), 3000);
}

// ---- popover open/close ---------------------------------------------------

function popOpen(): boolean {
  return !el("sched-pop").hidden;
}
function openPop(): void {
  el("sched-pop").hidden = false;
  el("sched-chev").classList.add("open");
}
function closePop(): void {
  el("sched-pop").hidden = true;
  el("sched-chev").classList.remove("open");
}

/** Reflect the selected kind: which controls show inside the popover. */
function syncKindUi(): void {
  const kind = el<HTMLSelectElement>("sched-kind").value;
  el("sched-once").hidden = kind !== "once";
  el("sched-recurring").hidden = kind !== "recurring";
  el("sched-auto-row").hidden = kind === "none";
  // Once applies instantly from a preset; only the custom path / recurring
  // need an explicit Set button.
  const customOpen = kind === "once" && !el("sched-custom").hidden;
  el("sched-actions").hidden = !(kind === "recurring" || customOpen);
}

// ---- saving ---------------------------------------------------------------

function setSummary(s: Schedule): void {
  el("sched-summary").textContent = scheduleBadge(s) || "Schedule";
}

async function save(s: Schedule, label: string): Promise<void> {
  if (!currentId) return;
  try {
    await invoke("set_schedule", { id: currentId, schedule: s });
    el("sched-fired").hidden = true;
    setSummary(s);
    showStatus(label);
    closePop();
  } catch (err) {
    showStatus(`Failed: ${String(err)}`, false);
  }
}

function autoStart(): boolean {
  return el<HTMLInputElement>("sched-autostart").checked;
}

/** One-tap preset → an absolute once-schedule. */
function scheduleOnceAt(d: Date): void {
  void save(
    {
      kind: "once",
      at: toLocalInput(d),
      weekdays: [],
      auto_start: autoStart(),
      last_fired: null,
    },
    `Scheduled ✓ ${d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    })}`,
  );
}

function presetDate(preset: string): Date {
  const d = new Date();
  if (preset === "tonight") {
    // Tonight stays tonight; the caller rejects it if the time already passed.
    const [h, m] = tonightTime.split(":").map(Number);
    d.setHours(h || 0, m || 0, 0, 0);
  } else if (preset === "tomorrow") {
    d.setDate(d.getDate() + 1);
    const [h, m] = tomorrowTime.split(":").map(Number);
    d.setHours(h || 0, m || 0, 0, 0);
  }
  return d;
}

// ---- setup (once) ---------------------------------------------------------

export function setupSchedule(onFiredResolved: (id: string) => void): void {
  firedResolved = onFiredResolved;
  void ensureNotificationPermission();

  el("sched-trigger").addEventListener("click", () => {
    if (popOpen()) closePop();
    else {
      void refreshPresets(); // pick up the latest Settings presets
      syncKindUi();
      openPop();
    }
  });

  el("sched-kind").addEventListener("change", () => {
    const kind = el<HTMLSelectElement>("sched-kind").value as ScheduleKind;
    el("sched-custom").hidden = true; // reset the custom fields on kind change
    showStatus("");
    if (kind === "none") {
      void save(
        { kind: "none", at: null, weekdays: [], auto_start: false, last_fired: null },
        "Schedule cleared",
      );
      return;
    }
    syncKindUi();
  });

  // Once presets.
  el("sched-once").addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button");
    if (!btn) return;
    if (btn.dataset.min) {
      scheduleOnceAt(new Date(Date.now() + Number(btn.dataset.min) * 60000));
    } else if (btn.dataset.preset === "custom") {
      el("sched-custom").hidden = false;
      const cdate = el<HTMLInputElement>("sched-cdate");
      const ctime = el<HTMLInputElement>("sched-ctime");
      if (!cdate.value || !ctime.value) {
        const d = new Date(Date.now() + 3600000);
        const at = toLocalInput(d);
        cdate.value = at.slice(0, 10);
        ctime.value = at.slice(11, 16);
      }
      syncKindUi();
      cdate.focus();
    } else if (btn.dataset.preset) {
      const d = presetDate(btn.dataset.preset);
      if (d.getTime() <= Date.now()) {
        showStatus("That time has already passed", false);
        return;
      }
      scheduleOnceAt(d);
    }
  });

  // Save button → custom once, or recurring.
  el("sched-save").addEventListener("click", () => {
    const kind = el<HTMLSelectElement>("sched-kind").value as ScheduleKind;
    if (kind === "once") {
      const cdate = el<HTMLInputElement>("sched-cdate").value;
      const ctime = el<HTMLInputElement>("sched-ctime").value;
      if (!cdate || !ctime) {
        showStatus("Pick a date and time", false);
        return;
      }
      scheduleOnceAt(new Date(`${cdate}T${ctime}`));
      return;
    }
    if (kind === "recurring") {
      const time = el<HTMLInputElement>("sched-time").value;
      const weekdays = [
        ...el("sched-recurring").querySelectorAll<HTMLInputElement>(
          "input[data-wd]",
        ),
      ]
        .filter((c) => c.checked)
        .map((c) => Number(c.dataset.wd));
      if (!time || weekdays.length === 0) {
        showStatus("Pick a time and at least one day", false);
        return;
      }
      void save(
        { kind: "recurring", at: time, weekdays, auto_start: autoStart(), last_fired: null },
        `Scheduled ✓ ${time}`,
      );
    }
  });

  // Fired-schedule action area (manual-start). Each action clears the prompt.
  const resolveFired = () => {
    const id = currentId;
    el("sched-fired").hidden = true;
    if (id) firedResolved(id);
  };
  el("fired-start").addEventListener("click", () => {
    if (currentId) void invoke("start_timer", { id: currentId });
    resolveFired();
  });
  const snooze = (mins: number) => {
    if (!currentId) return;
    void invoke("snooze_task", { id: currentId, minutes: mins });
    // Reflect the new one-shot time in the trigger summary right away
    // (backend re-armed it; otherwise the bar would stale to "Schedule").
    setSummary({
      kind: "once",
      at: toLocalInput(new Date(Date.now() + mins * 60000)),
      weekdays: [],
      auto_start: autoStart(),
    });
    resolveFired();
  };
  el("fired-snooze5").addEventListener("click", () => snooze(5));
  el("fired-snooze15").addEventListener("click", () => snooze(15));
  el("fired-dismiss").addEventListener("click", () => {
    if (currentId) void invoke("dismiss_fired_schedule", { id: currentId });
    resolveFired();
  });

  // Dismiss the popover on outside click / Escape.
  document.addEventListener("click", (e) => {
    if (!popOpen()) return;
    if (!(e.target as HTMLElement).closest("#sched-bar")) closePop();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && popOpen()) closePop();
  });
}

/** Hydrate the schedule UI from a task. */
export function loadSchedule(task: Task): void {
  currentId = task.id;
  const s = task.schedule;
  el<HTMLSelectElement>("sched-kind").value = s.kind;
  el<HTMLInputElement>("sched-autostart").checked = s.auto_start;

  el("sched-custom").hidden = true;
  const once = s.kind === "once" && s.at ? s.at : "";
  el<HTMLInputElement>("sched-cdate").value = once ? once.slice(0, 10) : "";
  el<HTMLInputElement>("sched-ctime").value = once ? once.slice(11, 16) : "";
  el<HTMLInputElement>("sched-time").value =
    s.kind === "recurring" && s.at ? s.at.slice(0, 5) : "";
  for (const cb of el("sched-recurring").querySelectorAll<HTMLInputElement>(
    "input[data-wd]",
  )) {
    cb.checked = s.weekdays.includes(Number(cb.dataset.wd));
  }

  el("sched-fired").hidden = true;
  showStatus("");
  setSummary(s);
  closePop();
  syncKindUi();
}

export function unloadSchedule(): void {
  currentId = null;
  closePop();
}

/** A schedule fired for `id`: if it's the open task, surface the Start button. */
export function onScheduleFired(id: string): void {
  if (currentId !== id) return;
  el("sched-fired").hidden = false;
  // A one-shot is now spent — reflect it in the trigger summary.
  setSummary({
    kind: "none",
    at: null,
    weekdays: [],
    auto_start: false,
  });
}
