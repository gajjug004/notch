use serde::{Deserialize, Serialize};

use crate::schedule::Schedule;
use crate::timer::Timer;

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
            w: 280,
            h: 470,
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

    // Phase 3: typed timer. `default` covers a missing field; the custom
    // deserializer also maps a present `null` (written as Option in Phase 2) to
    // the default, so old store files keep loading.
    #[serde(default, deserialize_with = "de_timer_or_default")]
    pub timer: Timer,

    // Phase 4: typed schedule. Null-tolerant for stores written before it existed.
    #[serde(default, deserialize_with = "de_schedule_or_default")]
    pub schedule: Schedule,
}

fn default_color() -> String {
    "#fff7b1".to_string() // sticky yellow
}

/// Map a present `null` (Phase 2 stored `timer` as an Option) or a valid object
/// to a Timer; anything missing/null falls back to Timer::default().
fn de_timer_or_default<'de, D>(deserializer: D) -> Result<Timer, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<Timer>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// Same null-tolerance for `schedule` (Phase 2 stored it as an Option).
fn de_schedule_or_default<'de, D>(deserializer: D) -> Result<Schedule, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<Schedule>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

impl Task {
    pub fn new() -> Self {
        Task {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            content: String::new(),
            color: default_color(),
            window: Geometry::default(),
            timer: Timer::default(),
            schedule: Schedule::default(),
        }
    }
}
