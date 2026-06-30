//! Telegram push integration.
//!
//! Fire-and-forget: sends are spawned on the async runtime and all errors are
//! swallowed so a missing token / network failure never disturbs the tick loop
//! (mirrors how desktop notifications are best-effort in `tick.rs`).
//!
//! Config lives in `settings.json`:
//!   telegramEnabled : bool
//!   telegramToken   : String  (bot token from @BotFather)
//!   telegramChatId  : String  (numeric user/chat id)

use tauri::{AppHandle, Runtime};

use crate::settings;

/// Read (enabled, token, chat_id) if fully configured AND enabled.
fn config<R: Runtime>(app: &AppHandle<R>) -> Option<(String, String)> {
    if !settings::get_bool(app, "telegramEnabled", false) {
        return None;
    }
    let token = settings::get_string(app, "telegramToken")?;
    let chat_id = settings::get_string(app, "telegramChatId")?;
    Some((token, chat_id))
}

/// Minimal HTML escape for Telegram `parse_mode=HTML` (only &, <, > matter).
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Build a message body: bold title line + optional description.
/// `prefix` is an emoji/label like "⏰ Time's up".
pub fn format_message(prefix: &str, title: &str, content: &str, extra: Option<&str>) -> String {
    let heading = if title.trim().is_empty() {
        "Untitled task"
    } else {
        title.trim()
    };
    let mut msg = format!("<b>{}</b>\n{}", esc(prefix), esc(heading));
    let desc = content.trim();
    if !desc.is_empty() {
        msg.push_str(&format!("\n\n{}", esc(desc)));
    }
    if let Some(extra) = extra {
        msg.push_str(&format!("\n\n<i>{}</i>", esc(extra)));
    }
    msg
}

// ---- event-specific templates ---------------------------------------------
// Each wraps the shared `format_message` builder so HTML-escaping stays in one
// place. Copy mirrors `ux-enhancement-plan.md`.

/// Countdown reached zero.
pub fn format_timer_done(title: &str, content: &str) -> String {
    format_message(
        "⏰ Timer finished",
        title,
        content,
        Some("Open Notch to reset, restart, or mark done."),
    )
}

/// A schedule fired. Hint differs by whether the timer auto-started.
pub fn format_schedule_fired(title: &str, content: &str, auto_start: bool) -> String {
    let hint = if auto_start {
        "Timer started automatically."
    } else {
        "Open Notch to start or snooze."
    };
    format_message("📌 Task due now", title, content, Some(hint))
}

/// A future schedule was set/updated. `due` is a preformatted local time.
pub fn format_task_scheduled(title: &str, content: &str, due: &str) -> String {
    format_message("🗓 Task scheduled", title, content, Some(&format!("Due: {due}")))
}

/// Settings "Send test" probe.
pub fn format_test_message() -> String {
    format_message(
        "✅ Notch connected",
        "Telegram alerts are ready.",
        "",
        Some("You will receive scheduled task and timer alerts here."),
    )
}

/// Spawn an async send if Telegram is enabled+configured. Never blocks, never errors out.
pub fn send<R: Runtime>(app: &AppHandle<R>, text: String) {
    let Some((token, chat_id)) = config(app) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let _ = post(&token, &chat_id, &text).await;
    });
}

/// One POST to the Bot API. Returns Err(message) for the Settings test path.
pub async fn post(token: &str, chat_id: &str, text: &str) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Telegram API {status}: {body}"))
    }
}
