use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerMode {
    Countdown,
    Stopwatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerState {
    Idle,    // never started, or reset
    Running, // tick loop is advancing it
    Paused,  // frozen, segment folded into base
    Done,    // countdown only: reached 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Timer {
    pub mode: TimerMode,
    /// Configured length for countdown. Ignored by stopwatch (kept for mode switch).
    pub duration_secs: u64,
    /// Countdown: seconds left. Updated by the tick loop. == duration when idle.
    pub remaining_secs: u64,
    /// Stopwatch: seconds counted up. Also total time for countdown.
    pub elapsed_secs: u64,
    pub state: TimerState,

    /// Monotonic run anchor. NEVER persisted, NEVER serialized.
    /// `Some` only while `state == Running`. Makes counting drift-free.
    #[serde(skip)]
    pub anchor: Option<RunAnchor>,
}

/// Captured the instant a timer starts/resumes. Lets every tick recompute the
/// true position from a single monotonic reference instead of accumulating
/// per-tick rounding error.
#[derive(Clone, Copy, Debug)]
pub struct RunAnchor {
    /// Monotonic clock reading at the moment Running began.
    pub started_at: Instant,
    /// Seconds already accumulated before this run segment (folded on each pause).
    pub base_secs: u64,
}

impl Default for Timer {
    fn default() -> Self {
        Timer {
            mode: TimerMode::Countdown,
            duration_secs: 25 * 60, // pomodoro default
            remaining_secs: 25 * 60,
            elapsed_secs: 0,
            state: TimerState::Idle,
            anchor: None,
        }
    }
}
