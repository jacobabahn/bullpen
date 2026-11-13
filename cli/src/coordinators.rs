//! A repo's coordinators, and who sees to a task (bullpen/coordinators.py): which one a
//! pane is, which are alive, and whose a task is; adding one, and dropping those whose
//! panes are gone (`up`), or whose pane the watcher said closed (`_coordinator_gone`).
//!
//! cfg["coordinators"] lists them, oldest (the primary) first: {"id": "c1", "block",
//! "session", "board": {"block", "session"}, "board_others"}. A task belongs to the one
//! that added it (t["coordinator"]). Who sees to it is that one alone while its pane is
//! running, else every running coordinator.

use crate::args::{pos, value, Arg, Args, Nargs};
use crate::jobs::{background, Job};
use bullpen_core::json::{obj, s, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::repo::{index_block, repo_root, unindex_block};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// The repo's coordinators, oldest (the primary) first (an entry that isn't an object is left out).
pub fn coordinators(repo: &Repo) -> Vec<&Obj> {
    let list = repo.cfg.get("coordinators").and_then(Value::as_array);
    list.into_iter().flatten().filter_map(Value::as_object).collect()
}

pub fn coordinator<'a>(repo: &'a Repo, cid: &str) -> Option<&'a Obj> {
    if cid.is_empty() {
        return None;
    }
    coordinators(repo).into_iter().find(|c| s(c, "id") == cid)
}

/// The coordinator whose pane is block (or whose board is, with boards).
pub fn coordinator_at<'a>(repo: &'a Repo, block: &str, boards: bool) -> Option<&'a Obj> {
    if block.is_empty() {
        return None;
    }
    coordinators(repo)
        .into_iter()
        .find(|c| s(c, "block") == block || (boards && s(obj(c, "board"), "block") == block))
}

/// The coordinator cid, to change (the caller saves the repo).
pub fn coordinator_mut<'a>(repo: &'a mut Repo, cid: &str) -> Option<&'a mut Obj> {
    let list = repo.cfg.get_mut("coordinators").and_then(Value::as_array_mut)?;
    list.iter_mut()
        .filter_map(Value::as_object_mut)
        .find(|c| !cid.is_empty() && s(c, "id") == cid)
}

/// A new coordinator in pane block: its ID.
pub fn add_coordinator(repo: &mut Repo, block: &str, session: &str) -> Res<String> {
    let n = repo.cfg.get("next_coordinator").and_then(Value::as_i64).unwrap_or(1);
    let cid = format!("c{n}");
    repo.cfg.insert("next_coordinator".into(), json!(n + 1));
    let c = json!({"id": cid, "block": block, "session": session, "board": null});
    match repo.cfg.get_mut("coordinators").and_then(Value::as_array_mut) {
        Some(list) => list.push(c),
        None => {
            repo.cfg.insert("coordinators".into(), json!([c]));
        }
    }
    index_block(block, repo, "coordinator")?;
    Ok(cid)
}

/// Forget coordinators whose panes are gone, and close their boards. Their tasks' news
/// goes to the others.
pub fn prune(repo: &mut Repo) -> Res<()> {
    let gone: Vec<Obj> = coordinators(repo).into_iter().cloned().collect();
    for c in gone {
        if !alive(Some(&c))? {
            drop_coordinator(repo, &c)?;
        }
    }
    Ok(())
}

/// Forget coordinator c, whose pane is gone, and close its board.
fn drop_coordinator(repo: &mut Repo, c: &Obj) -> Res<()> {
    let b = obj(c, "board");
    if !s(b, "block").is_empty() {
        crate::rex::rex_close(s(b, "session"), s(b, "block"))?;
        unindex_block(s(b, "block"))?;
    }
    unindex_block(s(c, "block"))?;
    if let Some(list) = repo.cfg.get_mut("coordinators").and_then(Value::as_array_mut) {
        if let Some(i) = list.iter().position(|o| o.as_object() == Some(c)) {
            list.remove(i);
        }
    }
    println!("coordinator {}: its pane is gone, so it is dropped", s(c, "id"));
    Ok(())
}

/// Seconds before a look at whether a coordinator's pane really closed.
const GONE_WAIT: &str = "5";

/// The watcher said coordinator c's pane is gone: a few seconds later, look in the
/// background (a job of its own: jobs.rs), and drop c if its pane really closed.
pub fn coordinator_gone(repo: &Repo, c: &Obj) {
    // the tests: no wait, and wait for the look
    let wait = std::env::var("BULLPEN_GONE_WAIT").unwrap_or_default();
    let cid = s(c, "id");
    let argv = vec![
        self_bin().display().to_string(),
        "_coordinator_gone".into(),
        cid.into(),
        "--wait".into(),
        if wait.is_empty() {
            GONE_WAIT.into()
        } else {
            wait.clone()
        },
    ];
    let log = repo.dir.join("jobs.log");
    let job = Job {
        argv,
        cwd: &repo.root,
        log: &log,
        what: format!("Looking at whether coordinator {cid}'s pane closed"),
        task: None,
        pass_fds: vec![],
    };
    background(&job, !wait.is_empty());
}

const GONE: &[Arg] = &[pos("id", Nargs::One, ""), value("--wait", "WAIT", "")];

/// `_coordinator_gone ID [--wait SECONDS]` (background): drop a coordinator whose pane
/// closed, only while Rex still answers for its session but not for its pane, and has no
/// pane of bullpen's coordinator label that no coordinator has. So a Rex shutdown or
/// restart drops nobody: `up` finds the panes again.
pub fn cmd_coordinator_gone(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_coordinator_gone", argv, GONE);
    let wait = match a.opt("--wait") {
        None => 5.0,
        Some(w) => match bullpen_core::text::strip(w).parse::<f64>() {
            Ok(n) => n,
            Err(_) => crate::args::usage_error(
                "_coordinator_gone",
                GONE,
                &format!("argument --wait: invalid float value: '{w}'"),
            ),
        },
    };
    if wait > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(wait));
    }
    let mut repo = Repo::new(repo_root()?)?;
    let Some(c) = coordinator(&repo, a.arg("id").unwrap_or("")).cloned() else {
        return Ok(0);
    };
    if alive(Some(&c))? {
        return Ok(0);
    }
    let Some(view) = crate::rex::rex_view(s(&c, "session"))? else {
        return Ok(0); // Rex is down, or restarted with a new session: nobody can tell
    };
    let taken: Vec<&str> = coordinators(&repo).into_iter().map(|o| s(o, "block")).collect();
    let label = crate::rex::coordinator_label(&repo);
    if crate::rex::find_panes(Some(&view), &label)
        .iter()
        .any(|h| !taken.contains(&h.block.as_str()))
    {
        return Ok(0); // a restart brought it back under a new ID
    }
    drop_coordinator(&mut repo, &c)?;
    repo.save()?;
    Ok(0)
}

/// Does coordinator c's board show the task? Only one c sees to, or one nobody sees
/// to, while c's board_others is off. With one coordinator (or c None), every task.
pub fn on_board(repo: &Repo, c: Option<&Obj>, t: &Obj) -> Res<bool> {
    let Some(c) = c.filter(|c| !c.is_empty()) else {
        return Ok(true);
    };
    if bullpen_core::json::truthy(c.get("board_others")) || coordinators(repo).len() < 2 {
        return Ok(true);
    }
    let sees = sees_to(repo, t)?;
    Ok(sees.is_empty() || sees.iter().any(|o| o.get("id") == c.get("id")))
}

/// The Rex session to work in: given (--session), else the primary coordinator's while
/// it exists (a Rex restart gives sessions new IDs), else the one we're running in.
pub fn session_for(repo: &Repo, given: Option<&str>) -> Res<String> {
    if let Some(s) = given.filter(|s| !s.is_empty()) {
        return Ok(s.into());
    }
    let c = primary(repo).map_or("", |c| s(c, "session"));
    let here = std::env::var("REX_SESSION").unwrap_or_default();
    if !c.is_empty() && (c == here || crate::rex::rex_view(c)?.is_some()) {
        return Ok(c.into());
    }
    Ok(if here.is_empty() { c.into() } else { here })
}

/// Is the coordinator's pane running?
pub fn alive(c: Option<&Obj>) -> Res<bool> {
    match c {
        Some(c) if !c.is_empty() => crate::rex::alive(s(c, "session"), s(c, "block")),
        _ => Ok(false),
    }
}

/// The task's coordinator, if it is still one (an older task may have none).
pub fn owner<'a>(repo: &'a Repo, t: &Obj) -> Option<&'a Obj> {
    coordinator(repo, s(t, "coordinator"))
}

/// The oldest coordinator, if there is one.
pub fn primary(repo: &Repo) -> Option<&Obj> {
    coordinators(repo).into_iter().next()
}

/// The coordinators who see to a task: its owner while the owner's pane is running,
/// else every running coordinator, else none (the task needs the person).
pub fn sees_to<'a>(repo: &'a Repo, t: &Obj) -> Res<Vec<&'a Obj>> {
    let own = owner(repo, t);
    if alive(own)? {
        return Ok(own.into_iter().collect());
    }
    let mut out = Vec::new();
    for c in coordinators(repo) {
        if alive(Some(c))? {
            out.push(c);
        }
    }
    Ok(out)
}

/// Where a task's panes live: with the coordinator who sees to it, else (none running)
/// with its owner, else the primary coordinator.
pub fn home_of(repo: &Repo, t: &Obj) -> Res<Obj> {
    let sees = sees_to(repo, t)?;
    let c = sees
        .first()
        .copied()
        .or_else(|| owner(repo, t))
        .or_else(|| primary(repo));
    Ok(c.cloned().unwrap_or_default())
}

/// The coordinator whose own pane ran this command, or None. Rex gives each pane its
/// block ID as REX_BLOCK. Unsure (no REX_BLOCK, or a stored block that is out of date)
/// counts as none.
pub fn calling_coordinator(repo: &Repo) -> Option<&Obj> {
    coordinator_at(repo, &std::env::var("REX_BLOCK").unwrap_or_default(), false)
}
