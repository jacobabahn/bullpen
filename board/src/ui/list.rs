//! The board's list: the header line, the rows of tasks under their groups, scrolled
//! to keep the selected task in view, and the compact layout's detail panel. The
//! Later group's rows are later.rs's. What each row says is rows.rs's.

use super::later::{draw_item, draw_item_detail, draw_later_line};
use super::rows::{self, DetailRow, Flag, Mark, Now, Row, Rows, TaskRow};
use super::{bold, dim, mark_style, put, rev, review_mark, risk_style, span, spinner, working_mark, Sizes, AMBER, TINT};
use crate::model::{self, fit, pad, width, Layout, Task};
use crate::image::{meter, View};
use crate::App;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// The header, the list and its scroll marks. Keeps the selected task in view, and
/// sets `app.top` and `app.page`; with the image look, hands them to image/ instead.
pub(super) fn draw_list(buf: &mut Buffer, area: Rect, app: &mut App, s: &Sizes, now: Now) {
    let w = s.w;
    let groups = app.groups();
    let head = head(app, &groups, w);
    let Rows { rows, idw } = meter::time(|| "rows".into(), || rows::build(app, w, s.narrow, now));
    if app.image {
        // the image look draws the header, the rows and the scroll marks, in images over
        // these blank cells, and scrolls the list by its rows' own heights, from the row
        // shown first last frame (image/ sets `app.top` and `app.page`). Its images take
        // every column but the last, as the text does
        // its header takes the rows above the list (three on a tall board, for its
        // status bar: the text's ↓ row comes off the bottom rows)
        let (row, rows_n) = (area.y + s.chrome as u16, s.avail as u16);
        let foot_n = s.h.saturating_sub(s.chrome + s.avail) as u16;
        let cols = w.saturating_sub(1) as u16;
        let rows = Rows { rows, idw };
        let base = meter::time(|| "rows".into(), || rows::unselected(app, w, s.narrow, now));
        app.view = Some(View { rows, base, top: app.top, head, foot: None, col: area.x, row0: area.y, row, rows_n, foot_n, cols });
        return;
    }
    draw_head(buf, area, &head, w);
    let top = scroll(&rows, app.top, s.avail);
    app.top = top;
    // a page, for PgDn and PgUp: the tasks (and items) this list has room for
    app.page = rows.iter().skip(top).take(s.avail).filter(|r| r.is_entry()).count().max(1);
    let app = &*app;
    for (j, row) in rows.iter().skip(top).take(s.avail).enumerate() {
        draw_row(buf, area, app, s, 2 + j, row, idw);
    }
    draw_marks(buf, area, s, top, rows.len());
}

/// The scroll marks: ↑ above the list when rows are above, ↓ under it (a short board's
/// at the end of its last row) when rows are below.
fn draw_marks(buf: &mut Buffer, area: Rect, s: &Sizes, top: usize, len: usize) {
    let w = s.w;
    if top + s.avail < len {
        // a short board's goes at the end of the list's last row, past the ages
        let (y, x) = if s.short { (1 + s.avail, w.saturating_sub(2)) } else { (2 + s.avail, w.saturating_sub(3)) };
        put(buf, area, y, x, span("↓", dim()));
    }
    if top > 0 {
        put(buf, area, 1, w.saturating_sub(3), span("↑", dim()));
    }
}

/// One row of the list, as text, at line y.
fn draw_row(buf: &mut Buffer, area: Rect, app: &App, s: &Sizes, y: usize, row: &Row, idw: usize) {
    let (w, layout) = (s.w, app.cfg.layout);
    // from under the ID to the right edge
    let detail_w = w.saturating_sub(5);
    if let Row::Task(TaskRow { flag: Some(f), .. }) | Row::Detail(DetailRow { flag: Some(f), .. }) = row {
        let c = if *f == Flag::Stopped { Color::Red } else { AMBER };
        put(buf, area, y, 0, span("▌", Style::new().fg(c)));
    }
    match row {
        Row::Head { name, count } if layout == Layout::Compact => {
            put(buf, area, y, 1, span(fit(&format!("{name}  {count}"), w.saturating_sub(4)), dim()))
        }
        Row::Head { name, count } => put(buf, area, y, 1, span(format!("{name}  {count}"), bold())),
        Row::Task(t) => {
            put(buf, area, y, 1, mark_span(app, &t.mark));
            if t.from_backlog {
                put(buf, area, y, 2, span("↻", dim()));
            }
            put(buf, area, y, 3, title_line(t, w, idw, s.narrow));
        }
        Row::Detail(d) => {
            // the selected task's lines sit on the tint, its whole block, and drop dim as its title does
            let style = if d.selected {
                Style::new().bg(TINT)
            } else if d.loud {
                Style::new()
            } else {
                dim()
            };
            let text = fit(&d.text, detail_w);
            let text = if d.selected { pad(&text, detail_w) } else { text };
            let (age, rest) = model::split_at_width(&text, d.lead);
            put(buf, area, y, 3, Line::from(vec![span(age, age_style(d.stale, d.selected)), span(rest, style)]));
        }
        Row::Gap => {}
        Row::Empty { text, first } => put(buf, area, y, 1, span(fit(text, w.saturating_sub(2)), if *first { bold() } else { dim() })),
        Row::Later { text, amber } => draw_later_line(buf, area, y, app, text, *amber, w),
        Row::Item(i) => draw_item(buf, area, y, app, i, idw, w),
        Row::ItemDetail(text, _) => draw_item_detail(buf, area, y, text, detail_w),
    }
}

/// A task's mark as text: the working and review marks turn, as does the spinner.
fn mark_span(app: &App, mark: &Mark) -> Span<'static> {
    match mark {
        Mark::Job => span(spinner(app), bold()),
        Mark::Tour(st) => span(spinner(app), mark_style(st)),
        Mark::Stuck(st) => span(model::mark(st), mark_style("needs")),
        Mark::Working => span(working_mark(app), Style::new()),
        Mark::Review => span(review_mark(app), mark_style("review")),
        Mark::Still(st) => span(model::mark(st), mark_style(st)),
    }
}

/// The header's right side: what you act on here, in the marks' colours.
fn counts(parts: &[(String, &'static str)], sep: &'static str) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (text, st)) in parts.iter().enumerate() {
        if i > 0 {
            spans.push(span(sep, dim()));
        }
        spans.push(span(text.clone(), mark_style(st)));
    }
    Line::from(spans)
}

/// The board's top line as values, fit to its columns: the text header and the image
/// look's (image/frame.rs) draw it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Head {
    /// the repository's name (bold)
    pub name: String,
    /// after it, dim: auto-accept, whose board this is, what it hides
    pub auto: String,
    /// the other repositories with something for you; amber when one needs you
    pub others: String,
    pub others_amber: bool,
    /// at the right edge: what needs you or is ready here, each with the status whose
    /// colour it takes, and what goes between them
    pub counts: Vec<(String, &'static str)>,
    pub sep: &'static str,
    /// each open group's count, in the board's order: the image look's status bar
    pub tallies: Vec<Tally>,
}

/// A group's count for the image look's status bar (image/bar.rs): a segment per task
/// in its status's colour, and under the bar the number and the word.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Tally {
    pub n: usize,
    /// what its tasks are, short: "need you", "ready", "in review"
    pub word: &'static str,
    /// the group's first status, whose colour it takes
    pub status: &'static str,
}

/// The open groups' tallies (Closed isn't one), in the board's order.
fn tallies(groups: &[(&'static str, Vec<&Task>)]) -> Vec<Tally> {
    groups
        .iter()
        .filter_map(|(heading, ts)| {
            let status = model::BOARD_GROUPS.iter().find(|(h, _)| h == heading)?.1.first()?;
            let word = match *status {
                "needs" if ts.len() == 1 => "needs you",
                "needs" => "need you",
                "triage" => "coordinator",
                "ready" => "ready",
                "finished" => "finished",
                "review" => "in review",
                "working" => "working",
                "todo" => "queued",
                _ => return None,
            };
            Some(Tally { n: ts.len(), word, status })
        })
        .collect()
}

/// The header: the repository, and what you act on.
pub(crate) fn head(app: &App, groups: &[(&'static str, Vec<&Task>)], w: usize) -> Head {
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
    let others_amber = app.others.counts.iter().any(|c| c.needs > 0);
    Head { name, auto, others: right.others, others_amber, counts: right.counts, sep: right.sep, tallies: tallies(groups) }
}

/// The header as text.
fn draw_head(buf: &mut Buffer, area: Rect, head: &Head, w: usize) {
    let others_x = 1 + width(&head.name) + width(&head.auto) + 3;
    if !head.others.is_empty() {
        let style = if head.others_amber { Style::new().fg(AMBER) } else { dim() };
        put(buf, area, 0, others_x - 3, span(" · ", dim()));
        put(buf, area, 0, others_x, span(head.others.clone(), style));
    }
    let count = counts(&head.counts, head.sep);
    put(buf, area, 0, w.saturating_sub(2 + count.width()), count);
    put(buf, area, 0, 1 + width(&head.name), span(head.auto.clone(), dim()));
    put(buf, area, 0, 1, span(head.name.clone(), bold()));
}

/// The first row to show, from `top`: keeps the selected task (or item) in view with
/// its details, starting at a heading or a task.
pub(super) fn scroll(rows: &[Row], top: usize, avail: usize) -> usize {
    scroll_by(rows, top, &vec![1; rows.len()], avail as u32)
}

/// The same, for rows of these heights in a list `room` high (the image look's, whose
/// rows aren't a cell each).
pub(crate) fn scroll_by(rows: &[Row], mut top: usize, heights: &[u32], room: u32) -> usize {
    // rows [a, b)'s height, a row past the end none
    let tall = |a: usize, b: usize| heights[a.min(heights.len())..b.min(heights.len())].iter().sum::<u32>();
    // the first row from which the rows up to `end` fit
    let fits_to = |end: usize| (0..=end).find(|&t| tall(t, end) <= room).unwrap_or(end);
    let sel_row = rows.iter().position(Row::is_selected);
    if let Some(i) = sel_row {
        let details = rows[i + 1..].iter().take_while(|r| r.is_detail()).count();
        // the row after its last detail too, so the next row shows there's more
        let end = i + details + 1;
        if i < top {
            top = i - usize::from(i > 0 && matches!(rows[i - 1], Row::Head { .. }));
        } else if tall(top, end + 1) > room {
            top = fits_to(end + 1).min(i); // with room for one row, the task's own
        }
    }
    top = top.min(fits_to(rows.len()));
    while top > 0 && top < rows.len() && (rows[top].is_detail() || matches!(rows[top], Row::Gap)) {
        top += 1;
    }
    if let Some(i) = sel_row {
        if tall(top, i + 1) > room {
            top = i;
        }
    }
    top
}

/// The compact layout's detail panel, above the message and the keys: the selected
/// task's title, then what there is to say about it.
pub(super) fn draw_panel(buf: &mut Buffer, area: Rect, app: &App, s: &Sizes, cur: &Task, now: Now) {
    let w = s.w;
    let yb = s.h.saturating_sub(s.footer);
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

/// A task's age's style: red once it has needed you STALE seconds, on the tint when selected.
fn age_style(stale: bool, selected: bool) -> Style {
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
fn title_line(t: &TaskRow, w: usize, idw: usize, narrow: bool) -> Line<'static> {
    let id = pad(&t.id, idw);
    let gap = if narrow { " " } else { "  " };
    let width_all = w.saturating_sub(5); // from column 3 to the header's right edge
    let age = &t.age;
    // a ready task's risk, a coloured word before its age
    let risk = t.risk.as_deref();
    let right = width(age) + risk.map_or(0, |r| width(r) + if age.is_empty() { 0 } else { 2 });
    let room = width_all.saturating_sub(idw + width(gap) + if right == 0 { 0 } else { right + 2 });
    let title = fit(&t.title, room);
    let middle = pad(&format!("{gap}{title}"), width_all.saturating_sub(idw + right));
    // the ID in reverse video, off the tint: that shows in any theme. Dim text is hard
    // to read on the tint, so the rest of the row drops it
    let (id_style, title_style, age_style) = if let Some(risky) = t.asked {
        let tone = if risky { Color::Red } else { Color::Green };
        (rev().add_modifier(Modifier::BOLD).fg(tone), bold().fg(tone), bold().fg(tone))
    } else if t.selected {
        (rev().add_modifier(Modifier::BOLD), Style::new().bg(TINT), age_style(t.stale, true))
    } else {
        (dim(), if t.quiet { dim() } else { Style::new() }, age_style(t.stale, false))
    };
    // the ID's padding goes with the title, so only the ID itself is reversed
    let id_pad = id[t.id.len()..].to_string();
    let mut spans = vec![span(t.id.clone(), id_style), span(id_pad + &middle, title_style)];
    if let Some(level) = risk {
        let style = if t.selected { risk_style(level).bg(TINT) } else { risk_style(level) };
        spans.push(span(level.to_string(), style));
        if !age.is_empty() {
            spans.push(span("  ".to_string(), if t.selected { Style::new().bg(TINT) } else { Style::new() }));
        }
    }
    spans.push(span(age.clone(), age_style));
    Line::from(spans)
}


#[cfg(test)]
#[path = "tests/test_list.rs"]
mod tests;
