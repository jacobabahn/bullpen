//! The push remote: a backup of the base branch, in local mode.
//!
//! config --push-remote names a remote (not origin: with origin, bullpen is in remote
//! mode and the setting does nothing). After accept merges into the local base branch,
//! the base branch alone is pushed there, in the background (`_push`), never forced.
//! Pushes take a lock and push the branch as it is then, so two accepts in a row can't
//! race: the later push carries both. A failed push leaves the accept standing, with
//! one news line. `up` catches up a push it missed, and doctor checks the remote.

use crate::args::{pos, value, Arg, Args, Nargs, SUPPRESS};
use crate::modes::{push_base, push_remote, quiet_git};
use crate::news::kind::Kind;
use crate::news::{news, Opts};
use bullpen_core::json::Obj;
use bullpen_core::proc::git;
use bullpen_core::repo::repo_root;
use bullpen_core::task::{clip, commits};
use bullpen_core::text::{split, splitlines, squash, strip};
use bullpen_core::{lock, Repo, Res};
use serde_json::json;

const PUSH: &[Arg] = &[pos("base", Nargs::One, ""), value("--task", "TASK", SUPPRESS)];

/// `_push` (bullpen): push the base branch to the push remote, one push at a time. A
/// failure is news (to the task's coordinator, or everyone), and exit 1.
pub fn cmd_push(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_push", argv, PUSH);
    let repo = Repo::new(repo_root()?)?;
    let Some(remote) = push_remote(&repo)? else {
        return Ok(0);
    };
    let base = a.arg("base").unwrap_or("");
    std::fs::create_dir_all(&repo.dir)?;
    let p = {
        let path = repo.dir.join("push.lock");
        let _lock = lock::lock(&path) // an earlier push finishes first
            .map_err(|e| bullpen_core::Fail(format!("can't take the push lock {}: {e}", path.display())))?;
        // The branch as it is now: a later accept's commits go too, never an older tip.
        let refspec = format!("refs/heads/{base}:refs/heads/{base}");
        quiet_git(&repo, &["push", "-q", &remote, &refspec], 60)?
    };
    if p.ok() {
        return Ok(0);
    }
    let task = a
        .opt("--task")
        .filter(|t| !t.is_empty())
        .and_then(|t| repo.task(t).ok());
    let out = if p.stderr.is_empty() { &p.stdout } else { &p.stderr };
    let why = push_reason(out, &remote, base);
    let (after, counts) = match &task {
        Some(t) => (
            format!("After {}, ", bullpen_core::json::s(t, "id")),
            "The accept still counts; ",
        ),
        None => (String::new(), ""),
    };
    // the next accept or `up` pushes again by itself: only a push that keeps failing is the person's
    let then = if why.starts_with("rejected") {
        "Tell the person: it stays behind until they do that"
    } else {
        "Nothing to do the first time; if it keeps failing, tell the person"
    };
    let line = format!(
        "{after}{base} wasn't pushed to {remote}: {why}. {counts}`bullpen doctor` says how far behind {remote} is. \
         {then}."
    );
    // the news log's task column when no task is behind a push
    let push_news: Obj = json!({"id": "push"}).as_object().cloned().unwrap_or_default();
    let everyone = task.is_none();
    news(
        &repo,
        task.as_ref().unwrap_or(&push_news),
        Kind::Push,
        &line,
        Opts {
            everyone,
            ..Opts::default()
        },
    )?;
    Ok(1)
}

/// Why git push (or ls-remote) failed, in a few words.
pub fn push_reason(out: &str, remote: &str, base: &str) -> String {
    if out.contains("[rejected]") || out.contains("non-fast-forward") || out.contains("fetch first") {
        return format!(
            "rejected, {remote} has commits {base} doesn't (bullpen never force-pushes: \
             `git pull {remote} {base}` by hand, then `git push {remote} {base}`)"
        );
    }
    let lines: Vec<&str> = splitlines(out)
        .into_iter()
        .map(strip)
        .filter(|l| !l.is_empty() && !l.starts_with("hint:"))
        .collect();
    let fatal: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| {
            ["fatal:", "error:", "remote:", "ERROR"]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .collect();
    let text = squash(out);
    let first = fatal.first().or(lines.first()).map(|l| l.to_string());
    let first = first.unwrap_or_else(|| {
        if text.is_empty() {
            "git push failed".into()
        } else {
            text
        }
    });
    clip(&first, 200)
}

/// `up`: push the base branch when it has commits the push remote didn't get (after an
/// offline spell, say), as far as this checkout last saw it. Says what it started.
pub fn catch_up_push(repo: &Repo) -> Res<Option<String>> {
    let Some(remote) = push_remote(repo)? else {
        return Ok(None);
    };
    if !git(&repo.root, &["remote", "get-url", &remote], false)?.ok() {
        return Ok(None);
    }
    let base = repo.base()?;
    let local = format!("refs/heads/{base}");
    if !git(&repo.root, &["rev-parse", "-q", "--verify", &local], false)?.ok() {
        return Ok(None);
    }
    let seen = format!("refs/remotes/{remote}/{base}");
    if git(&repo.root, &["rev-parse", "-q", "--verify", &seen], false)?.ok() && ahead(repo, &seen, &local)? == 0 {
        return Ok(None);
    }
    push_base(repo, &base, None)
}

/// How many commits `to` has that `from` doesn't.
fn ahead(repo: &Repo, from: &str, to: &str) -> Res<i64> {
    let p = git(&repo.root, &["rev-list", "--count", &format!("{from}..{to}")], true)?;
    strip(&p.stdout)
        .parse()
        .map_err(|_| bullpen_core::Fail(format!("git rev-list said {:?}", p.stdout)))
}

/// The repo, the push remote doctor checks and the base branch, or the check's result when there's
/// nothing to check (none set, remote mode, no such remote).
type Target<'a> = Result<(&'a Repo, String, String), (&'static str, String)>;

fn push_target<'a>(repo: Option<&'a Repo>, why_not: Option<&str>) -> Res<Target<'a>> {
    let Some(repo) = repo else {
        return Ok(Err((
            "skip",
            format!("can't tell: {}", why_not.unwrap_or("not in a git repository")),
        )));
    };
    let name = repo.cfg_str("push_remote");
    if name.is_empty() {
        let none = "none (`bullpen config --push-remote <remote>` backs up the base branch after each accept)";
        return Ok(Err(("skip", none.into())));
    }
    if repo.has_origin()? {
        return Ok(Err((
            "skip",
            format!("{name} is set, but this repo has an origin remote (remote mode), so nothing is pushed to it"),
        )));
    }
    if !git(&repo.root, &["remote", "get-url", name], false)?.ok() {
        return Ok(Err((
            "fail",
            format!("there's no remote {name}: `git remote add {name} <url>`, or `bullpen config --push-remote \"\"`"),
        )));
    }
    Ok(Ok((repo, name.to_string(), repo.base()?)))
}

/// doctor: the push remote is there, reachable without a prompt, and how far behind the base.
pub fn check_push(repo: Option<&Repo>, why_not: Option<&str>) -> Res<(&'static str, String)> {
    let (repo, name, base) = match push_target(repo, why_not)? {
        Ok(t) => t,
        Err(check) => return Ok(check),
    };
    let name = name.as_str();
    let local = format!("refs/heads/{base}");
    let p = quiet_git(repo, &["ls-remote", "--heads", name, &local], 20)?;
    if !p.ok() {
        let out = if p.stderr.is_empty() { &p.stdout } else { &p.stderr };
        let why = push_reason(out, name, &base);
        return Ok(("fail", format!("can't reach {name} without a prompt: {why}")));
    }
    let Some(theirs) = split(&p.stdout).first().map(|s| s.to_string()) else {
        return Ok(("fail", format!("{name} has no {base} yet: `git push {name} {base}`")));
    };
    let known = git(&repo.root, &["cat-file", "-e", &format!("{theirs}^{{commit}}")], false)?.ok();
    if !known || !git(&repo.root, &["merge-base", "--is-ancestor", &theirs, &local], false)?.ok() {
        return Ok((
            "fail",
            format!(
                "{name}'s {base} has commits this one doesn't, so pushes are rejected: \
                 `git pull {name} {base}`, then `git push {name} {base}`"
            ),
        ));
    }
    let n = ahead(repo, &theirs, &local)?;
    if n != 0 {
        return Ok((
            "fail",
            format!("{name} is {} behind {base}: `git push {name} {base}`", commits(n)),
        ));
    }
    Ok(("ok", format!("{name} has {base} as it is here")))
}

/// doctor --all-repos: the push remote's base branch as this checkout last saw it (its
/// last fetch or push), against the base. Local refs only: no network.
pub fn check_push_seen(repo: Option<&Repo>, why_not: Option<&str>) -> Res<(&'static str, String)> {
    let (repo, name, base) = match push_target(repo, why_not)? {
        Ok(t) => t,
        Err(check) => return Ok(check),
    };
    let local = format!("refs/heads/{base}");
    let seen = format!("refs/remotes/{name}/{base}");
    if !git(&repo.root, &["rev-parse", "-q", "--verify", &seen], false)?.ok() {
        return Ok((
            "skip",
            format!(
                "can't tell without the network: this checkout has never seen {name}'s {base} (`git fetch {name}`)"
            ),
        ));
    }
    if !git(&repo.root, &["merge-base", "--is-ancestor", &seen, &local], false)?.ok() {
        return Ok((
            "fail",
            format!(
                "{name}'s {base}, as this checkout last saw it, has commits this one doesn't, so pushes are \
                 rejected: `git pull {name} {base}`, then `git push {name} {base}`"
            ),
        ));
    }
    let n = ahead(repo, &seen, &local)?;
    if n != 0 {
        return Ok((
            "fail",
            format!(
                "{name} is {} behind {base}, as this checkout last saw it: `git push {name} {base}`",
                commits(n)
            ),
        ));
    }
    Ok((
        "ok",
        format!("{name} had {base} as it is here, when this checkout last saw it"),
    ))
}

#[cfg(test)]
#[path = "tests/test_push_remote.rs"]
mod tests;
