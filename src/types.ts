export interface NoteColor {
  bg: string;
  strip: string;
  ink: string;
}

export interface WindowGeom {
  x: number; // physical px, outer position
  y: number;
  w: number; // physical px, inner size
  h: number;
}

export interface NoteData {
  title: string;
  content: string; // plain text extracted from contenteditable
  color: NoteColor;
  window: WindowGeom;
}
