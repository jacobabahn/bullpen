//! `pick`: jump to any open task by typing part of it.

use crate::args::{flag, value, Arg, Args};
use crate::line_edit::{read_keys, term_size, Decoder, LineEdit, Raw, PASTE_OFF, PASTE_ON};
use crate::rex::{find_pane, find_panes, rex_api, rex_close, rex_view, set_layer, with_state};
use bullpen_core::json::{py_str, s, Obj};
use bullpen_core::paths::{self_bin, user_home};
use bullpen_core::repo::{find_repo, known_repos};
use bullpen_core::settings::TERMINAL;
use bullpen_core::task::is_closed;
use bullpen_core::text::{fit, len, ljust, strip};
use bullpen_core::words::{board_order, mark, mark_color, MARKED};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::io::{IsTerminal, Write};

const PICK_LABEL: &str = "bullpen: jump";
const PICK_BOUNDS: [f64; 4] = [0.15, 0.12, 0.7, 0.6]; // the middle of the window: x, y, w, h

const PICK: &[Arg] = &[
    flag("--pane", "open the picker in a floating pane (the jump key)"),
    value("--filter", "QUERY", "print the open tasks matching QUERY, best first"),
    value("--from", "FROM_BLOCK", "the pane you're in (default: this one)"),
    value("--session", "SESSION", "the session you're in (key bindings pass it)"),
];

/// What the picker lists first: what needs you, then what's ready, then the board's order.
fn pick_first(t: &Obj) -> u8 {
    match s(t, "status") {
        "needs" | "stopped" => 0,
        "ready" | "finished" => 1,
        _ => 2,
    }
}

/// (repo, task) of every open task bullpen knows: home's first, then the other
/// repositories', each with what needs you first, then what's ready, then the rest in
/// the board's order.
fn pick_items(home: Option<Repo>) -> Res<Vec<(std::rc::Rc<Repo>, Obj)>> {
    let mut repos = known_repos()?;
    if let Some(home) = home {
        repos.retain(|r| r.dir != home.dir);
        repos.insert(0, home);
    }
    let mut out = Vec::new();
    for r in repos {
        let r = std::rc::Rc::new(r);
        let mut tasks = board_order(r.tasks()?);
        tasks.sort_by_key(pick_first);
        out.extend(
            tasks
                .into_iter()
                .filter(|t| !is_closed(s(t, "status")))
                .map(|t| (r.clone(), t)),
        );
    }
    Ok(out)
}

fn root_name(r: &Repo) -> String {
    r.root
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned())
}

/// One line per task: its mark, ID and title (no age or sentence, so a search for "12"
/// finds BP-12, not a task 12m old), after its repository's name when the tasks come
/// from more than one.
fn pick_lines(items: &[(std::rc::Rc<Repo>, Obj)]) -> Vec<String> {
    let mut dirs: Vec<&std::path::Path> = items.iter().map(|(r, _)| r.dir.as_path()).collect();
    dirs.sort();
    dirs.dedup();
    let many = dirs.len() > 1;
    let rw = items.iter().map(|(r, _)| len(&root_name(r))).max().unwrap_or(0);
    let iw = items.iter().map(|(_, t)| len(s(t, "id"))).max().unwrap_or(0);
    items
        .iter()
        .map(|(r, t)| {
            let name = if many {
                format!("{}  ", ljust(&root_name(r), rw))
            } else {
                String::new()
            };
            format!(
                "{name}{} {}  {}",
                mark(s(t, "status")),
                ljust(s(t, "id"), iw),
                t.get("title").map_or(String::new(), py_str)
            )
        })
        .collect()
}

fn is_alnum(c: char) -> bool {
    c.is_alphanumeric()
}

/// How well text matches query, or None when it doesn't. Each word of the query must be
/// in one word of text, its letters in order but not necessarily together (case
/// ignored): "lgn" finds "login", but "digest" doesn't find "Migrate sessions table".
/// Letters together, and at the start of a word, score higher.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    let low = text.to_lowercase();
    let words: Vec<Vec<char>> = bullpen_core::text::split(&low)
        .iter()
        .map(|w| w.chars().collect())
        .collect();
    let mut total = 0;
    for word in bullpen_core::text::split(&query.to_lowercase()) {
        let word: Vec<char> = word.chars().collect();
        let best = words
            .iter()
            .flat_map(|low| {
                let word = &word;
                low.iter()
                    .enumerate()
                    .filter(move |(_, c)| **c == word[0])
                    .filter_map(move |(start, _)| word_score(word, low, start))
            })
            .max()?;
        total += best;
    }
    Some(total)
}

/// The score of word's letters found in low from start on, each as early as it can be.
fn word_score(word: &[char], low: &[char], start: usize) -> Option<i64> {
    let (mut score, mut prev, mut i) = (0i64, None::<usize>, start);
    for &c in word {
        i += low[i.min(low.len())..].iter().position(|&x| x == c)?;
        if let Some(p) = prev {
            score += if i == p + 1 { 3 } else { -((i - p - 1).min(5) as i64) };
        }
        if i == 0 || !is_alnum(low[i - 1]) {
            score += 2;
        }
        score += 1;
        prev = Some(i);
        i += 1;
    }
    Some(score)
}

/// The indexes of the lines matching query, best first; ties keep their order.
pub fn pick_filter(query: &str, lines: &[String]) -> Vec<usize> {
    let mut scored: Vec<(i64, usize)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| fuzzy_score(query, line).map(|s| (s, i)))
        .collect();
    scored.sort_by_key(|&(s, i)| (-s, i));
    scored.into_iter().map(|(_, i)| i).collect()
}

const PICK_UP: &[&str] = &["\x1b[A", "\x1bOA", "\x10", "\x0b"]; // ↑, ctrl+p, ctrl+k
const PICK_DOWN: &[&str] = &["\x1b[B", "\x1bOB", "\x0e"]; // ↓, ctrl+n
const PICK_PROMPT: &str = "jump to> ";
const PICK_HINT: &str = "⏎ go · esc close · ↑↓ move";

/// What a key does to the picker.
#[derive(Debug, PartialEq)]
pub enum Act {
    Choose,
    Cancel,
}

/// What the picker shows and does with each key, apart from the terminal.
pub struct Picker {
    pub lines: Vec<String>,
    /// each line's mark colour (an SGR code), as on the board
    colors: Vec<String>,
    edit: LineEdit,
    pub sel: usize,
    pub shown: Vec<usize>,
    pub msg: String,
}

impl Picker {
    pub fn new(lines: Vec<String>, colors: Vec<String>) -> Picker {
        let shown = pick_filter("", &lines);
        let msg = if lines.is_empty() {
            "no open tasks in any repository".into()
        } else {
            String::new()
        };
        Picker {
            lines,
            colors,
            edit: LineEdit::default(),
            sel: 0,
            shown,
            msg,
        }
    }

    pub fn query(&self) -> String {
        self.edit.text()
    }

    /// Apply one key: Choose or Cancel when it ends the picker, else None.
    pub fn key(&mut self, k: &str) -> Option<Act> {
        if k == "\r" || k == "\n" {
            return (!self.shown.is_empty()).then_some(Act::Choose);
        }
        if ["\x1b", "\x03", "\x07"].contains(&k) || (k == "\x04" && self.query().is_empty()) {
            return Some(Act::Cancel);
        }
        if PICK_UP.contains(&k) {
            self.sel = self.sel.saturating_sub(1);
        } else if PICK_DOWN.contains(&k) {
            self.sel = (self.sel + 1).min(self.shown.len().saturating_sub(1));
        } else {
            let query = self.query();
            if self.edit.key(k) && self.query() != query {
                self.search();
            }
        }
        None
    }

    fn search(&mut self) {
        self.sel = 0;
        self.msg.clear();
        self.shown = pick_filter(&self.query(), &self.lines);
    }

    pub fn chosen(&self) -> Option<usize> {
        self.shown.get(self.sel).copied()
    }

    /// The query as shown after the prompt, and the cursor's column on screen, from 1.
    pub fn typed(&mut self, width: i64) -> (String, i64) {
        let (shown, col) = self.edit.view(width - len(PICK_PROMPT) as i64 - 1);
        (shown, len(PICK_PROMPT) as i64 + col as i64 + 1)
    }

    /// The picker's lines: the query, a hint, then the matching tasks around the selected one.
    pub fn screen(&mut self, width: i64, height: i64) -> Vec<String> {
        let rows = (height - 2).max(1) as usize;
        let first = (self.sel + 1).saturating_sub(rows);
        let count = format!("{}/{}", self.shown.len(), self.lines.len());
        // a failure in the normal weight, after the count; else the keys, dim
        let hint = if self.msg.is_empty() {
            format!("\x1b[2m{}\x1b[0m", fit(&format!("{count}  {PICK_HINT}"), width))
        } else {
            format!(
                "\x1b[2m{count}\x1b[0m  {}",
                fit(&self.msg, (width - len(&count) as i64 - 2).max(0))
            )
        };
        let mut out = vec![format!("{PICK_PROMPT}{}", self.typed(width).0), hint];
        for (n, &i) in self.shown.iter().enumerate().skip(first).take(rows) {
            let line = self.paint(&fit(&self.lines[i], (width - 2).max(0)), i);
            out.push(if n == self.sel {
                format!("\x1b[7m> {line}\x1b[0m")
            } else {
                format!("  {line}")
            });
        }
        out
    }

    /// The line with its mark in colour; the reset keeps the selected row reversed.
    fn paint(&self, line: &str, i: usize) -> String {
        let chars: Vec<char> = line.chars().collect();
        let marks: Vec<&str> = MARKED.iter().map(|st| mark(st)).collect();
        let at = chars.iter().position(|c| marks.contains(&c.to_string().as_str()));
        match (self.colors.is_empty(), at) {
            (false, Some(at)) => {
                let part = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
                format!(
                    "{}\x1b[{}m{}\x1b[22;39m{}",
                    part(0, at),
                    self.colors[i],
                    chars[at],
                    part(at + 1, chars.len())
                )
            }
            _ => line.into(),
        }
    }
}

/// Run the picker on the terminal until a task is chosen (true) or it's closed (false).
/// choose(index) goes to that task and returns None, or why it couldn't.
fn pick_tui(p: &mut Picker, choose: impl Fn(usize) -> Option<String>) -> Res<bool> {
    let raw = Raw::new(0).map_err(|e| bullpen_core::Fail(format!("couldn't set up the terminal: {e}")))?;
    let mut out = std::io::stdout();
    let mut decoder = Decoder::default();
    let _ = write!(out, "{PASTE_ON}");
    let done = 'outer: loop {
        let (cols, rows) = term_size();
        let lines = p.screen(cols, rows);
        let col = p.typed(cols).1;
        let _ = write!(out, "\x1b[H\x1b[2J{}\x1b[1;{col}H", lines.join("\r\n"));
        let _ = out.flush();
        for k in read_keys(0, &mut decoder) {
            match p.key(&k) {
                Some(Act::Cancel) => break 'outer false,
                Some(Act::Choose) => {
                    p.msg = p.chosen().and_then(&choose).unwrap_or_default();
                    if p.msg.is_empty() {
                        break 'outer true;
                    }
                    break;
                }
                None => {}
            }
        }
    };
    drop(raw);
    let _ = write!(out, "{PASTE_OFF}\x1b[H\x1b[2J");
    let _ = out.flush();
    Ok(done)
}

/// `pick`: type part of any open task, in any repository, and go to it.
pub fn cmd_pick(argv: &[String]) -> Res<i32> {
    let a = Args::parse("pick", argv, PICK);
    let from_block = a
        .opt("--from")
        .map(str::to_string)
        .or_else(|| std::env::var("REX_BLOCK").ok());
    let session = a.opt("--session").unwrap_or("").to_string();
    if a.flag("--pane") {
        pick_pane(from_block.as_deref().unwrap_or(""), &session)?;
        return Ok(0);
    }
    let home = find_repo(from_block.as_deref().filter(|b| !b.is_empty())).ok();
    let items = pick_items(home)?;
    let lines = pick_lines(&items);
    if let Some(query) = a.opt("--filter") {
        for i in pick_filter(query, &lines) {
            println!("{}", lines[i]);
        }
        return Ok(0);
    }
    if !std::io::stdin().is_terminal() {
        return fail("pick needs a terminal (--filter prints the matching tasks instead)");
    }
    // Go to the task's pane; with none (queued, stopped, a pane that's gone), to its
    // board with the task selected; failing both, why it couldn't go there.
    let choose = |i: usize| -> Option<String> {
        let (r, t) = &items[i];
        let go = |more: &[&str]| {
            let mut cmd = std::process::Command::new(self_bin());
            cmd.args(["go", s(t, "id"), "--repo", &r.root_str()]).args(more);
            if !session.is_empty() {
                cmd.args(["--session", &session]);
            }
            cmd.stdin(std::process::Stdio::null()).output()
        };
        let code = |o: &std::io::Result<std::process::Output>| o.as_ref().map_or(1, |o| o.status.code().unwrap_or(1));
        let p = go(&[]);
        if code(&p) == 0 || code(&go(&["--board"])) == 0 {
            return None;
        }
        let err = p
            .map(|o| String::from_utf8_lossy(&o.stderr).replace("\r\n", "\n"))
            .unwrap_or_default();
        let err = strip(&err);
        let err = err.strip_prefix("bullpen: ").unwrap_or(err);
        Some(if err.is_empty() {
            "couldn't go there".into()
        } else {
            err.into()
        })
    };
    let colors = items.iter().map(|(_, t)| mark_color(t).to_string()).collect();
    pick_tui(&mut Picker::new(lines, colors), choose)?;
    Ok(0)
}

/// Open the picker in a floating pane in the middle of the window of the pane you're
/// in; it closes itself when you choose or press esc. One picker at a time.
fn pick_pane(from_block: &str, session: &str) -> Res<()> {
    let session = if session.is_empty() {
        std::env::var("REX_SESSION").unwrap_or_default()
    } else {
        session.to_string()
    };
    let view = rex_view(&session)?;
    for w in find_panes(view.as_ref(), PICK_LABEL) {
        rex_close(&session, &w.block)?;
    }
    let mut argv = vec![self_bin().to_string_lossy().into_owned(), "pick".into()];
    if !from_block.is_empty() {
        argv.extend(["--from".into(), from_block.into()]);
    }
    if !session.is_empty() {
        argv.extend(["--session".into(), session.clone()]);
    }
    let mut spec = json!({"bounds": PICK_BOUNDS, "layout": {"block": {
        "flavor": TERMINAL, "label": PICK_LABEL,
        "options": {"command": with_state(&argv), "cwd": user_home()}}}});
    let at = if from_block.is_empty() {
        None
    } else {
        find_pane(view.as_ref(), from_block, "")
    };
    if let Some(window) = at.and_then(|w| w.window) {
        spec["window_id"] = Value::from(window);
    }
    let r = rex_api(&session, "session.new_layer", &spec, true)?;
    let layer = r.as_ref().and_then(|r| r.get("layer_id")).map_or(String::new(), py_str);
    set_layer(&session, &layer, "normal", true)?;
    Ok(())
}

#[cfg(test)]
#[path = "test_pick.rs"]
mod tests;
