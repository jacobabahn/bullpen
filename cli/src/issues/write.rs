//! What bullpen writes back to a GitHub issue (docs/issues.md, "What's written back"), as
//! `issues_write` says: `labels` (the default) puts `bullpen: started` on an issue when its
//! task starts and `bullpen: declined` on it when the person says no; `comments` adds a
//! comment at each; `off` writes nothing. Unless off, an accepted issue's task closes it:
//! `Closes #N` at the end of its PR (remote mode, `closes_line`), or after the push to the
//! push remote (local mode, `close_fixed`, which `_push` calls). A task closed or dropped
//! unmerged takes `bullpen: started` off again (`unstarted`, which the one close step
//! calls). A task made from an issue records the issue's number and repository (`issue`),
//! and its writes go there.
//!
//! Every write goes through `gh_write`: always with `-R`, never with a prompt, and not at
//! all while issues are off or `issues_write` is off. What it writes is bullpen's fixed
//! words, the task's ID, the commit and the person's own reason for a no: never a name,
//! a brief, a diff or anything an agent wrote. A write that fails is one news line.

use super::repo::{gh_quiet, gh_said, issues_repo};
use crate::news::kind::Kind;
use crate::news::{news, tell_coordinator, Opts};
use bullpen_core::json::{obj, s, truthy, Obj};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

pub const STARTED: &str = "bullpen: started";
pub const DECLINED: &str = "bullpen: declined";

/// The labels bullpen makes in a repository once (`label create --force`: made, or set
/// to these if they were there): name, colour and description.
const LABELS: [(&str, &str, &str); 2] = [
    (STARTED, "1D76DB", "bullpen is working on it"),
    (DECLINED, "BFBFBF", "bullpen won't take it on"),
];

/// How long one gh write may take before it is given up.
const TIMEOUT: u64 = 60;

/// Does bullpen write to this repo's issues: issues on, and `issues_write` not off?
pub fn writes(repo: &Repo) -> bool {
    truthy(repo.cfg.get("issues")) && repo.cfg_str("issues_write") != "off"
}

/// Does it comment too (`issues_write comments`)?
fn comments(repo: &Repo) -> bool {
    repo.cfg_str("issues_write") == "comments"
}

/// What bullpen will write to the repository's issues, said once when issues come on
/// (`config --issues on`), with how to turn it off.
pub fn what_it_writes(repo: &Repo) -> String {
    let at = match issues_repo(repo) {
        Ok(slug) => format!("{slug}'s issues"),
        Err(_) => "the issues of the repository it reads".into(),
    };
    if repo.cfg_str("issues_write") == "off" {
        return format!("bullpen reads {at} and writes nothing to them (issues_write is off)");
    }
    let comments = if comments(repo) {
        ", a comment at each (with your reason for a no, if you give one)"
    } else {
        ""
    };
    format!(
        "bullpen will write to {at} on GitHub: the label `{STARTED}` when an issue's task starts, `{DECLINED}` when \
         you say no to one{comments}, and it closes an issue when its task is accepted. \
         `bullpen config --issues-write off` turns that off"
    )
}

/// The one way bullpen writes to GitHub: `gh <args> -R <slug>`, away from any terminal
/// (no prompt), and nothing at all unless `writes`. Err: why it failed, in a line.
fn gh_write(repo: &Repo, slug: &str, args: &[&str]) -> Result<(), String> {
    if !writes(repo) {
        return Ok(());
    }
    let mut argv = args.to_vec();
    argv.extend(["-R", slug]);
    let done = gh_quiet(repo, &argv, TIMEOUT).map_err(|e| format!("gh didn't run ({})", e.0))?;
    if done.ok() {
        return Ok(());
    }
    let said = gh_said(&done);
    Err(super::text::line(said.unwrap_or("gh failed"), 200))
}

/// Make bullpen's labels in the repository, once (`issue-labels` lists the repositories
/// that have them).
fn labels(repo: &Repo, slug: &str) -> Result<(), String> {
    let stamp = repo.dir.join("issue-labels");
    let made = std::fs::read_to_string(&stamp).unwrap_or_default();
    if made.lines().any(|l| l == slug) {
        return Ok(());
    }
    for (name, color, about) in LABELS {
        let args = [
            "label",
            "create",
            name,
            "--color",
            color,
            "--description",
            about,
            "--force",
        ];
        gh_write(repo, slug, &args)?;
    }
    let _ = std::fs::write(&stamp, format!("{made}{slug}\n"));
    Ok(())
}

/// The issue's repository, or why there isn't one.
fn slug(repo: &Repo) -> Result<String, String> {
    issues_repo(repo).map_err(|e| e.0)
}

/// Add and take off labels (made first), then comment, if `issues_write` comments.
/// labelled: whether the labels' edit went through, whatever came after.
fn mark(
    repo: &Repo,
    slug: Result<String, String>,
    n: i64,
    add: &str,
    remove: Option<&str>,
    comment: &str,
    labelled: &mut bool,
) -> Result<(), String> {
    let slug = slug?;
    labels(repo, &slug)?;
    let num = n.to_string();
    let mut args = vec!["issue", "edit", &num, "--add-label", add];
    if let Some(r) = remove {
        args.extend(["--remove-label", r]);
    }
    gh_write(repo, &slug, &args)?;
    *labelled = true;
    if comments(repo) {
        gh_write(repo, &slug, &["issue", "comment", &num, "--body", comment])?;
    }
    Ok(())
}

/// The issue number on a backlog item, if it is an issue's.
fn number_of_item(item: &Obj) -> Option<i64> {
    if s(item, "by") != "issue" {
        return None;
    }
    obj(item, "issue").get("number").and_then(Value::as_i64)
}

/// What a task made from an issue's item records of the issue (`issue: {number, repo}`),
/// so every later write goes to the issue it started from, whatever `issues_repo` says
/// by then. repo is "" when no repository could be told then.
pub fn for_task(repo: &Repo, item: &Obj) -> Option<Value> {
    let n = number_of_item(item)?;
    Some(json!({"number": n, "repo": slug(repo).unwrap_or_default()}))
}

/// The issue a task was made from, as the task recorded it: its number and repository
/// (Err: why there is none).
pub fn issue_of(t: &Obj) -> Option<(i64, Result<String, String>)> {
    let issue = obj(t, "issue");
    let n = issue.get("number").and_then(Value::as_i64)?;
    let slug = match s(issue, "repo") {
        "" => Err("no GitHub repository could be told when the task was made".to_string()),
        r => Ok(r.to_string()),
    };
    Some((n, slug))
}

/// A write that failed: one news line, to the task's coordinator (or everyone, about an
/// item).
fn told(repo: &Repo, about: &Obj, everyone: bool, line: &str) -> Res<()> {
    let o = Opts {
        everyone,
        ..Opts::default()
    };
    news(repo, about, Kind::IssueWrite, line, o)
}

fn failed_line(n: i64, who: &str, what: &str, why: &str) -> String {
    format!(
        "#{n} ({who}): bullpen couldn't {what} on GitHub: {why}. Nothing to do for the work; if it keeps failing, \
         tell the person (`bullpen config --issues-write off` stops the writes)"
    )
}

/// The task's worker started: `bullpen: started` on its issue (and `bullpen: declined`
/// off, from an earlier no), and with comments, "Being worked on as BP-3.". Once a task.
pub fn started(repo: &Repo, t: &mut Obj) -> Res<()> {
    if !writes(repo) || truthy(t.get("issue_started")) {
        return Ok(());
    }
    let Some((n, slug)) = issue_of(t) else { return Ok(()) };
    let id = s(t, "id").to_string();
    let comment = format!("Being worked on as {id}.");
    let mut labelled = false;
    let res = mark(repo, slug, n, STARTED, Some(DECLINED), &comment, &mut labelled);
    if labelled {
        // what close and drop take off again: only a label bullpen put there
        t.insert("issue_started".into(), json!(true));
        crate::saves::save(repo, t)?;
    }
    if let Err(why) = res {
        told(
            repo,
            t,
            false,
            &failed_line(n, &id, &format!("mark it `{STARTED}`"), &why),
        )?;
    }
    Ok(())
}

/// The person said no to an issue's item (`later L --drop`): `bullpen: declined` on the
/// issue, and with comments, a comment with their reason if they gave one.
pub fn declined(repo: &Repo, item: &Obj, why: Option<&str>) -> Res<()> {
    let Some(n) = number_of_item(item).filter(|_| writes(repo)) else {
        return Ok(());
    };
    let comment = match why.map(|w| super::text::line(w, 1000)).filter(|w| !w.is_empty()) {
        Some(w) => format!("Declined: {w}"),
        None => "Declined.".to_string(),
    };
    if let Err(e) = mark(repo, slug(repo), n, DECLINED, None, &comment, &mut false) {
        let id = s(item, "id");
        let about: Obj = json!({"id": id}).as_object().cloned().unwrap_or_default();
        told(
            repo,
            &about,
            true,
            &failed_line(n, id, &format!("mark it `{DECLINED}`"), &e),
        )?;
    }
    Ok(())
}

/// The task closed or was dropped without merging (close.rs's `close` calls this, before
/// the pane it may run in is closed): its issue stays open, and loses `bullpen: started`
/// (if bullpen put it there).
pub fn unstarted(repo: &Repo, t: &Obj) -> Res<()> {
    if !writes(repo) || !truthy(t.get("issue_started")) {
        return Ok(());
    }
    let Some((n, slug)) = issue_of(t) else { return Ok(()) };
    let num = n.to_string();
    let res = slug.and_then(|slug| gh_write(repo, &slug, &["issue", "edit", &num, "--remove-label", STARTED]));
    if let Err(why) = res {
        let line = failed_line(n, s(t, "id"), &format!("take `{STARTED}` off it"), &why);
        told(repo, t, false, &line)?;
    }
    Ok(())
}

/// Remote mode: the line a task's PR body ends with, so that GitHub closes its issue at
/// the merge (`Closes #12`, or `Closes owner/repo#12` when issues are read from another
/// repository than origin's).
pub fn closes_line(repo: &Repo, t: &Obj) -> Option<String> {
    let (n, slug) = issue_of(t).filter(|_| writes(repo))?;
    let slug = slug.ok()?;
    let origin = bullpen_core::proc::git(&repo.root, &["remote", "get-url", "origin"], false).ok()?;
    let same = super::repo::slug(&origin.stdout).is_some_and(|o| o.eq_ignore_ascii_case(&slug));
    Some(if same {
        format!("Closes #{n}")
    } else {
        format!("Closes {slug}#{n}")
    })
}

/// Local mode, after an accept's merge and its push to the push remote (pushed: it
/// went): close the task's issue with a comment naming the commit, or, not pushed, the
/// task. A failed close is one news line; the accept stands.
pub fn close_fixed(repo: &Repo, t: &Obj, pushed: bool) -> Res<()> {
    let Some((n, slug)) = issue_of(t).filter(|_| writes(repo)) else {
        return Ok(());
    };
    let id = s(t, "id");
    let commit: String = s(t, "merged").chars().take(7).collect();
    let comment = if pushed && !commit.is_empty() {
        format!("Fixed in {commit}")
    } else {
        format!("Fixed in {id} (not pushed yet)")
    };
    let num = n.to_string();
    let res = slug
        .clone()
        .and_then(|slug| gh_write(repo, &slug, &["issue", "close", &num, "--comment", &comment]));
    if let Err(why) = res {
        let by_hand = match slug {
            Ok(slug) => format!(" (`gh issue close {n} -R {slug}` closes it)"),
            Err(_) => String::new(),
        };
        let line =
            format!("{id} was accepted, but #{n} wasn't closed on GitHub: {why}. The accept still counts: tell the person{by_hand}");
        told(repo, t, false, &line)?;
    }
    Ok(())
}

/// `revert` of an accepted issue task: its issue stays closed (bullpen doesn't reopen
/// it). Whoever ran the revert reads so, and the task's coordinator hears it, to ask the
/// person.
pub fn reverted(repo: &Repo, t: &Obj) -> Res<()> {
    let Some((n, slug)) = issue_of(t).filter(|_| writes(repo)) else {
        return Ok(());
    };
    let id = s(t, "id");
    let reopen = match slug {
        Ok(slug) => format!(" (`gh issue reopen {n} -R {slug}` reopens it)"),
        Err(_) => String::new(),
    };
    let line = format!(
        "{id} closed #{n} on GitHub when it was accepted, and the revert leaves #{n} closed: tell the person, in case \
         it should be open again{reopen}"
    );
    println!("{line}");
    tell_coordinator(repo, t, Kind::IssueWrite, &line)
}
