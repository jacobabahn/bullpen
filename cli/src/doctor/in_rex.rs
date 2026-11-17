//! doctor's checks of bullpen's parts in Rex: its keys, and the watcher.

use super::{Check, DoctorCheck};
use crate::watcher::{watcher_copy, watcher_record, watcher_stamp_of};
use bullpen_core::json::{get, py_str, truthy, Obj};
use bullpen_core::paths::{home, rex};
use bullpen_core::proc::{run, which};
use serde_json::Value;

pub(super) const CHECKS: &[DoctorCheck] = &[KEYS, WATCHER];

const KEY_ACTIONS: [&str; 5] = [
    "bullpen_next",
    "bullpen_board",
    "bullpen_coordinator",
    "bullpen_jump",
    "bullpen_mission",
];

const KEYS: DoctorCheck = DoctorCheck {
    name: "keys",
    shared: true,
    run: |_| check_keys(),
    ..DoctorCheck::NEW
};

/// Rex lists the actions its config defines: bullpen's keys define four.
pub fn check_keys() -> Check {
    let r = rex();
    if which(&r).is_none() {
        return Ok(("skip", "can't tell without rex".into()));
    }
    let p = run(&[r.as_str(), "actions", "--json"], false)?;
    let actions = match serde_json::from_str::<Value>(&p.stdout) {
        Ok(Value::Object(o)) if p.ok() => o.get("actions").and_then(Value::as_array).cloned(),
        _ => None,
    };
    let Some(actions) = actions else {
        return Ok((
            "skip",
            "can't tell: `rex actions --json` didn't list the actions".into(),
        ));
    };
    let have: Vec<&Obj> = actions.iter().filter_map(Value::as_object).collect();
    // the last action of a name wins, as a dict built from the list keeps it
    let named = |n: &str| {
        have.iter()
            .rev()
            .find(|a| get(a, "name").and_then(Value::as_str) == Some(n))
            .copied()
    };
    let missing: Vec<&str> = KEY_ACTIONS.iter().copied().filter(|a| named(a).is_none()).collect();
    if !missing.is_empty() && missing.len() < KEY_ACTIONS.len() {
        // the line is in the config, but Rex loaded keys.lua before it had these
        return Ok((
            "fail",
            format!(
                "{} missing: Rex loaded an older keys.lua. Run `rex config reload`",
                missing.join(", ")
            ),
        ));
    }
    if !missing.is_empty() {
        return Ok(("fail", "not in your Rex config: run `bullpen keys --install`".into()));
    }
    let first = named(KEY_ACTIONS[0]).unwrap_or(have[0]);
    let source = match get(first, "source") {
        Some(v) if truthy(Some(v)) => py_str(v),
        _ => String::new(),
    };
    let src = source.rsplit_once(':').map_or(source.as_str(), |(a, _)| a);
    Ok((
        "ok",
        if src.is_empty() {
            "set up".into()
        } else {
            format!("set up, from {src}")
        },
    ))
}

const WATCHER: DoctorCheck = DoctorCheck {
    name: "watcher",
    shared: true,
    slow: true,
    run: |_| check_watcher(),
    ..DoctorCheck::NEW
};

pub fn check_watcher() -> Check {
    let w = watcher_record()?;
    if !truthy(w.get("block")) {
        return Ok(("fail", "never started: run `bullpen up`".into()));
    }
    let block = py_str(&w["block"]);
    let session = w
        .get("session")
        .filter(|v| !v.is_null())
        .map(py_str)
        .unwrap_or_default();
    if !crate::rex::alive(&session, &block)? {
        return Ok((
            "fail",
            format!("not running (its pane {block} is gone): run `bullpen up`"),
        ));
    }
    // against the copy of bullpen it runs: this one, unless its record names another
    let copy = watcher_copy(&w).unwrap_or_else(home);
    let other = !crate::copies::same(&copy, &home());
    if !crate::copies::there(&copy) {
        return Ok((
            "fail",
            format!(
                "running ({block}), but from a copy of bullpen that is gone ({}): run `bullpen up`",
                copy.display()
            ),
        ));
    }
    if w.get("stamp") != Some(&watcher_stamp_of(&copy)?) {
        let whose = if other {
            format!(" (it runs another copy of bullpen: {})", copy.display())
        } else {
            String::new()
        };
        return Ok((
            "fail",
            format!("running ({block}), but from an older watch.lua or bullpen: run `bullpen up` to restart it{whose}"),
        ));
    }
    let whose = if other {
        format!(", from another copy of bullpen ({})", copy.display())
    } else {
        String::new()
    };
    Ok(("ok", format!("running and current, {block}{whose}")))
}
