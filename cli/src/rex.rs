//! Rex (bullpen/rex.py), as far as the moved commands need it: whether a pane is there,
//! typing into one, the session's view and a pane's terminal. A session or block that
//! Python has as None is "" here: Rex is given `-s` only for a session there is.

use bullpen_core::fail::Res;
use bullpen_core::json::{truthy, Obj};
use bullpen_core::paths::rex;
use bullpen_core::proc::{run, Done};
use serde_json::Value;

/// rex, the words before, `-s session` when there's one, then the words after.
fn rex_argv(session: &str, before: &[&str], after: &[&str]) -> Vec<String> {
    let mut argv = vec![rex()];
    argv.extend(before.iter().map(|s| s.to_string()));
    if !session.is_empty() {
        argv.extend(["-s".to_string(), session.to_string()]);
    }
    argv.extend(after.iter().map(|s| s.to_string()));
    argv
}

/// Is the pane there? (`rex block inspect`, in its session when given.)
pub fn alive(session: &str, block: &str) -> Res<bool> {
    if block.is_empty() {
        return Ok(false);
    }
    let mut argv = vec![rex(), "block".into(), "inspect".into(), block.into()];
    if !session.is_empty() {
        argv.extend(["-s".into(), session.into()]);
    }
    Ok(run(&argv, false)?.ok())
}

/// Type a message (a line, or several) into a terminal and press Enter.
pub fn rex_type(session: &str, block: &str, text: &str) -> Res<()> {
    run(&rex_argv(session, &["send"], &["-b", block, text]), true)?;
    run(&rex_argv(session, &["send-key"], &["-b", block, "enter"]), true)?;
    Ok(())
}

/// Call a Rex server API method; -s fills in session_id. None when it fails or says
/// nothing; JSON that doesn't parse is an error, as json.loads raises.
pub fn rex_api(session: &str, method: &str, args: &Value, check: bool) -> Res<Option<Value>> {
    let args = bullpen_core::json::dumps_line(args);
    let p: Done = run(&rex_argv(session, &["api", "call"], &[method, &args]), check)?;
    if !p.ok() || p.stdout.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str(&p.stdout) {
        Ok(v) => Ok(Some(v)),
        Err(e) => bullpen_core::fail(format!("rex api {method} said what isn't JSON: {e}")),
    }
}

/// The session's windows, layers and blocks, or None if Rex can't say.
pub fn rex_view(session: &str) -> Res<Option<Obj>> {
    let v = rex_api(session, "session.view", &Value::Object(Obj::new()), false)?;
    Ok(match v {
        Some(Value::Object(o)) if o.contains_key("windows") => Some(o),
        _ => None,
    })
}

/// The terminal device of a pane's program, or None.
pub fn pane_tty(session: &str, block: &str) -> Res<Option<String>> {
    let p = run(&rex_argv(session, &["block", "call"], &["-b", block, "process"]), false)?;
    if !p.ok() || p.stdout.trim().is_empty() {
        return Ok(None);
    }
    let v: Value = match serde_json::from_str(&p.stdout) {
        Ok(v) => v,
        Err(e) => return bullpen_core::fail(format!("rex said what isn't JSON: {e}")),
    };
    let pid = v.get("child").and_then(|c| c.get("pid")).filter(|p| truthy(Some(p)));
    let Some(pid) = pid else { return Ok(None) };
    let pid = bullpen_core::json::py_str(pid);
    let tty = run(&["ps", "-o", "tty=", "-p", &pid], false)?.stdout;
    let tty = tty.trim();
    Ok(if tty.is_empty() || tty.starts_with('?') {
        None
    } else if tty.starts_with('/') {
        Some(tty.into())
    } else {
        Some(format!("/dev/{tty}"))
    })
}

/// A pane's screen, as `rex capture --format=vt` gives it (its colours as escape codes).
pub fn capture_vt(session: &str, block: &str) -> Res<String> {
    Ok(run(&rex_argv(session, &["capture"], &["-b", block, "--format=vt"]), false)?.stdout)
}
