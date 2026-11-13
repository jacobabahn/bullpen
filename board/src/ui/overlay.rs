//! What takes the whole board for a while: a text or a peek at a worker's screen, the
//! prompt a task's agent is at, and a coordinator's question with its answer line.

use super::bottom::{input_line, key_line};
use super::{bold, dim, put, span};
use crate::edit::LineEdit;
use crate::model::{self, fit, width};
use ansi_to_tui::IntoText;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

/// A full-screen text, or a peek at a worker's screen (`peek`): its title, its lines,
/// and a foot. A peek that can't read the screen (`failed`) says why, in bold.
pub(super) fn draw_overlay(buf: &mut Buffer, area: Rect, h: usize, title: &str, lines: &[String], peek: bool, failed: bool) {
    let w = area.width as usize;
    put(buf, area, 0, 1, span(fit(title, w.saturating_sub(2)), bold()));
    let failed = peek && failed;
    for (j, line) in lines.iter().take(h.saturating_sub(3)).enumerate() {
        let line = if failed { Line::from(span(line.clone(), bold())) } else { screen_line(line) };
        put(buf, area, 2 + j, 1, line);
    }
    let foot = if peek { peek_foot(w, !failed) } else { Line::from(span("any key returns", dim())) };
    put(buf, area, h.saturating_sub(1), 1, foot);
}

/// A line of a worker's screen, with the colours its escape codes give it.
fn screen_line(raw: &str) -> Line<'static> {
    match raw.into_text() {
        Ok(text) => text.lines.into_iter().next().unwrap_or_default(),
        Err(_) => Line::from(raw.to_string()),
    }
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

/// `i` on a coordinator's question: the task, the whole question wrapped, then the
/// line the answer is typed on. Returns the cursor's column and row.
pub(super) fn draw_ask(buf: &mut Buffer, area: Rect, h: usize, title: &str, question: &str, text: &LineEdit) -> (usize, usize) {
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

/// The prompt a task's agent is at: the end of its screen, then its options, one a
/// line, each with the key that presses it.
pub(super) fn draw_answer(buf: &mut Buffer, area: Rect, h: usize, title: &str, prompt: Option<&model::Prompt>) {
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

#[cfg(test)]
#[path = "test_overlay.rs"]
mod tests;
