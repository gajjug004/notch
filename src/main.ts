import "./styles.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { restore } from "./restore";
import { trackGeometry } from "./geometry";
import { writeText, flush } from "./store";

const appWindow = getCurrentWindow();

let textTimer: number | undefined;

function scheduleTextSave(title: string, content: string): void {
  if (textTimer) clearTimeout(textTimer);
  textTimer = window.setTimeout(() => writeText(title, content), 400);
}

window.addEventListener("DOMContentLoaded", async () => {
  await restore();
  await trackGeometry();

  const titleEl = document.getElementById("title") as HTMLInputElement;
  const bodyEl = document.getElementById("body") as HTMLDivElement;

  const onEdit = () => scheduleTextSave(titleEl.value, bodyEl.innerText);
  titleEl.addEventListener("input", onEdit);
  bodyEl.addEventListener("input", onEdit);

  // Flush pending debounced writes before the window actually closes.
  await appWindow.onCloseRequested(async () => {
    if (textTimer) clearTimeout(textTimer);
    await writeText(titleEl.value, bodyEl.innerText);
    await flush();
    // not preventing default: let the close proceed after flush resolves
  });
});
