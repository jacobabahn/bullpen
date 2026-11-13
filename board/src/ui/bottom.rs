//! The board's bottom rows: the message (or the y/n question) above the keys, and the
//! key row, or what's asked or typed in its place.

use super::{bold, dim, put, span, Sizes};
use crate::edit::LineEdit;
use crate::model::{self, fit, width, Action, Task};
use crate::{App, Kind, Mode};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// How many rows a message wraps to, at most.
const MESSAGE_LINES: usize = 3;

/// The message rows above the keys, at most `most` of them: the y/n question being
/// asked (red when risky), what just happened (a failure red, what worked green),
/// else what's still running.
pub(super) fn message(app: &App, w: usize, most: usize) -> Vec<Span<'static>> {
    let room = w.saturating_sub(2);
    let most = MESSAGE_LINES.min(most).max(1);
    if let Some((action, t)) = app.pending.as_ref().and_then(|(a, id)| Some((*a, app.tasks.iter().find(|t| &t.id == id)?))) {
        let tone = if model::confirm_risky(action, t, &app.cfg) { Color::Red } else { Color::Green };
        // the fullest words that fit the rows there are, else the shortest, cut
        let texts: Vec<String> = model::confirm_texts(action, t, &app.cfg).iter().map(|q| format!("{}: {q}", t.id)).collect();
        let text = texts.iter().find(|q| model::wrap(q, room, usize::MAX).len() <= most).unwrap_or(&texts[texts.len() - 1]);
        let style = bold().fg(tone);
        return model::wrap(text, room, most).into_iter().map(|l| span(l, style)).collect();
    }
    if !app.msg.is_empty() {
        let style = match app.msg.kind {
            Kind::Failed => bold().fg(Color::Red),
            Kind::Done => bold().fg(Color::Green),
            Kind::Info => bold(),
        };
        let lines = model::wrap(&app.msg.text, room, most);
        return lines.into_iter().map(|l| span(l, style)).collect();
    }
    if let Some((id, job)) = app.running.last() {
        let more = if app.running.len() > 1 { format!(" (+{} more)", app.running.len() - 1) } else { String::new() };
        return vec![span(fit(&format!("{} {id}…{more}", job.doing()), room), dim())];
    }
    vec![]
}

/// The message rows, then the last row: a line being typed, the y/n question's keys,
/// or the selected task's keys. Returns the cursor's column when a line is typed there.
pub(super) fn draw_bottom(buf: &mut Buffer, area: Rect, app: &App, s: &Sizes, msg: Vec<Span<'static>>, cur: Option<&Task>) -> Option<usize> {
    let w = s.w;
    let last = s.h.saturating_sub(1);
    if s.msg_on_keys {
        put(buf, area, last, 1, msg.into_iter().next().unwrap_or_default());
        return None;
    }
    let first = s.h.saturating_sub(1 + msg.len());
    for (j, line) in msg.into_iter().enumerate() {
        put(buf, area, first + j, 1, line);
    }
    if let Mode::Tell { id, text, .. } = &app.mode {
        return Some(input_line(buf, area, last, &format!("Tell {id}:"), text));
    } else if let Mode::Quit = &app.mode {
        let texts = ["y quits the board   any other key stays", "y quits   n stays"];
        put(buf, area, last, 1, span(model::first_fit(&texts.map(String::from), w.saturating_sub(2)), bold()));
    } else if let Mode::Stale { .. } = &app.mode {
        put(buf, area, last, 1, span(fit("n new tour   o the old one   esc cancels", w.saturating_sub(2)), bold()));
    } else if let Some((action, tid)) = &app.pending {
        let t = app.tasks.iter().find(|t| &t.id == tid);
        let risky = t.is_some_and(|t| model::confirm_risky(*action, t, &app.cfg));
        // the PR a drop closes, said here too: a narrow row has no room for it
        let pr = model::pr_short(t.and_then(|t| t.pr.as_deref()));
        let verb = match action {
            Action::Accept if risky => "accept anyway".to_string(),
            Action::Accept => "accept".into(),
            Action::Close => "close".into(),
            Action::Drop if !pr.is_empty() => format!("drop and close {pr}"),
            Action::Drop => "drop".into(),
            Action::DropForce => "discard and drop".into(),
        };
        let texts = [format!("y {verb}   any other key cancels"), format!("y {verb}   n cancels")];
        let tone = if risky { bold().fg(Color::Red) } else { bold() };
        put(buf, area, last, 1, span(model::first_fit(&texts, w.saturating_sub(2)), tone));
    } else {
        put(buf, area, last, 1, key_row(model::task_keys(cur), w));
    }
    None
}

/// The footer's keys: each key bold, what it does dim.
pub(super) fn key_line(keys: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(span("  ", Style::new()));
        }
        spans.push(span(*key, bold()));
        spans.push(span(format!(" {what}"), dim()));
    }
    Line::from(spans)
}

/// A label, then what's typed, at row y: the part around the cursor when it's longer
/// than the line. Returns the cursor's column.
pub(super) fn input_line(buf: &mut Buffer, area: Rect, y: usize, label: &str, text: &LineEdit) -> usize {
    let w = area.width as usize;
    let x = width(label) + 2;
    let (shown, at) = text.view(w.saturating_sub(x + 2));
    put(buf, area, y, 1, span(label.to_string(), bold()));
    put(buf, area, y, x, shown);
    x + at
}

/// The key line: the task's keys, then `?` at the right edge. `?` is never dropped:
/// with no room for "? keys" it goes bare, and then the task's last keys go first.
fn key_row(mut keys: Vec<(&str, &str)>, w: usize) -> Line<'static> {
    let room = w.saturating_sub(2); // from column 1, never into the last column
    let help = key_line(&[("?", "keys")]);
    if keys.is_empty() {
        return help;
    }
    let line = |keys: &[(&str, &str)], help: Line<'static>, gap: usize| {
        let left = key_line(keys);
        let fill = room.saturating_sub(left.width() + help.width());
        if fill < gap && !keys.is_empty() {
            return None;
        }
        let mut spans = left.spans;
        spans.push(span(" ".repeat(fill), Style::new()));
        spans.extend(help.spans);
        Some(Line::from(spans))
    };
    if let Some(l) = line(&keys, help, 3) {
        return l;
    }
    loop {
        if let Some(l) = line(&keys, Line::from(span("?", bold())), 2) {
            return l;
        }
        keys.pop();
    }
}

#[cfg(test)]
#[path = "test_bottom.rs"]
mod tests;
