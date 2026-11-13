//! The watcher's other two calls (bullpen/events.py): an agent that shows no progress
//! (`_stuck`), and the usage limit's reset (`_limit`), which continues the agents that
//! stopped at it.

use crate::agents::{is_limited, pane};
use crate::args::{pos, usage_error, Arg, Args, Nargs};
use crate::limit::{limit_active, limit_file, LIMIT_CONTINUE, LIMIT_GRACE};
use crate::modes::task_base_ref;
use crate::news::{news, say, Opts};
use crate::review::start_review;
use crate::rex::{alive, rex_type};
use crate::saves::save;
use crate::start::launch::hand_back;
use crate::start::peek::py_int;
use crate::stops::stop;
use bullpen_core::json::{obj, s, Obj};
use bullpen_core::repo::{known_repos, lookup_block};
use bullpen_core::task::clip;
use bullpen_core::text::split;
use bullpen_core::timeutil::{iso_seconds, now, seconds_since};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

const STUCK: &[Arg] = &[pos("block", Nargs::One, ""), pos("minutes", Nargs::One, "")];

/// `_stuck BLOCK MINUTES` (the watcher): a pane has said it's working, and nothing else,
/// for this many minutes. News once per quiet spell; the task carries on as it is.
pub fn cmd_stuck(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_stuck", argv, STUCK);
    let given = a.arg("minutes").unwrap_or("");
    let Some(minutes) = py_int(given) else {
        usage_error(
            "_stuck",
            STUCK,
            &format!("argument minutes: invalid int value: '{given}'"),
        );
    };
    let block = a.arg("block").unwrap_or("");
    let Some((repo, role)) = lookup_block(Some(block))? else {
        return Ok(0);
    };
    if role == "coordinator" || role == "board" || !bullpen_core::json::truthy(repo.cfg.get("stuck_after")) {
        return Ok(0);
    }
    let who = if role.ends_with(" review") {
        "reviewer"
    } else {
        "worker"
    };
    let mut t = repo.task(split(&role).first().copied().unwrap_or(""))?;
    let waits = if who == "reviewer" { "review" } else { "working" };
    if pane(&t, who).1 != block || bullpen_core::json::truthy(t.get("stuck")) || s(&t, "status") != waits {
        return Ok(0);
    }
    let span = std::time::Duration::from_secs(minutes.unsigned_abs() * 60);
    let since = if minutes >= 0 { now() - span } else { now() + span };
    t.insert("stuck".into(), json!({"who": who, "since": iso_seconds(&since)}));
    save(&repo, &mut t)?;
    let id = s(&t, "id").to_string();
    let line = if who == "reviewer" {
        format!("{id}'s reviewer has shown no progress for {minutes} min: `bullpen go {id} --review` shows it")
    } else {
        format!("{id} has shown no progress for {minutes} min: `bullpen peek {id}`")
    };
    say(&repo, &t, "stuck", &line)?;
    Ok(0)
}

/// `_limit` (the watcher): once the limit has reset, continue the agents that stopped at it.
pub fn cmd_limit(argv: &[String]) -> Res<i32> {
    Args::parse("_limit", argv, &[]);
    let file = limit_file();
    if !file.exists() {
        return Ok(0);
    }
    let mut waiting = false;
    for repo in known_repos()? {
        let mut done: Vec<(Obj, String)> = Vec::new();
        for mut t in repo.tasks()? {
            let resets = s(obj(&t, "limit"), "resets");
            if !is_limited(&t) || resets.is_empty() {
                continue; // no known reset time: the person (or coordinator) continues it
            }
            if seconds_since(resets, None) < LIMIT_GRACE {
                waiting = true;
                continue;
            }
            let said = continue_limited(&repo, &mut t)?;
            done.push((t, said));
        }
        if let Some((first, _)) = done.first() {
            let said: Vec<&str> = done.iter().map(|(_, said)| said.as_str()).collect();
            let line = format!("the usage limit has reset: continued {}", said.join(", "));
            news(
                &repo,
                first,
                "limit-reset",
                &line,
                Opts {
                    everyone: true,
                    ..Default::default()
                },
            )?;
        }
    }
    if !waiting && limit_active(None)?.is_none() {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(0)
}

/// Tell the task's limited agent to carry on, in a new pane if its old one is gone.
/// Returns what to call it in the news.
fn continue_limited(repo: &Repo, t: &mut Obj) -> Res<String> {
    let lim = match t.shift_remove("limit") {
        Some(Value::Object(lim)) => lim,
        _ => Obj::new(),
    };
    t.insert("limit_seen".into(), lim.get("text").cloned().unwrap_or(Value::Null));
    t.insert("why".into(), Value::Null);
    let who = s(&lim, "who").to_string();
    let id = s(t, "id").to_string();
    match try_continue(repo, t, &who, &id) {
        Ok(said) => Ok(said),
        Err(e) => {
            let e = clip(&e.to_string(), 120);
            let why = format!("Couldn't be continued after the usage limit: {e}");
            stop(repo, t, &who, "limit", &why, &[], "stopped")?;
            let whom = if s(t, "status") == "triage" {
                "its coordinator"
            } else {
                "the person"
            };
            Ok(format!("{id} (not continued, for {whom} to see to: {e})"))
        }
    }
}

fn try_continue(repo: &Repo, t: &mut Obj, who: &str, id: &str) -> Res<String> {
    if who == "reviewer" {
        let (session, block) = pane(t, "reviewer");
        if alive(&session, &block)? {
            rex_type(&session, &block, LIMIT_CONTINUE)?;
            save(repo, t)?;
            return Ok(format!("{id}'s reviewer"));
        }
        if let Some(r) = t
            .get_mut("review")
            .and_then(Value::as_object_mut)
            .filter(|r| !r.is_empty())
        {
            let round = r.get("round").and_then(Value::as_i64).unwrap_or(1);
            r.insert("round".into(), json!((round - 1).max(0))); // the round it didn't finish
        }
        let base_ref = task_base_ref(repo, t)?;
        start_review(repo, t, &base_ref, None)?;
        return Ok(format!("{id}'s reviewer (its pane was gone: started again)"));
    }
    let was_alive = alive(s(t, "session"), s(t, "block"))?;
    if let Some(failed) = hand_back(repo, t, LIMIT_CONTINUE)? {
        return fail(failed);
    }
    Ok(if was_alive {
        id.into()
    } else {
        format!("{id} (its pane was gone: resumed in a new one)")
    })
}
