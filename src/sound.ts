import { convertFileSrc } from "@tauri-apps/api/core";
import { resolveResource } from "@tauri-apps/api/path";
import { load } from "@tauri-apps/plugin-store";

let alertUrl: string | null = null;

/** Resolve the bundled chime + arm the autoplay unlock. Call once per window. */
export async function initSound(): Promise<void> {
  try {
    const path = await resolveResource("sounds/alert.ogg");
    alertUrl = convertFileSrc(path); // asset://localhost/...
  } catch {
    alertUrl = null;
  }
  armAudioUnlock();
}

/** WebKit blocks Audio.play() until a user gesture; unlock once per window. */
function armAudioUnlock(): void {
  const unlock = () => {
    if (!alertUrl) return;
    // Play the real asset muted inside the gesture — a src-less Audio rejects
    // and never satisfies WebKitGTK's media-activation unlock.
    const a = new Audio(alertUrl);
    a.muted = true;
    a.play()
      .then(() => {
        a.pause();
        a.currentTime = 0;
      })
      .catch(() => {});
    window.removeEventListener("pointerdown", unlock);
    window.removeEventListener("keydown", unlock);
  };
  window.addEventListener("pointerdown", unlock);
  window.addEventListener("keydown", unlock);
}

async function soundEnabled(): Promise<boolean> {
  try {
    const s = await load("settings.json");
    return ((await s.get<boolean>("soundOn")) ?? true) === true;
  } catch {
    return true;
  }
}

export async function playAlert(): Promise<void> {
  if (!alertUrl) return;
  if (!(await soundEnabled())) return;
  try {
    const audio = new Audio(alertUrl);
    audio.volume = 1.0;
    await audio.play();
  } catch {
    // autoplay blocked before first gesture — ignore
  }
}
