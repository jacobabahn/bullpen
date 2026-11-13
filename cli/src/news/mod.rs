//! News for the coordinators (bullpen/news.py): a news line, logged, and the outboxes
//! the watcher types into their panes. `bullpen news` (one-off and `--follow`) is in
//! read.rs. Python keeps its own `news()` (a twin) for the commands it still runs.

pub mod read;

use crate::coordinators::{alive, calling_coordinator, coordinators, sees_to};
use crate::rex;
use bullpen_core::json::{s, Obj};
use bullpen_core::paths::{safe, state};
use bullpen_core::text::{splitlines, squash, strip};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Repo, Res};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const NEWS_MARK: &str = "bullpen · ";

/// An outbox holds one message a line; a message of several lines (news with a diff
/// after it) keeps its line breaks as this, and gets them back when it is typed.
/// Claude Code takes a newline that comes in a typed burst as part of the message (a
/// paste), and only the Enter after it sends it.
pub const LINE_BREAK: &str = "\x1f";

/// What a line of news may say besides its words (news()'s keyword arguments).
#[derive(Default)]
pub struct Opts<'a> {
    /// for all the coordinators, as news that isn't about one task is
    pub everyone: bool,
    /// a coordinator not to tell (the one that ran the command)
    pub skip: Option<&'a str>,
    /// what to type in place of the line (the person's own words, not bullpen's)
    pub typed: Option<&'a str>,
    /// the IDs of the coordinators it is for, whoever owns what
    pub to: Option<Vec<String>>,
    /// lines that go after it, as they are (a diff): logged after it, each starting
    /// with a tab, and typed with it as one message
    pub more: Vec<String>,
}

/// One line of news, `bullpen · <what happened>`, for the coordinator that owns the
/// task (with everyone: for all of them): logged, and typed into their prompts unless
/// they follow the log (config --news monitor).
pub fn news(repo: &Repo, task: &Obj, event: &str, text: &str, o: Opts) -> Res<()> {
    let line = format!("{NEWS_MARK}{}", squash(text)); // one line: a newline would send it early
    let (mut to, waits) = match o.to {
        Some(to) if !to.is_empty() => (to, false),
        _ => news_to(repo, task, o.everyone)?,
    };
    if let Some(skip) = o.skip.filter(|s| !s.is_empty()) {
        if to.iter().any(|c| c == skip) {
            to.retain(|c| c != skip);
            if to.is_empty() {
                return Ok(()); // only the coordinator that did it would hear it
            }
        }
    }
    std::fs::create_dir_all(&repo.dir)?;
    let whom = if to.is_empty() {
        String::new()
    } else {
        format!(" [{}]", to.join(","))
    };
    let mut record = format!("{}{whom} {} {event}: {line}\n", iso_now(), s(task, "id"));
    for l in &o.more {
        record += &format!("\t{l}\n");
    }
    // one write: a reader never sees half
    append(&repo.dir.join("news.log"), &record)?;
    if repo.cfg_str("news") != "monitor" {
        // With no coordinator alive the line waits once, in the shared outbox: one copy
        // per coordinator would reach the first to come back once for each.
        let said = o.typed.filter(|t| !t.is_empty()).map_or(line, squash);
        let message = std::iter::once(said).chain(o.more).collect::<Vec<_>>().join(LINE_BREAK);
        type_news(repo, &message, if waits { &[] } else { &to })?;
    }
    Ok(())
}

/// news() with nothing but its words.
pub fn say(repo: &Repo, task: &Obj, event: &str, text: &str) -> Res<()> {
    news(repo, task, event, text, Opts::default())
}

/// (IDs, waits): the coordinators a line of news is for, those who see to the task
/// (every live one, with everyone); with none alive, all of them (waits is true: it has
/// to wait for one to come back). ([], true) when the repo has none.
pub fn news_to(repo: &Repo, task: &Obj, everyone: bool) -> Res<(Vec<String>, bool)> {
    let to: Vec<&Obj> = if everyone {
        let mut live = Vec::new();
        for c in coordinators(repo) {
            if alive(Some(c))? {
                live.push(c);
            }
        }
        live
    } else {
        sees_to(repo, task)?
    };
    let waits = to.is_empty();
    let from = if waits { coordinators(repo) } else { to };
    Ok((from.iter().map(|c| s(c, "id").to_string()).collect(), waits))
}

/// The news that the task is ready: `BP-3 is ready: <title> (<facts>)<then>`, the title
/// second, where a long line is cut last. A task that was ready before is "ready
/// again", "after a rebase" if its base moved since; a line word for word the one
/// before it about the task isn't sent again.
pub fn ready_news(repo: &Repo, t: &mut Obj, facts: &[String], then: &str, again: bool) -> Res<()> {
    let wt = s(t, "worktree");
    let base = if !wt.is_empty() && Path::new(wt).is_dir() {
        bullpen_core::proc::merge_base(wt, &crate::modes::task_base_ref(repo, t)?)?
    } else {
        None
    };
    let told = bullpen_core::json::get(t, "ready_told").cloned();
    let mut said = "";
    if told.is_some() || again {
        let moved = told.as_ref().is_some_and(|v| bullpen_core::json::truthy(Some(v)))
            && base
                .as_ref()
                .is_some_and(|b| told.as_ref().and_then(|v| v.as_str()) != Some(b.as_str()));
        said = if moved { " again after a rebase" } else { " again" };
    }
    t.insert("ready_told".into(), serde_json::json!(base.unwrap_or_default()));
    bullpen_core::json::write_obj(&repo.task_path(s(t, "id")), t)?; // not a save: the status didn't change here
    let facts: Vec<&str> = facts.iter().map(String::as_str).filter(|f| !f.is_empty()).collect();
    let mut text = format!("{} is ready{said}: {}", s(t, "id"), s(t, "title"));
    if !facts.is_empty() {
        text += &format!(" ({})", facts.join(", "));
    }
    text += then;
    if last_news(repo, s(t, "id")).as_deref() == Some(format!("{NEWS_MARK}{}", squash(&text)).as_str()) {
        return Ok(());
    }
    say(repo, t, "ready", &text)
}

/// The last line of news about task tid, as it was logged, or None.
pub fn last_news(repo: &Repo, tid: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(repo.dir.join("news.log")).ok()?;
    let size = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(65536))).ok()?;
    let mut data = Vec::new();
    f.read_to_end(&mut data).ok()?;
    let text = String::from_utf8_lossy(&data);
    splitlines(&text)
        .into_iter()
        .rev()
        .find_map(|line| match news_line(line) {
            Some((id, said)) if id == tid => Some(said.to_string()),
            _ => None,
        })
}

/// (task ID, what it said) of a news.log line: `<stamp>[ [to]] <id> <event>: <said>`
/// (the regex `\S+(?: \[[^\]]*\])? (\S+) [^ :]+: (.*)$`).
pub fn news_line(line: &str) -> Option<(&str, &str)> {
    let first = line.find(char::is_whitespace).filter(|i| *i > 0)?;
    let rest = &line[first..];
    let after_to = rest
        .strip_prefix(" [")
        .and_then(|r| r.find(']').map(|i| &r[i + 1..]))
        .and_then(|r| id_and_said(r));
    after_to.or_else(|| id_and_said(rest))
}

/// " <id> <event>: <said>" (no line break in it).
fn id_and_said(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.strip_prefix(' ')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (id, rest) = rest.split_at(end);
    let rest = rest.strip_prefix(' ').filter(|_| !id.is_empty())?;
    let end = rest.find([' ', ':']).filter(|i| *i > 0)?;
    let said = rest[end..].strip_prefix(": ")?;
    (!said.contains('\n')).then_some((id, said))
}

/// News of something the person may have done elsewhere (the board, a shell). The
/// coordinator whose pane ran it isn't told: it would only repeat what it just ran.
pub fn tell_coordinator(repo: &Repo, task: &Obj, event: &str, text: &str) -> Res<()> {
    let me = calling_coordinator(repo).map(|c| s(c, "id").to_string());
    let skip = me.as_deref().filter(|m| !m.is_empty());
    news(
        repo,
        task,
        event,
        text,
        Opts {
            skip,
            ..Opts::default()
        },
    )
}

/// The command a coordinator follows its news with (its guide and `up` name it).
pub fn follow_cmd() -> String {
    let me = vec![
        bullpen_core::paths::self_bin().display().to_string(),
        "news".into(),
        "--follow".into(),
    ];
    bullpen_core::text::shell_join(&rex::with_state(&me))
}

/// One line saying how news reaches the coordinator.
pub fn news_mode_line(repo: &Repo) -> String {
    if repo.cfg_str("news") == "monitor" {
        return format!(
            "news: by monitor. Nothing is typed: the coordinator must run the Monitor tool on `{}` (its guide says how).",
            follow_cmd()
        );
    }
    "news: typed into the coordinator's pane (`bullpen config --news monitor`: by Claude Code's Monitor tool instead)."
        .into()
}

/// Append text to a file, made when missing.
fn append(path: &Path, text: &str) -> Res<()> {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

/// Path.touch(): make the file, or update its time when it is there.
pub fn touch(path: &Path) -> Res<()> {
    let f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.set_modified(std::time::SystemTime::now())?;
    Ok(())
}

/// Typed news waiting for coordinator cid. Without one: news that came while no
/// coordinator was alive (or there was none), for every one alive when it's typed.
pub fn outbox_path(repo: &Repo, cid: Option<&str>) -> PathBuf {
    match cid {
        Some(cid) if !cid.is_empty() => repo.dir.join(format!("outbox-{}.txt", safe(cid))),
        _ => repo.dir.join("outbox.txt"),
    }
}

/// The outboxes in a repo's state folder, in order (glob outbox*.txt, sorted).
pub fn outboxes(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("outbox") && n.ends_with(".txt") && n.len() >= "outbox.txt".len())
        })
        .collect();
    out.sort();
    out
}

/// Type a line into these coordinators' prompts, now or once their input is empty.
pub fn type_news(repo: &Repo, line: &str, to: &[String]) -> Res<()> {
    std::fs::create_dir_all(&repo.dir)?;
    if to.is_empty() {
        append(&outbox_path(repo, None), &format!("{line}\n"))?;
    }
    for cid in to {
        append(&outbox_path(repo, Some(cid)), &format!("{line}\n"))?;
    }
    if !deliver(repo)? {
        touch(&state().join("outbox"))?; // the watcher retries while this exists
    }
    Ok(())
}

fn take_lines(path: &Path) -> Res<Vec<String>> {
    let lines = match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes).map_err(|e| bullpen_core::Fail(e.to_string()))?;
            splitlines(&text).into_iter().map(String::from).collect()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    Ok(lines)
}

fn add_lines(path: &Path, lines: &[String]) -> Res<()> {
    if !lines.is_empty() {
        append(path, &lines.iter().map(|l| format!("{l}\n")).collect::<String>())?;
    }
    Ok(())
}

/// Type each coordinator's waiting news into its pane, oldest first. News for a
/// coordinator whose pane is gone goes to the live ones instead. False if some has to
/// wait: no live coordinator, or the person is typing there.
pub fn deliver(repo: &Repo) -> Res<bool> {
    let files = outboxes(&repo.dir);
    if files.is_empty() {
        return Ok(true);
    }
    let mut live = Vec::new();
    for c in coordinators(repo) {
        if alive(Some(c))? {
            live.push(c);
        }
    }
    if live.is_empty() {
        return Ok(false);
    }
    for path in &files {
        let stem = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cid: Option<String> = (stem != "outbox").then(|| stem.chars().skip("outbox-".len()).collect());
        if cid.as_deref().is_some_and(|cid| live.iter().any(|c| s(c, "id") == cid)) {
            continue;
        }
        // shared, or for a coordinator that's gone: the live ones have it
        let lines = take_lines(path)?;
        for c in &live {
            add_lines(&outbox_path(repo, Some(s(c, "id"))), &lines)?;
        }
    }
    let mut done = true;
    for c in &live {
        let path = outbox_path(repo, Some(s(c, "id")));
        if !path.exists() {
            continue;
        }
        if has_draft(s(c, "session"), s(c, "block"))? {
            done = false;
            continue;
        }
        for line in take_lines(&path)? {
            rex::rex_type(s(c, "session"), s(c, "block"), &line.replace(LINE_BREAK, "\n"))?;
        }
    }
    Ok(done)
}

/// Is something half-typed in the coordinator's input? Typing news there would send it
/// along. (Reads Claude Code's input box: the last line starting ❯. Faint text there is
/// Claude Code's suggestion or placeholder, not the person's: a suggestion can show for
/// as long as the pane is idle, so it mustn't hold news.)
pub fn has_draft(session: &str, block: &str) -> Res<bool> {
    Ok(draft_in(&rex::capture_vt(session, block)?))
}

/// has_draft, for a captured screen.
pub fn draft_in(screen: &str) -> bool {
    let mut faint = false;
    let lines: Vec<Vec<(String, bool)>> = splitlines(screen)
        .into_iter()
        .map(|l| styled_line(l, &mut faint))
        .collect();
    let joined = |l: &[(String, bool)]| l.iter().map(|(t, _)| t.as_str()).collect::<String>();
    let lines: Vec<&Vec<(String, bool)>> = lines.iter().filter(|l| !strip(&joined(l)).is_empty()).collect();
    for line in lines[lines.len().saturating_sub(10)..].iter().rev() {
        if strip(&joined(line)).starts_with('❯') {
            let typed: String = line.iter().filter(|(_, f)| !f).map(|(t, _)| t.as_str()).collect();
            let after = typed.split_once('❯').map_or(typed.as_str(), |(_, rest)| rest);
            return !strip(after).is_empty();
        }
    }
    false
}

/// What VT_ESC matched at the start of a string: its length, and what it was.
enum Esc {
    /// `\x1b[<params>m`
    Sgr(String),
    /// `\x1b[<n>C`
    Forward(String),
    Other,
}

/// news.VT_ESC at the start of text: `\x1b\[([0-9;:]*)m|\x1b\[(\d*)C|\x1b\[[0-9;:?<=>]*[ -/]*[@-~]|\x1b[@-_]`.
fn vt_esc(text: &str) -> Option<(usize, Esc)> {
    let b = text.as_bytes();
    if b.first() != Some(&0x1b) {
        return None;
    }
    if b.get(1) == Some(&b'[') {
        let body = &text[2..];
        let n = body
            .bytes()
            .take_while(|c| c.is_ascii_digit() || *c == b';' || *c == b':')
            .count();
        if body.as_bytes().get(n) == Some(&b'm') {
            return Some((2 + n + 1, Esc::Sgr(body[..n].into())));
        }
        let d = body.bytes().take_while(u8::is_ascii_digit).count();
        if body.as_bytes().get(d) == Some(&b'C') {
            return Some((2 + d + 1, Esc::Forward(body[..d].into())));
        }
        let p = body.bytes().take_while(|c| (0x30..=0x3f).contains(c)).count();
        let i = p + body[p..].bytes().take_while(|c| (0x20..=0x2f).contains(c)).count();
        if body.as_bytes().get(i).is_some_and(|c| (0x40..=0x7e).contains(c)) {
            return Some((2 + i + 1, Esc::Other));
        }
    }
    b.get(1)
        .is_some_and(|c| (0x40..=0x5f).contains(c))
        .then_some((2, Esc::Other))
}

/// One line of a `rex capture --format=vt` screen as (text, faint) pieces. Faint (SGR
/// 2) carries on from line to line until reset, so the state is shared: pass the lines
/// of one capture in order.
pub fn styled_line(raw: &str, faint: &mut bool) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let (mut pos, mut i) = (0, 0);
    while i < raw.len() {
        let Some((n, esc)) = vt_esc(&raw[i..]) else {
            i += raw[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        out.push((raw[pos..i].into(), *faint));
        pos = i + n;
        i = pos;
        match esc {
            Esc::Sgr(params) => *faint = sgr_faint(&params, *faint),
            Esc::Forward(digits) => {
                let n = if digits.is_empty() {
                    1
                } else {
                    digits.parse().unwrap_or(0)
                };
                out.push((" ".repeat(n), *faint)); // cursor forward: a gap
            }
            Esc::Other => {}
        }
    }
    out.push((raw[pos..].replace('\r', ""), *faint));
    out.retain(|(t, _)| !t.is_empty());
    out
}

/// Is text faint after this SGR (`\x1b[<params>m`)?
pub fn sgr_faint(params: &str, mut faint: bool) -> bool {
    let ps: Vec<&str> = if params.is_empty() {
        vec!["0"]
    } else {
        params.split(';').collect()
    };
    let mut i = 0;
    while i < ps.len() {
        let p = ps[i].split(':').next().unwrap_or("");
        if matches!(p, "" | "0" | "22") {
            faint = false;
        } else if p == "2" {
            faint = true;
        } else if matches!(p, "38" | "48" | "58") && !ps[i].contains(':') && i + 1 < ps.len() {
            i += match ps[i + 1] {
                "5" => 2,
                "2" => 4,
                _ => 1,
            }; // a colour's own numbers
        }
        i += 1;
    }
    faint
}

#[cfg(test)]
#[path = "test_news.rs"]
mod tests;
