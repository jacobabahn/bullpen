//! `up`: a repo's coordinator, its board and the watcher.

use crate::args::{flag, value, Arg, Args};
use crate::board::ensure_board;
use crate::build::rebuild_if_stale;
use crate::coordinators::{add_coordinator, coordinator_at, coordinator_mut, coordinators, prune, session_for};
use crate::go::relink::relink;
use crate::guide::{give_guide, guide_file, guide_text, tell_guide_changes};
use crate::news::news_mode_line;
use crate::panes::gather;
use crate::push_remote::catch_up_push;
use crate::rex::{coordinator_label, rex_run};
use crate::watcher::ensure_watcher;
use bullpen_core::json::{s, strs};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::resolve;
use bullpen_core::repo::repo_root;
use bullpen_core::text::upper_first;
use bullpen_core::{fail, Repo, Res};
use serde_json::Value;

const UP: &[Arg] = &[
    flag("--here", "make the agent in this pane the coordinator"),
    value("--session", "SESSION", "Rex session (default: this pane's)"),
    flag("--no-board", "don't open the task board"),
    flag(
        "--verbose",
        "name each pane's block ID, and how news reaches the coordinator",
    ),
];

/// What happened, to what, its block IDs: one line, or a line each with --verbose.
type Said = Vec<(String, String, String)>;

/// `up`: start (or register) this repo's coordinator, its board and the watcher.
pub fn cmd_up(argv: &[String]) -> Res<i32> {
    let a = Args::parse("up", argv, UP);
    let mut repo = Repo::new(repo_root()?)?;
    let session = session_for(&repo, a.opt("--session"))?;
    if session.is_empty() {
        return fail("not inside Rex: pass --session");
    }
    relink(&mut repo, &session)?;
    prune(&mut repo)?;
    // pruned: only live ones are left
    let live: Vec<(String, String)> = coordinators(&repo)
        .iter()
        .map(|c| (s(c, "id").to_string(), s(c, "block").to_string()))
        .collect();
    let monitor = repo.cfg_str("news") == "monitor";
    let me = self_bin().display().to_string();
    let mut said: Said = Vec::new();
    let mine: Vec<String> = if a.flag("--here") {
        vec![here(&mut repo, &session, &live, monitor, &me)?]
    } else if !live.is_empty() {
        let name = if live.len() > 1 {
            format!("coordinators {}", ids(&live, " and ", false))
        } else {
            "coordinator".into()
        };
        said.push(("already running".into(), name, ids(&live, ", ", true)));
        live.iter().map(|(id, _)| id.clone()).collect()
    } else {
        let label = coordinator_label(&repo);
        let mut argv = resolve(&strs(&repo.cfg, "coordinator_cmd"));
        let prompt = format!(
            "You are the bullpen coordinator for {}. The bullpen CLI is {me}. {}Run `{me} ls`, then say in one \
             short line that you are ready.",
            repo.root.display(),
            if monitor {
                "News comes by monitor: start it first, as your guide says. "
            } else {
                ""
            }
        );
        argv.extend(["-n".into(), label.clone(), "--append-system-prompt-file".into()]);
        argv.extend([guide_file(&repo)?.display().to_string(), prompt]);
        let block = rex_run(&session, &repo.root, &label, &argv, true)?;
        let cid = add_coordinator(&mut repo, &block, &session)?;
        given_guide(&mut repo, &cid)?;
        said.push(("started".into(), "coordinator".into(), block));
        vec![cid]
    };
    repo.save()?;
    tell_guide_changes(&mut repo); // the others (all of them, without --here), after a pull
    if !a.flag("--no-board") {
        for cid in &mine {
            ensure_board(&mut repo, &session, cid, Some(&mut said))?;
        }
    }
    gather(&repo, &session)?;
    ensure_watcher(&session, Some(&mut said))?;
    if a.flag("--verbose") {
        for (what, name, ids) in &said {
            println!("{} {what}: {ids}", name.split_whitespace().next().unwrap_or(""));
        }
    } else if !said.is_empty() {
        println!("{}", up_line(&said, !a.flag("--here") && live.is_empty()));
    }
    if let Some(pushed) = catch_up_push(&repo)? {
        println!("{pushed}");
    }
    if let Some(rebuilt) = rebuild_if_stale(&repo) {
        println!("{rebuilt}");
    }
    if let Some(hint) = crate::doctor::up_hint() {
        println!("{hint}");
    }
    if a.flag("--verbose") || monitor {
        println!("{}", news_mode_line(&repo));
    }
    Ok(0)
}

/// `up --here`: the agent in this pane is a coordinator (it may be one already). Says
/// so, and how to read the guide; its ID.
fn here(repo: &mut Repo, session: &str, live: &[(String, String)], monitor: bool, me: &str) -> Res<String> {
    let block = std::env::var("REX_BLOCK").unwrap_or_default();
    if block.is_empty() {
        return fail("--here needs REX_BLOCK (run it from the agent's own Rex pane)");
    }
    let known = coordinator_at(repo, &block, false).map(|c| s(c, "id").to_string());
    let cid = match known {
        Some(cid) => {
            if let Some(c) = coordinator_mut(repo, &cid) {
                c.insert("session".into(), Value::String(session.into()));
            }
            print!("coordinator {cid}: this pane ({block}), already. ");
            cid
        }
        None => {
            let cid = add_coordinator(repo, &block, session)?;
            let others = if live.is_empty() {
                String::new()
            } else {
                format!(
                    " The other coordinators stay: {}. Tasks you add are yours.",
                    ids(live, ", ", true)
                )
            };
            print!("coordinator {cid}: this pane ({block}).{others} ");
            cid
        }
    };
    given_guide(repo, &cid)?; // it's told to read it now
    let follow = if monitor {
        " News comes by monitor: start it first, as the guide says."
    } else {
        ""
    };
    println!("Run `{me} guide` to read the coordinator's guide.{follow}");
    Ok(cid)
}

/// Coordinator cid has the guide as it is now.
fn given_guide(repo: &mut Repo, cid: &str) -> Res<()> {
    let text = guide_text()?;
    if let Some(c) = coordinator_mut(repo, cid) {
        give_guide(c, &text)?;
    }
    Ok(())
}

/// The coordinators' IDs, joined; with their blocks: "c1 (blk1)".
fn ids(live: &[(String, String)], sep: &str, blocks: bool) -> String {
    let each = live.iter().map(
        |(id, block)| {
            if blocks {
                format!("{id} ({block})")
            } else {
                id.clone()
            }
        },
    );
    each.collect::<Vec<_>>().join(sep)
}

/// `up`'s one line for people: "Coordinator, board and watcher started. Tell the
/// coordinator what you want." on a first run, else what started or was running
/// already.
pub fn up_line(said: &[(String, String, String)], fresh: bool) -> String {
    let mut by: Vec<(&str, Vec<&str>)> = Vec::new();
    for (what, name, _) in said {
        match by.iter_mut().find(|(w, _)| w == what) {
            Some((_, names)) => names.push(name),
            None => by.push((what, vec![name])),
        }
    }
    let parts = by.iter().map(|(what, names)| {
        let who = match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => names.join(""),
        };
        format!("{} {what}.", upper_first(&who))
    });
    let mut line = parts.collect::<Vec<_>>().join(" ");
    let started = by.iter().find(|(w, _)| *w == "started");
    if fresh && started.is_some_and(|(_, names)| names.contains(&"coordinator")) {
        line += " Tell the coordinator what you want.";
    }
    line
}

#[cfg(test)]
#[path = "test_up.rs"]
mod tests;
