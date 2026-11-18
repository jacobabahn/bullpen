//! The board's groups (core's table) and marks, and what it hides.

use super::*;

// The board groups tasks by what the person does next (core's BOARD_GROUPS). Colour
// only marks status; the one loud thing is the amber bar beside a task that needs you.
// A task its coordinator sees to (triage) sits dimmed below, with no bar.

pub use bullpen_core::words::BOARD_GROUPS;

/// Seconds a task can need you before its age turns red; each status's mark. `ls`
/// shows the same marks.
pub use bullpen_core::words::{mark, STALE};

/// The groups that have tasks, each sorted by ID; the closed ones only with `closed`.
pub fn board_groups(tasks: &[Task], closed: bool) -> Vec<(&'static str, Vec<&Task>)> {
    BOARD_GROUPS
        .iter()
        .filter(|(heading, _)| closed || *heading != "Closed")
        .filter_map(|(heading, statuses)| {
            let mut ts: Vec<&Task> = tasks.iter().filter(|t| statuses.contains(&t.status.as_str())).collect();
            ts.sort_by_key(|t| t.num());
            (!ts.is_empty()).then_some((*heading, ts))
        })
        .collect()
}

/// The groups this board shows: board_groups without the tasks it hides.
pub fn shown_groups<'a>(tasks: &'a [Task], cfg: &Config, me: Option<&str>, closed: bool) -> Vec<(&'static str, Vec<&'a Task>)> {
    board_groups(tasks, closed)
        .into_iter()
        .filter_map(|(heading, ts)| {
            let ts: Vec<&Task> = ts.into_iter().filter(|t| !cfg.hides(t, me)).collect();
            (!ts.is_empty()).then_some((heading, ts))
        })
        .collect()
}

/// How many tasks the board hides, by coordinator in the config's order: those it
/// would show with the others' tasks on (closed ones only with `closed`).
pub fn hidden(tasks: &[Task], cfg: &Config, me: Option<&str>, closed: bool) -> Vec<(String, usize)> {
    let hid: Vec<&Task> = board_groups(tasks, closed).into_iter().flat_map(|(_, ts)| ts).filter(|t| cfg.hides(t, me)).collect();
    cfg.coordinators
        .iter()
        .map(|c| (c.clone(), hid.iter().filter(|t| t.coordinator.as_deref() == Some(c)).count()))
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// What a board with nothing to show says instead, the first line bold: that there
/// are no tasks yet, or what the tasks it has are hidden behind (`o`, `f`).
pub fn empty_board(tasks: &[Task], cfg: &Config, me: Option<&str>, closed: bool) -> Vec<String> {
    if tasks.is_empty() {
        return vec!["No tasks yet.".into(), "Tell the coordinator what you want done.".into()];
    }
    let hidden = hidden(tasks, cfg, me, closed);
    let mut out = vec![if hidden.is_empty() { "Nothing open." } else { "Nothing of yours open." }.to_string()];
    let n: usize = hidden.iter().map(|(_, n)| n).sum();
    match &hidden[..] {
        [] => {}
        [(c, _)] => out.push(format!("o shows {n} of {c}'s.")),
        _ => out.push(format!("o shows {n} of other coordinators'.")),
    }
    let shut = tasks.iter().filter(|t| t.closed() && !cfg.hides(t, me)).count();
    if !closed && shut > 0 {
        out.push(format!("f shows {shut} closed."));
    }
    out
}

/// The header's note of what's hidden: "+4 in c2 · o shows", "" when nothing is.
/// Without `key` (on a crowded top line) it's "+4 in c2".
pub fn hidden_hint(hidden: &[(String, usize)], key: bool) -> String {
    if hidden.is_empty() {
        return String::new();
    }
    let n: usize = hidden.iter().map(|(_, n)| n).sum();
    let whose: Vec<&str> = hidden.iter().map(|(c, _)| c.as_str()).collect();
    let key = if key { " · o shows" } else { "" };
    format!("+{n} in {}{key}", whose.join(", "))
}

/// A task's title as the board shows it: another coordinator's starts with its ID.
pub fn shown_title(t: &Task, cfg: &Config, me: Option<&str>) -> String {
    match t.coordinator.as_deref() {
        Some(c) if cfg.theirs(t, me) => format!("{c} · {}", t.title),
        _ => t.title.clone(),
    }
}

#[cfg(test)]
#[path = "tests/test_groups.rs"]
mod tests;
