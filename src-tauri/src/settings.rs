use serde_json::Value;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

pub const SETTINGS_FILE: &str = "settings.json";

pub fn get_u64<R: Runtime>(app: &AppHandle<R>, key: &str, default: u64) -> u64 {
    app.store(SETTINGS_FILE)
        .ok()
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_u64())
        .unwrap_or(default)
}

pub fn get_string<R: Runtime>(app: &AppHandle<R>, key: &str) -> Option<String> {
    app.store(SETTINGS_FILE)
        .ok()
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|s| !s.trim().is_empty())
}

pub fn get_bool<R: Runtime>(app: &AppHandle<R>, key: &str, default: bool) -> bool {
    app.store(SETTINGS_FILE)
        .ok()
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

pub fn set_value<R: Runtime>(app: &AppHandle<R>, key: &str, value: Value) {
    if let Ok(store) = app.store(SETTINGS_FILE) {
        store.set(key, value);
        let _ = store.save();
    }
}
