use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;

use crate::task::Task;

pub const STORE_FILE: &str = "tasks.json";
pub const STORE_KEY: &str = "tasks"; // single key holding the whole map

#[derive(Default)]
pub struct AppState {
    // Mutex (not async): held only for tiny, synchronous critical sections.
    pub tasks: Mutex<HashMap<String, Task>>,
}

/// Write the entire in-memory map to the store and flush to disk.
/// Takes a snapshot under the lock, drops the guard, then does store I/O with
/// NO lock held (avoids deadlocks and holding the lock across slow I/O).
pub fn persist<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let snapshot: HashMap<String, Task> = {
        let state = app.state::<AppState>();
        let guard = state.tasks.lock().map_err(|e| e.to_string())?;
        guard.clone()
    }; // guard dropped here

    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_KEY,
        serde_json::to_value(&snapshot).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// Load the map from the store into memory at boot.
pub fn load_into_state<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let map: HashMap<String, Task> = match store.get(STORE_KEY) {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string())?,
        None => HashMap::new(),
    };
    let state = app.state::<AppState>();
    *state.tasks.lock().map_err(|e| e.to_string())? = map;
    Ok(())
}
