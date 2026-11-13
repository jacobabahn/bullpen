//! The push remote: a backup of the base branch, in local mode (bullpen/push_remote.py).
//! `up` catches up a push it missed, and doctor checks the remote; the push itself
//! (`_push`) is still Python's until T8.

use crate::modes::{push_base, push_remote, quiet_git};
use bullpen_core::proc::git;
use bullpen_core::task::{clip, commits};
use bullpen_core::text::{split, splitlines, squash, strip};
use bullpen_core::{Repo, Res};

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

/// doctor: the push remote is there, reachable without a prompt, and how far behind the base.
pub fn check_push(repo: Option<&Repo>, why_not: Option<&str>) -> Res<(&'static str, String)> {
    let Some(repo) = repo else {
        return Ok((
            "skip",
            format!("can't tell: {}", why_not.unwrap_or("not in a git repository")),
        ));
    };
    let name = repo.cfg_str("push_remote");
    if name.is_empty() {
        let none = "none (`bullpen config --push-remote <remote>` backs up the base branch after each accept)";
        return Ok(("skip", none.into()));
    }
    if repo.has_origin()? {
        return Ok((
            "skip",
            format!("{name} is set, but this repo has an origin remote (remote mode), so nothing is pushed to it"),
        ));
    }
    if !git(&repo.root, &["remote", "get-url", name], false)?.ok() {
        return Ok((
            "fail",
            format!("there's no remote {name}: `git remote add {name} <url>`, or `bullpen config --push-remote \"\"`"),
        ));
    }
    let base = repo.base()?;
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

#[cfg(test)]
#[path = "test_push_remote.rs"]
mod tests;
