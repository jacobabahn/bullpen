//! The board's bottom rows: the message (or the y/n question) above the keys, and the
//! key row, or what's asked or typed in its place. With the image look they're values
//! for image/ (`Foot`), except while something's typed or asked there.

use super::{bold, dim, put, span, Sizes};
use crate::edit::LineEdit;
use crate::model::{self, fit, width, Action, Task};
use crate::{App, Kind, Mode};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// How many rows a message wraps to, at most.
const MESSAGE_LINES: usize = 3;

/// The message rows above the keys, at most `most` of them: the y/n question being
/// asked (red when risky), what just happened (a failure red, what worked green),
/// else what's still running.
pub(crate) fn message(app: &App, w: usize, most: usize) -> Vec<Span<'static>> {
    let room = w.saturating_sub(2);
    let most = MESSAGE_LINES.min(most).max(1);
    if let Some(i) = asked_item(app) {
        return model::wrap(&model::item_confirm(i), room, most).into_iter().map(|l| span(l, bold().fg(Color::Red))).collect();
    }
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

/// The message and key rows as values, for the image look (image/frame.rs): each line and
/// the row it's on, counted from the list's foot. None while something is typed or asked
/// there (a tell, a y/n question, the quit or stale tour question): that stays text.
pub(super) fn foot(app: &App, s: &Sizes, msg: &[Span<'static>], cur: Option<&Task>) -> Option<Foot> {
    if app.pending.is_some() || matches!(app.mode, Mode::Tell { .. } | Mode::Quit | Mode::Stale { .. }) {
        return None;
    }
    // a board with no row under its list (too short) keeps them as text
    if s.h < 3 + s.avail {
        return None;
    }
    // the rows from the list's end (the text's ↓ row on a tall board) to the board's last
    let last = s.h.saturating_sub(3 + s.avail);
    let tone = |sp: &Span| match (sp.style.fg, sp.style.add_modifier.contains(Modifier::DIM)) {
        (Some(Color::Red), _) => Tone::Red,
        (Some(Color::Green), _) => Tone::Green,
        (_, true) => Tone::Dim,
        _ => Tone::Plain,
    };
    if s.msg_on_keys {
        let line = msg.first().map(|m| FootLine::Msg(m.content.to_string(), tone(m)));
        return Some(Foot { lines: line.into_iter().map(|l| (last as u16, l)).collect() });
    }
    let first = last.saturating_sub(msg.len());
    let mut lines: Vec<(u16, FootLine)> =
        msg.iter().enumerate().map(|(j, m)| ((first + j) as u16, FootLine::Msg(m.content.to_string(), tone(m)))).collect();
    lines.push((last as u16, FootLine::Keys(keys(app, cur))));
    Some(Foot { lines })
}

/// The selected item's keys, else the selected task's.
pub(crate) fn keys(app: &App, cur: Option<&Task>) -> Vec<(String, String)> {
    let keys = match app.current_item() {
        Some(i) => model::item_keys(i),
        None => model::task_keys(cur),
    };
    keys.into_iter().map(|(k, what)| (k.to_string(), what.to_string())).collect()
}

/// The bottom rows as values (the image look's): each line and its row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Foot {
    pub lines: Vec<(u16, FootLine)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum FootLine {
    /// a message row, in its tone
    Msg(String, Tone),
    /// the key row: each key and what it does; `?` goes at the right edge
    Keys(Vec<(String, String)>),
}

/// A message's colour: bold, bold red (a failure, a risky question), bold green (what
/// worked), or dim (what's still running).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tone {
    Plain,
    Red,
    Green,
    Dim,
}

/// The Later item whose drop the board is asking about.
pub(super) fn asked_item(app: &App) -> Option<&model::Item> {
    let (Action::DropItem, id) = app.pending.as_ref()? else { return None };
    app.later.iter().find(|i| &i.id == id)
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
    } else if let Some(line) = pending_keys(app, w) {
        put(buf, area, last, 1, line);
    } else {
        let keys = match app.current_item() {
            Some(i) => model::item_keys(i),
            None => model::task_keys(cur),
        };
        put(buf, area, last, 1, key_row(keys, w));
    }
    None
}

/// The key row while a y/n is asked about a task: what y does (red when risky), and
/// that any other key cancels. None when nothing is asked.
pub(crate) fn pending_keys(app: &App, w: usize) -> Option<Span<'static>> {
    let (action, tid) = app.pending.as_ref()?;
    let t = app.tasks.iter().find(|t| &t.id == tid);
    let risky = *action == Action::DropItem || t.is_some_and(|t| model::confirm_risky(*action, t, &app.cfg));
    // the PR a drop closes, said here too: a narrow row has no room for it
    let pr = model::pr_short(t.and_then(|t| t.pr.as_deref()));
    let verb = match action {
        Action::Accept if risky => "accept anyway".to_string(),
        Action::Accept => "accept".into(),
        Action::Close => "close".into(),
        Action::Drop if !pr.is_empty() => format!("drop and close {pr}"),
        Action::Drop => "drop".into(),
        Action::DropForce => "discard and drop".into(),
        Action::DropItem => "drop".into(),
    };
    let texts = [format!("y {verb}   any other key cancels"), format!("y {verb}   n cancels")];
    let tone = if risky { bold().fg(Color::Red) } else { bold() };
    Some(span(model::first_fit(&texts, w.saturating_sub(2)), tone))
}

/// The footer's keys: each key bold, what it does dim.
pub(crate) fn key_line(keys: &[(&str, &str)]) -> Line<'static> {
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
pub(crate) fn input_line(buf: &mut Buffer, area: Rect, y: usize, label: &str, text: &LineEdit) -> usize {
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
#[path = "tests/test_bottom.rs"]
mod tests;
