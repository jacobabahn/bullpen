//! Helpers the board's drawing tests share.

use crate::model::{Config, Layout, Task};
use crate::ui::draw;
use crate::App;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;

pub(crate) fn app(tasks: Vec<Task>, gap: usize, layout: Layout) -> App {
    let mut a = App::for_test(tasks);
    a.cfg = Config { bottom_gap: gap, layout, ..Config::default_for_test() };
    a
}

pub(crate) fn ready(id: &str) -> Task {
    Task { id: id.into(), title: format!("Task {id}"), status: "ready".into(), ..Default::default() }
}

/// The board's rows as text, trailing spaces cut.
pub(crate) fn screen(app: &mut App, w: u16, h: u16) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
        .collect()
}

pub(crate) fn row(s: &[String], from_bottom: usize) -> &str {
    &s[s.len() - 1 - from_bottom]
}

/// The colour of the text's first cell.
pub(crate) fn tone_of(app: &mut App, w: u16, h: u16, text: &str) -> Option<Color> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    let s: Vec<String> = (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect()).collect();
    let y = s.iter().position(|r| r.contains(text))?;
    let x = s[y][..s[y].find(text)?].chars().count();
    Some(buf[(x as u16, y as u16)].fg)
}

pub(crate) const QUESTION: &str = "Pick: candidate 4 (one rule for who sees to a task), 3 (reviews that ask before they block), or both. 4 changes how triage works for every coordinator; 3 only touches the reviewer's prompt and is easy to undo.";

pub(crate) fn asking(id: &str) -> Task {
    Task { status: "needs".into(), ask: Some(true), why: Some(QUESTION.into()), title: "Architecture round 2: review".into(), ..ready(id) }
}

pub(crate) fn asks(id: &str) -> Task {
    Task { status: "needs".into(), why: Some("Asks: python3 or python?".into()), ..ready(id) }
}
