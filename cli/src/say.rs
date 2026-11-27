//! `say`: the person says something to a repo's coordinator, from any pane. It's typed
//! into the coordinator's pane as if typed there, once its input is empty (as typed
//! news waits), whatever the repo's news setting: it's the person talking, not news.

use crate::args::{pos, value, Arg, Args, Nargs};
use crate::coordinators::{alive, calling_coordinator, coordinator, coordinators};
use crate::go::relink::relink;
use crate::go::repo_or_here;
use crate::host::Agent;
use crate::news::{type_news, LINE_BREAK};
use bullpen_core::json::{s, Obj};
use bullpen_core::{fail, Repo, Res};

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
    let block = std::env::var("REX_BLOCK").unwrap_or_default();
    let repo = repo_or_here(a.opt("--repo").unwrap_or(""), &block)?;
    let cid = which(&repo, a.opt("--coordinator").unwrap_or(""))?;
    if calling_coordinator(&repo).is_some_and(|c| s(c, "id") == cid) {
        return fail(format!("this is {cid}'s own pane: type it here"));
    }
    let (repo, c) = running(repo, &cid)?;
    say_to(&repo, &c, &text)
}

/// Coordinator cid's entry, its pane running: after a Rex restart its pane is found
/// again first (its ID changed, and maybe its session's), so the repo comes back read
/// anew. Fails when its pane is gone.
pub fn running(mut repo: Repo, cid: &str) -> Res<(Repo, Obj)> {
    if !alive(coordinator(&repo, cid))? {
        let stored = coordinator(&repo, cid).map(|c| s(c, "session").to_string());
        relink(&mut repo, &stored.unwrap_or_default())?;
        repo = Repo::new(&repo.root)?;
    }
    let c = match coordinator(&repo, cid) {
        Some(c) if alive(Some(c))? => c.clone(),
        _ => {
            return fail(format!(
                "{cid}'s pane in {} is gone: `bullpen up` there starts a coordinator again",
                repo.name()
            ))
        }
    };
    Ok((repo, c))
}

/// The coordinator named, else the repo's only one.
pub fn which(repo: &Repo, named: &str) -> Res<String> {
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
    let (cid, agent) = (s(c, "id"), Agent::of(c));
    let to = format!("{cid} in {}", repo.name());
    if agent.has_draft()? {
        let line = text.replace("\r\n", "\n").replace('\n', LINE_BREAK);
        type_news(repo, &line, &[cid.to_string()])?;
        println!("something is half-typed in {to}'s input: this is typed once it's empty");
        return Ok(0);
    }
    if let Some(why) = agent.tell(text)? {
        return fail(format!("couldn't type into {to}'s pane: {why}"));
    }
    println!("typed to {to}");
    Ok(0)
}
