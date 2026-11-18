//! A Rex restart brings panes back with new block IDs: `relink` finds bullpen's own
//! again by their labels. `go` runs it when the pane it would show is gone.

use crate::coordinators::alive;
use crate::news::kind::Kind;
use crate::panes::{find, View};
use crate::rex::{board_label, coordinator_label, find_pane, find_panes, review_label, rex_close, rex_view};
use crate::saves::save;
use crate::stops::stop;
use bullpen_core::json::{obj, py_str, s, truthy, Obj};
use bullpen_core::repo::{index_block, unindex_block};
use bullpen_core::task::{is_closed, is_limited, is_waiting, set_status, whose_turn};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// A value as text ("" for none).
fn text(v: Option<&Value>) -> String {
    v.filter(|v| !v.is_null()).map_or(String::new(), py_str)
}

/// Which sessions a pane recorded in one is looked for here: this session, or one that
/// no longer exists (a restart renamed it).
struct Ours<'a> {
    session: &'a str,
    view: &'a Obj,
    known: HashMap<String, bool>,
}

impl Ours<'_> {
    fn ours(&mut self, stored: &str) -> Res<bool> {
        let view = self.view;
        if stored.is_empty() || stored == self.session || stored == s(view, "session_id") || stored == s(view, "label")
        {
            return Ok(true);
        }
        if !self.known.contains_key(stored) {
            self.known.insert(stored.into(), rex_view(stored)?.is_none());
        }
        Ok(self.known[stored])
    }
}

/// Find the repo's coordinators, boards, workers and reviewers again in session, by
/// their labels. Panes that are gone are forgotten (a working task is stopped).
pub fn relink(repo: &mut Repo, session: &str) -> Res<()> {
    let Some(first) = rex_view(session)? else {
        return Ok(());
    };
    let mut ours = Ours {
        session,
        view: &first,
        known: HashMap::new(),
    };
    let mut view: View = Some(first.clone());
    let mut cs: Vec<Value> = repo
        .cfg
        .get("coordinators")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // A pane bullpen started is found again by its label; one made a coordinator with
    // `up --here` isn't (its label is its agent's): it's forgotten.
    let mut taken: HashSet<String> = HashSet::new();
    for c in cs.iter().filter_map(Value::as_object) {
        taken.insert(text(c.get("block")));
        taken.insert(text(obj(c, "board").get("block")));
    }
    let mut i = 0;
    while i < cs.len() {
        let Some(c) = cs[i].as_object_mut() else {
            i += 1;
            continue;
        };
        if find_pane(view.as_ref(), &text(c.get("block")), "").is_some() {
            c.insert("session".into(), json!(session));
            i += 1;
            continue;
        }
        if !ours.ours(&text(c.get("session")))? || alive(Some(&*c))? {
            i += 1;
            continue;
        }
        unindex_block(&text(c.get("block")))?;
        let hit = find_panes(view.as_ref(), &coordinator_label(repo))
            .into_iter()
            .find(|h| !taken.contains(&h.block));
        if let Some(hit) = hit {
            c.insert("block".into(), json!(hit.block));
            c.insert("session".into(), json!(session));
            taken.insert(hit.block.clone());
            index_block(&hit.block, repo, "coordinator")?;
            println!("coordinator {}: found again ({})", text(c.get("id")), hit.block);
            i += 1;
        } else {
            let b = text(obj(c, "board").get("block"));
            if find_pane(view.as_ref(), &b, "").is_some() {
                rex_close(session, &b)?; // its coordinator is gone
            }
            unindex_block(&b)?;
            println!("coordinator {}: its pane is gone, so it is dropped", text(c.get("id")));
            cs.remove(i);
        }
    }
    for c in cs.iter_mut().filter_map(Value::as_object_mut) {
        let b = obj(c, "board").clone();
        if !truthy(c.get("board")) {
            continue;
        }
        if find_pane(view.as_ref(), &text(b.get("block")), "").is_some() {
            if let Some(b) = c.get_mut("board").and_then(Value::as_object_mut) {
                b.insert("session".into(), json!(session));
            }
            continue;
        }
        if !ours.ours(&text(b.get("session")))? {
            continue;
        }
        unindex_block(&text(b.get("block")))?;
        // the board beside its coordinator, else any board nobody has
        let home = find_pane(view.as_ref(), &text(c.get("block")), "");
        let hits: Vec<_> = find_panes(view.as_ref(), &board_label(repo))
            .into_iter()
            .filter(|h| !taken.contains(&h.block))
            .collect();
        let beside = hits
            .iter()
            .find(|h| home.as_ref().is_some_and(|home| h.window == home.window));
        let hit = beside.or(hits.first()).map(|h| h.block.clone());
        match &hit {
            Some(hit) => {
                c.insert("board".into(), json!({"block": hit, "session": session}));
                taken.insert(hit.clone());
                index_block(hit, repo, "board")?;
                println!("board of {}: found again ({hit})", text(c.get("id")));
            }
            None => {
                c.insert("board".into(), Value::Null);
            }
        }
    }
    repo.cfg.insert("coordinators".into(), Value::Array(cs));
    repo.save()?;
    for mut t in repo.tasks()? {
        let block = text(t.get("block"));
        if is_closed(s(&t, "status")) || block.is_empty() || find(session, &mut view, &block, "")?.is_some() {
            continue;
        }
        if !ours.ours(&text(t.get("session")))? {
            continue;
        }
        unindex_block(&block)?;
        let id = s(&t, "id").to_string();
        match find(session, &mut view, "", &format!("bullpen: {id}"))? {
            Some(hit) => {
                t.insert("block".into(), json!(hit.block));
                t.insert("session".into(), json!(session));
                index_block(&hit.block, repo, &id)?;
                println!("{id}: found again ({})", hit.block);
            }
            None => {
                t.insert("block".into(), Value::Null);
                let st = s(&t, "status");
                if (st == "working" || is_waiting(st)) && !is_limited(&t) {
                    // a limited one is resumed later
                    let stopped = json!({"by": "worker", "kind": "pane_gone"});
                    set_status(&mut t, "stopped", Some("Its pane was gone"), Some(stopped));
                }
            }
        }
        save(repo, &mut t)?;
    }
    relink_reviewers(repo, session, &mut view)
}

fn relink_reviewers(repo: &Repo, session: &str, view: &mut View) -> Res<()> {
    for mut t in repo.tasks()? {
        let block = text(obj(&t, "review").get("block"));
        if is_closed(s(&t, "status")) || block.is_empty() || find(session, view, &block, "")?.is_some() {
            continue;
        }
        unindex_block(&block)?;
        let hit = find(session, view, "", &review_label(&t))?;
        if let Some(r) = t.get_mut("review").and_then(Value::as_object_mut) {
            r.insert("block".into(), hit.as_ref().map_or(Value::Null, |h| json!(h.block)));
            r.insert("session".into(), json!(session));
        }
        let id = s(&t, "id").to_string();
        if let Some(hit) = &hit {
            index_block(&hit.block, repo, &format!("{id} review"))?;
        } else if whose_turn(&t) == Some("reviewer") && !truthy(obj(&t, "review").get("verdict")) && !is_limited(&t) {
            // gone after a Rex restart (a background reviewer always is): its coordinator
            // hears of it, as of one that exited (a limited one starts again at the reset)
            let line =
                format!("{id}'s reviewer's pane was gone after a Rex restart: `bullpen review {id}` starts a new one");
            stop(
                repo,
                &mut t,
                "reviewer",
                "pane_gone",
                "Reviewer's pane was gone",
                &[line],
                Kind::Stopped,
            )?;
        }
        save(repo, &mut t)?;
    }
    Ok(())
}
