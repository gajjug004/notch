import {
  getCurrentWindow,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";
import { readNote } from "./store";

const appWindow = getCurrentWindow();

export async function restore(): Promise<void> {
  const note = await readNote();

  // 1. Geometry — apply BEFORE showing to avoid a visible jump.
  if (note.window) {
    const { x, y, w, h } = note.window;
    await appWindow.setSize(new PhysicalSize(w, h));
    await appWindow.setPosition(new PhysicalPosition(x, y));
  }

  // 2. Color
  if (note.color) {
    const root = document.documentElement.style;
    root.setProperty("--note-bg", note.color.bg);
    root.setProperty("--note-bg-strip", note.color.strip);
    root.setProperty("--note-ink", note.color.ink);
  }

  // 3. Text
  const titleEl = document.getElementById("title") as HTMLInputElement;
  const bodyEl = document.getElementById("body") as HTMLDivElement;
  titleEl.value = note.title ?? "";
  bodyEl.innerText = note.content ?? "";
}
