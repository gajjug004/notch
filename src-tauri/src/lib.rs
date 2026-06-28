use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WindowGeom {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub title: String,
    pub content: String,
    pub color: serde_json::Value, // tighten in Phase 5
    pub window: WindowGeom,
    // timer / schedule fields land in Phase 3 / 4
}

/// Phase 2/3: Rust becomes source of truth. For now this is a stub the JS
/// layer does not call — persistence happens via the store plugin in JS.
#[tauri::command]
async fn save_task(_task: Task) -> Result<(), String> {
    // TODO(phase2): write into Rust-owned state + store
    Ok(())
}

#[tauri::command]
async fn load_task() -> Result<Option<Task>, String> {
    // TODO(phase2): read from Rust-owned state
    Ok(None)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![save_task, load_task])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
