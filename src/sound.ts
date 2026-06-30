import { load } from "@tauri-apps/plugin-store";

// Synthesized alarm — a run of short beeps ("tee tee tee…") for ~3s via
// WebAudio. Once the context is resumed inside a user gesture, it plays at any
// later time (timer/schedule fire) without per-play activation, which is what
// HTMLAudioElement.play() trips on under WebKitGTK (NotAllowedError).
let ctx: AudioContext | null = null;

// Alarm shape.
const FREQ = 1000; // Hz — bright "tee"
const BEEP = 0.16; // seconds of tone
const GAP = 0.12; // seconds of silence between beeps
const DURATION = 3; // total alarm length
const VOLUME = 0.35;

/** Create the context and arm the gesture that resumes it. Call once. */
export async function initSound(): Promise<void> {
  try {
    ctx = new AudioContext();
  } catch {
    ctx = null;
  }
  armResume();
}

/** First user gesture resumes the (autoplay-suspended) context. */
function armResume(): void {
  const resume = () => {
    void ctx?.resume();
    window.removeEventListener("pointerdown", resume);
    window.removeEventListener("keydown", resume);
  };
  window.addEventListener("pointerdown", resume);
  window.addEventListener("keydown", resume);
}

async function soundEnabled(): Promise<boolean> {
  try {
    const s = await load("settings.json");
    return ((await s.get<boolean>("soundOn")) ?? true) === true;
  } catch {
    return true;
  }
}

/** Schedule one beep at time t on the shared context. */
function beepAt(c: AudioContext, t: number): void {
  const osc = c.createOscillator();
  const gain = c.createGain();
  osc.type = "square";
  osc.frequency.value = FREQ;
  osc.connect(gain);
  gain.connect(c.destination);
  // Quick attack/release so each beep is a clean "tee", no clicks.
  gain.gain.setValueAtTime(0.0001, t);
  gain.gain.exponentialRampToValueAtTime(VOLUME, t + 0.01);
  gain.gain.exponentialRampToValueAtTime(0.0001, t + BEEP);
  osc.start(t);
  osc.stop(t + BEEP);
}

export async function playAlert(): Promise<void> {
  if (!ctx || !(await soundEnabled())) return;
  try {
    if (ctx.state === "suspended") await ctx.resume();
    const start = ctx.currentTime;
    for (let t = start; t < start + DURATION; t += BEEP + GAP) {
      beepAt(ctx, t);
    }
  } catch {
    // playback blocked or device unavailable — ignore
  }
}
