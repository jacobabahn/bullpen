//! Drawing the board. It sits in a narrow pane, so every line is cut to fit.

use crate::model::{self, fit, pad, width, Action, Layout, Task, STALE};
use crate::{App, Mode};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
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

fn mark_style(status: &str) -> Style {
    match status {
        "needs" => bold().fg(AMBER),
        "stopped" => bold().fg(Color::Red),
        "ready" => bold().fg(Color::Green),
        "review" => bold().fg(Color::Blue),
        _ => dim(),
    }
}

fn spinner(app: &App) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(app.started.elapsed().as_millis() / 100) as usize % FRAMES.len()]
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

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.height = area.height;
    let (w, h) = (area.width as usize, area.height as usize);
    let buf = f.buffer_mut();

    if let Mode::Overlay { title, lines, peek } = &app.mode {
        put(buf, area, 0, 1, span(fit(title, w.saturating_sub(2)), bold()));
        for (j, line) in lines.iter().take(h.saturating_sub(3)).enumerate() {
            put(buf, area, 2 + j, 1, fit(line, w.saturating_sub(2)));
        }
        let foot = if peek.is_some() { "live · any key returns" } else { "any key returns" };
        put(buf, area, h.saturating_sub(1), 1, span(foot, dim()));
        return;
    }

    let rounds = app.cfg.review_rounds;
    let layout = app.cfg.layout;
    let groups = model::board_groups(&app.tasks, app.finished);
    let cur_id = app.current().map(|t| t.id.clone());
    let is_cur = |t: &Task| Some(&t.id) == cur_id.as_ref();
    // under the title, to the right edge
    let detail_w = |t: &Task| w.saturating_sub(5 + width(&t.id) + 2);

    // header: the repository, and how many tasks are open
    let open = app.tasks.iter().filter(|t| !t.closed()).count();
    put(buf, area, 0, 1, span(fit(&app.repo_name, w.saturating_sub(12)), bold()));
    let count = if open > 0 { format!("{open} open") } else { String::new() };
    put(buf, area, 0, w.saturating_sub(2 + width(&count)).max(1), span(count, dim()));

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
                // what a task needs from you gets two lines; anything else one
                let lines = if t.needs_you() { 2 } else { 1 };
                for d in model::wrap(&model::task_detail(t, rounds), detail_w(t), lines) {
                    if !d.is_empty() {
                        rows.push(Row::Detail(t, d));
                    }
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

    let now = model::now();
    for (j, row) in rows.iter().skip(top).take(avail).enumerate() {
        let y = 2 + j;
        if let Row::Title(t) | Row::Detail(t, _) = row {
            if t.needs_you() {
                let c = if t.status == "needs" { AMBER } else { Color::Red };
                put(buf, area, y, 0, span("▌", Style::new().fg(c)));
            }
        }
        match row {
            Row::Head(text) => put(buf, area, y, 1, span(text.clone(), bold())),
            Row::Title(t) => {
                // a task with a command running gets a spinner in place of its mark
                let mark = match app.job(&t.id) {
                    Some(_) => span(spinner(app), bold()),
                    None => span(model::mark(&t.status), mark_style(&t.status)),
                };
                put(buf, area, y, 1, mark);
                put(buf, area, y, 3, title_line(app, t, is_cur(t), w, now));
            }
            Row::Detail(t, text) => {
                put(buf, area, y, 3 + width(&t.id) + 2, span(fit(text, detail_w(t)), dim()));
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
        for (j, line) in model::wrap(&model::task_detail(cur, rounds), w.saturating_sub(2), 3).into_iter().enumerate() {
            put(buf, area, yb + 2 + j, 1, span(line, dim()));
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
            Action::Drop => "drop",
            Action::DropForce => "discard and drop",
        };
        put(buf, area, last, 1, span(fit(&format!("y {verb}   any other key cancels"), w.saturating_sub(2)), bold()));
    } else {
        let keys = model::task_keys(cur);
        let keys = if !keys.is_empty() && keys.chars().count() + 9 < w.saturating_sub(2) {
            format!("{keys}   ? keys")
        } else if keys.is_empty() {
            "? keys".into()
        } else {
            keys.into()
        };
        put(buf, area, last, 1, span(fit(&keys, w.saturating_sub(2)), dim()));
    }
}

/// A task's line: its ID, title and age, or the y/n question it's waiting on.
fn title_line(app: &App, t: &Task, selected: bool, w: usize, now: chrono::DateTime<chrono::FixedOffset>) -> Line<'static> {
    let left = format!("{}  ", t.id);
    let width_all = w.saturating_sub(5); // from column 3 to the header's right edge

    if let Some((action, tid)) = &app.pending {
        if *tid == t.id {
            // the row becomes the question, in place: nothing below moves
            let tone = if *action == Action::Accept { Color::Green } else { Color::Red };
            let q = model::confirm_text(*action, t, &app.cfg.merge);
            let yn = "y / n";
            let line = format!("{left}{}", fit(&q, width_all.saturating_sub(width(&left) + width(yn) + 2)));
            let line = pad(&line, width_all.saturating_sub(width(yn))) + yn;
            return Line::from(span(line, rev().add_modifier(Modifier::BOLD).fg(tone)));
        }
    }

    let age = model::ago(t.stamp(), now);
    let title = fit(&t.title, width_all.saturating_sub(width(&left) + width(&age) + 2));
    let middle = pad(&format!("  {title}"), width_all.saturating_sub(width(&t.id) + width(&age)));
    let mut base = Style::new();
    if selected {
        base = base.patch(rev());
    }
    if t.closed() {
        base = base.patch(dim());
    }
    let id_style = if selected { rev() } else { dim() };
    let age_style = if t.status == "needs" && model::seconds_in_status(t, now) >= STALE {
        let s = bold().fg(Color::Red);
        if selected { s.patch(rev()) } else { s }
    } else if selected {
        base
    } else {
        dim()
    };
    Line::from(vec![span(t.id.clone(), id_style), span(middle, base), span(age, age_style)])
}
