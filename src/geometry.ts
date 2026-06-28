import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeWindow } from "./store";

const appWindow = getCurrentWindow();

let geomTimer: number | undefined;

/** Debounce geometry writes — onMoved/onResized fire rapidly during a drag. */
function scheduleGeomSave(): void {
  if (geomTimer) clearTimeout(geomTimer);
  geomTimer = window.setTimeout(saveGeomNow, 300);
}

async function saveGeomNow(): Promise<void> {
  const pos = await appWindow.outerPosition(); // PhysicalPosition {x,y}
  const size = await appWindow.innerSize(); // PhysicalSize {width,height}
  await writeWindow({ x: pos.x, y: pos.y, w: size.width, h: size.height });
}

export async function trackGeometry(): Promise<void> {
  await appWindow.onMoved(() => scheduleGeomSave());
  await appWindow.onResized(() => scheduleGeomSave());
}
