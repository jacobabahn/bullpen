//! Drawing the board. It sits in a narrow pane, so every line is cut to fit.

use crate::model::{self, fit, pad, width, Action, Layout, Task, STALE};
use crate::{App, Mode};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ansi_to_tui::IntoText;
use ratatui::Frame;

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}
fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}
fn rev() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}
const AMBER: Color = Color::Yellow;
/// The selected task's row: a tint from the terminal's own palette, so status
/// colours still show on it.
const TINT: Color = Color::DarkGray;

fn mark_style(status: &str) -> Style {
    match status {
        "needs" => bold().fg(AMBER),
        "stopped" => bold().fg(Color::Red),
        "ready" | "finished" => bold().fg(Color::Green),
        "review" => bold().fg(Color::Blue),
        _ => dim(),
    }
}

fn frame(app: &App, frames: &[&'static str], ms: u128) -> &'static str {
    frames[(app.started.elapsed().as_millis() / ms) as usize % frames.len()]
}

/// A task with a command running (accept, drop, start, resume): a ring with a gap going round.
fn spinner(app: &App) -> &'static str {
    frame(app, &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"], 100)
}

/// A working task: the six-dot snake.
fn working_mark(app: &App) -> &'static str {
    frame(app, &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"], 100)
}

/// A task in review: a quarter arc turning.
fn review_mark(app: &App) -> &'static str {
    frame(app, &["◜", "◝", "◞", "◟"], 150)
}

/// Write a line at (x, y), never into the last column.
fn put(buf: &mut Buffer, area: Rect, y: usize, x: usize, line: impl Into<Line<'static>>) {
    let (w, h) = (area.width as usize, area.height as usize);
    if y < h && x + 1 < w {
        buf.set_line(area.x + x as u16, area.y + y as u16, &line.into(), (w - 1 - x) as u16);
    }
}

fn span(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(text.into(), style)
}

enum Row<'a> {
    Head(String),
    Title(&'a Task),
    Detail(&'a Task, String),
    Gap,
}

/// A line of a worker's screen, with the colours its escape codes give it.
fn screen_line(raw: &str) -> Line<'static> {
    match raw.into_text() {
        Ok(text) => text.lines.into_iter().next().unwrap_or_default(),
        Err(_) => Line::from(raw.to_string()),
    }
}

/// The header's right side: how many tasks are in each state, in the marks'
/// colours, dropping the least urgent until it fits.
fn counts(tasks: &[Task], room: usize) -> Line<'static> {
    let n = |sts: &[&str]| tasks.iter().filter(|t| sts.contains(&t.status.as_str())).count();
    let mut parts: Vec<(String, Style)> = [
        (n(&["needs"]), "needs"),
        (n(&["stopped"]), "stopped"),
        (n(&["ready"]), "ready"),
        (n(&["finished"]), "finished"),
        (n(&["triage"]), "triage"),
        (n(&["review"]), "review"),
        (n(&["working"]), "working"),
        (n(&["todo"]), "todo"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, st)| (format!("{} {n}", model::mark(st)), mark_style(st)))
    .collect();
    let total = |p: &[(String, Style)]| p.iter().map(|(s, _)| width(s)).sum::<usize>() + 2 * p.len().saturating_sub(1);
    while !parts.is_empty() && total(&parts) > room {
        parts.pop();
    }
    let mut spans = Vec::new();
    for (i, (text, style)) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(span("  ", Style::new()));
        }
        spans.push(span(text, style));
    }
    Line::from(spans)
}

/// The footer's keys: each key bold, what it does dim.
fn key_line(keys: &[(&str, &str)]) -> Line<'static> {
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

pub fn draw(f: &mut Frame, app: &mut App) {
    let full = f.area();
    app.height = full.height;
    let buf = f.buffer_mut();

    if let Mode::Overlay { title, lines, peek } = &app.mode {
        let (area, w, h) = (full, full.width as usize, full.height as usize);
        put(buf, area, 0, 1, span(fit(title, w.saturating_sub(2)), bold()));
        for (j, line) in lines.iter().take(h.saturating_sub(3)).enumerate() {
            put(buf, area, 2 + j, 1, screen_line(line));
        }
        let foot = if peek.is_some() { "live · any key returns" } else { "any key returns" };
        put(buf, area, h.saturating_sub(1), 1, span(foot, dim()));
        return;
    }

    let area = full;
    let (w, h) = (area.width as usize, area.height as usize);

    let layout = app.cfg.layout;
    let groups = model::board_groups(&app.tasks, app.finished);
    let cur_id = app.current().map(|t| t.id.clone());
    let is_cur = |t: &Task| Some(&t.id) == cur_id.as_ref();
    // IDs share a column as wide as the longest, so titles and details line up
    let idw = groups.iter().flat_map(|(_, ts)| ts).map(|t| width(&t.id)).max().unwrap_or(0);
    // under the title, to the right edge
    let detail_w = w.saturating_sub(5 + idw + 2);

    // header: the repository, and how many tasks are in each state
    let name = fit(&app.repo_name, w.saturating_sub(2).min(24));
    // a small marker when approved tasks accept themselves
    let auto = if app.cfg.auto_accept { " · auto-accept" } else { "" };
    // with several coordinators: whose board this is (other ones' tasks are dimmed)
    let auto = match app.me.as_deref() {
        Some(me) if app.cfg.coordinators.len() > 1 => format!(" · {me}{auto}"),
        _ => auto.to_string(),
    };
    let auto = auto.as_str();
    let count = counts(&app.tasks, w.saturating_sub(width(&name) + width(auto) + 5));
    put(buf, area, 0, w.saturating_sub(2 + count.width()).max(1), count);
    put(buf, area, 0, 1 + width(&name), span(auto, dim()));
    put(buf, area, 0, 1, span(name, bold()));

    let now = model::now();
    let mut rows: Vec<Row> = Vec::new();
    for (gi, (heading, ts)) in groups.iter().enumerate() {
        if gi > 0 {
            rows.push(Row::Gap);
        }
        if layout == Layout::Grouped {
            rows.push(Row::Head(format!("{heading}  {}", ts.len())));
        }
        for t in ts {
            rows.push(Row::Title(t));
            if layout == Layout::Grouped {
                // what a task needs from you, GitHub said, or the usage limit gets two lines; anything else one
                let lines = if t.flagged() || t.is_limited() || t.status == "finished" || !model::pr_phrase(t).is_empty() { 2 } else { 1 };
                for d in model::wrap(&model::detail_with_auto(t, &app.cfg, now), detail_w, lines) {
                    if !d.is_empty() {
                        rows.push(Row::Detail(t, d));
                    }
                }
                if !model::task_stat(t).is_empty() {
                    rows.push(Row::Detail(t, model::task_stat(t).into()));
                }
            }
        }
    }

    // keep the selected task in view, starting at a heading or a task
    let footer = if layout == Layout::Grouped { 3 } else { 7 };
    let avail = h.saturating_sub(3 + footer).max(1); // one line spare for the scroll mark
    let sel_row = rows.iter().position(|r| matches!(r, Row::Title(t) if is_cur(t)));
    let mut top = app.top;
    if let Some(i) = sel_row {
        if i < top {
            top = i - usize::from(i > 0 && matches!(rows[i - 1], Row::Head(_)));
        } else if i + 1 >= top + avail {
            top = i + 2 - avail;
        }
    }
    top = top.min(rows.len().saturating_sub(avail));
    while top > 0 && top < rows.len() && matches!(rows[top], Row::Detail(..) | Row::Gap) {
        top += 1;
    }
    if let Some(i) = sel_row {
        if i >= top + avail {
            top = i;
        }
    }
    app.top = top;

    if app.tasks.is_empty() {
        put(buf, area, 2, 1, span("No tasks yet.", bold()));
        put(buf, area, 3, 1, span(fit("Tell the coordinator what you want done.", w.saturating_sub(2)), dim()));
    }

    for (j, row) in rows.iter().skip(top).take(avail).enumerate() {
        let y = 2 + j;
        if let Row::Title(t) | Row::Detail(t, _) = row {
            if t.flagged() {
                let c = if t.status == "stopped" { Color::Red } else { AMBER };
                put(buf, area, y, 0, span("▌", Style::new().fg(c)));
            }
        }
        match row {
            Row::Head(text) => put(buf, area, y, 1, span(text.clone(), bold())),
            Row::Title(t) => {
                // a task with a command running gets a spinner in place of its mark
                let mark = match app.job(&t.id) {
                    Some(_) => span(spinner(app), bold()),
                    // an agent that has gone quiet: a still mark, in the colour of one that needs you
                    None if t.is_stuck() => span(model::mark(&t.status), mark_style("needs")),
                    // one stopped at the usage limit isn't working either: a still mark
                    None if t.is_limited() => span(model::mark(&t.status), mark_style(&t.status)),
                    None if t.status == "working" => span(working_mark(app), Style::new()),
                    None if t.status == "review" => span(review_mark(app), mark_style("review")),
                    None => span(model::mark(&t.status), mark_style(&t.status)),
                };
                put(buf, area, y, 1, mark);
                put(buf, area, y, 3, title_line(app, t, is_cur(t), w, idw, now));
            }
            Row::Detail(_, text) => {
                put(buf, area, y, 3 + idw + 2, span(fit(text, detail_w), dim()));
            }
            Row::Gap => {}
        }
    }
    if top + avail < rows.len() {
        put(buf, area, 2 + avail, w.saturating_sub(3), span("↓", dim()));
    }
    if top > 0 {
        put(buf, area, 1, w.saturating_sub(3), span("↑", dim()));
    }

    // bottom: the compact layout's detail panel, a message line, the keys
    let cur = app.current();
    if let (Layout::Compact, Some(cur)) = (layout, cur) {
        let yb = h.saturating_sub(footer);
        put(buf, area, yb, 1, span("─".repeat(w.saturating_sub(3)), dim()));
        put(buf, area, yb + 1, 1, span(fit(&format!("{}  {}", cur.id, cur.title), w.saturating_sub(2)), bold()));
        let stat = model::task_stat(cur);
        let mut lines = model::wrap(&model::detail_with_auto(cur, &app.cfg, now), w.saturating_sub(2), if stat.is_empty() { 3 } else { 2 });
        if !stat.is_empty() {
            lines.push(stat.into());
        }
        for (j, line) in lines.into_iter().enumerate() {
            put(buf, area, yb + 2 + j, 1, span(fit(&line, w.saturating_sub(2)), dim()));
        }
    }
    let last = h.saturating_sub(1);
    // the message line: what just happened, else what's still running
    let line = if !app.msg.is_empty() {
        span(fit(&app.msg, w.saturating_sub(2)), bold())
    } else if let Some((id, job)) = app.running.last() {
        let more = if app.running.len() > 1 { format!(" (+{} more)", app.running.len() - 1) } else { String::new() };
        span(fit(&format!("{} {id}…{more}", job.doing()), w.saturating_sub(2)), dim())
    } else {
        span("", dim())
    };
    put(buf, area, h.saturating_sub(2), 1, line);
    if let Mode::Tell { id, text } = &app.mode {
        let label = format!("Tell {id}:");
        let x = width(&label) + 2;
        // show the end of what's typed when it's longer than the line
        let room = w.saturating_sub(x + 2);
        let shown: String = {
            let mut s: Vec<char> = text.chars().collect();
            while width(&s.iter().collect::<String>()) > room && !s.is_empty() {
                s.remove(0);
            }
            s.into_iter().collect()
        };
        put(buf, area, last, 1, span(label, bold()));
        put(buf, area, last, x, shown.clone());
        f.set_cursor_position(Position::new(area.x + (x + width(&shown)) as u16, area.y + last as u16));
    } else if let Some((action, _)) = &app.pending {
        let verb = match action {
            Action::Accept => "accept",
            Action::Close => "close",
            Action::Drop => "drop",
            Action::DropForce => "discard and drop",
        };
        put(buf, area, last, 1, span(fit(&format!("y {verb}   any other key cancels"), w.saturating_sub(2)), bold()));
    } else {
        // the task's keys on the left; "? keys" at the right edge when there's room
        let keys = key_line(&model::task_keys(cur));
        let help = key_line(&[("?", "keys")]);
        let x = w.saturating_sub(2 + help.width());
        if keys.width() == 0 {
            put(buf, area, last, 1, help);
        } else {
            if 1 + keys.width() + 3 <= x {
                put(buf, area, last, x, help);
            }
            put(buf, area, last, 1, keys);
        }
    }
}

/// A task's line: its ID, title and age, or the y/n question it's waiting on.
fn title_line(app: &App, t: &Task, selected: bool, w: usize, idw: usize, now: chrono::DateTime<chrono::FixedOffset>) -> Line<'static> {
    let id = pad(&t.id, idw);
    let left = format!("{id}  ");
    let width_all = w.saturating_sub(5); // from column 3 to the header's right edge

    if let Some((action, tid)) = &app.pending {
        if *tid == t.id {
            // the row becomes the question, in place: nothing below moves
            let tone = if matches!(action, Action::Accept | Action::Close) { Color::Green } else { Color::Red };
            let q = model::confirm_text(*action, t, &app.cfg.merge);
            let yn = "y / n";
            let line = format!("{left}{}", fit(&q, width_all.saturating_sub(width(&left) + width(yn) + 2)));
            let line = pad(&line, width_all.saturating_sub(width(yn))) + yn;
            return Line::from(span(line, rev().add_modifier(Modifier::BOLD).fg(tone)));
        }
    }

    let age = model::ago(t.stamp(), now);
    let title = fit(&t.title, width_all.saturating_sub(width(&left) + width(&age) + 2));
    let middle = pad(&format!("  {title}"), width_all.saturating_sub(idw + width(&age)));
    let stale = t.status == "needs" && model::seconds_in_status(t, now) >= STALE;
    // dim text is hard to read on the tint, so the selected row drops it
    let (id_style, title_style, age_style) = if selected {
        let on = Style::new().bg(TINT);
        (on.add_modifier(Modifier::BOLD), on, if stale { on.fg(Color::Red).add_modifier(Modifier::BOLD) } else { on })
    } else {
        let title = if t.closed() || t.triage() || app.cfg.theirs(t, app.me.as_deref()) { dim() } else { Style::new() };
        (dim(), title, if stale { bold().fg(Color::Red) } else { dim() })
    };
    Line::from(vec![span(id, id_style), span(middle, title_style), span(age, age_style)])
}
