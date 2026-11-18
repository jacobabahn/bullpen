//! The Later group, last under Queued: the backlog's open items, folded to one line
//! ("Later · 6 · next: ..."), amber when something there waits on you, or with `L` a
//! row per item (`?` amber for one that asks, `◇` dim for a kept one), the selected one
//! with a line saying what its keys do, red while its drop is asked about.

use super::rows::{item_row, ItemRow, Row};
use super::{bold, dim, put, rev, span, spinner, AMBER, TINT};
use crate::model::{self, fit, pad, Item, Layout};
use crate::App;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

/// How many lines the selected item's detail gets.
const ITEM_LINES: usize = 2;

/// The Later group's rows (none for an empty backlog), after a blank row in the
/// grouped layout when something comes before.
pub(super) fn later_rows(app: &App, items: &[Item], after_rows: bool, detail_w: usize) -> Vec<Row> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    if after_rows && app.cfg.layout == Layout::Grouped {
        rows.push(Row::Gap);
    }
    if !app.later_open {
        let (line, amber) = model::later_line(items);
        rows.push(Row::Later { text: line, amber });
        return rows;
    }
    rows.push(Row::Head { name: "Later".into(), count: items.len() });
    for i in items {
        let row = item_row(app, i);
        let selected = row.selected;
        rows.push(Row::Item(row));
        if selected {
            for line in model::wrap(&model::item_detail(i), detail_w, ITEM_LINES) {
                rows.push(Row::ItemDetail(line));
            }
        }
    }
    rows
}

/// The folded line: "Later" bold, then the count and what's next, dim; all amber
/// when something there waits on you. The compact layout's is dim, as its headings are.
pub(super) fn draw_later_line(buf: &mut Buffer, area: Rect, y: usize, app: &App, text: &str, amber: bool, w: usize) {
    let text = fit(text, w.saturating_sub(4));
    let (head, rest) = text.split_at(text.find(" ·").unwrap_or(text.len()));
    let (head_style, rest_style) = match (amber, app.cfg.layout) {
        (true, _) => (bold().fg(AMBER), Style::new().fg(AMBER)),
        (false, Layout::Compact) => (dim(), dim()),
        (false, Layout::Grouped) => (bold(), dim()),
    };
    put(buf, area, y, 1, Line::from(vec![span(head, head_style), span(rest, rest_style)]));
}

/// An item's row: its mark (a spinner while it starts), its ID and title; the
/// selected one's ID reversed on the tint, as a task's.
pub(super) fn draw_item(buf: &mut Buffer, area: Rect, y: usize, app: &App, i: &ItemRow, idw: usize, w: usize) {
    let mark = match (i.job, i.asks) {
        (true, _) => span(spinner(app), bold()),
        (false, true) => span(i.mark, bold().fg(AMBER)),
        (false, false) => span(i.mark, dim()),
    };
    put(buf, area, y, 1, mark);
    let width_all = w.saturating_sub(5);
    let title = fit(&i.title, width_all.saturating_sub(idw + 2));
    let rest = pad(&format!("{}  {title}", " ".repeat(idw.saturating_sub(model::width(&i.id)))), width_all.saturating_sub(model::width(&i.id)));
    let (id_style, rest_style) = if i.asked {
        (rev().add_modifier(Modifier::BOLD).fg(Color::Red), bold().fg(Color::Red))
    } else if i.selected {
        (rev().add_modifier(Modifier::BOLD), Style::new().bg(TINT))
    } else {
        (dim(), Style::new())
    };
    let rest = if i.selected || i.asked { rest } else { rest.trim_end().to_string() };
    put(buf, area, y, 3, Line::from(vec![span(i.id.clone(), id_style), span(rest, rest_style)]));
}

/// The selected item's line: on the tint, as the selected task's are.
pub(super) fn draw_item_detail(buf: &mut Buffer, area: Rect, y: usize, text: &str, detail_w: usize) {
    put(buf, area, y, 3, span(pad(&fit(text, detail_w), detail_w), Style::new().bg(TINT)));
}

#[cfg(test)]
#[path = "tests/test_later.rs"]
mod tests;
