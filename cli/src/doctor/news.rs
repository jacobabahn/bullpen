//! doctor's `news`: with news by `wake`, the hook that parks the waiter is in place
//! (news/hook.rs) and has what it needs; with any other news, none of it is left over.

use super::checks::Check;
use crate::coordinators::{alive, coordinators};
use crate::news::hook::{self, FILE};
use crate::news::wake::{has_waiter, paused};
use bullpen_core::json::s;
use bullpen_core::paths::{safe, state};
use bullpen_core::proc::run;
use bullpen_core::Repo;

/// The Claude Code version the hook was checked on (asyncRewake on Stop and StopFailure).
pub const CLAUDE_AT_LEAST: (u64, u64, u64) = (2, 1, 325);

/// The first `x.y.z` in what `claude --version` says ("2.1.325 (Claude Code)").
pub fn version_in(text: &str) -> Option<(u64, u64, u64)> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find_map(|word| {
            let mut parts = word.split('.').map(str::parse::<u64>);
            match (parts.next()?, parts.next()?, parts.next()?) {
                (Ok(a), Ok(b), Ok(c)) => Some((a, b, c)),
                _ => None,
            }
        })
}

/// "Claude Code 2.1.300 is older than ...", if the coordinator's program says a version
/// and it is. One that can't be asked isn't a problem of the hook's.
fn too_old(repo: &Repo) -> Option<String> {
    let said = run(&[hook::program(repo)?.as_str(), "--version"], false).ok()?;
    let v = version_in(&said.stdout).filter(|v| *v < CLAUDE_AT_LEAST)?;
    let (a, b, c) = CLAUDE_AT_LEAST;
    Some(format!(
        "Claude Code {}.{}.{} is older than {a}.{b}.{c}, the version the hook was checked on: update it",
        v.0, v.1, v.2
    ))
}

/// Is coordinator pane block idle, by the watcher's note of it? (No note: not known.)
fn idle(block: &str) -> bool {
    let note = state().join("doing").join(format!("{}.json", safe(block)));
    let note = bullpen_core::json::read_obj(&note).ok().flatten().unwrap_or_default();
    matches!(s(&note, "state"), "idle" | "done")
}

/// The running, idle coordinators with no waiter (and no pause): each hears its news by
/// typing until its next turn ends.
fn unwatched(repo: &Repo) -> Vec<String> {
    let mut out = Vec::new();
    for c in coordinators(repo) {
        let cid = s(c, "id");
        if alive(Some(c)).unwrap_or(false) && idle(s(c, "block")) && !paused(repo, cid) && !has_waiter(repo, cid) {
            out.push(cid.to_string());
        }
    }
    out
}

pub fn check_news(repo: Option<&Repo>, why_not: Option<&str>) -> Check {
    let Some(repo) = repo else {
        return Ok((
            "skip",
            format!("can't tell: {}", why_not.unwrap_or("not in a git repository")),
        ));
    };
    let mode = repo.cfg_str("news");
    let settings = hook::read(&hook::path(repo));
    if mode != "wake" {
        let left = settings.ok().flatten().is_some_and(|st| !hook::found(&st).is_empty());
        if left {
            return Ok((
                "fail",
                format!(
                    "news is `{mode}`, but {FILE} still has bullpen's news hook: `bullpen doctor --fix` removes it"
                ),
            ));
        }
        return Ok(("skip", format!("news is `{mode}`: no hook to check")));
    }
    let mut wrong = Vec::new();
    if let Some(why) = hook::refusal(repo) {
        wrong.push(why);
    }
    match settings {
        Err(e) => wrong.push(format!("{e}: fix the file, then `bullpen up` writes the hook")),
        Ok(None) => wrong.push(format!("{FILE} isn't there: `bullpen up` writes the hook")),
        Ok(Some(st)) => {
            let (found, command) = (hook::found(&st), hook::command(repo));
            if found.is_empty() {
                wrong.push(format!("the news hook isn't in {FILE}: `bullpen up` writes it"));
            } else if !hook::current(&st, &command) {
                let runs = found.iter().find(|(_, c)| *c != command).map(|(_, c)| c.as_str());
                wrong.push(match runs {
                    Some(c) => format!("the news hook in {FILE} runs `{c}`, not `{command}`: `bullpen up` fixes it"),
                    None => format!("the news hook in {FILE} isn't as bullpen writes it: `bullpen up` fixes it"),
                });
            }
            if !hook::ignored(repo)? {
                wrong.push(format!(
                    "git doesn't ignore {FILE} (a committed file isn't ignored: `git rm --cached {FILE}`; else \
                     `bullpen up` adds it to .git/info/exclude)"
                ));
            }
        }
    }
    wrong.extend(too_old(repo));
    let unwatched = unwatched(repo);
    if !unwatched.is_empty() && wrong.is_empty() {
        let (it, its) = if unwatched.len() == 1 {
            ("it hears", "its")
        } else {
            ("they hear", "their")
        };
        wrong.push(format!(
            "{} idle with no news waiter: {it} news by typing until {its} next turn (say anything to start one; \
             if a turn's end leaves no waiter, the hook doesn't run in that session: `/hooks` there lists it)",
            match unwatched.as_slice() {
                [one] => format!("{one} is"),
                many => format!("{} are", many.join(", ")),
            }
        ));
    }
    if !wrong.is_empty() {
        return Ok(("fail", wrong.join("; ")));
    }
    Ok((
        "ok",
        format!("by wake: the hook is in {FILE} ({})", hook::EVENTS.join(", ")),
    ))
}

#[cfg(test)]
#[path = "test_news.rs"]
mod tests;
