//! The `?` list: every board key under its heading, then the marks, scrolled to fit.

use super::{bold, dim, mark_style, put, span, AMBER, REVIEW, SPINNER, WORKING};
use crate::model::{self, fit, pad};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

/// The `?` list: a heading, then one short line per key. The board wraps it to its
/// width; the long explanations are in docs/board.md.
pub const BOARD_KEYS: &[(&str, &[(&str, &str)])] = &[
    ("Moving", &[
        ("j k", "select a task (↑ ↓ too)"),
        ("PgDn", "a page down (ctrl+d: half)"),
        ("PgUp", "a page up (ctrl+u: half)"),
        ("G", "the last task (End; Home: first)"),
        ("n", "next that needs you, else ready"),
        ("⏎", "open its pane, or its reviewer's"),
        ("c", "back to the coordinator"),
        ("/", "pick any open task, anywhere"),
    ]),
    ("The selected task", &[
        ("a", "accept and merge, or close"),
        ("d", "drop it"),
        ("i", "answer its question"),
        ("t", "tell its worker something"),
        ("p", "peek at its worker's screen"),
        ("v", "view its diff"),
        ("g", "guided tour of its change"),
        ("s", "start it"),
        ("r", "resume a stopped worker"),
    ]),
    ("The board", &[
        ("l", "switch layout"),
        ("f", "show closed tasks too"),
        ("o", "other coordinators' tasks too"),
        ("esc", "put a red message away"),
        ("?", "this list"),
        ("q", "quit the board (y to be sure)"),
    ]),
    ("Later: the backlog", &[
        ("L", "show its items, or fold them"),
        ("y", "keep an idea that asks"),
        ("n", "drop the item (y to be sure)"),
        ("s", "start it (or ask for a brief)"),
        ("J K", "move it down, up the list"),
        ("⏎", "the item in full"),
    ]),
    ("In a tour", &[
        ("n p", "next, previous step"),
        ("j k", "scroll (space b too)"),
        ("c", "comment on the step"),
        ("s", "at the end: send the comments"),
        ("a", "accept (before the end: a twice)"),
        ("t", "tell its worker something"),
        ("⏎ v", "close it, open its pane or diff"),
        ("esc", "close it"),
    ]),
];

/// The `?` list's lines at width w: headings bold; each key bold in its column,
/// what it does wrapped under itself; then the marks.
fn help_lines(w: usize) -> Vec<Line<'static>> {
    const KEY: usize = 4; // the key column, then two spaces
    let mut out = Vec::new();
    let mut section = |heading: &str, entries: Vec<(String, &str, Style)>| {
        if !out.is_empty() {
            out.push(Line::default());
        }
        out.push(Line::from(span(fit(heading, w), bold())));
        for (key, what, key_style) in entries {
            let wrapped = model::wrap(what, w.saturating_sub(2 + KEY + 2), usize::MAX);
            for (i, text) in wrapped.into_iter().enumerate() {
                let key = if i == 0 { pad(&key, KEY) } else { " ".repeat(KEY) };
                out.push(Line::from(vec![span("  ", Style::new()), span(key, key_style), span(format!("  {text}"), Style::new())]));
            }
        }
    };
    for (heading, keys) in BOARD_KEYS {
        section(heading, keys.iter().map(|(k, d)| (k.to_string(), *d, bold())).collect());
    }
    // each mark as the board draws it, in its colour
    let m = |st: &str, what: &'static str| (model::mark(st).to_string(), what, mark_style(st));
    section("Marks", vec![
        m("needs", "needs you"),
        m("stopped", "stopped: its agent exited"),
        m("triage", "the coordinator is on it"),
        m("ready", "ready to accept"),
        m("finished", "finished: nothing to merge, or a branch to keep"),
        (format!("{} {}", REVIEW[0], model::mark("review")), "in review (it turns)", mark_style("review")),
        (format!("{} {}", WORKING[0], model::mark("working")), "working (it turns)", Style::new()),
        m("todo", "queued, not started"),
        m("done", "accepted"),
        m("dropped", "dropped"),
        (SPINNER[0].to_string(), "a command or its tour running", bold()),
        ("▌".to_string(), "needs you; red: stopped", Style::new().fg(AMBER)),
        (format!("{}↻", model::mark("todo")), "came from the backlog", dim()),
        ("?".to_string(), "an idea asking for a yes", bold().fg(AMBER)),
        ("◇".to_string(), "an idea on the list", dim()),
    ]);
    out
}

/// The `?` list from line `top`, with a foot saying how to scroll when it doesn't
/// all fit. Returns `top`, kept in range.
pub(super) fn draw_help(buf: &mut Buffer, area: Rect, h: usize, top: usize) -> usize {
    let w = area.width as usize;
    let lines = help_lines(w.saturating_sub(2));
    let room = h.saturating_sub(3).max(1); // the title, a blank line, the foot
    let top = top.min(lines.len().saturating_sub(room));
    put(buf, area, 0, 1, span(fit("Board keys", w.saturating_sub(2)), bold()));
    for (j, line) in lines.iter().skip(top).take(room).enumerate() {
        put(buf, area, 2 + j, 1, line.clone());
    }
    if top > 0 {
        put(buf, area, 1, w.saturating_sub(3), span("↑", dim()));
    }
    let more = top + room < lines.len();
    if more {
        put(buf, area, h.saturating_sub(2), w.saturating_sub(3), span("↓", dim()));
    }
    let foot = if more || top > 0 { "j k space scroll · esc returns" } else { "any key returns" };
    put(buf, area, h.saturating_sub(1), 1, span(fit(foot, w.saturating_sub(2)), dim()));
    top
}

#[cfg(test)]
#[path = "test_help.rs"]
mod tests;
