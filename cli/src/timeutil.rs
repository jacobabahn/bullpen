//! Time as bullpen writes it: ISO stamps with the local offset, ages, and the clock.

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};

pub type Stamp = DateTime<FixedOffset>;

pub fn now() -> Stamp {
    Local::now().fixed_offset()
}

/// core.now(): "2026-11-12T21:42:21-06:00".
pub fn iso_now() -> String {
    now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// With microseconds, as save_task stamps `since`.
pub fn iso_now_micro() -> String {
    now().format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
}

/// datetime.fromisoformat for the stamps bullpen writes. One without an offset is
/// read as local time (Python would fail to subtract it from an aware one).
pub fn parse(stamp: &str) -> Option<Stamp> {
    if let Ok(t) = DateTime::parse_from_rfc3339(stamp) {
        return Some(t);
    }
    for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(t) = NaiveDateTime::parse_from_str(stamp, f) {
            return Local.from_local_datetime(&t).single().map(|t| t.fixed_offset());
        }
    }
    None
}

/// core.seconds_since, as a float. A stamp that doesn't parse counts as now (0).
pub fn seconds_since(stamp: &str, at: Option<Stamp>) -> f64 {
    let at = at.unwrap_or_else(now);
    parse(stamp).map_or(0.0, |t| (at - t).num_microseconds().unwrap_or(0) as f64 / 1e6)
}

/// limit.clock: "3:00 PM", with the day when it isn't today: "Mon 12:00 AM", "Oct 9 3:00 PM".
pub fn clock(when: Stamp, at: Option<Stamp>) -> String {
    let at = at.unwrap_or_else(now).with_timezone(&Local);
    let when = when.with_timezone(&Local);
    let text = when.format("%-I:%M %p").to_string();
    let days = (when.date_naive() - at.date_naive()).num_days();
    if days == 0 {
        text
    } else if (1..7).contains(&days) {
        format!("{} {text}", when.format("%a"))
    } else {
        format!("{} {text}", when.format("%b %-d"))
    }
}
