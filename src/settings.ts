import "./settings.css";
import { invoke } from "@tauri-apps/api/core";
import { load } from "@tauri-apps/plugin-store";
import {
  disable,
  enable,
  isEnabled,
} from "@tauri-apps/plugin-autostart";

const el = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

const status = () => el("status");
function flash(msg: string): void {
  status().textContent = msg;
  window.setTimeout(() => {
    if (status().textContent === msg) status().textContent = "";
  }, 1500);
}

window.addEventListener("DOMContentLoaded", async () => {
  // Tab switching: toggle .active on the clicked tab + its matching panel.
  const tabs = document.querySelectorAll<HTMLButtonElement>(".tab");
  const panels = document.querySelectorAll<HTMLElement>(".panel");
  tabs.forEach((tab) =>
    tab.addEventListener("click", () => {
      const name = tab.dataset.tab;
      tabs.forEach((t) => t.classList.toggle("active", t === tab));
      panels.forEach((p) => p.classList.toggle("active", p.dataset.panel === name));
    }),
  );

  const store = await load("settings.json");

  const minsEl = el<HTMLInputElement>("default-mins");
  const presetEl = el<HTMLInputElement>("preset-mins");
  const tonightEl = el<HTMLInputElement>("tonight-time");
  const tomorrowEl = el<HTMLInputElement>("tomorrow-time");
  const soundEl = el<HTMLInputElement>("sound-on");
  const autoEl = el<HTMLInputElement>("autostart");
  const pauseEl = el<HTMLInputElement>("global-pause");
  const tgEnabledEl = el<HTMLInputElement>("tg-enabled");
  const tgTokenEl = el<HTMLInputElement>("tg-token");
  const tgChatEl = el<HTMLInputElement>("tg-chat");
  const tgTestEl = el<HTMLButtonElement>("tg-test");

  // Hydrate from store; autostart reflects real OS state, not the stored flag.
  const defaultSecs = (await store.get<number>("defaultCountdownSecs")) ?? 1500;
  minsEl.value = String(Math.max(1, Math.round(defaultSecs / 60)));
  presetEl.value = (await store.get<string>("schedulePresetMins")) ?? "15, 30, 60, 180";
  tonightEl.value = (await store.get<string>("tonightTime")) ?? "20:00";
  tomorrowEl.value = (await store.get<string>("tomorrowTime")) ?? "10:00";
  soundEl.checked = (await store.get<boolean>("soundOn")) ?? true;
  pauseEl.checked = (await store.get<boolean>("globalPause")) ?? false;
  autoEl.checked = await isEnabled();
  tgEnabledEl.checked = (await store.get<boolean>("telegramEnabled")) ?? false;
  tgTokenEl.value = (await store.get<string>("telegramToken")) ?? "";
  tgChatEl.value = (await store.get<string>("telegramChatId")) ?? "";

  minsEl.addEventListener("change", async () => {
    const mins = Math.max(1, Math.min(999, Number(minsEl.value) || 25));
    minsEl.value = String(mins);
    await store.set("defaultCountdownSecs", mins * 60);
    await store.save();
    flash("Saved");
  });

  // Quick presets: keep 1..1440 min, dedupe, preserve order.
  presetEl.addEventListener("change", async () => {
    const mins = [
      ...new Set(
        presetEl.value
          .split(",")
          .map((x) => parseInt(x.trim(), 10))
          .filter((n) => Number.isFinite(n) && n >= 1 && n <= 1440),
      ),
    ];
    if (mins.length === 0) mins.push(15, 30, 60, 180);
    presetEl.value = mins.join(", ");
    await store.set("schedulePresetMins", presetEl.value);
    await store.save();
    flash("Saved");
  });

  const saveTime = (key: string, input: HTMLInputElement, fallback: string) =>
    input.addEventListener("change", async () => {
      if (!input.value) input.value = fallback;
      await store.set(key, input.value);
      await store.save();
      flash("Saved");
    });
  saveTime("tonightTime", tonightEl, "20:00");
  saveTime("tomorrowTime", tomorrowEl, "10:00");

  soundEl.addEventListener("change", async () => {
    await store.set("soundOn", soundEl.checked);
    await store.save();
    flash("Saved");
  });

  autoEl.addEventListener("change", async () => {
    try {
      if (autoEl.checked) await enable();
      else await disable();
    } catch (e) {
      flash(`Autostart failed: ${e}`);
    }
    // Trust isEnabled() as truth; persist intent too.
    autoEl.checked = await isEnabled();
    await store.set("autostart", autoEl.checked);
    await store.save();
    flash("Saved");
  });

  pauseEl.addEventListener("change", async () => {
    await invoke(pauseEl.checked ? "pause_all" : "resume_all");
    flash(pauseEl.checked ? "Paused all" : "Resumed");
  });

  // ---- Telegram ----
  tgEnabledEl.addEventListener("change", async () => {
    await store.set("telegramEnabled", tgEnabledEl.checked);
    await store.save();
    flash("Saved");
  });

  const saveText = (key: string, input: HTMLInputElement) =>
    input.addEventListener("change", async () => {
      await store.set(key, input.value.trim());
      await store.save();
      flash("Saved");
    });
  saveText("telegramToken", tgTokenEl);
  saveText("telegramChatId", tgChatEl);

  // Inline, non-blocking format validation for the Telegram credentials.
  const TOKEN_RE = /^\d+:[\w-]+$/; // 123456:ABC-DEF...
  const CHAT_RE = /^-?\d+$/; // numeric (negative for groups/channels)
  const validate = (
    input: HTMLInputElement,
    hint: HTMLElement,
    re: RegExp,
  ): void => {
    const v = input.value.trim();
    if (!v) {
      hint.textContent = "";
      hint.className = "field-hint";
      return;
    }
    const ok = re.test(v);
    hint.textContent = ok ? "Looks valid" : "Unexpected format";
    hint.className = `field-hint ${ok ? "ok" : "err"}`;
  };
  const tgTokenHint = el("tg-token-hint");
  const tgChatHint = el("tg-chat-hint");
  tgTokenEl.addEventListener("input", () =>
    validate(tgTokenEl, tgTokenHint, TOKEN_RE),
  );
  tgChatEl.addEventListener("input", () =>
    validate(tgChatEl, tgChatHint, CHAT_RE),
  );
  validate(tgTokenEl, tgTokenHint, TOKEN_RE); // reflect hydrated values
  validate(tgChatEl, tgChatHint, CHAT_RE);

  tgTestEl.addEventListener("click", async () => {
    const token = tgTokenEl.value.trim();
    const chat_id = tgChatEl.value.trim();
    if (!token || !chat_id) {
      flash("Enter token and chat ID first");
      return;
    }
    // Persist current values so the test matches what's saved.
    await store.set("telegramToken", token);
    await store.set("telegramChatId", chat_id);
    await store.save();
    flash("Sending…");
    try {
      await invoke("telegram_test", { token, chatId: chat_id });
      // A successful test means the creds work — turn alerts on so real
      // events actually send (Send test bypasses this gate, which otherwise
      // makes "test works but nothing else arrives" a silent trap).
      if (!tgEnabledEl.checked) {
        tgEnabledEl.checked = true;
        await store.set("telegramEnabled", true);
        await store.save();
        flash("Test sent ✓ — alerts on");
      } else {
        flash("Test sent ✓");
      }
    } catch (e) {
      flash(`Failed: ${e}`);
    }
  });
});
