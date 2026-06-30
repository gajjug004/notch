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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_render_expected_copy() {
        let done = format_timer_done("Write report", "Q3 summary");
        assert_eq!(
            done,
            "<b>⏰ Timer finished</b>\nWrite report\n\nQ3 summary\n\n<i>Open Notch to reset, restart, or mark done.</i>"
        );

        let auto = format_schedule_fired("Standup", "", true);
        assert_eq!(
            auto,
            "<b>📌 Task due now</b>\nStandup\n\n<i>Timer started automatically.</i>"
        );
        let manual = format_schedule_fired("Standup", "", false);
        assert_eq!(
            manual,
            "<b>📌 Task due now</b>\nStandup\n\n<i>Open Notch to start or snooze.</i>"
        );

        let sched = format_task_scheduled("Call Bob", "ring twice", "Tue 30 Jun, 14:30");
        assert_eq!(
            sched,
            "<b>🗓 Task scheduled</b>\nCall Bob\n\nring twice\n\n<i>Due: Tue 30 Jun, 14:30</i>"
        );

        let test = format_test_message();
        assert_eq!(
            test,
            "<b>✅ Notch connected</b>\nTelegram alerts are ready.\n\n<i>You will receive scheduled task and timer alerts here.</i>"
        );
    }

    #[test]
    fn html_special_chars_are_escaped() {
        // <, >, & in title and content must become entities.
        let msg = format_timer_done("a<b>&c", "x > y & z < w");
        assert_eq!(
            msg,
            "<b>⏰ Timer finished</b>\na&lt;b&gt;&amp;c\n\nx &gt; y &amp; z &lt; w\n\n<i>Open Notch to reset, restart, or mark done.</i>"
        );
    }

    #[test]
    fn empty_title_falls_back() {
        let msg = format_task_scheduled("", "", "now");
        assert_eq!(msg, "<b>🗓 Task scheduled</b>\nUntitled task\n\n<i>Due: now</i>");
    }
}
