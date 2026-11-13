//! Starting the Rex watcher (lua/watch.lua), and replacing it when bullpen's code
//! changes (bullpen/watcher.py). Python keeps its copy (a twin) for its catch-up's
//! build (board_build.py); both write the same stamp.

use crate::rex::{alive, rex_api, rex_close, with_state};
use bullpen_core::json::{read_obj, s, write_obj, Obj};
use bullpen_core::paths::{home, rex, self_bin, state, user_home, watch_lua};
use bullpen_core::proc::{file_hash, which};
use bullpen_core::settings::TERMINAL;
use bullpen_core::{fail, Res};
use serde_json::{json, Value};
use std::path::PathBuf;

fn watcher_json() -> PathBuf {
    state().join("watcher.json")
}

/// A hash of bullpen's code: the entry scripts, the Python package's modules, and the
/// Rust build (target/bullpen) when there is one. The same bytes as watcher.py hashes.
pub fn code_hash() -> Res<String> {
    let me = self_bin();
    let mut data = std::fs::read(&me)?;
    data.extend(std::fs::read(me.with_file_name("bullpen-py"))?);
    let mut modules: Vec<PathBuf> = std::fs::read_dir(home().join("bullpen"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "py"))
        .collect();
    modules.sort();
    for m in modules {
        data.extend(m.file_name().unwrap_or_default().as_encoded_bytes());
        data.push(0);
        data.extend(std::fs::read(&m)?);
    }
    let built = home().join("target/bullpen");
    if built.is_file() {
        data.extend(b"target/bullpen\0");
        data.extend(std::fs::read(built)?);
    }
    Ok(bullpen_core::sha256::hex(&data))
}

/// What a watcher runs: a hash of watch.lua and of bullpen's code. A watcher started
/// from other contents is stale.
pub fn watcher_stamp() -> Res<Value> {
    Ok(json!({"watch": file_hash(watch_lua())?, "bullpen": code_hash()?}))
}

/// The watcher's record: its block, session and stamp ({} when none was started).
pub fn watcher_record() -> Res<Obj> {
    Ok(read_obj(&watcher_json())?.unwrap_or_default())
}

/// Start the watcher unless a current one runs; one started from an older watch.lua
/// or bullpen is closed and replaced. Says what it did on stdout, or into said (`up`).
pub fn ensure_watcher(session: &str, said: Option<&mut Vec<(String, String, String)>>) -> Res<()> {
    let w = watcher_record()?;
    let stamp = watcher_stamp()?;
    let running = alive(s(&w, "session"), s(&w, "block"))?;
    if running && w.get("stamp") == Some(&stamp) {
        return Ok(());
    }
    let rex = rex();
    let argv = vec![
        which(&rex).unwrap_or(rex),
        "do".into(),
        "--all".into(),
        watch_lua().display().to_string(),
        format!("bin={}", self_bin().display()),
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
    let record = json!({"block": block, "session": session, "stamp": stamp});
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

/// After a merge that may have changed the watcher (accept): make sure a current one
/// runs. Never fails the command that called it.
pub fn refresh_watcher(repo: &bullpen_core::Repo) {
    let go = || -> Res<()> {
        let session = crate::coordinators::session_for(repo, None)?;
        if !session.is_empty() {
            ensure_watcher(&session, None)?;
        }
        Ok(())
    };
    if let Err(e) = go() {
        eprintln!("bullpen: {e}");
    }
}

/// After a build: a running watcher started from the old one is replaced (none is
/// started). Never fails the build.
pub fn restart_watcher() {
    let go = || -> Res<()> {
        let w = watcher_record()?;
        if alive(s(&w, "session"), s(&w, "block"))? {
            ensure_watcher(s(&w, "session"), None)?;
        }
        Ok(())
    };
    if let Err(e) = go() {
        eprintln!("bullpen: {e}");
    }
}
