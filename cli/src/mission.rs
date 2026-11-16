//! `mission`: mission control, every repo on one screen, in a Rex session of its own
//! (`mission`). It makes the session, or a tab in it, when mission control isn't
//! running, then focuses it; `cmd+;` `m` runs it (lua/keys.lua). `--here` runs the
//! screen in this terminal: what its pane runs (the board's program, `--mission`).
//!
//! Where it runs is kept in <state>/mission.json: {"session", "block"}.

use crate::args::{flag, value, Arg, Args};
use crate::board::board_exe;
use crate::rex::{alive, rex_focus, rex_run, with_state};
use bullpen_core::json::{dumps, read_obj, s, Obj};
use bullpen_core::paths::{rex, self_bin, state, user_home};
use bullpen_core::proc::run;
use bullpen_core::{fail, Res};
use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

/// The session mission control lives in, and its pane's label.
pub const SESSION: &str = "mission";
pub const LABEL: &str = "bullpen: mission";

const MISSION: &[Arg] = &[
    flag("--here", "run mission control in this terminal (what its pane runs)"),
    value(
        "--from",
        "BLOCK",
        "the pane the key was pressed in (lua/keys.lua passes it; not needed)",
    ),
    value(
        "--session",
        "SESSION",
        "the session the key was pressed in (not needed)",
    ),
];

pub fn cmd_mission(argv: &[String]) -> Res<i32> {
    let a = Args::parse("mission", argv, MISSION);
    if a.flag("--here") {
        return here();
    }
    let (session, block, made) = match running()? {
        Some((session, block)) => (session, block, false),
        None => {
            let (session, block) = start()?;
            save(&session, &block)?;
            (session, block, true)
        }
    };
    rex_focus(&session, &block)?;
    let what = if made { "started" } else { "focused" };
    println!("mission control {what}: {block}");
    Ok(0)
}

fn record() -> PathBuf {
    state().join("mission.json")
}

/// Mission control's pane, while it's open: (session, block).
fn running() -> Res<Option<(String, String)>> {
    let rec: Obj = read_obj(&record()).ok().flatten().unwrap_or_default();
    let (session, block) = (s(&rec, "session"), s(&rec, "block"));
    Ok(alive(session, block)?.then(|| (session.to_string(), block.to_string())))
}

fn save(session: &str, block: &str) -> Res<()> {
    let text = dumps(&json!({"session": session, "block": block}));
    let _ = std::fs::create_dir_all(state());
    if let Err(e) = std::fs::write(record(), text + "\n") {
        return fail(format!("can't write {}: {e}", record().display()));
    }
    Ok(())
}

/// What mission control's pane runs.
fn argv() -> Vec<String> {
    with_state(&[self_bin().display().to_string(), "mission".into(), "--here".into()])
}

/// Start mission control: in a new tab of the `mission` session when there is one (its
/// pane was closed, or Rex restarted it), else in a new session of that name.
fn start() -> Res<(String, String)> {
    if let Some(session) = session_id()? {
        let block = rex_run(&session, std::path::Path::new(&user_home()), LABEL, &argv(), true)?;
        return Ok((session, block));
    }
    let mut cmd = vec![rex(), "new".into(), SESSION.into(), "--json".into()];
    cmd.extend(["--window".into(), "Mission".into(), "-c".into()]);
    cmd.push(user_home());
    cmd.push("--".into());
    cmd.extend(argv());
    let out = run(&cmd, true)?.stdout;
    let v: Value = match serde_json::from_str(&out) {
        Ok(v) => v,
        Err(e) => return fail(format!("rex new said what isn't JSON: {e}")),
    };
    let session = v.get("session_id").and_then(Value::as_str);
    let block = v.pointer("/initial_windows/0/block_ids/0").and_then(Value::as_str);
    match (session, block) {
        (Some(s), Some(b)) => Ok((s.into(), b.into())),
        _ => fail("rex new said no session or block"),
    }
}

/// The ID of the session labelled `mission`, if there is one.
fn session_id() -> Res<Option<String>> {
    let p = run(&[rex(), "ls".into(), "--json".into()], false)?;
    let v: Value = serde_json::from_str(&p.stdout).unwrap_or(Value::Null);
    let sessions = v.get("sessions").and_then(Value::as_array);
    let found = sessions
        .into_iter()
        .flatten()
        .find(|x| x.get("label").and_then(Value::as_str) == Some(SESSION));
    Ok(found
        .and_then(|x| x.get("session_id"))
        .and_then(Value::as_str)
        .map(String::from))
}

/// `--here`: the board's program as mission control, in this terminal.
fn here() -> Res<i32> {
    let exe = board_exe();
    if !exe.is_file() {
        return fail(format!(
            "mission control isn't built: run {} (needs cargo)",
            bullpen_core::paths::home().join("install.sh").display()
        ));
    }
    let err = std::process::Command::new(&exe)
        .args(["--mission".as_ref(), "--state".as_ref(), state().as_os_str()])
        .args(["--bullpen".as_ref(), self_bin().as_os_str()])
        .exec();
    fail(format!("couldn't run {}: {err}", exe.display()))
}
