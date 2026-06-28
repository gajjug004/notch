use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Default for Geometry {
    fn default() -> Self {
        // Cascade is applied at create time; this is just a safe fallback.
        Geometry {
            x: 120,
            y: 120,
            w: 260,
            h: 260,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String, // uuid v4, hyphenated; == window label
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub window: Geometry,

    // ---- Forward-compat for Phases 3 (timer) & 4 (schedule). ----
    // Present so old store files (without these keys) still deserialize, and
    // new writes start carrying them. Every field added later MUST be
    // #[serde(default)].
    #[serde(default)]
    pub timer: Option<serde_json::Value>, // becomes a typed Timer in Phase 3
    #[serde(default)]
    pub schedule: Option<serde_json::Value>, // becomes a typed Schedule in Phase 4
}

fn default_color() -> String {
    "#fff7b1".to_string() // sticky yellow
}

impl Task {
    pub fn new() -> Self {
        Task {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            content: String::new(),
            color: default_color(),
            window: Geometry::default(),
            timer: None,
            schedule: None,
        }
    }
}
