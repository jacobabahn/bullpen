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
fn counts(tasks: &[&Task], room: usize) -> Line<'static> {
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

    if matches!(app.mode, Mode::Overlay { .. } | Mode::Answer { .. }) {
        // the title, a blank line, at least one line and the foot
        let h = full.height as usize;
        app.height = (h - bottom_gap(app, h, 4)) as u16;
    }
    if let Mode::Overlay { title, lines, peek } = &app.mode {
        let (area, w, h) = (full, full.width as usize, app.height as usize);
        put(buf, area, 0, 1, span(fit(title, w.saturating_sub(2)), bold()));
        for (j, line) in lines.iter().take(h.saturating_sub(3)).enumerate() {
            put(buf, area, 2 + j, 1, screen_line(line));
        }
        let foot = if peek.is_some() { "live · any key returns" } else { "any key returns" };
        put(buf, area, h.saturating_sub(1), 1, span(foot, dim()));
        return;
    }
    if let Mode::Answer { title, prompt, .. } = &app.mode {
        draw_answer(buf, full, app.height as usize, title, prompt.as_ref());
        return;
    }

    let area = full;
    let (w, h) = (area.width as usize, area.height as usize);

    let layout = app.cfg.layout;
    let footer = if layout == Layout::Grouped { 3 } else { 7 };
    // from here on h is the board's height above the gap: everything at the
    // bottom (the detail panel, message line, keys, scroll mark) sits on top of it
    let h = h - bottom_gap(app, h, 4 + footer);
    app.height = h as u16;
    // by field, not app.groups(): app.top changes while these are in use
    let groups = model::shown_groups(&app.tasks, &app.cfg, app.me.as_deref(), app.finished);
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
    // with several coordinators: whose board this is, and how many of the other
    // ones' tasks it hides (`o` shows them, dimmed)
    let hint = model::hidden_hint(&model::hidden(&app.tasks, &app.cfg, app.me.as_deref(), app.finished));
    let hint = if hint.is_empty() { hint } else { format!(" · {hint}") };
    let auto = match app.me.as_deref() {
        Some(me) if app.cfg.coordinators.len() > 1 => format!(" · {me}{auto}{hint}"),
        _ => auto.to_string(),
    };
    let auto = auto.as_str();
    let shown: Vec<&Task> = groups.iter().flat_map(|(_, ts)| ts.iter().copied()).collect();
    let count = counts(&shown, w.saturating_sub(width(&name) + width(auto) + 5));
    let count_x = w.saturating_sub(2 + count.width()).max(1);
    // then the other repositories with something for you, in what room is left
    let others_x = 1 + width(&name) + width(auto) + 3;
    let room = count_x.saturating_sub(others_x + 2);
    if !app.others.counts.is_empty() && room >= 8 {
        let style = if app.others.counts.iter().any(|c| c.needs > 0) { Style::new().fg(AMBER) } else { dim() };
        put(buf, area, 0, others_x - 3, span(" · ", dim()));
        put(buf, area, 0, others_x, span(model::others_text(&app.others.counts, room), style));
    }
    put(buf, area, 0, count_x, count);
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
                // a working or in-review task: what its agent is doing now, on one line
                let doing = model::doing_now(t, now);
                // what a task needs from you, GitHub said, or the usage limit gets two lines; anything else one
                let lines = if t.flagged() || t.is_limited() || t.status == "finished" || !model::pr_phrase(t).is_empty() { 2 } else { 1 };
                let detail = if doing.is_empty() { model::wrap(&model::detail_with_auto(t, &app.cfg, now), detail_w, lines) } else { vec![doing] };
                for d in detail {
                    if !d.is_empty() {
                        rows.push(Row::Detail(t, d));
                    }
                }
                let stat = model::stat_line(t);
                if !stat.is_empty() {
                    rows.push(Row::Detail(t, stat));
                }
            }
        }
    }

    // keep the selected task in view, starting at a heading or a task
    let avail = h.saturating_sub(3 + footer).max(1); // one line spare for the scroll mark
    let sel_row = rows.iter().position(|r| matches!(r, Row::Title(t) if is_cur(t)));
    let mut top = app.top;
    if let Some(i) = sel_row {
        if i < top {
            top = i - usize::from(i > 0 && matches!(rows[i - 1], Row::Head(_)));
        } else if i + 1 >= top + avail {
            top = (i + 2 - avail).min(i); // with room for one row, the task's own
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
    if let Mode::Tell { id, text, reply } = &app.mode {
        let label = if *reply { format!("Answer {id}:") } else { format!("Tell {id}:") };
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
    } else if let Mode::Stale { id } = &app.mode {
        let ask = format!("{id}'s tour is stale: the code changed since. n new tour  o old one  esc cancels");
        put(buf, area, last, 1, span(fit(&ask, w.saturating_sub(2)), bold()));
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
    let title = fit(&model::shown_title(t, &app.cfg, app.me.as_deref()), width_all.saturating_sub(width(&left) + width(&age) + 2));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Config;
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
            finished: false,
            msg: String::new(),
            top: 0,
            pending: None,
            running: Vec::new(),
            mode: Mode::Normal,
            height: 0,
            started: Instant::now(),
            tx,
            peeking: false,
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

    #[test]
    fn the_header_says_what_is_hidden_and_o_labels_them() {
        let c = |id: &str, who: &str| Task { coordinator: Some(who.into()), ..ready(id) };
        let mut a = app(vec![c("BP-1", "c1"), c("BP-2", "c2"), c("BP-3", "c2")], 1, Layout::Grouped);
        a.cfg.coordinators = vec!["c1".into(), "c2".into()];
        a.me = Some("c1".into());
        let s = screen(&mut a, 60, 14);
        assert!(s[0].contains("repo · c1 · +2 in c2 · o shows"), "{s:#?}");
        assert!(s[0].ends_with("✔ 1"), "{s:#?}"); // counts what's shown
        assert!(!s.iter().any(|r| r.contains("BP-2")), "{s:#?}");
        a.cfg.board_others = vec!["c1".into()];
        let s = screen(&mut a, 60, 14);
        assert!(!s[0].contains("o shows") && s[0].ends_with("✔ 3"), "{s:#?}");
        assert!(s.iter().any(|r| r.contains("BP-2  c2 · Task BP-2")), "{s:#?}");
        assert!(s.iter().any(|r| r.contains("BP-1  Task BP-1")), "{s:#?}");
    }

    #[test]
    fn the_header_counts_the_other_repos_in_the_room_left() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        a.others.counts = vec![model::RepoCount { name: "shop".into(), needs: 1, ready: 2 }];
        let s = screen(&mut a, 60, 10);
        assert!(s[0].starts_with(" repo · shop: 1 needs you, 2 ready"), "{s:#?}");
        assert!(s[0].ends_with("✔ 1"), "{s:#?}"); // this repo's counts keep their place
        let s = screen(&mut a, 30, 10);
        assert!(s[0].starts_with(" repo · shop ●1 ✔2"), "{s:#?}");
        let s = screen(&mut a, 20, 10); // no room: left out
        assert!(!s[0].contains("shop"), "{s:#?}");
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
            "         Reading the diff",
            "",
            " Working  2",
            " ⠋ BP-1  Task BP-1                     now",
            "         Running the board's tests with c…",
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
    fn the_last_row_is_left_for_rexs_capsules() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        let s = screen(&mut a, 46, 12);
        assert_eq!(row(&s, 0), "");
        assert!(row(&s, 1).starts_with(" a accept  g tour  d drop"), "{s:#?}");
        assert!(row(&s, 1).ends_with("? keys"), "{s:#?}");

        a.msg = "Accepted BP-1".into();
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
        a.mode = Mode::Tell { id: "BP-1".into(), text: "Postgres".into(), reply: true };
        assert_eq!(row(&screen(&mut a, 40, 12), 1), " Answer BP-1: Postgres");
    }

    #[test]
    fn a_ready_task_says_whether_its_toured_and_a_tour_being_written_spins() {
        let run = |state: &str, commit: &str, opened: Option<&str>| {
            Some(model::TourRun { state: state.into(), commit: Some(commit.into()), opened: opened.map(String::from) })
        };
        let tasks = vec![
            Task { diffstat: Some("3 files, +12 −1".into()), ..ready("BP-1") },
            Task { tour: run("written", "abc", Some("abc")), ..ready("BP-2") },
            Task { tour: run("written", "def", Some("abc")), ..ready("BP-3") },
            Task { tour: run("writing", "", None), ..ready("BP-4") },
            Task { tour: run("failed", "", None), note: Some("tour failed: its tour doesn't parse".into()), ..ready("BP-5") },
        ];
        let mut a = app(tasks, 0, Layout::Grouped);
        let s = screen(&mut a, 60, 24);
        // a spinner turns in place of BP-4's mark
        assert!(!s[12].starts_with(" ✔") && s[12].ends_with("BP-4  Task BP-4"), "{s:#?}");
        let s: Vec<String> = s.into_iter().enumerate().map(|(i, r)| if i == 12 { r[r.find("BP-4").unwrap()..].into() } else { r }).collect();
        assert_eq!(s[2..18], [
            " Ready to accept  5",
            " ✔ BP-1  Task BP-1",
            "         Done.",
            "         3 files, +12 −1 · Not toured yet",
            " ✔ BP-2  Task BP-2",
            "         Done.",
            "         Toured",
            " ✔ BP-3  Task BP-3",
            "         Done.",
            "         Tour written, not opened yet",
            "BP-4  Task BP-4",
            "         Done.",
            "         Writing its tour…",
            " ✔ BP-5  Task BP-5",
            "         tour failed: its tour doesn't parse",
            "",
        ].map(String::from), "{s:#?}");
        assert!(row(&s, 0).starts_with(" a accept  g tour"), "{s:#?}");
    }

    #[test]
    fn a_stale_tour_asks_on_the_last_row() {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        a.mode = Mode::Stale { id: "BP-1".into() };
        let s = screen(&mut a, 90, 10);
        assert_eq!(row(&s, 1), " BP-1's tour is stale: the code changed since. n new tour  o old one  esc cancels");
    }

    #[test]
    fn a_gap_of_none_is_the_old_layout() {
        let mut a = app(vec![ready("BP-1")], 0, Layout::Grouped);
        a.msg = "Accepted BP-1".into();
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
        // grouped: header, blank, one row, scroll mark and three footer rows
        for h in [7u16, 4, 2] {
            let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
            let s = screen(&mut a, 40, h);
            assert!(row(&s, 0).starts_with(" a accept"), "height {h}: {s:#?}");
        }
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        let s = screen(&mut a, 40, 8);
        assert_eq!(row(&s, 0), "");
        assert!(row(&s, 1).starts_with(" a accept"), "{s:#?}");
        assert!(s[2].contains("BP-1"), "the task still shows: {s:#?}");
    }
}
