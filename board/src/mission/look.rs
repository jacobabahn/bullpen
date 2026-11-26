//! Mission control in the image look (`board_look image`, docs/mission.md): the frame's
//! values for the board's image code (image/), which draws, sends and places them as
//! it does the board's. The header is mission control's own (image/fleet.rs: its counts
//! as chips, and the fleet's bar, a run of segments for each repository with its
//! coordinator's mark and name); the list is the board's rows (ui/rows.rs), every
//! repository's tasks drawn as its board draws them, with a heading for each repository
//! (`Row::Repo`) and the inbox's tasks naming theirs; beside the list, from 80 columns,
//! the selected task's page (image/page.rs); the bottom rows are the message and the
//! keys, as on the board. What's typed or asked at the bottom, a peek and an answer stay
//! text. j and k only place images: the list is the board's pieces, and drawing ahead
//! presses them here off screen (`Host::moved`).

use super::draw::{self, list_width, message_rows, status_word, Size};
use super::items::{self, Row as MRow};
use super::Mission;
use crate::app::Note;
use crate::image::{fleet, Fleet, FleetRepo, Host, Ink, Page, Side, View};
use crate::model::{self, Look, TextSize};
use crate::ui::bottom::{Foot, FootLine, Tone};
use crate::ui::list::Head;
use crate::ui::rows::{self, RepoRow, Row, Rows};
use bullpen_core::fleet::RepoState;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::text::Span;
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::path::PathBuf;

type Now = bullpen_core::timeutil::Stamp;

/// The fewest rows the list may have as images: fewer, and mission control is text.
const LEAST_ROWS: u16 = 3;

/// The frame's view for the images, in `area`, with these message rows; None when the
/// pane is too short for them. The bottom rows are values unless something is typed or
/// asked there (then they're text, `foot` None).
pub(super) fn view(m: &Mission, area: Rect, msg: &[Span<'static>], now: Now) -> Option<View> {
    let (w, h) = (area.width as usize, area.height);
    let size = Size::of(w);
    let fleet = header(m, w, size, now);
    let head_n = fleet::rows(fleet.lines.len());
    let foot_n = 1 + msg.len().max(1) as u16;
    let rows_n = h.checked_sub(head_n + foot_n).filter(|n| *n >= LEAST_ROWS)?;
    // the list's columns (one between it and the page), and the page's
    let lw = if size == Size::One { w } else { list_width(w, size) };
    let cols = lw.saturating_sub(1).max(1) as u16;
    let side = (size != Size::One).then(|| page(m, now)).flatten().map(|page| Side { col: lw as u16, cols: w.saturating_sub(lw + 1).max(1) as u16, page });
    Some(View {
        rows: list(m, true, now),
        base: list(m, false, now),
        top: m.top,
        head: Head::default(),
        foot: foot(m, foot_n, msg),
        col: area.x,
        row0: area.y,
        row: area.y + head_n,
        rows_n,
        foot_n,
        cols,
        full: w.saturating_sub(1).max(1) as u16,
        fleet: Some(fleet),
        side,
    })
}

/// The list as the board's rows: the inbox's parts (needs you, then ready), each task
/// naming its repository; then each repository's heading and its other open tasks.
/// `selection`: with the selected task selected (else none is: the list's base image).
pub(super) fn list(m: &Mission, selection: bool, now: Now) -> Rows {
    let all = m.rows();
    let cur = m.current().filter(|_| selection);
    let mut out = Vec::new();
    if all.is_empty() {
        out.push(Row::Empty { text: "No repositories yet.".into(), first: true });
        out.push(Row::Empty { text: "`bullpen up` in one starts its bullpen.".into(), first: false });
    }
    for (j, row) in all.iter().enumerate() {
        match *row {
            MRow::Head(h) => {
                if j > 0 {
                    out.push(Row::Gap);
                }
                let count = all[j + 1..].iter().take_while(|r| matches!(r, MRow::Task(_, true))).count();
                let name = if h == items::INBOX[0].0 { "Needs you" } else { "Ready to accept" };
                out.push(Row::Head { name: name.into(), count });
            }
            MRow::Gap => out.push(Row::Gap),
            MRow::Repo(r) => out.push(Row::Repo(repo_row(&m.fleet.repos[r]))),
            MRow::Task(i, inbox) => out.extend(task_rows(m, i, inbox, cur == Some(i), now)),
        }
    }
    Rows { rows: out, idw: 0 }
}

/// A task's row and its lines, as its repository's board draws them; in the inbox, its
/// repository's name before its title; selected, ⏎ in its card's corner, by the name
/// the footer and the page give it.
fn task_rows(m: &Mission, i: items::Item, inbox: bool, selected: bool, now: Now) -> Vec<Row> {
    let r = &m.fleet.repos[i.repo];
    let Some(app) = m.boards.get(&r.repo.dir) else { return vec![] };
    let id = items::id(items::task(&m.fleet, i));
    let Some(t) = app.tasks.iter().find(|t| t.id == id) else { return vec![] };
    let mut row = rows::task_row(app, t, selected, true, now);
    if inbox {
        row.repo = r.repo.name();
    }
    row.dir = r.repo.dir.display().to_string();
    if selected && row.asked.is_none() {
        row.key = Some(("⏎".into(), model::enter_name(t).into()));
    }
    let mut out = vec![Row::Task(row)];
    rows::details(&mut out, app, t, selected, false, usize::MAX, now);
    out
}

/// A repository's heading: its name, its folder and how many are open, its
/// coordinators, and a blocked one's question.
fn repo_row(rs: &RepoState) -> RepoRow {
    let coordinators = if rs.coordinators.is_empty() {
        vec![("no coordinator".to_string(), "gone")]
    } else {
        rs.coordinators.iter().map(|c| (format!("{} {}", c.id, c.word()), c.word())).collect()
    };
    let question = rs.coordinators.iter().find(|c| c.word() == "blocked").map(|c| match c.text.trim() {
        "" => format!("{} is waiting on you in its pane.", c.id),
        q => format!("{} asks: {q}", c.id),
    });
    RepoRow { name: rs.repo.name(), place: format!("{} · {}", items::tilde(&rs.repo.root_str()), items::open_words(rs)), coordinators, question }
}

/// The header: the name, what's under way after it, the chips, the time, and the fleet's
/// bar packed into lines `w` columns wide.
fn header(m: &Mission, w: usize, size: Size, now: Now) -> Fleet {
    let t = items::totals(&m.fleet);
    let mut chips = Vec::new();
    if t.blocked > 0 {
        let words = if t.blocked == 1 { "1 coordinator blocked".into() } else { format!("{} coordinators blocked", t.blocked) };
        chips.push((words, format!("c{}", t.blocked), "needs"));
    }
    if t.needs > 0 {
        chips.push((bullpen_core::words::count_phrase(t.needs, "needs"), t.needs.to_string(), "needs"));
    }
    if t.ready > 0 {
        chips.push((bullpen_core::words::count_phrase(t.ready, "ready"), t.ready.to_string(), "ready"));
    }
    let (name, clock) = match size {
        Size::One => ("Mission", String::new()),
        _ => ("Mission control", now.format("%a %-d %b %H:%M").to_string()),
    };
    let repos = m.fleet.repos.iter().map(fleet_repo).collect();
    Fleet { name: name.into(), after: under_way(t, size), chips, clock, lines: fleet::lines(repos, w) }
}

/// What's under way, after the name: "3 of 9 running · 4 repos" (on a narrow pane,
/// "3/9 · 4 repos").
fn under_way(t: items::Totals, size: Size) -> String {
    let repos = if t.repos == 1 { "1 repo".to_string() } else { format!("{} repos", t.repos) };
    match (t.open, size) {
        (0, _) => repos,
        (_, Size::One) => format!("{}/{} · {repos}", t.running, t.open),
        _ if t.running == 0 => format!("{} open · {repos}", t.open),
        _ => format!("{} of {} running · {repos}", t.running, t.open),
    }
}

/// A repository in the bar: its name, its coordinators' state as one (what most needs
/// you first), its open tasks' statuses, and what needs you or is ready there.
fn fleet_repo(rs: &RepoState) -> FleetRepo {
    let words: Vec<&str> = rs.coordinators.iter().map(|c| c.word()).collect();
    let word = ["blocked", "gone", "working", "unknown"].into_iter().find(|w| words.contains(w));
    let word = if words.is_empty() { "gone" } else { word.unwrap_or("idle") };
    let said = match (rs.counts.needs, rs.counts.ready) {
        (0, 0) => String::new(),
        (a, 0) => bullpen_core::words::count_phrase(a, "needs"),
        (0, b) => format!("{b} ready"),
        (a, b) => format!("{} · {b} ready", bullpen_core::words::count_phrase(a, "needs")),
    };
    let tasks = rs.open.iter().map(|t| bar_status(items::status(t))).collect();
    FleetRepo { name: rs.repo.name(), word, tasks, said }
}

/// A status as the bar's colours know it.
fn bar_status(st: &str) -> &'static str {
    match st {
        "needs" => "needs",
        "stopped" => "stopped",
        "ready" => "ready",
        "finished" => "finished",
        "review" => "review",
        "triage" => "triage",
        "working" => "working",
        _ => "todo",
    }
}

/// The selected task's page: where it is, its title, status and age, its keys (`p` and
/// `⏎` first), what its agent is doing and where it stands, and what its worker said.
fn page(m: &Mission, now: Now) -> Option<Page> {
    let (app, t, item) = m.current_task()?;
    let ts = items::task(&m.fleet, item);
    let rs = &m.fleet.repos[item.repo];
    let mut keys: Vec<(String, String)> = vec![("p".into(), "peek".into()), ("⏎".into(), model::enter_name(t).into())];
    let own = model::task_keys(Some(t)).into_iter().filter(|(k, _)| !matches!(*k, "p" | "⏎"));
    keys.extend(own.map(|(k, what)| (k.to_string(), what.to_string())));
    let mut lines = Vec::new();
    if !ts.doing.is_empty() {
        lines.push((ts.doing.clone(), Ink::Text));
    }
    let detail = model::detail_with_auto(t, &app.cfg, now);
    if !detail.is_empty() {
        lines.push((detail, if t.needs_you() || t.awaits_accept() { Ink::Soft } else { Ink::Dim }));
    }
    let stat = model::stat_line(t);
    if !stat.is_empty() {
        lines.push((stat, Ink::Dim));
    }
    let summary = t.summary.as_deref().unwrap_or("").trim();
    let said = if matches!(t.status.as_str(), "review" | "ready" | "finished") { paragraphs(summary) } else { vec![] };
    Some(Page {
        id: t.id.clone(),
        place: format!("{} · {}", rs.repo.name(), items::tilde(&rs.repo.root_str())),
        title: t.title.clone(),
        mark: rows::task_row(app, t, false, true, now).mark,
        status: t.status.clone(),
        words: status_word(&t.status).to_string(),
        age: model::ago(t, now),
        keys,
        lines,
        said,
    })
}

/// A worker's summary as paragraphs: each non-empty line, without Markdown's heading
/// marks and its bold's asterisks, which the text shows as they are.
fn paragraphs(text: &str) -> Vec<String> {
    let plain = |l: &str| l.trim().trim_start_matches('#').trim().replace("**", "");
    text.lines().map(plain).filter(|l| !l.is_empty()).collect()
}

/// The bottom rows as values, `n` rows: the message's rows over the keys, the selected
/// task's then mission control's own. None while something is typed or asked there.
fn foot(m: &Mission, n: u16, msg: &[Span<'static>]) -> Option<Foot> {
    let asked = m.active().is_some_and(|a| a.pending.is_some() || !matches!(a.mode, crate::app::Mode::Normal));
    if m.quitting || asked {
        return None;
    }
    let last = n - 1;
    let tone = |sp: &Span| match (sp.style.fg, sp.style.add_modifier.contains(Modifier::DIM)) {
        (Some(Color::Red), _) => Tone::Red,
        (Some(Color::Green), _) => Tone::Green,
        (_, true) => Tone::Dim,
        _ => Tone::Plain,
    };
    let first = last.saturating_sub(msg.len() as u16);
    let mut lines: Vec<(u16, FootLine)> =
        msg.iter().enumerate().map(|(j, s)| (first + j as u16, FootLine::Msg(s.content.to_string(), tone(s)))).collect();
    let pair = |(k, what): (&str, &str)| (k.to_string(), what.to_string());
    let task = m.current_task().map_or(vec![], |(_, t, _)| model::task_keys(Some(t)).into_iter().map(pair).collect());
    let own = draw::OWN_KEYS.iter().copied().map(pair).collect();
    lines.push((last, FootLine::Mission(task, own)));
    Some(Foot { lines })
}

/// Mission control draws for the image look as its frame laid it out (draw.rs), in the
/// global settings' type; drawing ahead presses j and k on it.
impl Host for Mission {
    fn look(&self) -> Look {
        self.look
    }

    fn style(&self) -> (TextSize, Option<PathBuf>, bool) {
        (self.cfg.text_size, self.cfg.font.clone(), self.cfg.motion)
    }

    fn started(&self) -> std::time::Instant {
        self.started
    }

    fn view(&self) -> Option<View> {
        self.view.clone()
    }

    fn placed(&mut self, top: usize, page: usize) {
        (self.top, self.page) = (top, page);
    }

    /// Back to text, saying why on the message line.
    fn fall_back(&mut self, why: String) {
        self.look = Look::Text;
        self.msg = Note::info(why);
    }

    fn drawable(&self) -> bool {
        true
    }

    fn at_rest(&self) -> bool {
        !self.quitting && !self.board_open()
    }

    /// Press `key` on mission control and draw its frame off screen; then put back what
    /// the key changed (the selection, the message, the list's first row).
    fn moved(&mut self, key: char, area: Rect) -> Option<View> {
        let saved = (self.sel.clone(), self.sel_at, self.msg.clone(), self.view.take(), self.top, self.height);
        self.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
        let view = if self.sel != saved.0 {
            let viewport = Viewport::Fixed(area);
            let term = Terminal::with_options(ratatui::backend::TestBackend::new(area.width, area.height), TerminalOptions { viewport });
            term.ok().and_then(|mut t| t.draw(|f| draw::draw(f, self)).ok().map(|_| ())).and_then(|_| self.view.take())
        } else {
            None
        };
        (self.sel, self.sel_at, self.msg, self.view, self.top, self.height) = saved;
        view
    }
}

/// `--render FILE`: mission control as the image look draws it, `cols` columns wide
/// (the header, the whole list with the page beside it, the keys), at a cell of `cell`
/// pixels, in `theme`'s colours.
pub fn render(m: &mut Mission, cols: u16, cell: (u32, u32), theme: crate::image::Theme, path: &std::path::Path) -> Result<(), String> {
    m.image = true;
    for app in m.boards.values_mut() {
        app.image = true;
    }
    let now = model::now();
    let area = Rect { x: 0, y: 0, width: cols, height: u16::MAX / 2 };
    let mut view = view(m, area, &message_rows(m, cols as usize), now).ok_or("the pane is too short")?;
    // the whole list: the page beside it as tall as it says
    view.rows_n = u16::MAX / 4;
    crate::image::render_file(&view, m.cfg.text_size, cell, theme, m.cfg.font.as_deref(), path)
}

#[cfg(test)]
#[path = "tests/test_look.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/test_bench.rs"]
mod bench;
