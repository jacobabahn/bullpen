//! The person's notifications: when a task comes to need
//! them, or (notify all) is ready or done.
//!
//! A task that comes to need the person (a prompt, `ask`, a triage escalated), and with
//! notify "all" one that is ready, finished or auto-accepted, gets a native
//! notification: OSC 777 written to the terminal of the task's pane (else its board's,
//! else its coordinator's), which Rex for Mac shows, and opens that pane's tab when
//! clicked. It goes out when the change is saved (saves::save), so once per change.
//!
//! Several tasks that come to need the person at once (within NOTIFY_BURST seconds of a
//! notification for another task) get one more between them, not one each: they wait
//! in notify.json, and once none has come for NOTIFY_QUIET seconds the watcher's outbox
//! round (`_flush`) sends "3 tasks need you: BP-3, BP-4, BP-5".

use crate::agents::{pane, placed, waits_on};
use crate::args::Args;
use crate::coordinators::home_of;
use crate::news::{deliver, touch, NEWS_MARK};
use crate::rex::{pane_tty, rex_view};
use bullpen_core::config::load_config;
use bullpen_core::json::{self, obj, s, truthy, Obj};
use bullpen_core::paths::state;
use bullpen_core::task::clip;
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Fail, Repo, Res};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

const NOTIFY_BURST: f64 = 10.0; // seconds
const NOTIFY_QUIET: f64 = 3.0;

/// Is it "needs" (the person is needed) or "news"?
#[derive(Debug, PartialEq)]
pub enum Kind {
    Needs,
    News,
}

/// re.match(r"(Reviewer )?[Nn]eeds you\b:?\s*", why): (reviewer, where the rest starts).
fn needs_you(why: &str) -> Option<(bool, usize)> {
    let (reviewer, rest) = match why.strip_prefix("Reviewer ") {
        Some(r) if r.starts_with("Needs you") || r.starts_with("needs you") => (true, r),
        _ => (false, why),
    };
    let after = rest
        .strip_prefix("Needs you")
        .or_else(|| rest.strip_prefix("needs you"))?;
    if after.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None; // \b
    }
    let after_colon = after.strip_prefix(':').unwrap_or(after);
    let rest_start = bullpen_core::text::lstrip(after_colon);
    Some((reviewer, why.len() - rest_start.len()))
}

/// (kind, text) of the notification the task's change from old (as it was saved)
/// deserves, else None.
pub fn notice(t: &Obj, old: &Obj) -> Option<(Kind, String)> {
    let st = s(t, "status");
    let was = old.get("status").and_then(Value::as_str);
    let same_why = old.get("why").unwrap_or(&Value::Null) == t.get("why").unwrap_or(&Value::Null);
    let asked = truthy(t.get("ask")) && !(truthy(old.get("ask")) && same_why);
    let id = s(t, "id");
    if st == "needs" && (was != Some("needs") || asked) {
        let why = t
            .get("why")
            .filter(|w| truthy(Some(w)))
            .map_or("it is stopped".into(), json::py_str);
        if let Some((reviewer, at)) = needs_you(&why) {
            let who = if reviewer {
                format!("{id}'s reviewer")
            } else {
                id.to_string()
            };
            let rest = &why[at..];
            let tail = if rest.starts_with('(') {
                format!(" {rest}")
            } else if !rest.is_empty() {
                format!(": {rest}")
            } else {
                String::new()
            };
            return Some((Kind::Needs, format!("{who} needs you{tail}")));
        }
        return Some((Kind::Needs, format!("{id} needs you: {why}")));
    }
    if Some(st) == was {
        return None;
    }
    let title = t.get("title").map_or("None".into(), json::py_str);
    match st {
        "ready" => Some((Kind::News, format!("{id} is ready: {title}"))),
        "finished" => Some((Kind::News, format!("{id} finished: {title}"))),
        "done" if truthy(t.get("auto_accepted")) => Some((Kind::News, format!("{id} was auto-accepted: {title}"))),
        _ => None,
    }
}

/// The save hook: tell the person of the task's change, if notify says to and they
/// aren't looking. Never fails: a notification that can't go out is noted in notify.log.
pub fn notify(repo: &Repo, t: &Obj, old: &Obj) {
    let Some((kind, text)) = notice(t, old) else { return };
    let level = repo.cfg_str("notify");
    if level == "off" || (level == "needs" && kind != Kind::Needs) {
        return;
    }
    let sent = (|| -> Res<()> {
        if looking(repo, t)? {
            return log_notify(repo, s(t, "id"), &text, "not sent: the person is looking");
        }
        if kind == Kind::Needs && held(repo, t, &text)? {
            return log_notify(repo, s(t, "id"), &text, "held: another task needs you too");
        }
        send_notification(repo, t, &text)
    })();
    if let Err(e) = sent {
        let _ = log_notify(repo, s(t, "id"), &text, &format!("not sent: {e}"));
    }
}

fn send_notification(repo: &Repo, t: &Obj, text: &str) -> Res<()> {
    let title = format!("{NEWS_MARK}{}", repo.name());
    for (session, block) in notify_panes(repo, t)? {
        if let Some(tty) = pane_tty(&session, &block)? {
            if send_osc(&tty, &title, text)? {
                return log_notify(repo, s(t, "id"), text, &format!("sent to {block}"));
            }
        }
    }
    log_notify(repo, s(t, "id"), text, "not sent: no pane to send it from")
}

/// Seconds since the epoch, as time.time().
fn clock() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// A number in notify.json (st.get(key, 0)).
fn num(st: &Obj, key: &str) -> f64 {
    st.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// The IDs in notify.json's "ids" (st.get("ids") or []).
fn ids_of(st: &Obj) -> Vec<String> {
    json::strs(st, "ids")
}

/// Run f on notify.json, locked against the other bullpen processes the same burst
/// runs, and write it back (unless f fails): {"at": when the last went out, "ids": the
/// tasks it and those held since were for, "held": [[ID, text], ...] waiting, "last":
/// when the last was held}.
fn with_notify_state<T>(repo: &Repo, f: impl FnOnce(&mut Obj) -> Res<T>) -> Res<T> {
    std::fs::create_dir_all(&repo.dir)?;
    let _lock = bullpen_core::lock::lock(&repo.dir.join("notify.lock"))?;
    let path = repo.dir.join("notify.json");
    let mut st = match json::read(&path).map_err(Fail)? {
        Some(Value::Object(o)) => o,
        _ => Obj::new(),
    };
    let out = f(&mut st)?;
    json::write_obj(&path, &st)?;
    Ok(out)
}

/// Hold a task's needs-you notification if one for another task went out in the last
/// NOTIFY_BURST seconds; else note that this one goes out now. True if held.
fn held(repo: &Repo, t: &Obj, text: &str) -> Res<bool> {
    let id = s(t, "id").to_string();
    with_notify_state(repo, |st| {
        let now = clock();
        let mut ids = ids_of(st);
        if now - num(st, "at") < NOTIFY_BURST && ids.iter().any(|i| *i != id) {
            let held = st.get("held").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut held: Vec<Value> = held
                .into_iter()
                .filter(|h| h.get(0).and_then(Value::as_str) != Some(&id))
                .collect();
            held.push(json!([id, text]));
            st.insert("held".into(), Value::Array(held));
            if !ids.contains(&id) {
                ids.push(id.clone());
            }
            st.insert("ids".into(), json!(ids));
            st.insert("last".into(), json!(now));
            touch(&state().join("outbox"))?; // the watcher's outbox round sends it
            return Ok(true);
        }
        if now - num(st, "at") >= NOTIFY_BURST {
            ids.clear();
        }
        if !ids.contains(&id) {
            ids.push(id.clone());
        }
        st.insert("at".into(), json!(now));
        st.insert("ids".into(), json!(ids));
        Ok(false)
    })
}

/// Send the held notifications once none has come for NOTIFY_QUIET seconds: one, for
/// all the tasks of the burst. False while some still wait.
fn flush_notifications(repo: &Repo) -> Res<bool> {
    if !repo.dir.join("notify.json").exists() {
        return Ok(true);
    }
    let out = with_notify_state(repo, |st| {
        let hold = st.get("held").and_then(Value::as_array).cloned().unwrap_or_default();
        if hold.is_empty() {
            return Ok(None);
        }
        if clock() - num(st, "last") < NOTIFY_QUIET {
            return Ok(Some(None));
        }
        let ids = ids_of(st);
        let text = if hold.len() == 1 {
            hold[0].get(1).map_or("None".into(), json::py_str)
        } else {
            format!("{} tasks need you: {}", ids.len(), ids.join(", "))
        };
        st.insert("held".into(), json!([]));
        st.insert("at".into(), json!(clock()));
        let last = hold.last().and_then(|h| h.get(0)).map_or("None".into(), json::py_str);
        Ok(Some(Some((last, text))))
    })?;
    let (last, text) = match out {
        None => return Ok(true),
        Some(None) => return Ok(false),
        Some(Some(sent)) => sent,
    };
    let sent = repo.task(&last).and_then(|t| send_notification(repo, &t, &text));
    if let Err(e) = sent {
        append(
            &repo.dir.join("notify.log"),
            &format!("{} {last} not sent: {e}: {text}\n", iso_now()),
        )?;
    }
    Ok(true)
}

/// (session, block) of the panes a notification can come from, best first: the task's
/// reviewer if it is what needs the person, its worker, its coordinator's board, its
/// coordinator. Clicking it opens the pane's tab: a pane in the background has none, so
/// it's passed over (the board's Enter, or `go --next`, brings it up).
fn notify_panes(repo: &Repo, t: &Obj) -> Res<Vec<(String, String)>> {
    let c = home_of(repo, t)?;
    let b = obj(&c, "board");
    let mut out: Vec<(String, String)> = ["reviewer", "worker"]
        .into_iter()
        .filter(|who| placed(t, who) != "background" && (*who == "worker" || waits_on(t) == Some("reviewer")))
        .map(|who| pane(t, who))
        .collect();
    out.push((s(b, "session").into(), s(b, "block").into()));
    out.push((s(&c, "session").into(), s(&c, "block").into()));
    out.retain(|(_, block)| !block.is_empty());
    Ok(out)
}

/// Has the task's worker or reviewer pane, or its coordinator's board, focus in its
/// session's active window? Then the person sees it already. (Rex can't say whether its
/// app is in front, so this goes by the session alone.)
fn looking(repo: &Repo, t: &Obj) -> Res<bool> {
    let c = home_of(repo, t)?;
    let b = obj(&c, "board");
    let panes = [
        pane(t, "worker"),
        pane(t, "reviewer"),
        (s(b, "session").into(), s(b, "block").into()),
    ];
    let blocks: Vec<&str> = panes
        .iter()
        .filter(|(_, b)| !b.is_empty())
        .map(|(_, b)| b.as_str())
        .collect();
    let mut sessions: Vec<&str> = Vec::new();
    for (session, block) in &panes {
        if !block.is_empty() && !sessions.contains(&session.as_str()) {
            sessions.push(session);
        }
    }
    for session in sessions {
        let v = rex_view(session)?.unwrap_or_default();
        let active = v.get("active_window_id").unwrap_or(&Value::Null);
        for w in v.get("windows").and_then(Value::as_array).into_iter().flatten() {
            let focused = w.get("focused_block_id").and_then(Value::as_str);
            if w.get("window_id").unwrap_or(&Value::Null) == active && focused.is_some_and(|f| blocks.contains(&f)) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Write a notification (OSC 777) to a terminal device. False if it can't be opened.
pub fn send_osc(tty: &str, title: &str, body: &str) -> Res<bool> {
    let data = osc(title, body);
    let f = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(tty);
    let Ok(mut f) = f else { return Ok(false) };
    // in one write, so it lands between two of the program's own
    f.write(data.as_bytes()).map_err(py_os_error)?;
    Ok(true)
}

use std::os::unix::fs::OpenOptionsExt;

/// The OSC 777 sequence: the title ends at the first ";", so it has none; the body may.
pub fn osc(title: &str, body: &str) -> String {
    let clean = |text: &str, n: usize, also: char| {
        let text: String = text
            .chars()
            .map(|c| if c < ' ' || c == '\x7f' || c == also { ' ' } else { c })
            .collect();
        clip(&text, n)
    };
    format!(
        "\x1b]777;notify;{};{}\x07",
        clean(title, 60, ';'),
        clean(body, 240, '\0')
    )
}

/// An OSError as Python words it: "[Errno 11] Resource temporarily unavailable".
fn py_os_error(e: std::io::Error) -> Fail {
    let said = e.to_string();
    match e.raw_os_error() {
        Some(n) => Fail(format!(
            "[Errno {n}] {}",
            said.trim_end_matches(&format!(" (os error {n})"))
        )),
        None => Fail(said),
    }
}

fn append(path: &Path, text: &str) -> Res<()> {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

fn log_notify(repo: &Repo, tid: &str, text: &str, how: &str) -> Res<()> {
    append(
        &repo.dir.join("notify.log"),
        &format!("{} {tid} {how}: {text}\n", iso_now()),
    )
}

/// The repo whose state folder is d: its root from its config, its folder d.
fn repo_at(d: &Path) -> Res<Repo> {
    let cfg = json::read_obj(&d.join("config.json"))?.unwrap_or_default();
    let root = cfg
        .get("root")
        .filter(|r| truthy(Some(r)))
        .map_or("/".into(), json::py_str);
    let mut repo = Repo::new(root)?;
    repo.dir = d.to_path_buf();
    repo.cfg = load_config(d)?;
    Ok(repo)
}

/// The state folders (<state>/repos/*) that have a file matching want.
fn folders_with(want: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(repos) = std::fs::read_dir(state().join("repos")) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = repos
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|d| {
            std::fs::read_dir(d).is_ok_and(|files| {
                files
                    .filter_map(|f| f.ok())
                    .any(|f| f.file_name().to_str().is_some_and(&want))
            })
        })
        .collect();
    out.sort();
    out
}

/// `_flush` (the watcher): deliver news, and send notifications, that had to wait.
pub fn cmd_flush(argv: &[String]) -> Res<i32> {
    Args::parse("_flush", argv, &[]);
    let mut waiting = false;
    let outbox = |n: &str| n.starts_with("outbox") && n.ends_with(".txt") && n.len() >= "outbox.txt".len();
    for d in folders_with(outbox) {
        if !deliver(&repo_at(&d)?)? {
            waiting = true;
        }
    }
    for d in folders_with(|n| n == "notify.json") {
        if !flush_notifications(&repo_at(&d)?)? {
            waiting = true;
        }
    }
    if !waiting {
        match std::fs::remove_file(state().join("outbox")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(0)
}

#[cfg(test)]
#[path = "test_notifications.rs"]
mod tests;
