//! Starting the Rex watcher (lua/watch.lua) from the copy of bullpen the repo's
//! coordinator uses, and replacing it when that copy's watch.lua or build changes (its
//! stamp: their hashes): after a merge (`refresh_watcher`) or a build.

use crate::rex::{alive, rex_api, rex_close, with_state};
use bullpen_core::json::{read_obj, s, write_obj, Obj};
use bullpen_core::paths::{home, rex, state, user_home};
use bullpen_core::proc::{file_hash, which};
use bullpen_core::settings::TERMINAL;
use bullpen_core::{fail, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn watcher_json() -> PathBuf {
    state().join("watcher.json")
}

/// What a watcher from the copy of bullpen at copy runs: a hash of its watch.lua, and
/// the bullpen it calls, by its build's stamp (the source hash target/bullpen was built
/// from; "" before the first `bullpen build`). A watcher started from other contents is stale.
pub fn watcher_stamp_of(copy: &Path) -> Res<Value> {
    let built = crate::build::CLI.stamp_in(copy)?;
    Ok(json!({"watch": file_hash(copy.join("lua/watch.lua"))?, "bullpen": s(&built, "source")}))
}

/// The watcher's record: its block, session, stamp and the copy of bullpen it runs
/// ({} when none was started).
pub fn watcher_record() -> Res<Obj> {
    Ok(read_obj(&watcher_json())?.unwrap_or_default())
}

/// The copy of bullpen the recorded watcher runs (its watch.lua, the bin/bullpen it
/// calls), or None for one started before that was recorded.
pub fn watcher_copy(w: &Obj) -> Option<PathBuf> {
    Some(s(w, "home")).filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Start the watcher from the copy of bullpen at copy unless a current one from it runs;
/// one started from an older watch.lua or bullpen, or from another copy, is closed and
/// replaced. There is one watcher for the whole state folder (every repo's panes). Says
/// what it did on stdout, or into said (`up`).
pub fn ensure_watcher(session: &str, copy: &Path, said: Option<&mut Vec<(String, String, String)>>) -> Res<()> {
    let w = watcher_record()?;
    let stamp = watcher_stamp_of(copy)?;
    let running = alive(s(&w, "session"), s(&w, "block"))?;
    let home = copy.display().to_string();
    if running && w.get("stamp") == Some(&stamp) {
        match watcher_copy(&w) {
            Some(theirs) if !crate::copies::same(&theirs, copy) => {} // the same contents, another copy's bin
            Some(_) => return Ok(()),
            None => {
                // started before the copy was recorded, from these very contents: it is this copy's
                let mut w = w;
                w.insert("home".into(), json!(home));
                return write_obj(&watcher_json(), &w).map_err(Into::into);
            }
        }
    }
    let rex = rex();
    let argv = vec![
        which(&rex).unwrap_or(rex),
        "do".into(),
        "--all".into(),
        copy.join("lua/watch.lua").display().to_string(),
        format!("bin={}", copy.join("bin/bullpen").display()),
        format!("state={}", state().display()),
    ];
    // Detached: it runs without a tab of its own.
    let args = json!({"flavor": TERMINAL, "label": "bullpen: watcher",
                      "options": {"command": with_state(&argv), "cwd": user_home()}});
    let r = rex_api(session, "session.new_block", &args, true)?;
    let block = match r.as_ref().and_then(|r| r.get("block_id")) {
        Some(Value::String(b)) if !b.is_empty() => b.clone(),
        _ => return fail("couldn't start the watcher"),
    };
    if running {
        rex_close(s(&w, "session"), s(&w, "block"))?; // only once the new one is up
    }
    let record = json!({"block": block, "session": session, "stamp": stamp, "home": home});
    write_obj(&watcher_json(), record.as_object().expect("an object"))?;
    let what = if running {
        "restarted (watch.lua or bullpen changed)"
    } else {
        "started"
    };
    match said {
        Some(said) => said.push((what.into(), "watcher".into(), block)),
        None => println!("watcher {what}: {block}"),
    }
    Ok(())
}

/// The copy of bullpen whose watcher a command in repo sees to: the one the repo's
/// coordinator was started from (copies.rs), whichever copy runs the command. None when
/// that copy is gone (doctor says so: no guess), or, with no copy recorded for the repo,
/// when a watcher from another copy runs: it is left alone.
fn copy_for(repo: &bullpen_core::Repo) -> Res<Option<PathBuf>> {
    if let Some(copy) = crate::copies::coordinator_copy(repo) {
        return Ok(crate::copies::there(&copy).then_some(copy));
    }
    let w = watcher_record()?;
    let others = watcher_copy(&w).is_some_and(|c| !crate::copies::same(&c, &home()));
    if others && alive(s(&w, "session"), s(&w, "block"))? {
        return Ok(None);
    }
    Ok(Some(home()))
}

/// After a merge that may have changed the watcher (accept): make sure a current one
/// runs, from the repo's copy of bullpen (copy_for), never this one in its place. Never
/// fails the command that called it.
pub fn refresh_watcher(repo: &bullpen_core::Repo) {
    let go = || -> Res<()> {
        let session = crate::coordinators::session_for(repo, None)?;
        let Some(copy) = copy_for(repo)? else { return Ok(()) };
        if !session.is_empty() {
            ensure_watcher(&session, &copy, None)?;
        }
        Ok(())
    };
    if let Err(e) = go() {
        eprintln!("bullpen: {e}");
    }
}

/// After a build of this copy: a running watcher of this copy's, started from the old
/// build, is replaced (none is started, and another copy's is left alone). Never fails
/// the build.
pub fn restart_watcher() {
    let go = || -> Res<()> {
        let w = watcher_record()?;
        let ours = watcher_copy(&w).is_none_or(|c| crate::copies::same(&c, &home()));
        if ours && alive(s(&w, "session"), s(&w, "block"))? {
            ensure_watcher(s(&w, "session"), &home(), None)?;
        }
        Ok(())
    };
    if let Err(e) = go() {
        eprintln!("bullpen: {e}");
    }
}
