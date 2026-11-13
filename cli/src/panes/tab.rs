//! The workers tab (Tab): a capsule in its coordinator's workers tab (`bullpen: <repo>
//! workers`), one tab per coordinator, made without taking focus the first time a pane
//! goes there and found again by its label. Shown and hidden as any capsule: `go`
//! switches to the tab, the hide key minimizes it and goes back to the board's tab. Rex
//! keeps one pane up in a tab that has only floating ones, so the last one up there
//! stays up. Rex closes the tab when its last pane closes.

use super::float::{call, first_block, key};
use super::View;
use crate::coordinators::home_of;
use crate::rex::{find_pane, rex_api, rex_close, rex_view, set_layer};
use bullpen_core::json::{py_str, truthy, Obj};
use bullpen_core::settings::TERMINAL;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// Where a pane floats in a workers tab: nearly all of it, as the tab holds nothing else.
const TAB_BOUNDS: [f64; 4] = [0.02, 0.02, 0.96, 0.9];

/// The label of coordinator c's workers tab: the first coordinator's has no ID in it.
pub fn tab_label(repo: &Repo, c: &Obj) -> String {
    let cid = c.get("id").filter(|v| truthy(Some(v))).map(py_str);
    let mine = cid.filter(|cid| cid != "c1").map(|cid| format!(" {cid}"));
    format!("bullpen: {} workers{}", repo.name(), mine.unwrap_or_default())
}

/// The window ID of coordinator c's workers tab, or None.
fn workers_tab(repo: &Repo, view: &View, c: &Obj) -> Option<String> {
    let label = tab_label(repo, c);
    let view = view.as_ref()?;
    let made = view.get("tabs").and_then(|t| t.get(&label)).filter(|w| truthy(Some(w)));
    if let Some(w) = made {
        return Some(py_str(w));
    }
    let windows = view.get("windows").and_then(Value::as_array);
    windows
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|w| w.get("label").and_then(Value::as_str) == Some(label.as_str()))
        .map(|w| py_str(&w["window_id"]))
}

pub(super) fn place(repo: &Repo, t: &Obj, session: &str, spec: Value) -> Res<String> {
    let view = rex_view(session)?;
    let c = home_of(repo, t)?;
    let w = workers_tab(repo, &view, &c);
    let layout = json!({"block": spec});
    if let Some(w) = w {
        let r = call(
            session,
            "session.new_layer",
            &json!({"window_id": w, "bounds": TAB_BOUNDS, "layout": layout}),
        )?;
        set_layer(session, &key(&r, "layer_id")?, "minimized", false)?;
        return first_block(&r);
    }
    let args = json!({"focus": false, "window_label": tab_label(repo, &c), "layout": layout});
    let block = first_block(&call(session, "session.new_window", &args)?)?;
    rex_api(
        session,
        "session.float_block",
        &json!({"block_id": block, "bounds": TAB_BOUNDS}),
        false,
    )?;
    Ok(block) // the tab's only pane: it stays up
}

pub(super) fn keep_home(repo: &Repo, session: &str, view: &mut View, block: &str, c: &Obj) -> Res<Option<String>> {
    let at = find_pane(view.as_ref(), block, "");
    let w = workers_tab(repo, view, c);
    let Some(at) = at else { return Ok(None) };
    if at.window == w {
        return Ok(None);
    }
    Ok(if into_tab(repo, session, view, block, c, w)? {
        at.window
    } else {
        None
    })
}

/// Move block, a pane, into coordinator c's workers tab (w, or a new one), floating. Rex
/// places a block only beside another, and one placed beside a floating block joins its
/// layer: so it's placed beside a stand-in, on a layer (or in a tab) of its own, which
/// then closes. Did it move?
fn into_tab(repo: &Repo, session: &str, view: &mut View, block: &str, c: &Obj, w: Option<String>) -> Res<bool> {
    let stand_in = json!({"flavor": TERMINAL, "label": "bullpen: stand-in", "options": {"command": ["sleep", "60"]}});
    let layout = json!({"block": stand_in});
    let r = match &w {
        Some(w) => rex_api(
            session,
            "session.new_layer",
            &json!({"window_id": w, "bounds": TAB_BOUNDS, "layout": layout}),
            false,
        )?,
        None => {
            let label = tab_label(repo, c);
            let args = json!({"focus": false, "window_label": label, "layout": layout});
            let r = rex_api(session, "session.new_window", &args, false)?.filter(|r| truthy(Some(r)));
            if let (Some(r), Some(v)) = (&r, view.as_mut()) {
                // later panes go there too
                let tabs = v.entry("tabs").or_insert_with(|| json!({}));
                if let Some(tabs) = tabs.as_object_mut() {
                    tabs.insert(label, r.get("window_id").cloned().unwrap_or(Value::Null));
                }
            }
            r
        }
    };
    let Some(r) = r.filter(|r| truthy(Some(r))) else {
        return Ok(false);
    };
    let stand_in = first_block(&r)?;
    let moved = rex_api(
        session,
        "session.move_block",
        &json!({"block_id": block, "anchor_block_id": stand_in}),
        false,
    )?
    .is_some();
    if moved && w.is_none() {
        rex_api(
            session,
            "session.float_block",
            &json!({"block_id": block, "bounds": TAB_BOUNDS}),
            false,
        )?;
    }
    rex_close(session, &stand_in)?;
    if moved && w.is_some() {
        set_layer(session, &key(&r, "layer_id")?, "minimized", false)?;
    }
    Ok(moved)
}
