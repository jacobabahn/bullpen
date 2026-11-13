//! Drawing the board. It sits in a narrow pane, so every line is cut to fit.

use crate::model::{self, fit, pad, width, Action, Layout, Task, STALE};
use crate::{App, Kind, Mode};
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
/// Under the selected task's rows: a tint from the terminal's own palette, so status
/// colours still show on it. Some themes make it the background colour, so the ID
/// in reverse video is what says which task is selected in every theme.
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
    frame(app, SPINNER, 100)
}

/// A working task: the six-dot snake, at two frames a second. It says "nothing for you
/// here", so it moves slowly; the command spinner is feedback and stays fast.
fn working_mark(app: &App) -> &'static str {
    frame(app, WORKING, MARK_MS)
}

/// A task in review: a quarter arc turning, as slowly as the working mark.
fn review_mark(app: &App) -> &'static str {
    frame(app, REVIEW, MARK_MS)
}

/// How long the working and review marks hold each frame.
pub const MARK_MS: u128 = 500;

const SPINNER: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

const WORKING: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const REVIEW: &[&str] = &["◜", "◝", "◞", "◟"];

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
    /// a line under a task; loud when it says what the task needs from you; its first
    /// columns (on a narrow board) the task's age
    Detail(&'a Task, String, bool, usize),
    Gap,
}

/// A line of a worker's screen, with the colours its escape codes give it.
fn screen_line(raw: &str) -> Line<'static> {
    match raw.into_text() {
        Ok(text) => text.lines.into_iter().next().unwrap_or_default(),
        Err(_) => Line::from(raw.to_string()),
    }
}

/// The header's right side: what you act on here, in the marks' colours.
fn counts(parts: Vec<(String, &'static str)>, sep: &'static str) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (text, st)) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(span(sep, dim()));
        }
        spans.push(span(text, mark_style(st)));
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

/// A peek's foot: it's live (while it reads the screen), the keys that act from it, and esc. Keys drop from the
/// right of the middle until it fits; esc stays.
fn peek_foot(w: usize, live: bool) -> Line<'static> {
    let mut keys = vec![("t", "tell"), ("⏎", "open"), ("i", "answer"), ("a", "accept"), ("v", "diff")];
    loop {
        let mut spans = if live { vec![span("live · ", dim())] } else { vec![] };
        spans.extend(key_line(&keys).spans);
        if !keys.is_empty() {
            spans.push(span("  ", Style::new()));
        }
        spans.extend(key_line(&[("esc", "returns")]).spans);
        let line = Line::from(spans);
        if line.width() <= w.saturating_sub(2) || keys.is_empty() {
            return line;
        }
        keys.pop();
    }
}

/// The empty rows to leave at the bottom (the config's board_bottom_gap): Rex draws
/// minimized panes' capsules over the bottom right of the window, so the board's
/// last lines would sit under them. A board too short for its other lines and
/// `need` more drops the gap first.
fn bottom_gap(app: &App, h: usize, need: usize) -> usize {
    let gap = app.cfg.bottom_gap;
    if h >= need + gap { gap } else { 0 }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let full = f.area();
    let buf = f.buffer_mut();

    if matches!(app.mode, Mode::Overlay { .. } | Mode::Answer { .. } | Mode::Help { .. } | Mode::Tell { reply: true, .. }) {
        // the title, a blank line, at least one line and the foot
        let h = full.height as usize;
        app.height = (h - bottom_gap(app, h, 4)) as u16;
    }
    if let Mode::Overlay { title, lines, peek } = &app.mode {
        let (area, w, h) = (full, full.width as usize, app.height as usize);
        put(buf, area, 0, 1, span(fit(title, w.saturating_sub(2)), bold()));
        // a peek that can't read the screen says why, in bold
        let failed = peek.is_some() && app.peek_failed;
        for (j, line) in lines.iter().take(h.saturating_sub(3)).enumerate() {
            let line = if failed { Line::from(span(line.clone(), bold())) } else { screen_line(line) };
            put(buf, area, 2 + j, 1, line);
        }
        let foot = if peek.is_some() { peek_foot(w, !failed) } else { Line::from(span("any key returns", dim())) };
        put(buf, area, h.saturating_sub(1), 1, foot);
        return;
    }
    if let Mode::Help { top } = &mut app.mode {
        *top = draw_help(buf, full, app.height as usize, *top);
        return;
    }
    if let Mode::Answer { title, prompt, .. } = &app.mode {
        draw_answer(buf, full, app.height as usize, title, prompt.as_ref());
        return;
    }
    if let Mode::Tell { id, text, reply: true } = &app.mode {
        let t = app.tasks.iter().find(|t| t.id == *id);
        let title = t.map_or(id.clone(), |t| format!("{}  {}", t.id, t.title));
        let question = t.and_then(|t| t.why.clone()).filter(|q| !q.is_empty()).unwrap_or("Waiting for you.".into());
        let (x, y) = draw_ask(buf, full, app.height as usize, &title, &question, text);
        f.set_cursor_position(Position::new(full.x + x as u16, full.y + y as u16));
        return;
    }

    let area = full;
    let (w, h) = (area.width as usize, area.height as usize);

    let layout = app.cfg.layout;
    // a short board gives the list the rows a tall one spends on space: no row of its
    // own for the scroll mark, no blank line above the keys, and a message on the key row
    let short = h < SHORT;
    // a narrow one puts one space after the IDs, and (grouped) a task's age on its detail line
    let narrow = w < NARROW;
    let footer = match (layout, short) {
        (Layout::Grouped, false) => 3,
        (Layout::Grouped, true) => 1,
        (Layout::Compact, false) => 7,
        (Layout::Compact, true) => 6,
    };
    // the rows above the list: the top line and a blank one, and on a tall board one
    // under it for the scroll mark
    let chrome = if short { 2 } else { 3 };
    // from here on h is the board's height above the gap: everything at the
    // bottom (the detail panel, message line, keys, scroll mark) sits on top of it
    let h = h - bottom_gap(app, h, 4 + footer);
    app.height = h as u16;
    // the message (or the y/n question) wraps; its rows past the first come off the
    // list (while one row of it is left). On a short board it's one row, on the key
    // row unless something else is asked there
    let msg = message(app, w, if short { 1 } else { h.saturating_sub(chrome + 1 + footer) + 1 });
    let msg_on_keys = short && !msg.is_empty() && app.pending.is_none() && matches!(app.mode, Mode::Normal);
    let footer = match (short, msg_on_keys) {
        (true, true) => footer,
        (true, false) => footer + msg.len(),
        (false, _) => footer + msg.len().saturating_sub(1),
    };
    // by field, not app.groups(): app.top changes while these are in use
    let groups = model::shown_groups(&app.tasks, &app.cfg, app.me.as_deref(), app.closed);
    let cur_id = app.current().map(|t| t.id.clone());
    let is_cur = |t: &Task| Some(&t.id) == cur_id.as_ref();
    // IDs share a column as wide as the longest, so titles and details line up
    let idw = groups.iter().flat_map(|(_, ts)| ts).map(|t| width(&t.id)).max().unwrap_or(0);
    let age_on_detail = narrow && layout == Layout::Grouped;
    // from under the ID to the right edge
    let detail_w = w.saturating_sub(5);

    // header: the repository, and what you act on
    let name = fit(&app.repo_name, w.saturating_sub(2).min(24));
    // a small marker when approved tasks accept themselves
    let auto = if app.cfg.auto_accept { " · auto-accept" } else { "" };
    // with several coordinators: whose board this is, and how many of the other
    // ones' tasks it hides (`o` shows them, dimmed)
    let hidden = model::hidden(&app.tasks, &app.cfg, app.me.as_deref(), app.closed);
    let left = |key: bool| {
        let hint = model::hidden_hint(&hidden, key);
        let hint = if hint.is_empty() { hint } else { format!(" · {hint}") };
        match app.me.as_deref() {
            Some(me) if app.cfg.coordinators.len() > 1 => format!(" · {me}{auto}{hint}"),
            _ => auto.to_string(),
        }
    };
    // then the other repositories with something for you, and what needs you or
    // is ready here, at the right edge; the other repositories get the room first,
    // and on a crowded line the hint drops its "o shows"
    let shown: Vec<&Task> = groups.iter().flat_map(|(_, ts)| ts.iter().copied()).collect();
    let (needs, ready) = model::acts(&shown);
    let top = |auto: &str| model::top_right(needs, ready, &app.others.counts, w.saturating_sub(width(&name) + width(auto) + 3));
    let (mut auto, mut right) = (left(true), top(&left(true)));
    if !right.whole && !hidden.is_empty() {
        auto = left(false);
        right = top(&auto);
    }
    let others_x = 1 + width(&name) + width(&auto) + 3;
    if !right.others.is_empty() {
        let style = if app.others.counts.iter().any(|c| c.needs > 0) { Style::new().fg(AMBER) } else { dim() };
        put(buf, area, 0, others_x - 3, span(" · ", dim()));
        put(buf, area, 0, others_x, span(right.others, style));
    }
    let count = counts(right.counts, right.sep);
    put(buf, area, 0, w.saturating_sub(2 + count.width()), count);
    put(buf, area, 0, 1 + width(&name), span(auto, dim()));
    put(buf, area, 0, 1, span(name, bold()));

    let now = model::now();
    let mut rows: Vec<Row> = Vec::new();
    for (gi, (heading, ts)) in groups.iter().enumerate() {
        // the compact layout's group names, dim, sit where the grouped one leaves a blank row
        if gi > 0 && layout == Layout::Grouped {
            rows.push(Row::Gap);
        }
        rows.push(Row::Head(format!("{heading}  {}", ts.len())));
        for t in ts {
            rows.push(Row::Title(t));
            if layout == Layout::Grouped {
                // a working or in-review task: what its agent is doing now, on one line
                let doing = model::doing_now(t, now);
                // what a task needs from you, GitHub said, or the usage limit gets two lines,
                // as does one its coordinator is on (when it comes to you); anything else
                // one; the selected task all it has to say, up to SELECTED_LINES
                let lines = if is_cur(t) {
                    SELECTED_LINES
                } else if !doing.is_empty() {
                    1
                } else if t.flagged() || t.is_limited() || t.triage() || t.status == "finished" || !model::pr_phrase(t).is_empty() {
                    2
                } else {
                    1
                };
                let text = if doing.is_empty() { model::detail_with_auto(t, &app.cfg, now) } else { doing };
                // on a narrow board the age starts the first line, off the title's
                let age = if age_on_detail { model::ago(t.stamp(), now) } else { String::new() };
                let text = match (age.is_empty(), text.is_empty()) {
                    (true, _) => text,
                    (false, true) => age.clone(),
                    (false, false) => format!("{age} · {text}"),
                };
                for (i, d) in model::wrap(&text, detail_w, lines).into_iter().enumerate() {
                    if !d.is_empty() {
                        let lead = if i == 0 { width(&age) } else { 0 };
                        rows.push(Row::Detail(t, d, loud_detail(app, t), lead));
                    }
                }
                let stat = model::stat_line(t);
                if !stat.is_empty() {
                    rows.push(Row::Detail(t, stat, false, 0));
                }
            }
        }
    }

    // keep the selected task in view with its details, starting at a heading or a task
    let avail = h.saturating_sub(chrome + footer).max(1);
    let sel_row = rows.iter().position(|r| matches!(r, Row::Title(t) if is_cur(t)));
    let mut top = app.top;
    if let Some(i) = sel_row {
        let details = rows[i + 1..].iter().take_while(|r| matches!(r, Row::Detail(..))).count();
        // the row after its last detail too, so the next row shows there's more
        let end = i + details + 1;
        if i < top {
            top = i - usize::from(i > 0 && matches!(rows[i - 1], Row::Head(_)));
        } else if end >= top + avail {
            top = (end + 1).saturating_sub(avail).min(i); // with room for one row, the task's own
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
    // a page, for PgDn and PgUp: the tasks this list has room for
    app.page = rows.iter().skip(top).take(avail).filter(|r| matches!(r, Row::Title(_))).count().max(1);

    if groups.is_empty() {
        let lines = model::empty_board(&app.tasks, &app.cfg, app.me.as_deref(), app.closed);
        for (j, line) in lines.iter().enumerate() {
            put(buf, area, 2 + j, 1, span(fit(line, w.saturating_sub(2)), if j == 0 { bold() } else { dim() }));
        }
    }

    for (j, row) in rows.iter().skip(top).take(avail).enumerate() {
        let y = 2 + j;
        if let Row::Title(t) | Row::Detail(t, ..) = row {
            if t.flagged() {
                let c = if t.status == "stopped" { Color::Red } else { AMBER };
                put(buf, area, y, 0, span("▌", Style::new().fg(c)));
            }
        }
        match row {
            Row::Head(text) if layout == Layout::Compact => put(buf, area, y, 1, span(fit(text, w.saturating_sub(4)), dim())),
            Row::Head(text) => put(buf, area, y, 1, span(text.clone(), bold())),
            Row::Title(t) => {
                // a task with a command running gets a spinner in place of its mark
                let mark = match app.job(&t.id) {
                    Some(_) => span(spinner(app), bold()),
                    // its tour being written: the same spinner, until it opens
                    None if t.tour_writing() => span(spinner(app), mark_style(&t.status)),
                    // an agent that has gone quiet: a still mark, in the colour of one that needs you
                    None if t.is_stuck() => span(model::mark(&t.status), mark_style("needs")),
                    // one stopped at the usage limit isn't working either: a still mark
                    None if t.is_limited() => span(model::mark(&t.status), mark_style(&t.status)),
                    None if t.status == "working" => span(working_mark(app), Style::new()),
                    None if t.status == "review" => span(review_mark(app), mark_style("review")),
                    None => span(model::mark(&t.status), mark_style(&t.status)),
                };
                put(buf, area, y, 1, mark);
                put(buf, area, y, 3, title_line(app, t, is_cur(t), w, idw, narrow, !age_on_detail, now));
            }
            Row::Detail(t, text, loud, lead) => {
                // the selected task's lines sit on the tint, its whole block, and drop dim as its title does
                let style = if is_cur(t) {
                    Style::new().bg(TINT)
                } else if *loud {
                    Style::new()
                } else {
                    dim()
                };
                let text = fit(text, detail_w);
                let text = if is_cur(t) { pad(&text, detail_w) } else { text };
                let (age, rest) = model::split_at_width(&text, *lead);
                put(buf, area, y, 3, Line::from(vec![span(age, age_style(t, is_cur(t), now)), span(rest, style)]));
            }
            Row::Gap => {}
        }
    }
    if top + avail < rows.len() {
        // a short board's goes at the end of the list's last row, past the ages
        let (y, x) = if short { (1 + avail, w.saturating_sub(2)) } else { (2 + avail, w.saturating_sub(3)) };
        put(buf, area, y, x, span("↓", dim()));
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
        let stat = model::stat_line(cur);
        // what its agent is doing now first, then the rest of what there is to say
        let doing = model::doing_now(cur, now);
        let room = 3 - usize::from(!stat.is_empty()) - usize::from(!doing.is_empty());
        let mut lines = model::wrap(&model::detail_with_auto(cur, &app.cfg, now), w.saturating_sub(2), room);
        if !doing.is_empty() {
            lines.insert(0, doing);
        }
        if !stat.is_empty() {
            lines.push(stat);
        }
        for (j, line) in lines.into_iter().enumerate() {
            put(buf, area, yb + 2 + j, 1, span(fit(&line, w.saturating_sub(2)), dim()));
        }
    }
    let last = h.saturating_sub(1);
    if msg_on_keys {
        put(buf, area, last, 1, msg.into_iter().next().unwrap_or_default());
        return;
    }
    let first = h.saturating_sub(1 + msg.len());
    for (j, line) in msg.into_iter().enumerate() {
        put(buf, area, first + j, 1, line);
    }
    if let Mode::Tell { id, text, .. } = &app.mode {
        let x = input_line(buf, area, last, &format!("Tell {id}:"), text);
        f.set_cursor_position(Position::new(area.x + x as u16, area.y + last as u16));
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
}

/// A board with fewer rows than this is short: the list gets the rows of space.
const SHORT: usize = 12;
/// A board with fewer columns than this is narrow: the titles get the room.
const NARROW: usize = 36;

/// How many rows a message wraps to, at most.
const MESSAGE_LINES: usize = 3;

/// The message rows above the keys, at most `most` of them: the y/n question being
/// asked (red when risky), what just happened (a failure red, what worked green),
/// else what's still running.
fn message(app: &App, w: usize, most: usize) -> Vec<Span<'static>> {
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

/// How many lines of detail the selected task gets.
const SELECTED_LINES: usize = 8;

/// A label, then what's typed, at row y: the end of it when it's longer than the
/// line. Returns the cursor's column.
fn input_line(buf: &mut Buffer, area: Rect, y: usize, label: &str, text: &str) -> usize {
    let w = area.width as usize;
    let x = width(label) + 2;
    let room = w.saturating_sub(x + 2);
    let mut s: Vec<char> = text.chars().collect();
    while width(&s.iter().collect::<String>()) > room && !s.is_empty() {
        s.remove(0);
    }
    let shown: String = s.into_iter().collect();
    put(buf, area, y, 1, span(label.to_string(), bold()));
    put(buf, area, y, x, shown.clone());
    x + width(&shown)
}

/// `i` on a coordinator's question: the task, the whole question wrapped, then the
/// line the answer is typed on. Returns the cursor's column and row.
fn draw_ask(buf: &mut Buffer, area: Rect, h: usize, title: &str, question: &str, text: &str) -> (usize, usize) {
    let w = area.width as usize;
    put(buf, area, 0, 1, span(fit(title, w.saturating_sub(2)), bold()));
    // the title, a gap, the question, a gap, the answer and the foot
    let room = h.saturating_sub(5).max(1);
    for (j, line) in model::wrap(question, w.saturating_sub(2), room).into_iter().enumerate() {
        put(buf, area, 2 + j, 1, span(line, Style::new()));
    }
    let y = h.saturating_sub(2);
    let x = input_line(buf, area, y, "Answer:", text);
    put(buf, area, h.saturating_sub(1), 1, span(fit("⏎ sends · esc returns", w.saturating_sub(2)), dim()));
    (x, y)
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
    for (heading, keys) in crate::BOARD_KEYS {
        section(heading, keys.iter().map(|(k, d)| (k.to_string(), *d, bold())).collect());
    }
    // each mark as the board draws it, in its colour
    let m = |st: &str, what: &'static str| (model::mark(st).to_string(), what, mark_style(st));
    section("Marks", vec![
        m("needs", "needs you"),
        m("stopped", "stopped: its agent exited"),
        m("triage", "the coordinator is on it"),
        m("ready", "ready to accept"),
        m("finished", "finished, nothing to merge"),
        (format!("{} {}", REVIEW[0], model::mark("review")), "in review (it turns)", mark_style("review")),
        (format!("{} {}", WORKING[0], model::mark("working")), "working (it turns)", Style::new()),
        m("todo", "queued, not started"),
        m("done", "accepted"),
        m("dropped", "dropped"),
        (SPINNER[0].to_string(), "a command or its tour running", bold()),
        ("▌".to_string(), "needs you; red: stopped", Style::new().fg(AMBER)),
    ]);
    out
}

/// The `?` list from line `top`, with a foot saying how to scroll when it doesn't
/// all fit. Returns `top`, kept in range.
fn draw_help(buf: &mut Buffer, area: Rect, h: usize, top: usize) -> usize {
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

/// The prompt a task's agent is at: the end of its screen, then its options, one a
/// line, each with the key that presses it.
fn draw_answer(buf: &mut Buffer, area: Rect, h: usize, title: &str, prompt: Option<&model::Prompt>) {
    let w = area.width as usize;
    put(buf, area, 0, 1, span(fit(title, w.saturating_sub(2)), bold()));
    let (screen, options): (&[String], Vec<Line<'static>>) = match prompt {
        None => (&[], vec![Line::from(span("Reading its screen…", dim()))]),
        Some(p) if p.question.is_none() || p.options.is_empty() => {
            (&p.screen, vec![Line::from(span("No prompt to answer on its screen now.", bold()))])
        }
        Some(p) => {
            let lines = p.options.iter().map(|(key, label)| {
                Line::from(vec![span(format!("{key} "), bold()), span(fit(label, w.saturating_sub(4 + width(key))), Style::new())])
            });
            (&p.screen, lines.collect())
        }
    };
    // the title, a gap, the screen, a gap, the options and the foot
    let room = h.saturating_sub(4 + options.len());
    let shown = &screen[screen.len().saturating_sub(room)..];
    for (j, line) in shown.iter().enumerate() {
        put(buf, area, 2 + j, 1, screen_line(line));
    }
    let y = h.saturating_sub(1 + options.len());
    for (j, line) in options.into_iter().enumerate() {
        put(buf, area, y + j, 1, line);
    }
    let foot = "live · ⏎ opens its pane · esc returns";
    put(buf, area, h.saturating_sub(1), 1, span(fit(foot, w.saturating_sub(2)), dim()));
}

/// Is a task's detail at normal weight? What you read and act on is (needs you, stopped,
/// ready, finished); background is dim (working, queued, in review, the coordinator's,
/// closed), and so is all of another coordinator's task, as its title is.
fn loud_detail(app: &App, t: &Task) -> bool {
    matches!(t.status.as_str(), "needs" | "stopped" | "ready" | "finished") && !app.cfg.theirs(t, app.me.as_deref())
}

/// A task's age's style: red once it has needed you STALE seconds, on the tint when selected.
fn age_style(t: &Task, selected: bool, now: chrono::DateTime<chrono::FixedOffset>) -> Style {
    let stale = t.needs_you() && model::seconds_in_status(t, now) >= STALE;
    match (selected, stale) {
        (true, true) => Style::new().bg(TINT).fg(Color::Red).add_modifier(Modifier::BOLD),
        (true, false) => Style::new().bg(TINT),
        (false, true) => bold().fg(Color::Red),
        (false, false) => dim(),
    }
}

/// A task's line: its ID, title and age (on a narrow board one space after the ID,
/// and without the age when its detail line has it). Asked y/n about, it keeps its
/// title, in the question's colour, with "y / n" for the age; the question is above the keys.
#[allow(clippy::too_many_arguments)]
fn title_line(
    app: &App,
    t: &Task,
    selected: bool,
    w: usize,
    idw: usize,
    narrow: bool,
    with_age: bool,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Line<'static> {
    let id = pad(&t.id, idw);
    let gap = if narrow { " " } else { "  " };
    let width_all = w.saturating_sub(5); // from column 3 to the header's right edge
    let asked = app.pending.as_ref().filter(|(_, tid)| *tid == t.id);

    let age = match asked {
        Some(_) => "y / n".to_string(),
        None if with_age => model::ago(t.stamp(), now),
        None => String::new(),
    };
    let room = width_all.saturating_sub(idw + width(gap) + if age.is_empty() { 0 } else { width(&age) + 2 });
    let title = fit(&model::shown_title(t, &app.cfg, app.me.as_deref()), room);
    let middle = pad(&format!("{gap}{title}"), width_all.saturating_sub(idw + width(&age)));
    // the ID in reverse video, off the tint: that shows in any theme. Dim text is hard
    // to read on the tint, so the rest of the row drops it
    let (id_style, title_style, age_style) = if let Some((action, _)) = asked {
        let tone = if model::confirm_risky(*action, t, &app.cfg) { Color::Red } else { Color::Green };
        (rev().add_modifier(Modifier::BOLD).fg(tone), bold().fg(tone), bold().fg(tone))
    } else if selected {
        (rev().add_modifier(Modifier::BOLD), Style::new().bg(TINT), age_style(t, true, now))
    } else {
        let title = if t.closed() || t.triage() || app.cfg.theirs(t, app.me.as_deref()) { dim() } else { Style::new() };
        (dim(), title, age_style(t, false, now))
    };
    // the ID's padding goes with the title, so only the ID itself is reversed
    let id_pad = id[t.id.len()..].to_string();
    Line::from(vec![span(t.id.clone(), id_style), span(id_pad + &middle, title_style), span(age, age_style)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Config;
    use crate::Note;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::time::Instant;

    fn app(tasks: Vec<Task>, gap: usize, layout: Layout) -> App {
        let (tx, _) = std::sync::mpsc::channel();
        App {
            dir: Default::default(),
            root: Default::default(),
            bullpen: Default::default(),
            me: None,
            repo_name: "repo".into(),
            cfg: Config { bottom_gap: gap, layout, ..Config::default_for_test() },
            tasks,
            others: Default::default(),
            sel: None,
            closed: false,
            msg: Note::default(),
            top: 0,
            pending: None,
            running: Vec::new(),
            mode: Mode::Normal,
            height: 0,
            page: 1,
            started: Instant::now(),
            tx,
            peeking: false,
            peek_failed: false,
            peek_due: Instant::now(),
            exe: None,
        }
    }

    fn ready(id: &str) -> Task {
        Task { id: id.into(), title: format!("Task {id}"), status: "ready".into(), ..Default::default() }
    }

    /// The board's rows as text, trailing spaces cut.
    fn screen(app: &mut App, w: u16, h: u16) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect()
    }

    fn row(s: &[String], from_bottom: usize) -> &str {
        &s[s.len() - 1 - from_bottom]
    }

    /// The colour of the text's first cell.
    fn tone_of(app: &mut App, w: u16, h: u16, text: &str) -> Option<Color> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        let s: Vec<String> = (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect()).collect();
        let y = s.iter().position(|r| r.contains(text))?;
        let x = s[y][..s[y].find(text)?].chars().count();
        Some(buf[(x as u16, y as u16)].fg)
    }

    #[test]
    fn page_down_and_up_move_a_screenful_at_42_columns() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let tasks: Vec<Task> = (1..=30).map(|n| ready(&format!("BP-{n}"))).collect();
        let mut a = app(tasks, 0, Layout::Compact);
        let s = screen(&mut a, 42, 24);
        let shown = a.page;
        assert!(shown > 3 && shown < 30, "a page: {shown}");
        assert!(s.iter().any(|r| r.contains(&format!("BP-{shown} "))), "{s:#?}");
        let key = |a: &mut App, code: KeyCode, m: KeyModifiers| {
            a.key(KeyEvent::new(code, m));
            screen(a, 42, 24)
        };
        // a page down selects the task a page on, and the list scrolls to it
        let s = key(&mut a, KeyCode::PageDown, KeyModifiers::NONE);
        let sel = format!("BP-{}", 1 + shown);
        assert_eq!(a.sel.as_deref(), Some(sel.as_str()));
        assert!(!s.iter().any(|r| r.contains("BP-1 ")), "scrolled: {s:#?}");
        assert!(s.iter().any(|r| r.contains(&format!("{sel} "))));
        // half a page (of what shows now: the heading's row has scrolled off) with
        // ctrl+u, and ctrl+d doesn't drop
        let half = a.page / 2;
        key(&mut a, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(a.sel, Some(format!("BP-{}", 1 + shown - half)));
        key(&mut a, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(a.sel.as_deref(), Some(sel.as_str()));
        assert!(a.pending.is_none());
        // G and End go to the last, Home to the first, and the page keys stop at the ends
        let s = key(&mut a, KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(a.sel.as_deref(), Some("BP-30"));
        assert!(s.iter().any(|r| r.contains("BP-30 ")), "{s:#?}");
        key(&mut a, KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(a.sel.as_deref(), Some("BP-30"));
        let s = key(&mut a, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(a.sel.as_deref(), Some("BP-1"));
        assert!(s.iter().any(|r| r.contains("BP-1 ")));
        key(&mut a, KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(a.sel.as_deref(), Some("BP-1"));
        key(&mut a, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(a.sel.as_deref(), Some("BP-30"));
    }

    #[test]
    fn a_page_in_the_grouped_layout_counts_the_tasks_with_their_details() {
        let tasks: Vec<Task> = (1..=30).map(|n| ready(&format!("BP-{n}"))).collect();
        let mut a = app(tasks, 0, Layout::Grouped);
        let s = screen(&mut a, 42, 24);
        let titles = s.iter().filter(|r| r.contains("Task BP-")).count();
        assert_eq!(a.page, titles, "{s:#?}");
    }

    #[test]
    fn a_risky_confirm_names_the_risk_in_red() {
        use crate::model::{Gh, Review};
        let review = |v: &str| Some(Review { round: Some(1), verdict: Some(v.into()), kind: None, block: None });
        let gh = |c: &str| Some(Gh { checks: Some(c.into()), ..Default::default() });
        let pr = |n: u32| Some(format!("https://x/pull/{n}"));
        let safe = Task { pr: pr(41), review: review("approved"), gh: gh("passing"), ..ready("BP-7") };
        let risky = Task { pr: pr(42), review: review("changes"), gh: gh("failing"), ..ready("BP-8") };
        let working = Task { pr: pr(43), status: "working".into(), block: Some("w9".into()), ..ready("BP-9") };
        let mut a = app(vec![safe, risky, working], 1, Layout::Grouped);

        a.pending = Some((Action::Accept, "BP-8".into()));
        let s = screen(&mut a, 42, 24);
        // the task keeps its title, y / n for its age, and the whole question is above the keys
        assert!(s.iter().any(|r| r == " ✔ BP-8  Task BP-8                 y / n"), "{s:#?}");
        assert_eq!(
            s[20..23],
            [
                " BP-8: Checks failing, review asks for",
                " changes. Accept and rebase-merge anyway?",
                " y accept anyway   any other key cancels",
            ]
            .map(String::from),
            "{s:#?}"
        );
        assert_eq!(tone_of(&mut a, 42, 24, "BP-8: Checks failing"), Some(Color::Red));
        assert_eq!(tone_of(&mut a, 42, 24, "Task BP-8"), Some(Color::Red));
        assert_eq!(tone_of(&mut a, 42, 24, "y accept anyway"), Some(Color::Red));
        // with fewer rows, the shorter words that fit
        let s = screen(&mut a, 42, 11);
        assert_eq!(row(&s, 2), " BP-8: Checks failing. Accept anyway?", "{s:#?}");
        assert!(s.iter().any(|r| r.contains("BP-8  Task BP-8")), "{s:#?}");

        // a safe accept stays green, its key row plain
        a.pending = Some((Action::Accept, "BP-7".into()));
        let s = screen(&mut a, 42, 24);
        assert_eq!(row(&s, 2), " BP-7: Accept and rebase-merge?", "{s:#?}");
        assert_eq!(row(&s, 1), " y accept   any other key cancels");
        assert_eq!(tone_of(&mut a, 42, 24, "BP-7: Accept"), Some(Color::Green));
        assert_eq!(tone_of(&mut a, 42, 24, "y accept"), Some(Color::Reset));

        // a drop says whose agent it stops, and the key row is red too
        a.pending = Some((Action::Drop, "BP-9".into()));
        let s = screen(&mut a, 42, 24);
        assert_eq!(s[20..22], [" BP-9: Its worker is still running. Drop", " it and close PR #43?"].map(String::from), "{s:#?}");
        assert_eq!(row(&s, 1), " y drop and close PR #43   n cancels");
        assert_eq!(tone_of(&mut a, 42, 24, "y drop"), Some(Color::Red));
        assert!(s.iter().any(|r| r.contains("BP-9  Task BP-9") && r.ends_with("y / n")), "{s:#?}");
    }

    #[test]
    fn the_header_says_what_is_hidden_and_o_labels_them() {
        let c = |id: &str, who: &str| Task { coordinator: Some(who.into()), ..ready(id) };
        let mut a = app(vec![c("BP-1", "c1"), c("BP-2", "c2"), c("BP-3", "c2")], 1, Layout::Grouped);
        a.cfg.coordinators = vec!["c1".into(), "c2".into()];
        a.me = Some("c1".into());
        let s = screen(&mut a, 60, 14);
        assert!(s[0].contains("repo · c1 · +2 in c2 · o shows"), "{s:#?}");
        assert!(s[0].ends_with("1 ready"), "{s:#?}"); // counts what's shown
        assert!(!s.iter().any(|r| r.contains("BP-2")), "{s:#?}");
        a.cfg.board_others = vec!["c1".into()];
        let s = screen(&mut a, 60, 14);
        assert!(!s[0].contains("o shows") && s[0].ends_with("3 ready"), "{s:#?}");
        assert!(s.iter().any(|r| r.contains("BP-2  c2 · Task BP-2")), "{s:#?}");
        assert!(s.iter().any(|r| r.contains("BP-1  Task BP-1")), "{s:#?}");
    }

    #[test]
    fn the_header_counts_the_other_repos_in_the_room_left() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        a.others.counts = vec![model::RepoCount { name: "shop".into(), needs: 1, ready: 2 }];
        let s = screen(&mut a, 60, 10);
        assert_eq!(s[0], " repo · shop: 1 needs you, 2 ready                 1 ready", "{s:#?}");
        // at the board's usual 42 columns this repo's count shrinks to its mark first
        let s = screen(&mut a, 42, 10);
        assert_eq!(s[0], " repo · shop: 1 needs you, 2 ready    ✔1", "{s:#?}");
        let s = screen(&mut a, 30, 10);
        assert_eq!(s[0], " repo · shop ●1 ✔2   1 ready", "{s:#?}");
        let s = screen(&mut a, 24, 10); // cut, never under 8 columns
        assert_eq!(s[0], " repo · shop ●1 ✔2  ✔1", "{s:#?}");
        let s = screen(&mut a, 23, 10);
        assert_eq!(s[0], " repo · shop ●1…   ✔1", "{s:#?}");
        let s = screen(&mut a, 20, 10); // the other repo's marks stay whole before this one's count
        assert_eq!(s[0], " repo · shop ●1 ✔2", "{s:#?}");
        let s = screen(&mut a, 17, 10); // no room for them: this repo's count comes back
        assert_eq!(s[0], " repo   1 ready", "{s:#?}");
    }

    #[test]
    fn a_crowded_top_line_drops_o_shows_before_the_counts() {
        // c1's board at 42 columns, with c3's tasks hidden and another repo ready
        let c = |id: &str, who: &str, status: &str| Task { coordinator: Some(who.into()), status: status.into(), ..ready(id) };
        let tasks = vec![c("BP-1", "c1", "needs"), c("BP-2", "c1", "ready"), c("BP-3", "c3", "working"), c("BP-4", "c3", "ready")];
        let mut a = app(tasks, 1, Layout::Grouped);
        a.repo_name = "bullpen".into();
        a.cfg.coordinators = vec!["c1".into(), "c3".into()];
        a.me = Some("c1".into());
        let s = screen(&mut a, 60, 10); // room for it all
        assert_eq!(s[0], " bullpen · c1 · +2 in c3 · o shows   1 needs you · 1 ready", "{s:#?}");
        let s = screen(&mut a, 42, 10);
        assert_eq!(s[0], " bullpen · c1 · +2 in c3           ●1 ✔1", "{s:#?}");
        a.others.counts = vec![model::RepoCount { name: "jj-evolved".into(), needs: 0, ready: 1 }];
        let s = screen(&mut a, 42, 10);
        assert_eq!(s[0], " bullpen · c1 · +2 in c3 · jj-evolved ✔1", "{s:#?}");
        let s = screen(&mut a, 50, 10);
        assert_eq!(s[0], " bullpen · c1 · +2 in c3 · jj-evolved ✔1   ●1 ✔1", "{s:#?}");
    }

    fn busy(id: &str, status: &str, text: &str) -> Task {
        let at = model::now().timestamp() - 5;
        let doing = Some(model::Doing { state: "working".into(), text: text.into(), at });
        let review = Some(model::Review { round: Some(1), ..Default::default() });
        Task { since: Some(model::now().to_rfc3339()), doing, review, status: status.into(), ..ready(id) }
    }

    #[test]
    fn a_working_task_shows_what_its_agent_is_doing() {
        let tasks = vec![
            busy("BP-1", "working", "Running the board's tests with cargo test in board/"),
            busy("BP-2", "review", "Reading the diff"),
            Task { doing: None, ..busy("BP-3", "working", "") },
        ];
        let mut a = app(tasks.clone(), 0, Layout::Grouped);
        let s = screen(&mut a, 44, 16);
        assert_eq!(s[2..12], [
            " In review  1",
            " ◜ BP-2  Task BP-2                     now",
            "   Reading the diff",
            "",
            " Working  2",
            " ⠋ BP-1  Task BP-1                     now",
            "   Running the board's tests with cargo …",
            " ⠋ BP-3  Task BP-3                     now",
            "",
            "",
        ].map(String::from), "{s:#?}");

        // compact: in the detail at the bottom, above the rest
        let mut a = app(tasks, 0, Layout::Compact);
        a.sel = Some("BP-1".into());
        let s = screen(&mut a, 44, 16);
        assert_eq!(s[10..13], [
            " BP-1  Task BP-1",
            " Running the board's tests with cargo test…",
            "",
        ].map(String::from), "{s:#?}");
        a.sel = Some("BP-2".into());
        let s = screen(&mut a, 44, 16);
        assert_eq!(s[11..13], [" Reading the diff", " Reviewer on round 1 of 2."].map(String::from), "{s:#?}");
        a.sel = Some("BP-3".into());
        assert!(!screen(&mut a, 44, 16).iter().any(|r| r.contains("Running")));
    }

    #[test]
    fn a_message_wraps_to_three_rows_in_its_colour_above_the_keys() {
        let ts: Vec<Task> = (1..=4).map(|i| ready(&format!("BP-{i}"))).collect();
        let mut a = app(ts, 1, Layout::Grouped);
        let quiet = screen(&mut a, 42, 16);
        a.msg = Note::failed(
            "BP-1 can't be accepted: its branch has conflicts with main in board/src/ui.rs. Rebase it, then accept it again.",
        );
        let mut term = Terminal::new(TestBackend::new(42, 16)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let s: Vec<String> =
            (0..16).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        // three rows, the last cut, then the keys and the capsules' row
        assert_eq!(
            s[11..14],
            [
                " BP-1 can't be accepted: its branch has",
                " conflicts with main in board/src/ui.rs.",
                " Rebase it, then accept it again.",
            ]
            .map(String::from),
            "{s:#?}"
        );
        assert!(row(&s, 1).starts_with(" a accept"), "{s:#?}");
        assert_eq!(row(&s, 0), "");
        assert_eq!(buf[(1, 11)].fg, Color::Red);
        assert_eq!(buf[(1, 13)].fg, Color::Red);
        // the list gives up two rows for it, and says there's more below
        assert_eq!(s[10], "", "{s:#?}");
        assert!(s[9].ends_with('↓'), "{s:#?}");
        assert_eq!(s[2..9], quiet[2..9], "{s:#?}");

        // a longer one is cut at three rows
        a.msg = Note::failed("word ".repeat(40));
        let s = screen(&mut a, 42, 16);
        assert!(s[13].ends_with('…') && s[10].is_empty(), "{s:#?}");

        // what worked is green, and one row
        a.msg = Note::done("Accepted BP-1.");
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        assert_eq!(buf[(1, 13)].symbol(), "A");
        assert_eq!(buf[(1, 13)].fg, Color::Green);

        // in the compact layout the detail panel moves up over the list instead
        a.cfg.layout = Layout::Compact;
        a.msg = Note::failed("word ".repeat(12));
        let s = screen(&mut a, 42, 16);
        assert!(s[13].starts_with(" word") && s[12].starts_with(" word") && s[14].starts_with(" a accept"), "{s:#?}");
        assert!(s[7].starts_with(" ───"), "{s:#?}");
        assert!(s[8].starts_with(" BP-1  Task BP-1"), "{s:#?}");
    }

    #[test]
    fn the_last_row_is_left_for_rexs_capsules() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        let s = screen(&mut a, 46, 12);
        assert_eq!(row(&s, 0), "");
        assert!(row(&s, 1).starts_with(" a accept  g tour  d drop"), "{s:#?}");
        assert!(row(&s, 1).ends_with("? keys"), "{s:#?}");

        a.msg = Note::done("Accepted BP-1");
        let s = screen(&mut a, 40, 12);
        assert_eq!(row(&s, 2), " Accepted BP-1");
        assert_eq!(row(&s, 0), "");

        a.pending = Some((Action::Drop, "BP-1".into()));
        let s = screen(&mut a, 40, 12);
        assert_eq!(row(&s, 1), " y drop   any other key cancels");
        assert_eq!(row(&s, 0), "");

        a.pending = None;
        a.mode = Mode::Tell { id: "BP-1".into(), text: "hi".into(), reply: false };
        let s = screen(&mut a, 40, 12);
        assert_eq!(row(&s, 1), " Tell BP-1: hi");
        assert_eq!(row(&s, 0), "");

        a.mode = Mode::Overlay { title: "Keys".into(), lines: vec!["one".into()], peek: None };
        let s = screen(&mut a, 40, 12);
        assert_eq!(row(&s, 1), " any key returns");
        assert_eq!(row(&s, 0), "");
    }

    #[test]
    fn a_peek_says_which_keys_act_from_it_and_q_asks_on_the_key_row() {
        let mut a = app(vec![ready("BP-1")], 0, Layout::Grouped);
        a.mode = Mode::Overlay { title: "BP-1  Fix it".into(), lines: vec!["$ cargo test".into()], peek: Some("BP-1".into()) };
        let s = screen(&mut a, 42, 12);
        assert_eq!(row(&s, 0), " live · t tell  ⏎ open  esc returns");
        let s = screen(&mut a, 80, 12);
        assert_eq!(row(&s, 0), " live · t tell  ⏎ open  i answer  a accept  v diff  esc returns");
        let s = screen(&mut a, 24, 12);
        assert_eq!(row(&s, 0), " live · esc returns");

        a.mode = Mode::Quit;
        let s = screen(&mut a, 42, 12);
        assert_eq!(row(&s, 0), " y quits the board   any other key stays");
        let s = screen(&mut a, 30, 12);
        assert_eq!(row(&s, 0), " y quits   n stays");
    }

    #[test]
    fn a_prompt_shows_its_screen_then_its_options() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        let p = model::Prompt {
            who: "worker".into(),
            question: Some("Do you want to proceed?".into()),
            options: vec![("1".into(), "Yes".into()), ("2".into(), "Yes, and always allow access to /tmp/x".into()), ("3".into(), "No".into())],
            id: Some("ab12".into()),
            screen: (1..=9).map(|n| format!("line {n}")).chain(["Do you want to proceed?".to_string()]).collect(),
        };
        a.mode = Mode::Answer { id: "BP-1".into(), title: "BP-1  Fix it".into(), prompt: Some(p.clone()) };
        let s = screen(&mut a, 40, 12);
        assert_eq!(s, [
            " BP-1  Fix it",
            "",
            " line 7",
            " line 8",
            " line 9",
            " Do you want to proceed?",
            "",
            " 1 Yes",
            " 2 Yes, and always allow access to /t…",
            " 3 No",
            " live · ⏎ opens its pane · esc returns",
            "",
        ].map(String::from), "{s:#?}");
        a.mode = Mode::Answer { id: "BP-1".into(), title: "BP-1  Fix it".into(), prompt: Some(model::Prompt { question: None, options: vec![], ..p }) };
        let s = screen(&mut a, 40, 12);
        assert_eq!(s[9..11], [" No prompt to answer on its screen now.", " live · ⏎ opens its pane · esc returns"].map(String::from), "{s:#?}");
    }

    #[test]
    fn a_ready_task_says_whether_its_toured_and_a_tour_being_written_spins() {
        let run = |state: &str, commit: &str, opened: Option<&str>| {
            Some(model::TourRun { state: state.into(), commit: Some(commit.into()), opened: opened.map(String::from), ..Default::default() })
        };
        let tasks = vec![
            Task { diffstat: Some("3 files, +12 −1".into()), ..ready("BP-1") },
            Task { tour: run("written", "abc", Some("abc")), ..ready("BP-2") },
            Task { tour: run("written", "def", Some("abc")), ..ready("BP-3") },
            Task { tour: run("writing", "", None), ..ready("BP-4") },
            Task { tour: run("failed", "", None), note: Some("tour failed: its tour doesn't parse".into()), ..ready("BP-5") },
        ];
        // a tour of code the worker has changed since is no tour of this code
        let old = Task { tour: Some(model::TourRun { stale: Some(true), ..run("written", "abc", None).unwrap() }), ..ready("BP-6") };
        assert_eq!(model::stat_line(&old), "Not toured yet");
        // even once its old tour is opened anyway
        let opened = Task { tour: Some(model::TourRun { stale: Some(true), ..run("written", "abc", Some("abc")).unwrap() }), ..ready("BP-6") };
        assert_eq!(model::stat_line(&opened), "Not toured yet");
        let mut a = app(tasks, 0, Layout::Grouped);
        let s = screen(&mut a, 60, 24);
        // a spinner turns in place of BP-4's mark
        assert!(!s[12].starts_with(" ✔") && s[12].ends_with("BP-4  Task BP-4"), "{s:#?}");
        let s: Vec<String> = s.into_iter().enumerate().map(|(i, r)| if i == 12 { r[r.find("BP-4").unwrap()..].into() } else { r }).collect();
        assert_eq!(s[2..18], [
            " Ready to accept  5",
            " ✔ BP-1  Task BP-1",
            "   Done.",
            "   3 files, +12 −1 · Not toured yet",
            " ✔ BP-2  Task BP-2",
            "   Done.",
            "   Toured",
            " ✔ BP-3  Task BP-3",
            "   Done.",
            "   Tour written, not opened yet",
            "BP-4  Task BP-4",
            "   Done.",
            "   Writing its tour…",
            " ✔ BP-5  Task BP-5",
            "   tour failed: its tour doesn't parse",
            "",
        ].map(String::from), "{s:#?}");
        assert!(row(&s, 0).starts_with(" a accept  g tour"), "{s:#?}");
    }

    const QUESTION: &str = "Pick: candidate 4 (one rule for who sees to a task), 3 (reviews that ask before they block), or both. 4 changes how triage works for every coordinator; 3 only touches the reviewer's prompt and is easy to undo.";

    fn asking(id: &str) -> Task {
        Task { status: "needs".into(), ask: Some(true), why: Some(QUESTION.into()), title: "Architecture round 2: review".into(), ..ready(id) }
    }

    #[test]
    fn i_on_a_question_shows_all_of_it_above_the_answer() {
        let mut a = app(vec![asking("BP-31")], 1, Layout::Grouped);
        a.mode = Mode::Tell { id: "BP-31".into(), text: "Postgres".into(), reply: true };
        let mut term = Terminal::new(TestBackend::new(42, 16)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let s: Vec<String> = (0..16).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        assert_eq!(s, [
            " BP-31  Architecture round 2: review",
            "",
            " Pick: candidate 4 (one rule for who sees",
            " to a task), 3 (reviews that ask before",
            " they block), or both. 4 changes how",
            " triage works for every coordinator; 3",
            " only touches the reviewer's prompt and",
            " is easy to undo.",
            "",
            "",
            "",
            "",
            "",
            " Answer: Postgres",
            " ⏎ sends · esc returns",
            "",
        ].map(String::from), "{s:#?}");
        // the question at normal weight, the cursor after what's typed
        assert!(!buf[(1, 2)].modifier.contains(Modifier::DIM));
        assert_eq!(term.get_cursor_position().unwrap(), Position::new(17, 13));

        // too short for all of it: cut with " …", the answer line kept
        let s = screen(&mut a, 42, 8);
        assert_eq!(s[2..], [
            " Pick: candidate 4 (one rule for who sees",
            " to a task), 3 (reviews that ask before …",
            "",
            " Answer: Postgres",
            " ⏎ sends · esc returns",
            "",
        ].map(String::from), "{s:#?}");

        // a tell is still the one line at the bottom of the board
        a.mode = Mode::Tell { id: "BP-31".into(), text: "hi".into(), reply: false };
        assert_eq!(row(&screen(&mut a, 42, 16), 1), " Tell BP-31: hi");
    }

    #[test]
    fn the_selected_task_says_all_it_has_to_say_from_under_its_id() {
        let stopped = Task { status: "stopped".into(), ..ready("BP-2") };
        let mut a = app(vec![asking("BP-1"), stopped, asking("BP-3"), ready("BP-4")], 0, Layout::Grouped);
        a.sel = Some("BP-1".into());
        let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let s: Vec<String> = (0..24).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        // all of it under the ID for the selected task (up to eight lines), two for the others
        assert_eq!(s[3..17], [
            "▌● BP-1  Architecture round 2: review",
            "▌  Pick: candidate 4 (one rule for who",
            "▌  sees to a task), 3 (reviews that ask",
            "▌  before they block), or both. 4",
            "▌  changes how triage works for every",
            "▌  coordinator; 3 only touches the",
            "▌  reviewer's prompt and is easy to",
            "▌  undo.",
            "▌■ BP-2  Task BP-2",
            "▌  Its agent exited. r resumes it.",
            "▌● BP-3  Architecture round 2: review",
            "▌  Pick: candidate 4 (one rule for who",
            "▌  sees to a task), 3 (reviews that ask…",
            "",
        ].map(String::from), "{s:#?}");
        // what needs you is at normal weight, and so is what a ready task says
        assert!(!buf[(3, 4)].modifier.contains(Modifier::DIM));
        assert!(!buf[(3, 12)].modifier.contains(Modifier::DIM)); // stopped
        assert!(!buf[(3, 14)].modifier.contains(Modifier::DIM));
        let done = s.iter().position(|r| r == "   Done.").unwrap();
        assert!(!buf[(3, done as u16)].modifier.contains(Modifier::DIM));

        // selecting another task gives it the room, and BP-1 goes back to two lines
        a.sel = Some("BP-3".into());
        let s = screen(&mut a, 42, 24);
        assert_eq!(s[4..7], [
            "▌  Pick: candidate 4 (one rule for who",
            "▌  sees to a task), 3 (reviews that ask…",
            "▌■ BP-2  Task BP-2",
        ].map(String::from), "{s:#?}");
        assert_eq!(s[8], "▌● BP-3  Architecture round 2: review", "{s:#?}");
        assert_eq!(s[15], "▌  undo.", "{s:#?}");
    }

    #[test]
    fn another_coordinators_question_stays_dim() {
        let c = |id: &str, who: &str| Task { coordinator: Some(who.into()), ..asking(id) };
        let mut a = app(vec![c("BP-1", "c1"), c("BP-2", "c2"), ready("BP-3")], 0, Layout::Grouped);
        a.cfg.coordinators = vec!["c1".into(), "c2".into()];
        a.cfg.board_others = vec!["c1".into()];
        a.me = Some("c1".into());
        a.sel = Some("BP-3".into());
        let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let s: Vec<String> = (0..24).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        let mine = s.iter().position(|r| r.contains("BP-1")).unwrap() + 1;
        let theirs = s.iter().position(|r| r.contains("BP-2")).unwrap() + 1;
        assert!(s[theirs].starts_with("▌  Pick:"), "{s:#?}");
        assert!(!buf[(3, mine as u16)].modifier.contains(Modifier::DIM), "{s:#?}");
        assert!(buf[(3, theirs as u16)].modifier.contains(Modifier::DIM), "{s:#?}");

        // selected, it's on the tint like any selected task, and not dim there
        a.sel = Some("BP-2".into());
        let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let cell = &term.backend().buffer()[(3, theirs as u16)];
        assert!(!cell.modifier.contains(Modifier::DIM));
        assert_eq!(cell.bg, TINT);
    }

    #[test]
    fn the_selected_task_has_its_id_reversed_and_its_whole_block_tinted() {
        let mut a = app(vec![asking("BP-1"), ready("BP-12")], 0, Layout::Grouped);
        a.sel = Some("BP-1".into());
        let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let s: Vec<String> = (0..24).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        let title = s.iter().position(|r| r.contains("BP-1 ")).unwrap() as u16;
        assert_eq!(s[title as usize], "▌● BP-1   Architecture round 2: review", "{s:#?}");
        // the ID itself in reverse video, and not its padding: that shows in any theme
        for x in 3..7 {
            assert!(buf[(x, title)].modifier.contains(Modifier::REVERSED | Modifier::BOLD), "column {x}");
            assert_eq!(buf[(x, title)].bg, Color::Reset, "column {x}");
        }
        assert!(!buf[(7, title)].modifier.contains(Modifier::REVERSED));
        // the rest of its title row and every detail row on the tint, from column 3
        // to the right edge, so the whole block reads as selected
        let details = (title + 1..24).take_while(|&y| s[y as usize].starts_with("▌  ")).count() as u16;
        assert_eq!(details, 7, "{s:#?}");
        for y in title..=title + details {
            let from = if y == title { 7 } else { 3 };
            for x in from..40 {
                assert_eq!(buf[(x, y)].bg, TINT, "row {y}, column {x}: {s:#?}");
                assert!(!buf[(x, y)].modifier.contains(Modifier::DIM), "row {y}, column {x}");
            }
            assert_eq!(buf[(40, y)].bg, Color::Reset, "row {y}");
            // the amber bar stays as it is
            assert_eq!(buf[(0, y)].symbol(), "▌");
            assert_eq!(buf[(0, y)].fg, AMBER);
            assert_eq!(buf[(0, y)].bg, Color::Reset);
        }
        // another task: no reverse, no tint; a ready task's details at normal weight
        let other = s.iter().position(|r| r.contains("BP-12")).unwrap() as u16;
        assert!(!buf[(3, other)].modifier.contains(Modifier::REVERSED));
        for y in other..other + 3 {
            assert!((0..42).all(|x| buf[(x, y)].bg == Color::Reset), "row {y}");
        }
        assert!(!buf[(3, other + 1)].modifier.contains(Modifier::DIM));

        // in the compact layout, the title row alone, the same way
        a.cfg.layout = Layout::Compact;
        let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let y = (0..24).find(|&y| buf[(3, y)].symbol() == "B" && buf[(6, y)].symbol() == "1").unwrap();
        assert!(buf[(3, y)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buf[(10, y)].bg, TINT);
        assert_eq!(buf[(10, y + 1)].bg, Color::Reset);
    }

    #[test]
    fn a_selected_task_low_on_the_board_scrolls_its_details_into_view() {
        let mut a = app((1..=7).map(|i| asking(&format!("BP-{i}"))).collect(), 0, Layout::Grouped);
        a.sel = Some("BP-7".into());
        let s = screen(&mut a, 42, 16);
        let i = s.iter().position(|r| r.contains("BP-7")).unwrap();
        assert_eq!(s[i + 7], "▌  undo.", "{s:#?}");
        assert!(s[1].ends_with('↑'), "{s:#?}");
    }

    #[test]
    fn a_stale_tour_asks_on_the_last_row() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        a.mode = Mode::Stale { id: "BP-1".into() };
        a.msg = Note::info("BP-1's tour is stale: the code changed.");
        let s = screen(&mut a, 70, 10);
        assert_eq!(row(&s, 2), " BP-1's tour is stale: the code changed.");
        assert_eq!(row(&s, 1), " n new tour   o the old one   esc cancels");
    }

    #[test]
    fn the_details_at_42_columns_say_when_a_task_comes_to_you_and_open_for_needs() {
        let ago = |m| (model::now() - chrono::Duration::minutes(m)).to_rfc3339();
        let t = |id: &str, st: &str, title: &str, why: &str, m| Task {
            title: title.into(),
            why: Some(why.into()).filter(|w: &String| !w.is_empty()),
            since: Some(ago(m)),
            ..Task { id: id.into(), status: st.into(), ..Default::default() }
        };
        let finished = Task { summary: Some("# Recorded the demo video".into()), ..t("BP-64", "finished", "Record the demo video", "", 30) };
        let mut a = app(vec![
            t("BP-61", "needs", "Fix the login redirect", "Failed (coordinator didn't resolve it)", 4),
            t("BP-62", "triage", "Retry flaky upload test", "Ended its turn without saying ready", 3),
            finished,
        ], 1, Layout::Grouped);
        a.settle();
        let s = screen(&mut a, 42, 24);
        let at = |head: &str| s.iter().position(|r| r.contains(head)).unwrap();
        // not selected, a task its coordinator is on still gets room for when it's yours
        let i = at("BP-62");
        assert_eq!(s[i + 1..i + 3], ["   Ended its turn without ready. Yours", "   in 7 min."].map(String::from), "{s:#?}");
        // a finished task doesn't spend its two lines on "a closes it": the key row says that
        let i = at("BP-64");
        assert_eq!(s[i + 1], "   Finished, nothing to merge: Recorded", "{s:#?}");
        assert_eq!(s[i + 2], "   the demo video.", "{s:#?}");
        // a needs task without a question: ⏎ opens it, and says so
        assert!(row(&s, 1).starts_with(" ⏎ open  t tell  p peek"), "{s:#?}");
    }

    #[test]
    fn only_background_details_are_dim_and_a_stopped_tasks_age_turns_red() {
        let ago = |m| (model::now() - chrono::Duration::minutes(m)).to_rfc3339();
        let t = |id: &str, st: &str, m| Task {
            title: format!("Task {id}"),
            why: matches!(st, "stopped" | "needs" | "triage").then(|| "Failed (exit 1)".into()),
            since: Some(ago(m)),
            ..Task { id: id.into(), status: st.into(), ..Default::default() }
        };
        let mut a = app(vec![
            t("BP-1", "needs", 2),
            t("BP-2", "stopped", 6),
            t("BP-3", "stopped", 4),
            t("BP-4", "ready", 9),
            t("BP-5", "finished", 9),
            Task { since: Some(ago(9)), ..busy("BP-6", "working", "Running the tests") },
            t("BP-7", "todo", 9),
            t("BP-8", "triage", 1),
            t("BP-9", "review", 9),
            t("BP-10", "done", 9),
        ], 1, Layout::Grouped);
        a.closed = true; // closed tasks shown
        a.sel = Some("BP-1".into());
        let mut term = Terminal::new(TestBackend::new(42, 48)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let s: Vec<String> = (0..48).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        let title = |id: &str| s.iter().position(|r| r.contains(&format!("{id} "))).unwrap_or_else(|| panic!("{id}: {s:#?}")) as u16;
        let dim_detail = |id: &str| {
            let y = title(id) + 1;
            assert!(s[y as usize].len() > 3 && s[y as usize][..].contains(|c: char| c.is_alphanumeric()), "{id} has no detail: {s:#?}");
            buf[(3, y)].modifier.contains(Modifier::DIM)
        };
        // what you read and act on at normal weight
        for id in ["BP-2", "BP-3", "BP-4", "BP-5"] {
            assert!(!dim_detail(id), "{id}: {s:#?}");
        }
        // the background dim: working, queued, the coordinator's, in review, closed
        for id in ["BP-6", "BP-7", "BP-8", "BP-9", "BP-10"] {
            assert!(dim_detail(id), "{id}: {s:#?}");
        }
        // a stopped task's age is red after 5 minutes, as a needs task's is; not before
        let age = |id: &str| buf[(39, title(id))].clone();
        assert_eq!(s[title("BP-2") as usize].trim_end().rsplit(' ').next(), Some("6m"), "{s:#?}");
        assert_eq!(age("BP-2").fg, Color::Red);
        assert!(!age("BP-2").modifier.contains(Modifier::DIM));
        assert_ne!(age("BP-3").fg, Color::Red);
        assert!(age("BP-3").modifier.contains(Modifier::DIM));
        assert_ne!(age("BP-6").fg, Color::Red); // a working task's age never is
    }

    #[test]
    fn the_working_and_review_marks_turn_twice_a_second_and_a_command_spinner_fast() {
        let working = Task { status: "working".into(), ..ready("BP-1") };
        let review = Task { status: "review".into(), ..ready("BP-2") };
        let mut a = app(vec![working, review, ready("BP-3")], 0, Layout::Grouped);
        let marks = |a: &mut App, ms: u64| {
            a.started = Instant::now() - std::time::Duration::from_millis(ms);
            let s = screen(a, 42, 24);
            ["BP-1", "BP-2"].map(|id| s.iter().find(|r| r.contains(id)).unwrap().chars().nth(1).unwrap())
        };
        // the same frame through the first half second, the next one after it
        assert_eq!(marks(&mut a, 0), marks(&mut a, 450));
        let (first, later) = (marks(&mut a, 0), marks(&mut a, 550));
        assert_ne!(first[0], later[0]);
        assert_ne!(first[1], later[1]);
        // a command's spinner is feedback: it moves every tenth of a second
        a.started = Instant::now();
        assert_ne!(spinner(&a), { a.started = Instant::now() - std::time::Duration::from_millis(150); spinner(&a) });
    }

    #[test]
    fn the_help_fits_a_narrow_board_and_scrolls_to_its_end() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        a.mode = Mode::Help { top: 0 };
        let s = screen(&mut a, 42, 24);
        assert_eq!(s[..6], [
            " Board keys",
            "",
            " Moving",
            "   j k   select a task (↑ ↓ too)",
            "   PgDn  a page down (ctrl+d: half)",
            "   PgUp  a page up (ctrl+u: half)",
        ].map(String::from), "{s:#?}");
        // one short line per key: nothing is cut or wrapped at 42 columns
        assert!(!s.iter().any(|r| r.ends_with('…')), "{s:#?}");
        assert!(s.iter().all(|r| !r.starts_with("        ")), "{s:#?}");
        assert!(row(&s, 2).ends_with("↓"), "{s:#?}");
        assert_eq!(row(&s, 1), " j k space scroll · esc returns");
        assert_eq!(row(&s, 0), "", "the gap for Rex's capsules");

        // scrolled past the end, it stops at the last line: the marks' legend
        a.mode = Mode::Help { top: 999 };
        let s = screen(&mut a, 42, 24);
        let Mode::Help { top } = a.mode else { panic!() };
        assert!(top < 999 && top > 0);
        assert!(s[1].ends_with("↑"), "{s:#?}");
        assert_eq!(row(&s, 2), "   ▌     needs you; red: stopped", "{s:#?}");
        for line in ["   ●     needs you", "   ✔     ready to accept", "   ◜ ◐   in review (it turns)", "   ·     accepted"] {
            assert!(s.contains(&line.to_string()), "{line}: {s:#?}");
        }

        // narrower, a long line wraps under its own text
        a.mode = Mode::Help { top: 0 };
        let s = screen(&mut a, 30, 24);
        let i = s.iter().position(|r| r.starts_with("   ⏎")).unwrap();
        assert_eq!(s[i..i + 2], ["   ⏎     open its pane, or", "         its reviewer's"].map(String::from), "{s:#?}");

        // tall enough for all of it: no scrolling to offer
        let s = screen(&mut a, 42, 80);
        assert_eq!(row(&s, 1), " any key returns");
        assert!(!s.iter().any(|r| r.ends_with('↓') || r.ends_with('↑')), "{s:#?}");
    }

    #[test]
    fn the_key_line_never_drops_its_question_mark() {
        let keys = |t: &Task, w: u16| {
            let mut a = app(vec![t.clone()], 0, Layout::Grouped);
            a.sel = Some("BP-1".into());
            row(&screen(&mut a, w, 12), 0).to_string()
        };
        let t = Task { status: "ready".into(), worktree: Some("/w".into()), ..ready("BP-1") };
        assert_eq!(keys(&t, 60), format!(" a accept  g tour  d drop  ⏎ open  v diff{}? keys", " ".repeat(12)));
        // no room for "? keys": a bare ? at the right edge
        assert_eq!(keys(&t, 46), " a accept  g tour  d drop  ⏎ open  v diff   ?");
        // less room: the task's last keys go, never the ?
        assert_eq!(keys(&t, 42), " a accept  g tour  d drop  ⏎ open       ?");
        assert_eq!(keys(&t, 20), " a accept         ?");
    }

    #[test]
    fn a_gap_of_none_is_the_old_layout() {
        let mut a = app(vec![ready("BP-1")], 0, Layout::Grouped);
        a.msg = Note::done("Accepted BP-1");
        let s = screen(&mut a, 46, 12);
        assert_eq!(row(&s, 1), " Accepted BP-1");
        assert!(row(&s, 0).starts_with(" a accept  g tour  d drop"), "{s:#?}");
        assert!(row(&s, 0).ends_with("? keys"), "{s:#?}");
    }

    #[test]
    fn the_compact_panel_and_scroll_mark_move_up_too() {
        let tasks: Vec<Task> = (1..=9).map(|i| ready(&format!("BP-{i}"))).collect();
        let lines = |gap| {
            let mut a = app(tasks.clone(), gap, Layout::Compact);
            screen(&mut a, 40, 16)
        };
        let (old, new) = (lines(0), lines(1));
        // everything from the top down is the same; the bottom rows move up by one
        assert_eq!(row(&new, 0), "");
        for k in 0..8 {
            assert_eq!(row(&new, k + 1), row(&old, k), "row {k} from the bottom: {new:#?}");
        }
        assert!(row(&new, 7).starts_with(" ─"), "the panel's rule: {new:#?}");
        assert!(row(&new, 8).ends_with("↓"), "the scroll mark: {new:#?}");
        // and the list has one row fewer
        assert_eq!(&new[..7], &old[..7]);
    }

    #[test]
    fn a_short_board_drops_the_gap_before_the_keys() {
        // grouped and short: header, blank, one row and the keys
        for h in [5u16, 4, 2] {
            let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
            let s = screen(&mut a, 40, h);
            assert!(row(&s, 0).starts_with(" a accept"), "height {h}: {s:#?}");
        }
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        let s = screen(&mut a, 40, 6);
        assert_eq!(row(&s, 0), "");
        assert!(row(&s, 1).starts_with(" a accept"), "{s:#?}");
        assert!(s[2].contains("BP-1"), "the task still shows: {s:#?}");
    }

    #[test]
    fn a_board_with_tasks_but_none_shown_says_why() {
        let closed = |id: &str| Task { status: "done".into(), ..ready(id) };
        let mut a = app(vec![closed("BP-1"), closed("BP-2")], 1, Layout::Grouped);
        let s = screen(&mut a, 42, 12);
        assert_eq!(s[2..4], [" Nothing open.", " f shows 2 closed."].map(String::from), "{s:#?}");
        a.closed = true;
        let s = screen(&mut a, 42, 12);
        assert_eq!(s[2], " Closed  2", "{s:#?}");

        // only another coordinator's: o shows them
        let c3 = |id: &str| Task { coordinator: Some("c3".into()), ..ready(id) };
        let mut a = app(vec![c3("BP-3"), c3("BP-4"), Task { coordinator: Some("c1".into()), ..closed("BP-5") }], 1, Layout::Grouped);
        a.cfg.coordinators = vec!["c1".into(), "c3".into()];
        a.me = Some("c1".into());
        let s = screen(&mut a, 42, 12);
        assert_eq!(s[2..5], [" Nothing of yours open.", " o shows 2 of c3's.", " f shows 1 closed."].map(String::from), "{s:#?}");

        // and none at all
        let mut a = app(vec![], 1, Layout::Grouped);
        let s = screen(&mut a, 42, 12);
        assert_eq!(s[2], " No tasks yet.", "{s:#?}");
    }

    fn asks(id: &str) -> Task {
        Task { status: "needs".into(), why: Some("Asks: python3 or python?".into()), ..ready(id) }
    }

    #[test]
    fn a_short_board_gives_the_list_its_spare_rows() {
        let tasks = vec![asks("BP-1"), asks("BP-2"), ready("BP-3")];
        let mut a = app(tasks.clone(), 1, Layout::Grouped);
        let s = screen(&mut a, 42, 10);
        // header, blank, seven list rows, the keys, the capsules' row
        assert_eq!(s[1..9], [
            "",
            " Needs you  2",
            "▌● BP-1  Task BP-1",
            "▌  Asks: python3 or python?",
            "▌● BP-2  Task BP-2",
            "▌  Asks: python3 or python?",
            "                                        ↓",
            " ⏎ open  t tell  p peek            ? keys",
        ].map(String::from), "{s:#?}");
        assert_eq!(row(&s, 0), "");
        // the scroll mark is past the ages, and the list moves down under it
        a.sel = Some("BP-3".into());
        let s = screen(&mut a, 42, 10);
        assert!(s[1].ends_with('↑') && s[3].contains("BP-3"), "{s:#?}");

        // a message takes the key row, and the list keeps its rows
        a.sel = None;
        a.msg = Note::done("Accepted BP-0.");
        let s = screen(&mut a, 42, 10);
        assert_eq!(row(&s, 1), " Accepted BP-0.");
        assert!(s[7].ends_with('↓'), "{s:#?}");
        // a y/n question gets a row of its own above the keys
        a.msg = Note::default();
        a.pending = Some((Action::Drop, "BP-1".into()));
        let s = screen(&mut a, 42, 10);
        assert_eq!(row(&s, 2), " BP-1: Drop it?");
        assert_eq!(row(&s, 1), " y drop   any other key cancels");

        // a tall board keeps its blank rows
        let mut a = app(tasks, 1, Layout::Grouped);
        let s = screen(&mut a, 42, 12);
        assert_eq!(s[8], "", "{s:#?}");
        assert!(s[7].ends_with('↓') && !s[7].contains("Ready"), "{s:#?}");
    }

    #[test]
    fn a_narrow_board_puts_one_space_after_the_id_and_the_age_on_the_detail() {
        let old = (model::now() - chrono::Duration::minutes(12)).to_rfc3339();
        let tasks = vec![
            Task { title: "Fix the login redirect after sign-in".into(), since: Some(old), ..asks("BP-3") },
            Task { title: "Paginate the orders list".into(), why: None, ..ready("BP-10") },
        ];
        let mut a = app(tasks.clone(), 0, Layout::Grouped);
        let s = screen(&mut a, 30, 16);
        assert_eq!(s[2..10], [
            " Needs you  1",
            "▌● BP-3  Fix the login redi…",
            "▌  12m · Asks: python3 or",
            "▌  python?",
            "",
            " Ready to accept  1",
            " ✔ BP-10 Paginate the order…",
            "   Done.",
        ].map(String::from), "{s:#?}");
        // a needs task's age is still red after five minutes
        assert_eq!(tone_of(&mut a, 30, 14, "12m"), Some(Color::Red));

        // at 36 columns the age is back on the title
        let s = screen(&mut a, 36, 16);
        assert!(s[3].ends_with("12m") && s[3].contains("BP-3   Fix"), "{s:#?}");
        assert_eq!(s[4], "▌  Asks: python3 or python?", "{s:#?}");

        // compact: no detail lines, so the age stays on the title
        let mut a = app(tasks, 0, Layout::Compact);
        let s = screen(&mut a, 30, 20);
        assert!(s.iter().any(|r| r.starts_with("▌● BP-3  Fix the l") && r.ends_with("12m")), "{s:#?}");
    }

    #[test]
    fn the_compact_layout_names_its_groups_dim() {
        let tasks = vec![asks("BP-1"), ready("BP-2"), Task { status: "working".into(), ..ready("BP-3") }];
        let mut a = app(tasks, 0, Layout::Compact);
        a.sel = Some("BP-2".into());
        let s = screen(&mut a, 42, 20);
        assert_eq!(s[2..8], [
            " Needs you  1",
            "▌● BP-1  Task BP-1",
            " Ready to accept  1",
            " ✔ BP-2  Task BP-2",
            " Working  1",
            " ⠋ BP-3  Task BP-3",
        ].map(String::from), "{s:#?}");
        let mut term = Terminal::new(TestBackend::new(42, 20)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        assert!(term.backend().buffer()[(1, 4)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_peek_that_cant_read_the_screen_says_why_and_isnt_live() {
        let mut a = app(vec![ready("BP-3")], 0, Layout::Grouped);
        a.mode = Mode::Overlay { title: "BP-3  Fix it".into(), lines: vec!["BP-3's agent isn't running.".into()], peek: Some("BP-3".into()) };
        a.peek_failed = true;
        let s = screen(&mut a, 42, 12);
        assert_eq!(s[2], " BP-3's agent isn't running.");
        assert_eq!(row(&s, 0), " t tell  ⏎ open  i answer  esc returns");
        a.peek_failed = false;
        let s = screen(&mut a, 42, 12);
        assert!(row(&s, 0).starts_with(" live · "), "{s:#?}");
    }
}
