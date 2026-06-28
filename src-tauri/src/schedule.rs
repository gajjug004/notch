use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDateTime, NaiveTime, TimeZone,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleKind {
    #[default]
    None,
    Once,
    Recurring,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Schedule {
    /// none | once | recurring
    pub kind: ScheduleKind,

    /// For `once`: absolute instant (RFC3339 or bare datetime-local).
    /// For `recurring`: only the time-of-day component is used.
    #[serde(default)]
    pub at: Option<String>,

    /// Recurring only. chrono weekday numbers Mon=0 .. Sun=6.
    #[serde(default)]
    pub weekdays: Vec<u8>,

    /// Auto-start the timer on fire. If false, only notify + emit schedule-fired.
    #[serde(default)]
    pub auto_start: bool,

    /// Duplicate-fire guard: RFC3339 of the last scheduled instant we fired for.
    #[serde(default)]
    pub last_fired: Option<String>,
}

/// Next instant this schedule should fire, or None.
/// `once`: returns `at` (the loop fires when at <= now, guarded by last_fired).
/// `recurring`: soonest matching weekday at `at`'s time-of-day, scanning
///   today + the next 7 days.
pub fn next_fire_time(sched: &Schedule, now: DateTime<Local>) -> Option<DateTime<Local>> {
    match sched.kind {
        ScheduleKind::None => None,

        ScheduleKind::Once => parse_local(sched.at.as_deref()?),

        ScheduleKind::Recurring => {
            if sched.weekdays.is_empty() {
                return None;
            }
            let time = parse_local(sched.at.as_deref()?)?.time();
            let today = now.date_naive();
            let mut best: Option<DateTime<Local>> = None;
            for offset in 0..=7 {
                let day = today + Duration::days(offset);
                let wd = day.weekday().num_days_from_monday() as u8;
                if !sched.weekdays.contains(&wd) {
                    continue;
                }
                let naive = NaiveDateTime::new(day, time);
                let dt = match Local.from_local_datetime(&naive) {
                    LocalResult::Single(dt) => dt,
                    LocalResult::Ambiguous(dt, _) => dt, // earlier of the two
                    LocalResult::None => continue,       // skipped hour: skip day
                };
                if offset == 0 {
                    // Today already fired? look at later days.
                    if sched.last_fired.as_deref() == Some(dt.to_rfc3339().as_str()) {
                        continue;
                    }
                    if dt <= now {
                        return Some(dt); // today's slot is due right now
                    }
                }
                if best.map_or(true, |b| dt < b) {
                    best = Some(dt);
                }
            }
            best
        }
    }
}

/// Parse a full RFC3339, a bare datetime-local ("2026-06-28T14:30[:00]"), or a
/// bare time-of-day ("14:30[:00]") into a Local DateTime. Bare values are Local.
pub fn parse_local(s: &str) -> Option<DateTime<Local>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Local));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            match Local.from_local_datetime(&naive) {
                LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => return Some(dt),
                LocalResult::None => {}
            }
        }
    }
    for fmt in ["%H:%M:%S", "%H:%M"] {
        if let Ok(t) = NaiveTime::parse_from_str(s, fmt) {
            let naive = NaiveDateTime::new(Local::now().date_naive(), t);
            match Local.from_local_datetime(&naive) {
                LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => return Some(dt),
                LocalResult::None => {}
            }
        }
    }
    None
}
