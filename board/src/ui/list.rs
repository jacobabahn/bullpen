//! The board's list: the header line, the rows of tasks under their groups, scrolled
//! to keep the selected task in view, and the compact layout's detail panel. The
//! Later group's rows are later.rs's.

use super::later::{draw_item, draw_item_detail, draw_later_line, later_rows};
use super::{bold, dim, mark_style, put, rev, review_mark, span, spinner, working_mark, Sizes, AMBER, TINT};
use crate::model::{self, fit, pad, width, Item, Layout, Task, STALE};
use crate::App;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

type Now = chrono::DateTime<chrono::FixedOffset>;

pub(super) enum Row<'a> {
    Head(String),
    Title(&'a Task),
    /// a line under a task; loud when it says what the task needs from you; its first
    /// columns (on a narrow board) the task's age
    Detail(&'a Task, String, bool, usize),
    Gap,
    /// what a board with no tasks to show says instead; the first line bold
    Empty(String, bool),
    /// the Later group folded to one line; amber when something there waits on you
    Later(String, bool),
    /// an item of the Later group, expanded
    Item(&'a Item),
    /// a line under the selected item
    ItemDetail(String),
}

impl Row<'_> {
    /// A line under a task or an item: scrolling keeps them with it.
    fn is_detail(&self) -> bool {
        matches!(self, Row::Detail(..) | Row::ItemDetail(..))
    }
}

/// How many lines of detail the selected task gets.
const SELECTED_LINES: usize = 8;

/// The header, the list and its scroll marks. Keeps the selected task in view, and
/// sets `app.top` and `app.page`.
pub(super) fn draw_list(buf: &mut Buffer, area: Rect, app: &mut App, s: &Sizes, now: Now) {
    let w = s.w;
    let layout = app.cfg.layout;
    // by field, not app.groups(): app.top changes while these are in use
    let groups = model::shown_groups(&app.tasks, &app.cfg, app.me.as_deref(), app.closed);
    let cur_id = app.current().map(|t| t.id.clone());
    let is_cur = |t: &Task| Some(&t.id) == cur_id.as_ref();
    let item_id = app.current_item().map(|i| i.id.clone());
    // the selected task's row, or the selected item's
    let is_sel = |r: &Row| match r {
        Row::Title(t) => is_cur(t),
        Row::Item(i) => Some(&i.id) == item_id.as_ref(),
        _ => false,
    };
    // IDs share a column as wide as the longest, so titles and details line up
    let items = app.later.iter().filter(|_| app.later_open).map(|i| width(&i.id));
    let idw = groups.iter().flat_map(|(_, ts)| ts).map(|t| width(&t.id)).chain(items).max().unwrap_or(0);
    let age_on_detail = s.narrow && layout == Layout::Grouped;
    // from under the ID to the right edge
    let detail_w = w.saturating_sub(5);

    header(buf, area, app, &groups, w);

    // by copy, as the groups are by field: app.top changes while the rows are in use
    let later = app.later.clone();
    let rows = build_rows(app, &groups, &later, &is_cur, age_on_detail, detail_w, now);
    let top = scroll(&rows, app.top, s.avail, &is_sel);
    app.top = top;
    // a page, for PgDn and PgUp: the tasks (and items) this list has room for
    app.page = rows.iter().skip(top).take(s.avail).filter(|r| matches!(r, Row::Title(_) | Row::Item(_))).count().max(1);
    let app = &*app;

    for (j, row) in rows.iter().skip(top).take(s.avail).enumerate() {
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
                if t.is_from_backlog() {
                    put(buf, area, y, 2, span("↻", dim()));
                }
                put(buf, area, y, 3, title_line(app, t, is_cur(t), w, idw, s.narrow, !age_on_detail, now));
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
            Row::Empty(text, first) => put(buf, area, y, 1, span(fit(text, w.saturating_sub(2)), if *first { bold() } else { dim() })),
            Row::Later(text, amber) => draw_later_line(buf, area, y, app, text, *amber, w),
            Row::Item(i) => draw_item(buf, area, y, app, i, idw, w),
            Row::ItemDetail(text) => draw_item_detail(buf, area, y, text, detail_w),
        }
    }
    if top + s.avail < rows.len() {
        // a short board's goes at the end of the list's last row, past the ages
        let (y, x) = if s.short { (1 + s.avail, w.saturating_sub(2)) } else { (2 + s.avail, w.saturating_sub(3)) };
        put(buf, area, y, x, span("↓", dim()));
    }
    if top > 0 {
        put(buf, area, 1, w.saturating_sub(3), span("↑", dim()));
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

/// The header: the repository, and what you act on.
fn header(buf: &mut Buffer, area: Rect, app: &App, groups: &[(&'static str, Vec<&Task>)], w: usize) {
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
}

/// The list's rows: each group's heading, then its tasks, each (grouped) with its
/// detail lines; the Later group after the open ones, before Closed. With no group
/// to show, what the board says instead.
#[allow(clippy::too_many_arguments)]
fn build_rows<'a>(
    app: &App,
    groups: &[(&'static str, Vec<&'a Task>)],
    later: &'a [Item],
    is_cur: &dyn Fn(&Task) -> bool,
    age_on_detail: bool,
    detail_w: usize,
    now: Now,
) -> Vec<Row<'a>> {
    let layout = app.cfg.layout;
    let mut rows: Vec<Row> = Vec::new();
    if groups.is_empty() {
        let lines = model::empty_board(&app.tasks, &app.cfg, app.me.as_deref(), app.closed);
        rows.extend(lines.into_iter().enumerate().map(|(j, l)| Row::Empty(l, j == 0)));
    }
    let mut later_due = true;
    for (gi, (heading, ts)) in groups.iter().enumerate() {
        if *heading == "Closed" {
            later_due = false;
            rows.extend(later_rows(app, later, !rows.is_empty(), detail_w));
        }
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
                let age = if age_on_detail { model::ago(t, now) } else { String::new() };
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
    if later_due {
        rows.extend(later_rows(app, later, !rows.is_empty(), detail_w));
    }
    rows
}

/// The first row to show, from `top`: keeps the selected task (or item) in view with
/// its details, starting at a heading or a task.
fn scroll(rows: &[Row], mut top: usize, avail: usize, is_sel: &dyn Fn(&Row) -> bool) -> usize {
    let sel_row = rows.iter().position(is_sel);
    if let Some(i) = sel_row {
        let details = rows[i + 1..].iter().take_while(|r| r.is_detail()).count();
        // the row after its last detail too, so the next row shows there's more
        let end = i + details + 1;
        if i < top {
            top = i - usize::from(i > 0 && matches!(rows[i - 1], Row::Head(_)));
        } else if end >= top + avail {
            top = (end + 1).saturating_sub(avail).min(i); // with room for one row, the task's own
        }
    }
    top = top.min(rows.len().saturating_sub(avail));
    while top > 0 && top < rows.len() && (rows[top].is_detail() || matches!(rows[top], Row::Gap)) {
        top += 1;
    }
    if let Some(i) = sel_row {
        if i >= top + avail {
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

/// Is a task's detail at normal weight? What you read and act on is (needs you, stopped,
/// ready, finished); background is dim (working, queued, in review, the coordinator's,
/// closed), and so is all of another coordinator's task, as its title is.
fn loud_detail(app: &App, t: &Task) -> bool {
    matches!(t.status.as_str(), "needs" | "stopped" | "ready" | "finished") && !app.cfg.theirs(t, app.me.as_deref())
}

/// A task's age's style: red once it has needed you STALE seconds, on the tint when selected.
fn age_style(t: &Task, selected: bool, now: Now) -> Style {
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
    now: Now,
) -> Line<'static> {
    let id = pad(&t.id, idw);
    let gap = if narrow { " " } else { "  " };
    let width_all = w.saturating_sub(5); // from column 3 to the header's right edge
    let asked = app.pending.as_ref().filter(|(_, tid)| *tid == t.id);

    let age = match asked {
        Some(_) => "y / n".to_string(),
        None if with_age => model::ago(t, now),
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
#[path = "test_list.rs"]
mod tests;
