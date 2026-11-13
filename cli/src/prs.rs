//! Following a task's pull request after ready (lua/watch.lua runs
//! `_prs` while <state>/prs exists): its checks and the feedback on it, a merge or close
//! outside bullpen, and a waiting auto-accept. The save hook that marks a PR to follow is
//! saves.rs's (`followed`, `follow_saved`).

use crate::args::Args;
use crate::auto_accept::{checks, first_of, pr_view, run_auto_accept};
use crate::news::{news, Opts};
use crate::review::close_reviewer;
use crate::rex::alive;
use crate::saves::{followed, save};
use crate::start::launch::hand_back;
use crate::stops::stop;
use bullpen_core::config::load_config;
use bullpen_core::json::{obj, py_str, s, truthy, write_obj, Obj};
use bullpen_core::paths::state;
use bullpen_core::repo::config_files;
use bullpen_core::task::{clip, pr_short, worker_idle};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::path::Path;

/// `_prs` (watcher): look at the PRs of tasks past ready: failed checks, people's
/// comments, a merge or close outside bullpen.
pub fn cmd_prs(argv: &[String]) -> Res<i32> {
    Args::parse("_prs", argv, &[]);
    let mut following = false;
    for cfg in config_files()? {
        let root = match bullpen_core::json::read(&cfg) {
            Ok(Some(Value::Object(o))) => s(&o, "root").to_string(),
            Ok(_) => String::new(),
            Err(e) => return bullpen_core::fail(e),
        };
        if root.is_empty() || !Path::new(&root).is_dir() {
            continue;
        }
        let mut repo = Repo::new(&root)?;
        let dir = cfg.parent().unwrap_or(Path::new("")).to_path_buf();
        repo.cfg = load_config(&dir)?;
        repo.dir = dir;
        let tasks: Vec<Obj> = repo.tasks()?.into_iter().filter(followed).collect();
        if tasks.is_empty() {
            continue; // local mode has no PRs to follow (followed goes by the task's mode)
        }
        for t in &tasks {
            follow_pr(&repo, t)?;
        }
        following = following || repo.tasks()?.iter().any(followed);
    }
    if !following {
        match std::fs::remove_file(state().join("prs")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(0)
}

fn is_bot(author: Option<&Value>) -> bool {
    let empty = Obj::new();
    let a = author.and_then(Value::as_object).unwrap_or(&empty);
    truthy(a.get("is_bot")) || first_of(a, &["login"], "").ends_with("[bot]")
}

/// [(key, who, what)] for the people's comments and reviews on a PR. An approval is not
/// feedback; a review with no text means comments on the code.
pub fn feedback(pr: &Obj) -> Vec<(Value, String, String)> {
    let mut out = Vec::new();
    let (none, empty) = (Vec::new(), Obj::new());
    let list = |key| pr.get(key).and_then(Value::as_array).unwrap_or(&none).iter();
    let who = |o: &Obj| {
        first_of(
            o.get("author").and_then(Value::as_object).unwrap_or(&empty),
            &["login"],
            "someone",
        )
    };
    let key = |o: &Obj, keys: &[&str]| keys.iter().find_map(|k| o.get(*k).filter(|v| truthy(Some(v)))).cloned();
    for c in list("comments") {
        let c = c.as_object().unwrap_or(&empty);
        let body = c.get("body").map_or(String::new(), text_of);
        if !is_bot(c.get("author")) && !bullpen_core::text::strip(&body).is_empty() {
            let k = key(c, &["id", "url", "createdAt"])
                .unwrap_or_else(|| c.get("createdAt").cloned().unwrap_or(Value::Null));
            out.push((k, who(c), clip(&body, 300)));
        }
    }
    for r in list("reviews") {
        let r = r.as_object().unwrap_or(&empty);
        let state = s(r, "state");
        if is_bot(r.get("author")) || !matches!(state, "CHANGES_REQUESTED" | "COMMENTED") {
            continue;
        }
        let body = clip(&r.get("body").map_or(String::new(), text_of), 300);
        let what = if state == "CHANGES_REQUESTED" {
            let on = if body.is_empty() {
                String::new()
            } else {
                format!(": {body}")
            };
            format!("requested changes{on}")
        } else if body.is_empty() {
            "left comments on the code".to_string()
        } else {
            format!("reviewed: {body}")
        };
        let k = key(r, &["id"]).unwrap_or_else(|| r.get("submittedAt").cloned().unwrap_or(Value::Null));
        out.push((k, who(r), what));
    }
    out
}

/// A JSON value as the text it holds (None and false: none).
fn text_of(v: &Value) -> String {
    if truthy(Some(v)) {
        py_str(v)
    } else {
        String::new()
    }
}

/// Values in Python's sorted order, for the strings and numbers gh gives.
fn sorted(mut v: Vec<Value>) -> Vec<Value> {
    v.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        (Some(a), Some(b)) => a.cmp(b),
        _ => a.as_f64().partial_cmp(&b.as_f64()).unwrap_or(std::cmp::Ordering::Equal),
    });
    v.dedup();
    v
}

fn follow_pr(repo: &Repo, t: &Obj) -> Res<()> {
    let Some(pr) = pr_view(repo, t)? else {
        return Ok(()); // offline or rate limited: next time
    };
    // gh took a while: accept, a verdict or an event may have changed the task meanwhile
    let mut t = repo.task(s(t, "id"))?;
    if !followed(&t) {
        return Ok(());
    }
    let id = s(&t, "id").to_string();
    let url = py_str(t.get("pr").unwrap_or(&Value::Null));
    let pr_name = pr_short(&url);
    if !t.get("gh").is_some_and(Value::is_object) {
        t.insert("gh".into(), json!({}));
    }
    let state = first_of(&pr, &["state"], "OPEN").to_lowercase();
    let path = repo.task_path(&id);
    if state == "merged" || state == "closed" {
        gh(&mut t).insert("state".into(), json!(state));
        write_obj(&path, &t)?;
        let verb = if state == "merged" { "accept" } else { "drop" };
        let line = format!("{id}'s {pr_name} was {state} outside bullpen: `bullpen {verb} {id}` cleans up");
        return news(repo, &t, &format!("pr-{state}"), &line, Opts::default());
    }
    let (summary, failed) = checks(&pr);
    let head = first_of(&pr, &["headRefOid"], "");
    let g = gh(&mut t);
    let list = |g: &Obj, key: &str| g.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let seen_failed = list(g, "failed");
    let new_failed: Vec<String> = failed
        .iter()
        .filter(|n| !seen_failed.contains(&json!(format!("{head}:{n}"))))
        .cloned()
        .collect();
    let mut all_failed = seen_failed.clone();
    all_failed.extend(failed.iter().map(|n| json!(format!("{head}:{n}"))));
    g.insert("failed".into(), json!(sorted(all_failed)));
    g.insert("checks".into(), json!(summary));
    let seen = list(g, "seen");
    let new_notes: Vec<_> = feedback(&pr)
        .into_iter()
        .filter(|(k, _, _)| !seen.contains(k))
        .collect();
    let mut all_seen = seen.clone();
    all_seen.extend(new_notes.iter().map(|(k, _, _)| k.clone()));
    g.insert("seen".into(), json!(sorted(all_seen)));
    let new = g.get("new").and_then(Value::as_i64).unwrap_or(0) + new_notes.len() as i64;
    g.insert("new".into(), json!(new));
    if truthy(t.get("auto_wait")) && (!failed.is_empty() || !new_notes.is_empty()) {
        t.shift_remove("auto_wait"); // the person (or the worker) has something to look at first
        let (why, reason) = if failed.is_empty() {
            ("its PR has new comments", "comments")
        } else {
            ("its PR's checks failed", "checks")
        };
        crate::auto_accept::refused(repo, &t, reason, why);
        news(
            repo,
            &t,
            "ready",
            &format!("{id} not auto-accepted: {why}"),
            Opts::default(),
        )?;
    } else if truthy(t.get("auto_wait")) && summary != Some("running") && s(&t, "status") == "ready" {
        write_obj(&path, &t)?;
        return run_auto_accept(repo, &mut t);
    }
    if new_failed.is_empty() && new_notes.is_empty() {
        write_obj(&path, &t)?; // not a save: the status didn't change
        return Ok(());
    }
    let (mut lines, mut asks) = (Vec::new(), Vec::new());
    if !new_failed.is_empty() {
        let names = new_failed.join(", ");
        lines.push(format!("{id}'s PR checks failed: {names}"));
        asks.push(format!(
            "These checks failed on your pull request ({url}): {names}. Look at them (`gh pr checks {url}`) and fix them."
        ));
    }
    if !new_notes.is_empty() {
        let said: Vec<String> = new_notes.iter().map(|(_, who, what)| format!("{who} {what}")).collect();
        let said = said.join("; ");
        let n = new_notes.len();
        let s_ = if n != 1 { "s" } else { "" };
        lines.push(format!("{id}'s PR has {n} new comment{s_}: {}", clip(&said, 200)));
        asks.push(format!(
            "New feedback on your pull request ({url}): {said}. Read it in full with `gh pr view {url} --comments` \
             and address it; if you disagree, say why in your ready summary."
        ));
    }
    send_back(repo, &mut t, &lines, &asks.join(" "))
}

/// The task's "gh" record, to change (an object: follow_pr makes it one).
fn gh(t: &mut Obj) -> &mut Obj {
    if !t.get("gh").is_some_and(Value::is_object) {
        t.insert("gh".into(), json!({}));
    }
    t.get_mut("gh").and_then(Value::as_object_mut).expect("gh is an object")
}

/// PR feedback goes to the task's worker, which fixes it and runs ready again. An
/// unfinished review is moot once the work changes: close it, giving its round back.
fn send_back(repo: &Repo, t: &mut Obj, lines: &[String], ask: &str) -> Res<()> {
    let id = s(t, "id").to_string();
    let r = obj(t, "review");
    if truthy(r.get("block")) {
        let unfinished = !truthy(r.get("verdict"));
        let round = r.get("round").and_then(Value::as_i64).unwrap_or(1);
        close_reviewer(t)?;
        if unfinished {
            if let Some(Value::Object(r)) = t.get_mut("review") {
                r.insert("round".into(), json!(0.max(round - 1)));
            }
        }
    }
    let first = lines[0].strip_prefix(&format!("{id}'s ")).unwrap_or(&lines[0]);
    let why = first.trim_end_matches('.').to_string();
    let each =
        |f: &dyn Fn(&str) -> String| -> Vec<String> { lines.iter().map(|l| f(l.trim_end_matches('.'))).collect() };
    if !worker_idle(t) {
        // typing now would answer its prompt: keep the feedback for later
        write_obj(&repo.task_path(&id), t)?;
        let at = if s(t, "status") == "needs" {
            "on a prompt"
        } else {
            "stopped mid-turn"
        };
        let tell = format!("`bullpen tell {id}` it when it's free");
        for line in each(&|l| format!("{l}. Not sent to its worker, which is {at}: {tell}")) {
            news(repo, t, "pr-feedback", &line, Opts::default())?;
        }
        return Ok(());
    }
    if alive(s(t, "session"), s(t, "block"))? {
        let msg = clip(&format!("{ask} Then commit and run your ready command again."), 2000);
        if let Some(failed) = hand_back(repo, t, &msg, "pr")? {
            // its reviewer, if it had one, is closed: the coordinator passes this on
            let said =
                each(&|l| format!("{l}. The message didn't reach its worker ({failed}): `bullpen tell {id}` it"));
            stop(repo, t, "worker", "not_sent", &why, &said, "pr-feedback")?;
            return Ok(());
        }
        gh(t).insert("sent".into(), json!(true)); // the board says so once its worker is on it
        save(repo, t)?;
        for line in each(&|l| format!("{l}. Sent back to its worker.")) {
            news(repo, t, "pr-feedback", &line, Opts::default())?;
        }
        return Ok(());
    }
    gh(t).insert("sent".into(), json!(true));
    let said = each(&|l| format!("{l}. Its worker's pane is gone: `bullpen resume {id}` restarts it"));
    stop(repo, t, "worker", "not_sent", &why, &said, "pr-feedback")?;
    Ok(())
}

#[cfg(test)]
#[path = "test_prs.rs"]
mod tests;
