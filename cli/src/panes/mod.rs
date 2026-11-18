//! Where a worker's, reviewer's or diff's pane lives: one placement
//! each, with the same steps (place, show, hide, find), so the code that makes, shows,
//! hides or finds a pane again never asks which one it is (docs/code.md, "Where a pane
//! lives"). Here: `place`, `show`, `hide`, `find`, `gather` (keeping each pane home),
//! where a pane is now (`placement_at`) and where a new one goes (`for_new`). A
//! placement that does more than focus has a file of its own: float.rs, tab.rs,
//! background.rs.
//!
//! A view is the session's, as `rex_view` gives it (None when Rex can't say). Finding a
//! background pane notes the session's detached blocks in it ("detached"), and making a
//! workers tab notes the tab ("tabs"), so a view is passed on to be added to.

mod background;
mod float;
mod tab;

use crate::agents::placed;
use crate::coordinators::{home_of, primary};
use crate::rex::{find_pane, rex_focus, rex_view, with_state, Where, HIDE_HINT};
use bullpen_core::json::{obj, py_str, s, Obj};
use bullpen_core::settings::{FLOAT_BOUNDS, TERMINAL};
use bullpen_core::task::is_closed;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::collections::HashSet;

#[allow(unused_imports)] // the tests use it
pub use tab::tab_label;

/// A session's view, as rex_view gives it, or None when Rex can't say.
pub type View = Option<Obj>;

/// One way a pane can live. A pane bullpen placed is found again in the session's view,
/// by its ID or its label (a Rex restart gives it a new ID).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// A minimized floating pane (a capsule) in the window of its task's coordinator:
    /// `go` brings it up, the hide key minimizes it again.
    Float,
    /// A capsule in its coordinator's workers tab (tab.rs).
    Tab,
    /// A tab of its own. Shown by focusing it; the hide key has nothing to do there.
    Tabs,
    /// A detached block: running, but in no tab (background.rs).
    Background,
}

pub use Placement::{Background, Float, Tab, Tabs};

impl Placement {
    pub fn name(self) -> &'static str {
        match self {
            Float => "float",
            Tab => "tab",
            Tabs => "tabs",
            Background => "background",
        }
    }

    /// The placement by its name (a setting's or a task's), or None.
    pub fn named(name: &str) -> Option<Placement> {
        [Float, Tab, Tabs, Background].into_iter().find(|p| p.name() == name)
    }

    /// Does `gather` keep its panes in their coordinator's tab?
    fn gathers(self) -> bool {
        matches!(self, Float | Tab)
    }

    /// Is it a capsule (Python's isinstance(p, Float))?
    fn floats(self) -> bool {
        matches!(self, Float | Tab)
    }

    /// Start argv in task t's worktree, in a new pane labelled label, and return its
    /// block. hint: it's an agent's pane, which the hide key puts away; focus: show it now.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        self,
        repo: &Repo,
        t: &Obj,
        session: &str,
        label: &str,
        argv: &[String],
        hint: bool,
        focus: bool,
    ) -> Res<String> {
        let spec = spec(t, label, argv, hint);
        match self {
            Float => float::place(repo, t, session, spec),
            Tab => tab::place(repo, t, session, spec),
            Tabs => crate::rex::rex_run(
                session,
                std::path::Path::new(&py_str(&t["worktree"])),
                label,
                argv,
                focus,
            ),
            Background => background::place(repo, session, spec, focus),
        }
    }

    /// Bring up the pane at where (find's). False if the caller should focus it.
    pub fn show(self, repo: &Repo, session: &str, at: &Where, view: &mut View) -> Res<bool> {
        match self {
            Float | Tab => float::show(session, at),
            Tabs => {
                rex_focus(session, &at.block)?;
                Ok(true)
            }
            Background => background::show(repo, session, at, view),
        }
    }

    /// Put the pane at where away. False if this placement has no away to put it.
    pub fn hide(self, session: &str, at: &Where) -> Res<bool> {
        match self {
            Float | Tab => float::hide(session, at),
            Tabs => Ok(false),
            Background => background::hide(session, at),
        }
    }

    /// gather: the pane is in another tab than its coordinator's (c, at home): move it
    /// there. The window it left, else None.
    fn keep_home(
        self,
        repo: &Repo,
        session: &str,
        view: &mut View,
        block: &str,
        c: &Obj,
        home: &Where,
    ) -> Res<Option<String>> {
        match self {
            Float => float::keep_home(session, view, block, c, home),
            Tab => tab::keep_home(repo, session, view, block, c),
            _ => Ok(None),
        }
    }
}

/// Where a detached block is, as find gives it: in no window, on no layer.
pub fn detached(block: &str) -> Where {
    let mut layer = Obj::new();
    layer.insert("kind".into(), json!("detached"));
    Where {
        window: None,
        layer,
        block: block.into(),
    }
}

/// FLOAT_BOUNDS as JSON.
fn float_bounds() -> Value {
    json!(FLOAT_BOUNDS)
}

/// The terminal block a placement makes for task t (the hint: the hide key's, in its label).
fn spec(t: &Obj, label: &str, argv: &[String], hint: bool) -> Value {
    let label = format!("{label}{}", if hint { HIDE_HINT } else { "" });
    let cwd = t.get("worktree").map_or("None".into(), py_str);
    json!({"flavor": TERMINAL, "label": label, "options": {"command": with_state(argv), "cwd": cwd}})
}

/// A block in the tab a background pane comes up over: its task's coordinator's, else
/// the primary coordinator's, else whatever has focus in the active window.
fn anchor_for(repo: &Repo, view: &Obj, block: &str) -> Res<Option<String>> {
    let mut cs = Vec::new();
    if let Some(t) = owner_task(repo, block)? {
        cs.push(home_of(repo, &t)?);
    }
    cs.push(primary(repo).cloned().unwrap_or_default());
    for c in cs {
        if find_pane(Some(view), s(&c, "block"), "").is_some() {
            return Ok(Some(py_str(&c["block"])));
        }
    }
    Ok(focused_block(view))
}

/// The block that has focus in the view's active window, if it says.
fn focused_block(view: &Obj) -> Option<String> {
    let active = view.get("active_window_id");
    let windows = view.get("windows").and_then(Value::as_array);
    let w = windows
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|w| w.get("window_id") == active.or(Some(&Value::Null)))?;
    w.get("focused_block_id").filter(|b| !b.is_null()).map(py_str)
}

/// The block of the task's worker or reviewer pane ("" for none).
fn block_of(t: &Obj, who: &str) -> String {
    crate::agents::pane(t, who).1
}

/// The task whose worker or reviewer pane block is, or None.
fn owner_task(repo: &Repo, block: &str) -> Res<Option<Obj>> {
    Ok(repo
        .tasks()?
        .into_iter()
        .find(|t| block == block_of(t, "worker") || block == text(obj(t, "review").get("block"))))
}

/// A value as the str a block is ("" for none).
fn text(v: Option<&Value>) -> String {
    v.filter(|v| !v.is_null()).map_or(String::new(), py_str)
}

fn setting(repo: &Repo, key: &str) -> Option<Placement> {
    Placement::named(repo.cfg.get(key).and_then(Value::as_str).unwrap_or(""))
}

/// The placement for a new pane of task t's: role is "worker", "reviewer" or "diff".
/// Workers go where the task's own choice (`start --pane`) says, else the `workers`
/// setting; reviewers follow the `reviewers` setting, or with "same" the workers'. A
/// diff floats, or gets a tab when workers get one each: never in the background, since
/// it's made to be looked at.
pub fn for_new(repo: &Repo, t: &Obj, role: &str) -> Placement {
    let own = Placement::named(t.get("placement").and_then(Value::as_str).unwrap_or(""));
    let workers = own.or_else(|| setting(repo, "workers")).unwrap_or(Float);
    if role == "diff" {
        return if workers == Tabs { Tabs } else { Float };
    }
    if role == "reviewer" && repo.cfg.get("reviewers").and_then(Value::as_str) != Some("same") {
        return setting(repo, "reviewers").unwrap_or(Background);
    }
    workers
}

/// The placement task t's worker or reviewer pane was made with: recorded since
/// background panes came; before that, workers and reviewers followed `workers`.
pub fn made_with(repo: &Repo, t: &Obj, role: &str) -> Placement {
    Placement::named(&placed(t, role)).unwrap_or(if repo.cfg_str("workers") == "tabs" { Tabs } else { Float })
}

/// The placement that shows and hides a pane, by where it is now (find's), not by the
/// setting: a pane started before the setting changed keeps the way it was placed. A
/// background pane that's up floats like a capsule: made says which it is.
pub fn placement_at(at: Option<&Where>, made: Option<Placement>) -> Option<Placement> {
    let at = at?;
    if made == Some(Background) || at.kind() == "detached" {
        return Some(Background);
    }
    if at.kind() == "floating" {
        return Some(made.filter(|m| m.floats()).unwrap_or(Float));
    }
    Some(Tabs)
}

/// The placement block (a worker's or reviewer's pane) was made with, or None.
fn made(repo: &Repo, block: &str) -> Res<Option<Placement>> {
    let Some(t) = owner_task(repo, block)? else {
        return Ok(None);
    };
    let role = if block_of(&t, "worker") == block {
        "worker"
    } else {
        "reviewer"
    };
    Ok(Some(made_with(repo, &t, role)))
}

/// Start a pane for task t where its role's placement puts it: (block, the placement's
/// name), which the caller records with the pane.
#[allow(clippy::too_many_arguments)]
pub fn place(
    repo: &Repo,
    t: &Obj,
    session: &str,
    label: &str,
    argv: &[String],
    role: &str,
    hint: bool,
    focus: bool,
) -> Res<(String, &'static str)> {
    let p = for_new(repo, t, role);
    Ok((p.place(repo, t, session, label, argv, hint, focus)?, p.name()))
}

/// Where a worker's or reviewer's pane is, by ID, else by label, wherever it lives, or
/// None. A detached one has no window, and its layer's kind is "detached".
pub fn find(session: &str, view: &mut View, block: &str, label: &str) -> Res<Option<Where>> {
    // the view, as a tab's would be looked through; then the detached blocks
    if let Some(hit) = find_pane(view.as_ref(), block, label) {
        return Ok(Some(hit));
    }
    background::find(session, view, block, label)
}

/// The blocks of every worker and reviewer pane the repo knows.
fn mine(repo: &Repo) -> Res<HashSet<String>> {
    let mut out = HashSet::new();
    for t in repo.tasks()? {
        for b in [block_of(&t, "worker"), text(obj(&t, "review").get("block"))] {
            if !b.is_empty() {
                out.insert(b);
            }
        }
    }
    Ok(out)
}

/// Put away every worker's or reviewer's pane that floats over a tab, but not the pane
/// keep (a block): a capsule is minimized, a background pane detached.
fn put_away_others(repo: &Repo, session: &str, view: &Obj, keep: &str) -> Res<()> {
    let ours = mine(repo)?;
    let windows = view.get("windows").and_then(Value::as_array);
    for w in windows.into_iter().flatten().filter_map(Value::as_object) {
        let layers = w.get("layers").and_then(Value::as_array);
        for layer in layers.into_iter().flatten().filter_map(Value::as_object) {
            if s(layer, "kind") != "floating" || s(layer, "state") == "minimized" {
                continue;
            }
            let blocks: Vec<&Obj> = layer
                .get("blocks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .collect();
            if blocks.iter().any(|b| s(b, "block_id") == keep) {
                continue;
            }
            if let Some(b) = blocks.iter().map(|b| s(b, "block_id")).find(|b| ours.contains(*b)) {
                let at = Where {
                    window: Some(py_str(&w["window_id"])),
                    layer: layer.clone(),
                    block: b.into(),
                };
                if let Some(p) = placement_at(Some(&at), made(repo, b)?) {
                    p.hide(session, &at)?;
                }
            }
        }
    }
    Ok(())
}

/// Focus a pane. A worker's or reviewer's comes up from where it's put away, and any
/// other of theirs that's up is put away.
pub fn show(repo: &Repo, session: &str, block: &str) -> Res<()> {
    gather(repo, session)?;
    let mut view = rex_view(session)?;
    if view.is_none() {
        return rex_focus(session, block);
    }
    let target = find(session, &mut view, block, "")?;
    let shown = match placement_at(target.as_ref(), made(repo, block)?) {
        Some(p) => p.show(repo, session, target.as_ref().expect("placed"), &mut view)?,
        None => false,
    };
    if !shown {
        rex_focus(session, block)?;
    }
    // after, not before: Rex won't minimize the last pane up in a workers tab
    put_away_others(repo, session, view.as_ref().expect("a view"), block)
}

/// Put a pane away. False if it isn't somewhere bullpen can put it away from.
pub fn hide(repo: &Repo, session: &str, block: &str) -> Res<bool> {
    let mut view = rex_view(session)?;
    let at = find(session, &mut view, block, "")?;
    match placement_at(at.as_ref(), made(repo, block)?) {
        Some(p) => p.hide(session, at.as_ref().expect("placed")),
        None => Ok(false),
    }
}

/// Keep every open task's worker and reviewer pane where its placement says, in its
/// coordinator's tab (the primary coordinator's, when its own is gone). A coordinator
/// that was restarted opens in a new tab: the panes move there, and a tab that moving
/// them left empty closes. A workers tab's panes are kept in their coordinator's
/// workers tab instead. Tabs and background panes are left where they are. Moving a
/// pane focuses it in Rex: focus goes back to where it was.
pub fn gather(repo: &Repo, session: &str) -> Res<()> {
    let mut panes = Vec::new();
    for t in repo.tasks()? {
        if is_closed(s(&t, "status")) {
            continue;
        }
        for (b, p) in [
            (block_of(&t, "worker"), made_with(repo, &t, "worker")),
            (text(obj(&t, "review").get("block")), made_with(repo, &t, "reviewer")),
        ] {
            if !b.is_empty() && p.gathers() {
                panes.push((t.clone(), b, p));
            }
        }
    }
    if panes.is_empty() {
        return Ok(());
    }
    let mut view = rex_view(session)?;
    let fallback = primary(repo).cloned().unwrap_or_default();
    let mut emptied = HashSet::new();
    for (t, b, p) in panes {
        let mut c = home_of(repo, &t)?;
        let mut home = find_pane(view.as_ref(), s(&c, "block"), "");
        if home.is_none() {
            c = fallback.clone();
            home = find_pane(view.as_ref(), s(&c, "block"), "");
        }
        if let Some(home) = home {
            if let Some(left) = p.keep_home(repo, session, &mut view, &b, &c, &home)? {
                emptied.insert(left);
            }
        }
    }
    if emptied.is_empty() {
        return Ok(());
    }
    if let Some(was) = view.as_ref().and_then(focused_block) {
        rex_focus(session, &was)?;
    }
    let now = rex_view(session)?.unwrap_or_default();
    for w in now.get("windows").and_then(Value::as_array).into_iter().flatten() {
        let Some(w) = w.as_object() else { continue };
        let id = w.get("window_id").map_or(String::new(), py_str);
        let layers = w.get("layers").and_then(Value::as_array);
        let full = layers
            .into_iter()
            .flatten()
            .any(|l| l.get("blocks").is_some_and(|b| bullpen_core::json::truthy(Some(b))));
        if emptied.contains(&id) && !full {
            crate::rex::rex_api(
                session,
                "session.close_window",
                &json!({"window_id": w["window_id"]}),
                false,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/test_panes.rs"]
mod tests;
