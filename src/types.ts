export interface Geometry {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type TimerMode = "countdown" | "stopwatch";
export type TimerState = "idle" | "running" | "paused" | "done";

export interface Timer {
  mode: TimerMode;
  duration_secs: number;
  remaining_secs: number;
  elapsed_secs: number;
  state: TimerState;
}

export type ScheduleKind = "none" | "once" | "recurring";

export interface Schedule {
  kind: ScheduleKind;
  at: string | null;
  weekdays: number[];
  auto_start: boolean;
  last_fired?: string | null;
}

export interface Task {
  id: string;
  title: string;
  content: string;
  color: string;
  window: Geometry;
  timer: Timer;
  schedule: Schedule;
}
