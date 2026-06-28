import { load, type Store } from "@tauri-apps/plugin-store";
import type { NoteColor, NoteData, WindowGeom } from "./types";

const STORE_FILE = "note.json";

let _store: Store | null = null;

/** Memoized store handle — load() is async and must only run once. */
export async function getStore(): Promise<Store> {
  if (!_store) {
    // autoSave: 400 → plugin debounces writes to disk by 400ms after a set().
    // We still call save() explicitly on close to guarantee a flush.
    _store = await load(STORE_FILE, { autoSave: 400 });
  }
  return _store;
}

export async function readNote(): Promise<Partial<NoteData>> {
  const s = await getStore();
  return {
    title: (await s.get<string>("title")) ?? "",
    content: (await s.get<string>("content")) ?? "",
    color: (await s.get<NoteColor>("color")) ?? undefined,
    window: (await s.get<WindowGeom>("window")) ?? undefined,
  };
}

export async function writeText(title: string, content: string): Promise<void> {
  const s = await getStore();
  await s.set("title", title);
  await s.set("content", content);
  // no explicit save(): autoSave debounce handles disk write
}

export async function writeWindow(window: WindowGeom): Promise<void> {
  const s = await getStore();
  await s.set("window", window);
}

export async function flush(): Promise<void> {
  const s = await getStore();
  await s.save(); // force write now
}
