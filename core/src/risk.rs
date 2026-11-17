//! A task's risk label, as `ls`, `show`, the news and the board say it. The command
//! line works it out when the task is ready (cli/src/risk.rs) and keeps it on the task
//! (`risk`: level, why, and its two parts, rules and review); this only reads it
//! (`stored`, `shown`) and says it (`label`).

use crate::json::{obj, s, Obj};

/// The levels, lowest first.
pub const LEVELS: [&str; 3] = ["low", "medium", "high"];

/// A level's place in LEVELS (0 for one it doesn't know).
pub fn rank(level: &str) -> usize {
    LEVELS.iter().position(|l| *l == level).unwrap_or(0)
}

/// (level, why) of a task's label, whatever its status, or None when it has none.
pub fn stored(t: &Obj) -> Option<(&str, &str)> {
    let r = obj(t, "risk");
    let level = s(r, "level");
    LEVELS.contains(&level).then(|| (level, s(r, "why")))
}

/// (level, why) of a ready task's label: what the board and `ls` show, or None.
pub fn shown(t: &Obj) -> Option<(&str, &str)> {
    if s(t, "status") != "ready" {
        return None;
    }
    stored(t)
}

/// "risk: high · changes how accept merges".
pub fn label(level: &str, why: &str) -> String {
    match why {
        "" => format!("risk: {level}"),
        why => format!("risk: {level} · {why}"),
    }
}

/// The SGR colour of a level's word: green, amber (yellow), bold red.
pub fn level_sgr(level: &str) -> &'static str {
    match level {
        "high" => "1;31",
        "medium" => "33",
        _ => "32",
    }
}

#[cfg(test)]
#[path = "test_risk.rs"]
mod tests;
