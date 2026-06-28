export interface Geometry {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Task {
  id: string;
  title: string;
  content: string;
  color: string;
  window: Geometry;
  timer?: unknown; // Phase 3
  schedule?: unknown; // Phase 4
}
