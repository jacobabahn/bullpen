//! One task line, as the board says it, for `ls` and `show` (its
//! words are core's words.rs): its mark, ID, title, age and the board's sentence.

use bullpen_core::json::{s, Obj};
use bullpen_core::risk::level_sgr;
use bullpen_core::task::is_closed;
use bullpen_core::text::{fit, len, ljust, width, wrap};
use bullpen_core::words::{
    ago, board_detail, from_mark, is_limited, is_stuck, mark, mark_color, pr_phrase, stale, Every,
};
use bullpen_core::{Repo, Res};

fn sgr(code: &str, text: &str, color: bool) -> String {
    if color && !text.is_empty() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.into()
    }
}

/// A closed task's title, or one its coordinator is on, is dim.
fn title_code(t: &Obj) -> &'static str {
    let st = s(t, "status");
    if is_closed(st) || st == "triage" {
        "2"
    } else {
        "0"
    }
}

/// The mark, the ID and the title, coloured as on the board.
fn head(t: &Obj, tid: &str, title: &str, color: bool) -> String {
    // a task from the backlog has a dim ↻ in the space after its mark
    let from = from_mark(t);
    format!(
        "{}{}{}  {}",
        sgr(mark_color(t), mark(s(t, "status")), color),
        if from == " " {
            from.to_string()
        } else {
            sgr("2", from, color)
        },
        sgr("2", tid, color),
        sgr(title_code(t), title, color)
    )
}

fn age_code(t: &Obj) -> &'static str {
    if stale(t, None) {
        "1;31"
    } else {
        "2"
    }
}

/// One task as the board says it: mark, ID, title, age, then the board's sentence.
/// Cut to `cut` columns when given (the title keeps room for the age); in colour with
/// color, as on the board: the mark in its status's colour, the rest dim.
pub fn task_line(repo: &Repo, t: &Obj, every: &Every, idw: usize, cut: Option<i64>, color: bool) -> Res<String> {
    let tid = ljust(s(t, "id"), idw);
    let (risk, when) = (risk_word(t), ago(t, None));
    let when = match (risk, when.is_empty()) {
        ("", _) => when,
        (r, true) => r.to_string(),
        (r, false) => format!("{r}  {when}"),
    };
    let mut text = board_detail(&repo.cfg, t, every, None)?;
    let head_w = (2 + len(&tid) + 2) as i64;
    let age_room = if when.is_empty() { 0 } else { len(&when) as i64 + 2 };
    let title = match cut {
        None => s(t, "title").to_string(),
        Some(w) => fit(s(t, "title"), (w - head_w - age_room).max(1)),
    };
    if let Some(w) = cut {
        let room = w - head_w - width(&title) as i64 - age_room - 2;
        text = if room >= 4 { fit(&text, room) } else { String::new() };
    }
    let mut parts = vec![head(t, &tid, &title, color)];
    if !risk.is_empty() {
        parts.push(sgr(level_sgr(risk), risk, color));
    }
    let when = ago(t, None);
    if !when.is_empty() {
        parts.push(sgr(age_code(t), &when, color));
    }
    if !text.is_empty() {
        parts.push(sgr("2", &text, color));
    }
    Ok(parts.join("  "))
}

/// The board's rows for a task, for a terminal too narrow for task_line: mark, ID,
/// title and the age at the right edge, then the board's sentence under the title in
/// `lines` lines (by default the board's: two for one that needs you, else one).
pub fn task_rows(
    repo: &Repo,
    t: &Obj,
    every: &Every,
    idw: usize,
    cols: i64,
    color: bool,
    lines: Option<usize>,
) -> Res<Vec<String>> {
    let st = s(t, "status");
    let tid = ljust(s(t, "id"), idw);
    let risk = risk_word(t);
    let when = ago(t, None);
    let when = match (risk, when.is_empty()) {
        ("", _) => when,
        (r, true) => r.to_string(),
        (r, false) => format!("{r}  {when}"),
    };
    let w = cols - 1; // never into the last column
    let head_w = (2 + len(&tid) + 2) as i64;
    let title = fit(s(t, "title"), (w - head_w - len(&when) as i64 - 2).max(1));
    let pad = if when.is_empty() {
        String::new()
    } else {
        " ".repeat((w - head_w - width(&title) as i64 - len(&when) as i64).max(2) as usize)
    };
    let age = ago(t, None);
    let right = match risk {
        "" => sgr(age_code(t), &when, color),
        r if age.is_empty() => sgr(level_sgr(r), r, color),
        r => format!("{}  {}", sgr(level_sgr(r), r, color), sgr(age_code(t), &age, color)),
    };
    let mut rows = vec![format!("{}{pad}{right}", head(t, &tid, &title, color))];
    let text = board_detail(&repo.cfg, t, every, None)?;
    let two =
        matches!(st, "needs" | "stopped" | "finished") || is_stuck(t) || is_limited(t) || !pr_phrase(t).is_empty();
    let n = lines.unwrap_or(if two { 2 } else { 1 });
    for line in wrap(&text, w - head_w, n) {
        if !line.is_empty() {
            rows.push(format!("{}{}", " ".repeat(head_w as usize), sgr("2", &line, color)));
        }
    }
    Ok(rows)
}

/// A ready task's risk level, the word before its age ("" for none).
fn risk_word(t: &Obj) -> &str {
    bullpen_core::risk::shown(t).map_or("", |(level, _)| level)
}

/// The terminal's width when stdout is one, else None: piped output is whole.
pub fn term_width() -> Option<i64> {
    use std::io::IsTerminal;
    std::io::stdout()
        .is_terminal()
        .then(|| crate::help::term_width() as i64)
}

/// In colour: on a terminal, unless NO_COLOR is set.
pub fn colored() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}
