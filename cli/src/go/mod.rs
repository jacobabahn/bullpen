//! Getting around: `go`, `hide` and `adopt`, and putting a repo's panes
//! where they belong (relink.rs). `repo_or_here` is the repo a `--repo` names (`say`'s
//! too).

pub mod relink;

use crate::agents::pane;
use crate::args::{flag, pos, value, Arg, Args, Nargs};
use crate::board::{ensure_board, realpath};
use crate::coordinators::{
    alive, calling_coordinator, coordinator_at, coordinators, home_of, on_board, owner, primary, session_for,
};
use crate::panes::{gather, hide, show};
use crate::rex::{find_pane, rex_focus, rex_select, rex_view};
use crate::saves::save;
use bullpen_core::json::{self, dumps_line, obj, py_str, s, truthy, Obj};
use bullpen_core::paths::{safe, state};
use bullpen_core::repo::{find_repo, known_repos, lookup_block, repo_root};
use bullpen_core::task::{is_status, on_reviewer};
use bullpen_core::{fail, Fail, Repo, Res};
use relink::relink;
use serde_json::{json, Value};
use std::path::Path;

const GO: &[Arg] = &[
    pos("id", Nargs::Maybe, "the task (default: the coordinator)"),
    flag(
        "--next",
        "the next task that needs you; once none does, the board on one ready to accept",
    ),
    flag(
        "--all-repos",
        "with --next: in any repository bullpen knows, not just this one",
    ),
    flag(
        "--board",
        "the task board (with an ID: the task's board, with that task selected)",
    ),
    flag("--review", "the task's reviewer, not its worker"),
    value("--from", "FROM_BLOCK", "the pane you're in (default: this one)"),
    value("--session", "SESSION", "the session you're in (key bindings pass it)"),
    value(
        "--repo",
        "REPO",
        "the repository the task is in (default: the pane's, else this folder's)",
    ),
    flag("--json", "print the target instead of focusing it"),
    flag(
        "--switch",
        "and have the Rex app show the pane's session (mission control's ⏎)",
    ),
];

const HIDE: &[Arg] = &[
    value("--from", "FROM_BLOCK", "the pane to put away (default: this one)"),
    value("--session", "SESSION", "the session you're in (key bindings pass it)"),
];

const ADOPT: &[Arg] = &[pos("ID", Nargs::Some, "the tasks")];

/// `go`'s arguments, which a `go --next` that finds nothing changes as it goes on.
struct Go {
    id: String,
    next: bool,
    all_repos: bool,
    board: bool,
    review: bool,
    from_block: String,
    session: String,
    repo: String,
    json: bool,
    switch: bool,
}

/// A value as text ("" for none).
fn text(v: Option<&Value>) -> String {
    v.filter(|v| !v.is_null()).map_or(String::new(), py_str)
}

/// --from, else the pane this runs in (REX_BLOCK).
fn from_block(a: &Args) -> String {
    a.opt("--from")
        .map(str::to_string)
        .unwrap_or_else(|| std::env::var("REX_BLOCK").unwrap_or_default())
}

/// The pane's target: (repo, session, block, name).
type Target = (Repo, String, String, String);

/// What `go` should focus.
fn go_target(repo: &Repo, g: &Go) -> Res<Target> {
    if !g.next {
        let (session, block, name) = pane_target(repo, g)?;
        return Ok((repo.clone(), session, block, name));
    }
    // only over the tasks the board of the pane's coordinator shows
    let c = pane_coordinator(repo, &g.from_block)?;
    let mut needs: Vec<(Repo, Obj)> = Vec::new();
    for t in repo.tasks()? {
        if s(&t, "status") == "needs" && !needing(&t).1.is_empty() && on_board(repo, c.as_ref(), &t)? {
            needs.push((repo.clone(), t));
        }
    }
    if g.all_repos {
        // and every other repository's, whoever's board they're on
        for r in known_repos()? {
            if r.dir == repo.dir {
                continue;
            }
            for t in r.tasks()? {
                if s(&t, "status") == "needs" && !needing(&t).1.is_empty() {
                    needs.push((r.clone(), t));
                }
            }
        }
    }
    needs.sort_by_key(|(_, t)| since(t));
    if needs.is_empty() {
        let any = if g.all_repos { " in any repository" } else { "" };
        return fail(format!("nothing needs you{any}"));
    }
    let blocks: Vec<String> = needs.iter().map(|(_, t)| needing(t).1).collect();
    let i = match blocks.iter().position(|b| *b == g.from_block) {
        Some(i) => (i + 1) % needs.len(),
        None => 0,
    };
    let (r, t) = &needs[i];
    let (session, block) = needing(t);
    let name = if r.dir == repo.dir {
        s(t, "id").to_string()
    } else {
        format!("{} {}", r.name(), s(t, "id"))
    };
    Ok((r.clone(), session, block, name))
}

/// When the task entered its status (else was last saved), to order by.
fn since(t: &Obj) -> String {
    [s(t, "since"), s(t, "updated")]
        .into_iter()
        .find(|v| !v.is_empty())
        .unwrap_or("")
        .into()
}

/// (session, block) of the pane of a task that needs you: its reviewer's when the
/// reviewer is what stopped (and still has a pane), else its worker's.
fn needing(t: &Obj) -> (String, String) {
    if on_reviewer(t) && !pane(t, "reviewer").1.is_empty() {
        return pane(t, "reviewer");
    }
    pane(t, "worker")
}

/// `go --next` once nothing needs you: (repo, task) of the task ready to accept, else
/// finished, that has waited longest, on a board (the pane's coordinator's, in this
/// repository), or None.
fn next_to_accept(repo: &Repo, g: &Go) -> Res<Option<(Repo, Obj)>> {
    let c = pane_coordinator(repo, &g.from_block)?;
    let done = |t: &Obj| matches!(s(t, "status"), "ready" | "finished");
    let mut found = Vec::new();
    for t in repo.tasks()? {
        if done(&t) && on_board(repo, c.as_ref(), &t)? {
            found.push((repo.clone(), t));
        }
    }
    if g.all_repos {
        for r in known_repos()? {
            if r.dir != repo.dir {
                found.extend(r.tasks()?.into_iter().filter(done).map(|t| (r.clone(), t)));
            }
        }
    }
    let mut boarded = Vec::new();
    for (r, t) in found {
        if task_board(&r, &t).is_ok() {
            boarded.push((r, t));
        }
    }
    boarded.sort_by_key(|(_, t)| (s(t, "status") != "ready", since(t)));
    Ok(boarded.into_iter().next())
}

/// (session, block, name) of a task's pane, its reviewer's, the coordinator's or the board's.
fn pane_target(repo: &Repo, g: &Go) -> Res<(String, String, String)> {
    if !g.id.is_empty() && g.board {
        let (_, b) = task_board(repo, &repo.task(&g.id)?)?;
        return Ok((text(b.get("session")), text(b.get("block")), "board".into()));
    }
    if !g.id.is_empty() && g.review {
        let t = repo.task(&g.id)?;
        let (session, block) = pane(&t, "reviewer");
        if block.is_empty() {
            return fail(format!("{} has no reviewer running", s(&t, "id")));
        }
        return Ok((session, block, format!("{}'s reviewer", s(&t, "id"))));
    }
    if !g.id.is_empty() {
        let t = repo.task(&g.id)?;
        let (session, block) = pane(&t, "worker");
        let id = s(&t, "id");
        if block.is_empty() {
            let hint = if s(&t, "status") == "stopped" {
                format!("`bullpen resume {id}` starts it again")
            } else {
                is_status(&t)
                    .replacen(&format!("{id} is "), "it's ", 1)
                    .replacen(&format!("{id} "), "it ", 1)
            };
            return fail(format!("{id} has no pane: {hint}"));
        }
        return Ok((session, block, id.into()));
    }
    let Some(c) = coordinator_for(repo, &g.from_block)? else {
        return fail("no coordinator: run `bullpen up`");
    };
    if g.board {
        let Some(b) = any_board(repo, &c) else {
            return fail("no board: run `bullpen up`");
        };
        return Ok((text(b.get("session")), text(b.get("block")), "board".into()));
    }
    Ok((text(c.get("session")), text(c.get("block")), "coordinator".into()))
}

/// Coordinator c's board, else the first coordinator's that has one.
fn any_board(repo: &Repo, c: &Obj) -> Option<Obj> {
    let own = c.get("board").filter(|b| truthy(Some(b)));
    let other = || {
        coordinators(repo)
            .into_iter()
            .find_map(|o| o.get("board").filter(|b| truthy(Some(b))))
    };
    own.or_else(other).and_then(Value::as_object).cloned()
}

/// (coordinator, board) of a board that shows the task: its owner's, else any other
/// that shows it (one showing other coordinators' tasks too).
fn task_board(repo: &Repo, t: &Obj) -> Res<(Obj, Obj)> {
    let own = home_of(repo, t)?;
    let mut boards = vec![own.clone()];
    boards.extend(coordinators(repo).into_iter().cloned());
    boards.retain(|c| truthy(c.get("board")));
    if boards.is_empty() {
        return fail("no board: run `bullpen up`");
    }
    let mut shows = Vec::new();
    for c in boards {
        if on_board(repo, Some(&c), t)? {
            shows.push(c);
        }
    }
    // a board that's running first: one that was quit would only fail
    let mut pick = None;
    for c in &shows {
        let b = obj(c, "board");
        if crate::rex::alive(s(b, "session"), s(b, "block"))? {
            pick = Some(c.clone());
            break;
        }
    }
    let Some(c) = pick.or_else(|| shows.first().cloned()) else {
        return fail(format!(
            "no board shows {}: it is {}'s, which has no board (o on a board shows it)",
            s(t, "id"),
            text(own.get("id"))
        ));
    };
    let b = obj(&c, "board").clone();
    Ok((c, b))
}

/// Ask the board the task is on to select it. The board reads select/<its coordinator>
/// when the state directory changes, selects the task and removes the file; it ignores
/// one older than 30 seconds (no board read it then).
fn select_on_board(repo: &Repo, tid: &str) -> Res<()> {
    let (c, _) = task_board(repo, &repo.task(tid)?)?;
    let cid = Some(text(c.get("id")))
        .filter(|c| !c.is_empty())
        .unwrap_or("board".into());
    let path = repo.dir.join("select").join(safe(&cid));
    std::fs::create_dir_all(repo.dir.join("select"))?;
    std::fs::write(path, format!("{}\n", s(&repo.task(tid)?, "id")))?;
    Ok(())
}

/// The coordinator a pane goes with (pane_coordinator), else the primary one.
fn coordinator_for(repo: &Repo, block: &str) -> Res<Option<Obj>> {
    Ok(pane_coordinator(repo, block)?.or_else(|| primary(repo).cloned()))
}

/// The coordinator a pane goes with: itself, the one beside its board, or the one
/// owning its task; None for any other pane.
fn pane_coordinator(repo: &Repo, block: &str) -> Res<Option<Obj>> {
    if let Some(c) = coordinator_at(repo, block, true) {
        return Ok(Some(c.clone()));
    }
    if block.is_empty() {
        return Ok(None);
    }
    let entry = json::read_obj(&state().join("blocks").join(format!("{}.json", safe(block))))?.unwrap_or_default();
    let role = s(&entry, "role");
    if s(&entry, "repo") == repo.root_str() && !matches!(role, "coordinator" | "board") {
        let tid = role.split_whitespace().next().unwrap_or("");
        if let Ok(t) = repo.task(tid) {
            return Ok(owner(repo, &t).cloned());
        }
    }
    Ok(None)
}

/// The session to relink in: --session, else the one this runs in, else stored.
fn relink_session(g: &Go, stored: &str) -> String {
    let here = std::env::var("REX_SESSION").unwrap_or_default();
    let found = [g.session.as_str(), here.as_str(), stored]
        .into_iter()
        .find(|s| !s.is_empty());
    found.unwrap_or("").to_string()
}

/// `go`: focus a task's pane (or the coordinator's, or the board's).
pub fn cmd_go(argv: &[String]) -> Res<i32> {
    let a = Args::parse("go", argv, GO);
    let mut g = Go {
        id: a.arg("id").unwrap_or("").into(),
        next: a.flag("--next"),
        all_repos: a.flag("--all-repos"),
        board: a.flag("--board"),
        review: a.flag("--review"),
        from_block: from_block(&a),
        session: a.opt("--session").unwrap_or("").into(),
        repo: a.opt("--repo").unwrap_or("").into(),
        json: a.flag("--json"),
        switch: a.flag("--switch"),
    };
    if g.all_repos && !g.next {
        return fail("--all-repos goes with --next");
    }
    let mut home = repo_or_here(&g.repo, &g.from_block)?;
    if g.board && g.id.is_empty() {
        home = board_for_go(home, &g)?;
    }
    let (mut repo, mut session, mut block, mut name) = match go_target(&home, &g) {
        Ok(target) => target,
        Err(Fail(e)) if g.next && e.starts_with("nothing needs you") => {
            let boards = coordinators(&home).iter().any(|c| truthy(c.get("board")));
            if boards {
                // a board that was quit starts again, as `go --board` does
                home = board_for_go(home, &g)?;
            }
            if let Some((r, t)) = next_to_accept(&home, &g)? {
                // nothing needs you, but something is yours to accept or close: the board, on it
                let id = s(&t, "id");
                let name = if r.dir == home.dir {
                    id.to_string()
                } else {
                    format!("{} {id}", r.name())
                };
                eprintln!("bullpen: {e}; {name} is {}, on the board", s(&t, "status"));
                (g.next, g.board, g.id, home) = (false, true, id.to_string(), r);
            } else if boards {
                // A key that does nothing looks broken: show the board, where that's plain.
                eprintln!("bullpen: {e}; showing the board");
                (g.next, g.board) = (false, true);
            } else {
                return Err(Fail(e));
            }
            go_target(&home, &g)?
        }
        Err(e) => return Err(e),
    };
    if !crate::rex::alive(&session, &block)? {
        // after a Rex restart its ID changed (and maybe its session's)
        relink(&mut repo, &relink_session(&g, &session))?;
        home = Repo::new(&home.root)?;
        (repo, session, block, name) = go_target(&home, &g)?;
        if !crate::rex::alive(&session, &block)? {
            return fail(format!("{name}'s pane is gone"));
        }
    }
    if g.json {
        let or_null = |v: &str| if v.is_empty() { Value::Null } else { json!(v) };
        println!(
            "{}",
            dumps_line(&json!({"session": or_null(&session), "block": block, "name": name}))
        );
    } else {
        if g.board && !g.id.is_empty() {
            select_on_board(&repo, &g.id)?;
        }
        show(&repo, &session, &block)?;
        if g.switch {
            switch_to(&session, &block, &name)?;
        }
    }
    Ok(0)
}

/// `go --switch`: the pane is shown and focused in its own session; have the app show
/// that session, on the pane's window. The pane itself doesn't move.
fn switch_to(session: &str, block: &str, name: &str) -> Res<()> {
    let view = rex_view(session)?;
    let window = find_pane(view.as_ref(), block, "").and_then(|w| w.window);
    match rex_select(session, window.as_deref())? {
        None => Ok(()),
        Some(why) => fail(format!(
            "{name}'s pane is up in its own session, but Rex didn't show it: {why}"
        )),
    }
}

/// `go --board`: when the board it would show isn't running, start the pane's
/// coordinator's (as `up` would), rather than refuse. The repo, read again if a Rex
/// restart had to be caught up with.
fn board_for_go(mut repo: Repo, g: &Go) -> Res<Repo> {
    let mut c = coordinator_for(&repo, &g.from_block)?;
    if c.is_some() && !alive(c.as_ref())? {
        // after a Rex restart its ID changed (and maybe its session's)
        let stored = text(c.as_ref().and_then(|c| c.get("session")));
        relink(&mut repo, &relink_session(g, &stored))?;
        repo = Repo::new(&repo.root)?;
        c = coordinator_for(&repo, &g.from_block)?;
    }
    let Some(c) = c else { return Ok(repo) };
    if !alive(Some(&c))? {
        return Ok(repo); // go_target says why
    }
    let up = match any_board(&repo, &c) {
        Some(b) => crate::rex::alive(s(&b, "session"), s(&b, "block"))?,
        None => false,
    };
    if !up {
        ensure_board(&mut repo, &text(c.get("session")), &text(c.get("id")), None)?;
    }
    Ok(repo)
}

/// The repository a `--repo` names (`go`'s, `say`'s): path, else the one pane block or
/// this folder belongs to.
pub fn repo_or_here(path: &str, block: &str) -> Res<Repo> {
    if path.is_empty() {
        return find_repo(Some(block));
    }
    let repo = Repo::new(realpath(Path::new(path)))?;
    if !repo.dir.join("config.json").exists() {
        return fail(format!("bullpen has no tasks for {path}"));
    }
    Ok(repo)
}

/// `hide`: put the worker or reviewer pane you're in away (back into its capsule, or into
/// the background) and go back to the board. Exits 3 if its placement has no away (a tab
/// of its own), or it isn't one of bullpen's panes, so a key binding can let the key through.
pub fn cmd_hide(argv: &[String]) -> Res<i32> {
    let a = Args::parse("hide", argv, HIDE);
    let from = from_block(&a);
    let Some((repo, role)) = lookup_block(Some(&from))? else {
        return Ok(3);
    };
    if matches!(role.as_str(), "coordinator" | "board") {
        return Ok(3);
    }
    let t = repo.task(role.split_whitespace().next().unwrap_or(""))?;
    let who = if role.ends_with(" review") {
        "reviewer"
    } else {
        "worker"
    };
    let session = match a.opt("--session").filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => pane(&t, who).0,
    };
    if !hide(&repo, &session, &from)? {
        return Ok(3);
    }
    let c = home_of(&repo, &t)?;
    for p in [obj(&c, "board").clone(), c.clone()] {
        if crate::rex::alive(s(&p, "session"), s(&p, "block"))? {
            rex_focus(s(&p, "session"), &text(p.get("block")))?;
            break;
        }
    }
    Ok(0)
}

/// `adopt`: make the coordinator in this pane the task's own: its news comes here.
pub fn cmd_adopt(argv: &[String]) -> Res<i32> {
    let a = Args::parse("adopt", argv, ADOPT);
    let repo = Repo::new(repo_root()?)?;
    let Some(c) = calling_coordinator(&repo).cloned() else {
        return fail("run `adopt` from a coordinator's own pane (`bullpen up --here` makes one)");
    };
    let cid = c.get("id").cloned().unwrap_or(Value::Null);
    for tid in a.args("ID") {
        let mut t = repo.task(tid)?;
        let id = s(&t, "id").to_string();
        let was = t.get("coordinator").cloned().unwrap_or(Value::Null);
        if was == cid {
            println!("{id} is yours already");
            continue;
        }
        t.insert("coordinator".into(), cid.clone());
        save(&repo, &mut t)?;
        let was = if truthy(Some(&was)) {
            py_str(&was)
        } else {
            "nobody".into()
        };
        println!("{id} is yours now (was {was}'s): its news comes here");
    }
    let session = session_for(&repo, None)?;
    if !session.is_empty() {
        gather(&repo, &session)?; // its panes move to this coordinator's tab
    }
    Ok(0)
}
