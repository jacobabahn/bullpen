//! `say`: the person says something to a repo's coordinator, from any pane. It's typed
//! into the coordinator's pane as if typed there, once its input is empty (as typed
//! news waits), whatever the repo's news setting: it's the person talking, not news.

use crate::args::{pos, value, Arg, Args, Nargs};
use crate::board::realpath;
use crate::coordinators::{alive, calling_coordinator, coordinator, coordinators};
use crate::go::relink::relink;
use crate::news::{has_draft, type_news, LINE_BREAK};
use crate::rex::type_into;
use bullpen_core::json::{s, Obj};
use bullpen_core::repo::find_repo;
use bullpen_core::{fail, Repo, Res};
use std::path::Path;

const SAY: &[Arg] = &[
    pos("text", Nargs::Some, "what to say"),
    value(
        "--coordinator",
        "ID",
        "the coordinator (default: the repository's only one)",
    ),
    value(
        "--repo",
        "REPO",
        "the repository (default: the pane's, else this folder's)",
    ),
];

/// `say`: type TEXT into a repo's coordinator's pane.
pub fn cmd_say(argv: &[String]) -> Res<i32> {
    let a = Args::parse("say", argv, SAY);
    let text = a.args("text").join(" ");
    if text.trim().is_empty() {
        return fail("say what? give the words to type");
    }
    let mut repo = say_repo(a.opt("--repo").unwrap_or(""))?;
    let cid = which(&repo, a.opt("--coordinator").unwrap_or(""))?;
    if calling_coordinator(&repo).is_some_and(|c| s(c, "id") == cid) {
        return fail(format!("this is {cid}'s own pane: type it here"));
    }
    if !alive(coordinator(&repo, &cid))? {
        // after a Rex restart its ID changed (and maybe its session's)
        let stored = coordinator(&repo, &cid).map(|c| s(c, "session").to_string());
        relink(&mut repo, &stored.unwrap_or_default())?;
        repo = Repo::new(&repo.root)?;
    }
    let c = match coordinator(&repo, &cid) {
        Some(c) if alive(Some(c))? => c.clone(),
        _ => {
            return fail(format!(
                "{cid}'s pane in {} is gone: `bullpen up` there starts a coordinator again",
                repo.name()
            ))
        }
    };
    say_to(&repo, &c, &text)
}

/// The repository: --repo, else the one the pane or folder belongs to (as `go`).
fn say_repo(path: &str) -> Res<Repo> {
    if path.is_empty() {
        return find_repo(Some(&std::env::var("REX_BLOCK").unwrap_or_default()));
    }
    let repo = Repo::new(realpath(Path::new(path)))?;
    if !repo.dir.join("config.json").exists() {
        return fail(format!("bullpen has no tasks for {path}"));
    }
    Ok(repo)
}

/// The coordinator to say it to: the one named, else the repo's only one.
fn which(repo: &Repo, named: &str) -> Res<String> {
    let ids: Vec<&str> = coordinators(repo).iter().map(|c| s(c, "id")).collect();
    let name = repo.name();
    if ids.is_empty() {
        return fail(format!("{name} has no coordinator: `bullpen up` there starts one"));
    }
    if !named.is_empty() {
        if ids.contains(&named) {
            return Ok(named.into());
        }
        return fail(format!("{name} has no coordinator {named}; it has {}", ids.join(", ")));
    }
    if ids.len() > 1 {
        return fail(format!(
            "{name} has {} coordinators ({}): say which with --coordinator",
            ids.len(),
            ids.join(", ")
        ));
    }
    Ok(ids[0].into())
}

/// Type text into c's pane now, or, while something is half-typed there, once it's
/// empty (the watcher types it, as it does news).
fn say_to(repo: &Repo, c: &Obj, text: &str) -> Res<i32> {
    let (cid, session, block) = (s(c, "id"), s(c, "session"), s(c, "block"));
    let to = format!("{cid} in {}", repo.name());
    if has_draft(session, block)? {
        let line = text.replace("\r\n", "\n").replace('\n', LINE_BREAK);
        type_news(repo, &line, &[cid.to_string()])?;
        println!("something is half-typed in {to}'s input: this is typed once it's empty");
        return Ok(0);
    }
    if let Some(why) = type_into(session, block, text)? {
        return fail(format!("couldn't type into {to}'s pane: {why}"));
    }
    println!("typed to {to}");
    Ok(0)
}
