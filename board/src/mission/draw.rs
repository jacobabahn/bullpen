//! Drawing mission control. It lays itself out for the terminal it's in: from WIDE
//! columns the list and the selected task's detail sit side by side in frames, and
//! every task gets a line of what it's doing; from TWO_COLUMNS the same, with the
//! repos' tasks a line each; under that one column, the selected task's detail under
//! it in the list. The top line has the totals and the time, the bottom rows the
//! message and the keys, as on the board (whose questions, answers and typed lines
//! these are).

use super::items::{self, Item, Row};
use super::Mission;
use crate::app::{App, Kind, Mode};
use crate::model::{self, fit, pad, width, Task};
use crate::ui::bottom::{input_line, key_line, message, pending_keys};
use crate::ui::overlay::{draw_answer, draw_ask};
use crate::ui::{bold, dim, frame_at, mark_style, put, rev, span, AMBER, REVIEW, SPINNER, TINT, WORKING};
use bullpen_core::fleet::TaskState;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Widget};
use ratatui::Frame;

type Now = bullpen_core::timeutil::Stamp;

/// From this many columns, the wide layout: a detail line under every task.
pub const WIDE: usize = 140;
/// From this many, two columns: the list, and the selected task's detail beside it.
pub const TWO_COLUMNS: usize = 80;
/// How many lines of detail the selected task gets in one column.
const SELECTED_LINES: usize = 6;
/// How many rows a message wraps to, at most.
const MESSAGE_LINES: usize = 3;

/// The layout for a width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Wide,
    Two,
    One,
}

impl Size {
    pub fn of(w: usize) -> Size {
        if w >= WIDE {
            Size::Wide
        } else if w >= TWO_COLUMNS {
            Size::Two
        } else {
            Size::One
        }
    }
}

/// The list's width in two columns: under half of a wide screen, half of a smaller one.
fn list_width(w: usize, size: Size) -> usize {
    match size {
        Size::Wide => (w * 45 / 100).clamp(60, 90),
        _ => w / 2,
    }
}

pub fn draw(f: &mut Frame, m: &mut Mission) {
    let area = f.area();
    m.height = area.height;
    let (w, h) = (area.width as usize, area.height as usize);
    // a prompt being answered, or a coordinator's question, takes the whole screen
    if let Some(app) = m.active() {
        if let Mode::Answer { title, prompt, .. } = &app.mode {
            draw_answer(f.buffer_mut(), area, h, title, prompt.as_ref());
            return;
        }
        if let Mode::Tell { id, text, reply: true } = &app.mode {
            let t = app.tasks.iter().find(|t| t.id == *id);
            let title = t.map_or(id.clone(), |t| format!("{}  {}", t.id, t.title));
            let question = t.and_then(|t| t.why.clone()).filter(|q| !q.is_empty()).unwrap_or("Waiting for you.".into());
            let (x, y) = draw_ask(f.buffer_mut(), area, h, &title, &question, text);
            f.set_cursor_position(Position::new(area.x + x as u16, area.y + y as u16));
            return;
        }
    }
    let now = model::now();
    let size = Size::of(w);
    let msg = message_rows(m, w);
    top_line(f.buffer_mut(), area, m, size, now);
    // the rows between the top line and the message and keys
    let body = Rect { x: area.x, y: area.y + 1, width: area.width, height: h.saturating_sub(2 + msg.len()) as u16 };
    if size == Size::One {
        let list = Rect { y: body.y + 1, height: body.height.saturating_sub(1), ..body };
        draw_list(f.buffer_mut(), list, m, size, now);
    } else {
        let lw = list_width(w, size) as u16;
        let left = Rect { width: lw, ..body };
        let right = Rect { x: body.x + lw, width: body.width - lw, ..body };
        let buf = f.buffer_mut();
        Block::bordered().title(" Inbox and repos ").border_style(dim()).render(left, buf);
        Block::bordered().title(" Detail ").border_style(dim()).render(right, buf);
        draw_list(buf, inner(left), m, size, now);
        draw_detail(buf, inner(right), m, now);
    }
    if let Some(x) = bottom(f.buffer_mut(), area, m, msg) {
        f.set_cursor_position(Position::new(area.x + x as u16, area.y + area.height.saturating_sub(1)));
    }
}

/// Inside a frame's border, with a column of space each side.
fn inner(r: Rect) -> Rect {
    Rect { x: r.x + 1, y: r.y + 1, width: r.width.saturating_sub(2), height: r.height.saturating_sub(2) }
}

/// "MISSION CONTROL   3 need you · 2 ready · 4 of 9 running · 2 repos", the time at
/// the right; in one column "MISSION  ●3 ✔2 4/9".
fn top_line(buf: &mut Buffer, area: Rect, m: &Mission, size: Size, now: Now) {
    let w = area.width as usize;
    let t = items::totals(&m.fleet);
    let (name, totals, clock) = match size {
        Size::One => ("MISSION", items::totals_marks(t), now.format("%H:%M").to_string()),
        _ => ("MISSION CONTROL", items::totals_words(t), now.format("%a %-d %b %H:%M").to_string()),
    };
    put(buf, area, 0, 1, span(name, bold()));
    let x = 1 + width(name) + 3;
    let room = w.saturating_sub(x + width(&clock) + 4);
    let style = if t.needs > 0 { Style::new().fg(AMBER) } else { dim() };
    put(buf, area, 0, x, span(fit(&totals, room), style));
    put(buf, area, 0, w.saturating_sub(2 + width(&clock)), span(clock, dim()));
}

/// A line of the list, as drawn: what it is, and which task it belongs to.
enum Drawn {
    Head(&'static str),
    Repo(usize),
    Title(Item, bool),
    Detail(Item, String, bool),
    Gap,
}

impl Drawn {
    fn item(&self) -> Option<Item> {
        match self {
            Drawn::Title(i, _) | Drawn::Detail(i, ..) => Some(*i),
            _ => None,
        }
    }
}

/// How many detail lines a task gets: in the inbox, or anywhere on a wide screen, one
/// (what its agent is doing, else where it stands); in one column only the selected
/// task's, up to SELECTED_LINES (two columns have the detail beside).
fn detail_lines(size: Size, inbox: bool, selected: bool) -> usize {
    match size {
        Size::One if selected => SELECTED_LINES,
        Size::One => 0,
        Size::Wide => 1,
        Size::Two => usize::from(inbox),
    }
}

/// Is a task's detail at normal weight: what you read and act on is, the rest is dim.
fn loud(t: &TaskState) -> bool {
    items::in_inbox(t)
}

fn drawn_lines(m: &Mission, rows: &[Row], size: Size, w: usize, cur: Option<Item>) -> Vec<Drawn> {
    let mut out = Vec::new();
    for row in rows {
        match *row {
            Row::Head(h) => out.push(Drawn::Head(h)),
            Row::Gap => out.push(Drawn::Gap),
            Row::Repo(r) => out.push(Drawn::Repo(r)),
            Row::Task(i, inbox) => {
                out.push(Drawn::Title(i, inbox));
                let t = items::task(&m.fleet, i);
                let n = detail_lines(size, inbox, cur == Some(i));
                if n > 0 && !t.words.is_empty() {
                    for line in model::wrap(&t.words, w.saturating_sub(6), n) {
                        out.push(Drawn::Detail(i, line, loud(t)));
                    }
                }
            }
        }
    }
    out
}

/// The first line to show: keeps the selected task's lines in view, with the heading
/// above it when it's the first under one.
fn scroll(lines: &[Drawn], top: usize, avail: usize, cur: Option<Item>) -> usize {
    let mut top = top.min(lines.len().saturating_sub(avail));
    let Some(cur) = cur else { return top };
    let Some(first) = lines.iter().position(|l| matches!(l, Drawn::Title(i, _) if *i == cur)) else { return top };
    let last = lines.iter().rposition(|l| l.item() == Some(cur)).unwrap_or(first);
    let head = first > 0 && matches!(lines[first - 1], Drawn::Head(_) | Drawn::Repo(_));
    if first < top + usize::from(head) {
        top = first - usize::from(head);
    } else if last >= top + avail {
        top = (last + 1).saturating_sub(avail).min(first);
    }
    top
}

fn draw_list(buf: &mut Buffer, area: Rect, m: &mut Mission, size: Size, now: Now) {
    let (w, avail) = (area.width as usize, (area.height as usize).max(1));
    let rows = m.rows();
    let cur = m.current();
    let lines = drawn_lines(m, &rows, size, w, cur);
    if lines.is_empty() {
        put(buf, area, 0, 1, span("No repositories yet.", bold()));
        put(buf, area, 1, 1, span(fit("`bullpen up` in one starts its bullpen.", w.saturating_sub(2)), dim()));
        return;
    }
    m.top = scroll(&lines, m.top, avail, cur);
    let shown: Vec<&Drawn> = lines.iter().skip(m.top).take(avail).collect();
    m.page = shown.iter().filter(|l| matches!(l, Drawn::Title(..))).count().max(1);
    let m = &*m;
    // IDs share a column, and so do the inbox's repo names
    let idw = rows.iter().filter_map(|r| if let Row::Task(i, _) = r { Some(width(items::id(items::task(&m.fleet, *i)))) } else { None }).max().unwrap_or(0);
    let repow = m.fleet.repos.iter().map(|r| width(&r.repo.name())).max().unwrap_or(0).min(16);
    for (y, line) in shown.into_iter().enumerate() {
        match line {
            Drawn::Head(h) => put(buf, area, y, 1, span(*h, bold())),
            Drawn::Gap => {}
            Drawn::Repo(r) => put(buf, area, y, 0, repo_line(m, *r, w, size)),
            Drawn::Title(i, inbox) => {
                let t = items::task(&m.fleet, *i);
                bar(buf, area, y, t);
                put(buf, area, y, 1, task_mark(m, *i, t));
                let repo = if *inbox { m.fleet.repos[i.repo].repo.name() } else { String::new() };
                put(buf, area, y, 3, title_line(m, *i, t, &repo, repow, idw, cur == Some(*i), w, now));
            }
            Drawn::Detail(i, text, loud) => {
                let t = items::task(&m.fleet, *i);
                bar(buf, area, y, t);
                let style = if cur == Some(*i) {
                    Style::new().bg(TINT)
                } else if *loud {
                    Style::new()
                } else {
                    dim()
                };
                let room = w.saturating_sub(7);
                let text = if cur == Some(*i) { pad(&fit(text, room), room) } else { fit(text, room) };
                put(buf, area, y, 6, span(text, style));
            }
        }
    }
    if m.top + avail < lines.len() {
        put(buf, area, avail - 1, w.saturating_sub(2), span("↓", dim()));
    }
    if m.top > 0 {
        put(buf, area, 0, w.saturating_sub(2), span("↑", dim()));
    }
}

/// The amber (or red) bar beside a task that needs you, or whose agent went quiet.
fn bar(buf: &mut Buffer, area: Rect, y: usize, t: &TaskState) {
    let st = items::status(t);
    if matches!(st, "needs" | "stopped") || bullpen_core::words::is_stuck(&t.task) {
        let c = if st == "stopped" { Color::Red } else { AMBER };
        put(buf, area, y, 0, span("▌", Style::new().fg(c)));
    }
}

/// A task's mark: a spinner while a command runs on it, the board's moving marks for
/// working and in review, else its status's.
fn task_mark(m: &Mission, i: Item, t: &TaskState) -> Span<'static> {
    let st = items::status(t);
    let board = m.boards.get(&m.fleet.repos[i.repo].repo.dir);
    let still = bullpen_core::words::is_stuck(&t.task) || bullpen_core::words::is_limited(&t.task);
    if board.and_then(|b| b.job(items::id(t))).is_some() {
        span(frame_at(m.started, SPINNER, 100), bold())
    } else if still {
        span(t.mark, mark_style(if bullpen_core::words::is_stuck(&t.task) { "needs" } else { st }))
    } else if st == "working" {
        span(frame_at(m.started, WORKING, crate::ui::MARK_MS), Style::new())
    } else if st == "review" {
        span(frame_at(m.started, REVIEW, crate::ui::MARK_MS), mark_style("review"))
    } else {
        span(t.mark, mark_style(st))
    }
}

/// A task's line: (in the inbox) its repo, its ID, title and age at the right edge. Asked
/// y/n about, it says "y / n" for the age, in the question's colour.
#[allow(clippy::too_many_arguments)]
fn title_line(m: &Mission, i: Item, t: &TaskState, repo: &str, repow: usize, idw: usize, selected: bool, w: usize, now: Now) -> Line<'static> {
    let id = items::id(t).to_string();
    let board = m.boards.get(&m.fleet.repos[i.repo].repo.dir);
    let asked = board.and_then(|b| b.pending.as_ref()).filter(|(_, tid)| *tid == id);
    let age = if asked.is_some() { "y / n".to_string() } else { bullpen_core::words::ago(&t.task, Some(now)) };
    let all = w.saturating_sub(4);
    let lead = if repo.is_empty() { String::new() } else { format!("{} ", pad(&fit(repo, repow), repow)) };
    let title = bullpen_core::json::s(&t.task, "title");
    let room = all.saturating_sub(width(&lead) + idw + 2 + width(&age) + 2);
    let middle = pad(&format!("  {}", fit(title, room)), all.saturating_sub(width(&lead) + idw + width(&age)));
    let stale = matches!(items::status(t), "needs" | "stopped")
        && bullpen_core::task::stamp(&t.task).is_some_and(|s| bullpen_core::timeutil::seconds_since(s, Some(now)) >= model::STALE);
    let age_style = if stale { bold().fg(Color::Red) } else { dim() };
    let (lead_style, id_style, title_style, age_style) = match (asked, selected) {
        (Some((action, _)), _) => {
            let risky = board.and_then(|b| b.tasks.iter().find(|x| x.id == id).map(|x| model::confirm_risky(*action, x, &b.cfg)));
            let tone = if risky == Some(true) { Color::Red } else { Color::Green };
            (bold().fg(tone), rev().add_modifier(Modifier::BOLD).fg(tone), bold().fg(tone), bold().fg(tone))
        }
        (None, true) => (Style::new().bg(TINT), rev().add_modifier(Modifier::BOLD), Style::new().bg(TINT), age_style.bg(TINT)),
        (None, false) => (dim(), dim(), if items::status(t) == "triage" { dim() } else { Style::new() }, age_style),
    };
    let id_pad = " ".repeat(idw.saturating_sub(width(&id)));
    Line::from(vec![span(lead, lead_style), span(id, id_style), span(id_pad + &middle, title_style), span(age, age_style)])
}

/// A repo's line: its name, its root, its coordinators, and how many tasks are open at
/// the right ("bullpen  ~/dev/bullpen   c3 ● working   5 open"). Without room the root
/// goes, then the coordinators' words become marks.
fn repo_line(m: &Mission, r: usize, w: usize, size: Size) -> Line<'static> {
    let rs = &m.fleet.repos[r];
    let name = rs.repo.name();
    let root = items::tilde(&rs.repo.root_str());
    let open = items::open_words(rs);
    let room = w.saturating_sub(3 + width(&name) + width(&open) + 2);
    let full = items::coordinators_words(rs, false);
    let coords = if width(&full) <= room && size != Size::One { full } else { items::coordinators_words(rs, true) };
    let root = if width(&root) + 3 + width(&coords) <= room { root } else { String::new() };
    let gone = rs.coordinators.is_empty() || rs.coordinators.iter().any(|c| c.word() == "gone");
    let cstyle = if gone { Style::new().fg(AMBER) } else { dim() };
    let mid = if root.is_empty() { String::new() } else { format!("  {root}") };
    let fill = w.saturating_sub(1 + width(&name) + width(&mid) + 3 + width(&coords) + width(&open) + 1);
    Line::from(vec![
        span(format!(" {name}"), bold()),
        span(mid, dim()),
        span("   ", Style::new()),
        span(fit(&coords, room), cstyle),
        span(" ".repeat(fill), Style::new()),
        span(open, dim()),
    ])
}

/// The selected task's detail: its title, its repo, where it stands and what its agent
/// is doing, the size of its change and its tour, and what its worker said it did.
fn draw_detail(buf: &mut Buffer, area: Rect, m: &Mission, now: Now) {
    let w = area.width as usize;
    let Some((app, t, item)) = m.current_task() else {
        put(buf, area, 0, 1, span("Nothing open in any repository.", dim()));
        return;
    };
    let ts = items::task(&m.fleet, item);
    let rs = &m.fleet.repos[item.repo];
    let room = w.saturating_sub(2);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(span(fit(&format!("{}  {}", t.id, t.title), room), bold())));
    let place = format!("{} · {}", rs.repo.name(), items::tilde(&rs.repo.root_str()));
    lines.push(Line::from(span(fit(&place, room), dim())));
    lines.push(Line::default());
    let age = model::ago(t, now);
    let status = if age.is_empty() { status_word(&t.status).to_string() } else { format!("{} · {age}", status_word(&t.status)) };
    lines.push(Line::from(vec![span(format!("{} ", ts.mark), mark_style(&t.status)), span(status, bold())]));
    for (text, style) in detail_texts(app, t, ts, now) {
        lines.extend(model::wrap(&text, room, usize::MAX).into_iter().map(|l| Line::from(span(l, style))));
    }
    let summary = t.summary.as_deref().unwrap_or("").trim();
    if !summary.is_empty() && matches!(t.status.as_str(), "review" | "ready" | "finished") {
        lines.push(Line::default());
        lines.push(Line::from(span("What its worker said", bold())));
        let wrapped = summary.lines().flat_map(|l| model::wrap(l, room, usize::MAX));
        lines.extend(wrapped.map(|l| Line::from(span(l, dim()))));
    }
    for (y, l) in lines.into_iter().take(area.height as usize).enumerate() {
        put(buf, area, y, 1, l);
    }
}

/// What the detail says under the status, each with its style: what its agent is doing
/// now, where it stands (with auto-accept and the model it chose), the size of its
/// change and its tour.
fn detail_texts(app: &App, t: &Task, ts: &TaskState, now: Now) -> Vec<(String, Style)> {
    let loud = if matches!(t.status.as_str(), "needs" | "stopped" | "ready" | "finished") { Style::new() } else { dim() };
    let mut out = Vec::new();
    if !ts.doing.is_empty() {
        out.push((ts.doing.clone(), Style::new()));
    }
    let detail = model::detail_with_auto(t, &app.cfg, now);
    if !detail.is_empty() {
        out.push((detail, loud));
    }
    let stat = model::stat_line(t);
    if !stat.is_empty() {
        out.push((stat, dim()));
    }
    out
}

/// A status in a few words, for the detail.
fn status_word(st: &str) -> &str {
    match st {
        "needs" => "needs you",
        "stopped" => "stopped: its agent exited",
        "triage" => "its coordinator is on it",
        "ready" => "ready to accept",
        "finished" => "finished",
        "review" => "in review",
        "working" => "working",
        "todo" => "queued",
        other => other,
    }
}

/// The message rows above the keys: a y/n being asked, what just happened, else what's
/// still running in any repo.
fn message_rows(m: &Mission, w: usize) -> Vec<Span<'static>> {
    if let Some(app) = m.active().filter(|a| a.pending.is_some()) {
        return message(app, w, MESSAGE_LINES);
    }
    if !m.msg.is_empty() {
        let style = match m.msg.kind {
            Kind::Failed => bold().fg(Color::Red),
            Kind::Done => bold().fg(Color::Green),
            Kind::Info => bold(),
        };
        return model::wrap(&m.msg.text, w.saturating_sub(2), MESSAGE_LINES).into_iter().map(|l| span(l, style)).collect();
    }
    let running: Vec<(String, &'static str)> =
        m.boards.values().flat_map(|a| a.running.iter().map(|(id, job)| (id.clone(), job.doing()))).collect();
    match running.first() {
        Some((id, doing)) => {
            let more = if running.len() > 1 { format!(" (+{} more)", running.len() - 1) } else { String::new() };
            vec![span(fit(&format!("{doing} {id}…{more}"), w.saturating_sub(2)), dim())]
        }
        None => Vec::new(),
    }
}

/// The message rows, then the last row: a line being typed, what y does, or the keys.
/// The cursor's column when a line is typed there.
fn bottom(buf: &mut Buffer, area: Rect, m: &Mission, msg: Vec<Span<'static>>) -> Option<usize> {
    let (w, h) = (area.width as usize, area.height as usize);
    let last = h.saturating_sub(1);
    for (j, l) in msg.iter().enumerate() {
        put(buf, area, (last + j).saturating_sub(msg.len()), 1, l.clone());
    }
    if m.quitting {
        let texts = ["y quits mission control   any other key stays", "y quits   n stays"].map(String::from);
        put(buf, area, last, 1, span(model::first_fit(&texts, w.saturating_sub(2)), bold()));
        return None;
    }
    if let Some(app) = m.active() {
        if let Mode::Tell { id, text, .. } = &app.mode {
            return Some(input_line(buf, area, last, &format!("Tell {id}:"), text));
        }
        if let Mode::Stale { .. } = &app.mode {
            put(buf, area, last, 1, span(fit("n new tour   o the old one   esc cancels", w.saturating_sub(2)), bold()));
            return None;
        }
        if let Some(line) = pending_keys(app, w) {
            put(buf, area, last, 1, line);
            return None;
        }
    }
    put(buf, area, last, 1, key_row(m, w));
    None
}

/// The selected task's keys (the board's, but p: the look is a later screen), then n
/// and / and q. Keys drop from the task's end until the row fits.
fn key_row(m: &Mission, w: usize) -> Line<'static> {
    let mut keys: Vec<(&str, &str)> = m.current_task().map_or(Vec::new(), |(_, t, _)| model::task_keys(Some(t)));
    keys.retain(|(k, _)| *k != "p");
    let own = [("n", "next"), ("/", "find"), ("q", "quit")];
    loop {
        let all: Vec<(&str, &str)> = keys.iter().chain(own.iter()).copied().collect();
        let line = key_line(&all);
        if line.width() + 2 <= w || keys.is_empty() {
            return line;
        }
        keys.pop();
    }
}

#[cfg(test)]
#[path = "test_draw.rs"]
mod tests;
