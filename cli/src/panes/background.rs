//! The background (Background): a detached block, running but in no tab, so nothing
//! shows it. `go` floats it over its coordinator's tab, with focus; the hide key
//! detaches it again. Rex's view lists only placed blocks, so a detached one is found
//! through session.list_blocks. Rex keeps detached blocks only while its server runs
//! (rexurrect doesn't save them either).

use super::float::{call, key};
use super::{anchor_for, detached, float_bounds, View};
use crate::rex::{is_label, rex_api, rex_view, set_layer, Where};
use bullpen_core::json::{py_str, s, truthy};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

pub(super) fn place(repo: &Repo, session: &str, spec: Value, focus: bool) -> Res<String> {
    let block = key(&call(session, "session.new_block", &spec)?, "block_id")?;
    if focus {
        let mut view = rex_view(session)?;
        if view.is_some() {
            show(repo, session, &detached(&block), &mut view)?;
        }
    }
    Ok(block)
}

pub(super) fn show(repo: &Repo, session: &str, at: &Where, view: &mut View) -> Res<bool> {
    if at.kind() == "floating" {
        return set_layer(session, &at.layer_id(), "normal", true);
    }
    if at.kind() != "detached" {
        return Ok(false); // moved into a tab by hand: focus it there
    }
    let Some(v) = view.as_ref() else { return Ok(false) };
    let Some(anchor) = anchor_for(repo, v, &at.block)? else {
        return Ok(false);
    };
    let args = json!({"block_id": at.block, "anchor_block_id": anchor});
    if rex_api(session, "session.move_block", &args, false)?.is_none() {
        return Ok(false);
    }
    let r = rex_api(
        session,
        "session.float_block",
        &json!({"block_id": at.block, "bounds": float_bounds()}),
        false,
    )?;
    match r.filter(|r| truthy(Some(r))) {
        Some(r) => set_layer(session, &key(&r, "layer_id")?, "normal", true),
        None => Ok(false),
    }
}

pub(super) fn hide(session: &str, at: &Where) -> Res<bool> {
    Ok(rex_api(session, "session.detach_block", &json!({"block_id": at.block}), false)?.is_some())
}

/// A detached block with this ID, else (no ID given) this label. The session's detached
/// blocks are asked for once per view, however many panes are looked for.
pub(super) fn find(session: &str, view: &mut View, block: &str, label: &str) -> Res<Option<Where>> {
    let Some(v) = view.as_mut() else { return Ok(None) };
    if !v.contains_key("detached") {
        let listed = rex_api(session, "session.list_blocks", &json!({}), false)?;
        let blocks = listed.as_ref().and_then(|l| l.get("blocks")).and_then(Value::as_array);
        let gone: Vec<Value> = blocks
            .into_iter()
            .flatten()
            .filter(|b| b.get("placement").and_then(Value::as_str) == Some("detached"))
            .cloned()
            .collect();
        v.insert("detached".into(), Value::Array(gone));
    }
    let list = v.get("detached").and_then(Value::as_array);
    for b in list.into_iter().flatten().filter_map(Value::as_object) {
        let hit = if block.is_empty() {
            is_label(b, label)
        } else {
            s(b, "block_id") == block
        };
        if hit {
            return Ok(Some(detached(&py_str(&b["block_id"]))));
        }
    }
    Ok(None)
}
