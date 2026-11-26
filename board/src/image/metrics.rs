//! The image look's type sizes and the room around them, in pixels, for a text size
//! and a cell's height (render.rs lays the rows out with them): a title's, a detail's,
//! a label's and the lines under a task's sizes, each line's height, and the room over
//! a task, inside a card and between groups. Small, medium and large are one look at
//! three sizes.

use crate::model::TextSize;

/// The type's sizes and the space it gets, in pixels, for a text size and a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// a task's title, the top line
    pub title: f32,
    /// a detail line, a task's ID and age, a group's name and the message
    pub detail: f32,
    /// a label: a chip's word (a task's risk, the header's counts) and a key in its cap
    pub label: f32,
    /// a title line's height, and a detail line's
    pub line: f32,
    pub detail_line: f32,
    /// the lines under a task: their type, a size under a detail's (the ID's, the
    /// age's), and their height
    pub body: f32,
    pub body_line: f32,
    /// the space above a task's (or an item's) first line: what's between two cards,
    /// and the card's room over its type (`inset`); and the space above a heading
    pub pad: f32,
    pub head_pad: f32,
    /// the space between groups: more than is under a group's name, so the name sits
    /// with its own tasks
    pub gap: f32,
    /// the space between a title's last line and the task's first detail line
    pub title_gap: f32,
    /// the room a card (the selected task's, or a wash) has inside it over its block's
    /// first line and under its last (a little more: a line's type sits low in it,
    /// with its descenders). The room under is part of the block's last row, so a card
    /// ends where its block does
    pub inset: f32,
    pub below: f32,
}

impl Metrics {
    /// Every size sets its type at a share of a cell's height (medium's title about a
    /// terminal's own type) and gives the rows the height it needs: a detail smaller
    /// than a title, and room, a little between a task's title and its lines, a card's
    /// room round its type, and a few pixels between one card and the next. Small is
    /// medium's look in smaller type, so more tasks fit. A label is smaller than a
    /// detail at every size.
    pub fn of(size: TextSize, cell_h: u32) -> Metrics {
        let ch = cell_h as f32;
        let (t, d, l) = match size {
            TextSize::Small => (0.68, 0.61, 0.52),
            // a terminal's own type is about 0.8 of its cell's height
            TextSize::Medium => (0.80, 0.70, 0.60),
            TextSize::Large => (0.92, 0.80, 0.69),
        };
        let (title, detail, label) = ((t * ch).max(6.0), (d * ch).max(6.0), (l * ch).max(5.0));
        // a task's lines are smaller than its title by two steps, after a gap, so the
        // two read apart though they start at the same edge
        let body = (BODY * detail).max(6.0);
        Metrics {
            title,
            detail,
            label,
            line: (1.40 * title).round(),
            detail_line: (1.42 * detail).round(),
            body,
            body_line: (1.42 * body).round(),
            pad: (BETWEEN * title).round() + (ROOM * title).round(),
            head_pad: (0.5 * title).round(),
            gap: (0.5 * ch).round(),
            title_gap: (0.22 * title).round(),
            inset: (ROOM * title).round(),
            below: (ROOM * title).round() + (0.32 * title).round(),
        }
    }
}

/// A card's room over its type, of a title's size (its line has as much again over its
/// capitals), and the space between two cards.
const ROOM: f32 = 0.38;
const BETWEEN: f32 = 0.42;

/// The type of the lines under a task, of a detail's size (medium's is 0.62 of a cell).
const BODY: f32 = 0.886;
