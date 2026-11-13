//! The capsule (Float): a minimized floating pane in the window of its task's
//! coordinator. `go` brings it up, the hide key minimizes it again. The workers tab's
//! panes (tab.rs) are shown and hidden the same way.

use super::{float_bounds, View};
use crate::coordinators::home_of;
use crate::rex::{find_pane, rex_api, rex_view, set_layer, Where};
use bullpen_core::json::{py_str, truthy, Obj};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

/// A Rex API call that must answer (Python's check=True and then r[...]).
pub(super) fn call(session: &str, method: &str, args: &Value) -> Res<Value> {
    match rex_api(session, method, args, true)? {
        Some(v) => Ok(v),
        None => fail(format!("rex api {method} said nothing")),
    }
}

/// A value's key as text (Python's r[key]).
pub(super) fn key(v: &Value, k: &str) -> Res<String> {
    match v.get(k) {
        Some(x) => Ok(py_str(x)),
        None => fail(format!("rex said no {k}")),
    }
}

/// The first of a reply's block_ids.
pub(super) fn first_block(v: &Value) -> Res<String> {
    match v.get("block_ids").and_then(|b| b.get(0)) {
        Some(b) => Ok(py_str(b)),
        None => fail("rex said no block_ids"),
    }
}

pub(super) fn place(repo: &Repo, t: &Obj, session: &str, spec: Value) -> Res<String> {
    let mut args = json!({"bounds": float_bounds(), "layout": {"block": spec}});
    let view = rex_view(session)?;
    let home = home_of(repo, t)?;
    if let Some(at) = find_pane(view.as_ref(), bullpen_core::json::s(&home, "block"), "") {
        args["window_id"] = json!(at.window);
    }
    let r = call(session, "session.new_layer", &args)?;
    set_layer(session, &key(&r, "layer_id")?, "minimized", false)?;
    first_block(&r)
}

pub(super) fn show(session: &str, at: &Where) -> Res<bool> {
    set_layer(session, &at.layer_id(), "normal", true)
}

pub(super) fn hide(session: &str, at: &Where) -> Res<bool> {
    set_layer(session, &at.layer_id(), "minimized", false)?;
    Ok(true)
}

pub(super) fn keep_home(session: &str, view: &mut View, block: &str, c: &Obj, home: &Where) -> Res<Option<String>> {
    let Some(at) = find_pane(view.as_ref(), block, "") else {
        return Ok(None);
    };
    if at.window == home.window {
        return Ok(None);
    }
    let anchor = c.get("block").cloned().unwrap_or(Value::Null);
    let moved = rex_api(
        session,
        "session.move_block",
        &json!({"block_id": block, "anchor_block_id": anchor}),
        false,
    )?;
    if moved.is_none() {
        return Ok(None);
    }
    let r = rex_api(
        session,
        "session.float_block",
        &json!({"block_id": block, "bounds": float_bounds()}),
        false,
    )?;
    if let Some(r) = r.filter(|r| truthy(Some(r))) {
        set_layer(session, &key(&r, "layer_id")?, "minimized", false)?;
    }
    Ok(at.window)
}
