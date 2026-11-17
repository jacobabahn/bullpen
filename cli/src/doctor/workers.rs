//! doctor's `workers`: each running Claude Code worker has its Stop hook (stop_hook/),
//! which says what it waits on when its turn ends. A task started before workers had
//! one isn't a problem: its worker does without, until it starts again.

use super::Check;
use crate::rex::alive;
use crate::stop_hook::{missing, rewrite};
use bullpen_core::json::{s, truthy, Obj};
use bullpen_core::task::is_closed;
use bullpen_core::{Repo, Res};

/// The tasks whose worker's pane is alive.
fn running(repo: &Repo) -> Res<Vec<Obj>> {
    let mut out = Vec::new();
    for t in repo.tasks()? {
        if !is_closed(s(&t, "status")) && truthy(t.get("block")) && alive(s(&t, "session"), s(&t, "block"))? {
            out.push(t);
        }
    }
    Ok(out)
}

pub fn check_workers(repo: Option<&Repo>, why_not: Option<&str>) -> Check {
    let Some(repo) = repo else {
        return Ok((
            "skip",
            format!("can't tell: {}", why_not.unwrap_or("not in a git repository")),
        ));
    };
    let tasks = running(repo)?;
    if tasks.is_empty() {
        return Ok(("skip", "no worker is running".into()));
    }
    let wrong: Vec<String> = tasks
        .iter()
        .filter_map(|t| missing(repo, t).map(|why| format!("{}'s worker has no Stop hook: {why}", s(t, "id"))))
        .collect();
    if !wrong.is_empty() {
        return Ok((
            "fail",
            format!("{}: `bullpen doctor --fix` writes it again", wrong.join("; ")),
        ));
    }
    let hooked = tasks.iter().filter(|t| truthy(t.get("stop_hook"))).count();
    let before = tasks.len() - hooked;
    let mut text = match hooked {
        1 => "1 running Claude worker has its Stop hook".to_string(),
        n => format!("{n} running Claude workers have their Stop hook"),
    };
    if before > 0 {
        text += &format!(" ({before} other running, started before workers had one, or not Claude Code)");
    }
    Ok(("ok", text))
}

/// `doctor --fix`: write the Stop hook again where it's missing. What was done, each.
pub fn fix_workers(repo: &Repo) -> Res<Vec<String>> {
    let mut did = Vec::new();
    for t in running(repo)? {
        if missing(repo, &t).is_some() {
            rewrite(repo, &t)?;
            did.push(format!("wrote {}'s Stop hook again", s(&t, "id")));
        }
    }
    Ok(did)
}
