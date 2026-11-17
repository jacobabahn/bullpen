//! Rex: running its commands, and finding, typing into and closing
//! panes. A session or block that Python has as None is "" here: Rex is given `-s` only
//! for a session there is.

use bullpen_core::fail::Res;
use bullpen_core::json::{py_str, s, truthy, Obj};
use bullpen_core::paths::rex;
use bullpen_core::proc::{run, Done};
use bullpen_core::Repo;
use serde_json::{json, Value};
use std::path::Path;

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

/// Type msg into the task's worker. None if it went through, else why not (the caller
/// leaves the task where it was and says so).
pub fn type_to_worker(t: &Obj, msg: &str) -> Res<Option<String>> {
    type_into(s(t, "session"), s(t, "block"), msg)
}

/// Type msg into a pane and press Enter. None if it went through, else why not.
pub fn type_into(session: &str, block: &str, msg: &str) -> Res<Option<String>> {
    for argv in [
        rex_argv(session, &["send"], &["-b", block, msg]),
        rex_argv(session, &["send-key"], &["-b", block, "enter"]),
    ] {
        let p = run(&argv, false)?;
        if !p.ok() {
            let said = if p.stderr.is_empty() { &p.stdout } else { &p.stderr };
            let said = bullpen_core::text::squash(said);
            let why = if said.is_empty() {
                "typing into its pane failed".into()
            } else {
                said
            };
            return Ok(Some(bullpen_core::task::clip(&why, 160)));
        }
    }
    Ok(None)
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

/// Panes Rex starts don't inherit our environment: carry bullpen's own settings
/// (BULLPEN_BOARD too, so a board runs the program doctor and `up` expect of it).
pub fn with_state(argv: &[String]) -> Vec<String> {
    let env: Vec<String> = ["BULLPEN_STATE", "BULLPEN_GH", "BULLPEN_REX", "BULLPEN_BOARD"]
        .iter()
        .filter_map(|k| {
            std::env::var(k)
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| format!("{k}={v}"))
        })
        .collect();
    if env.is_empty() {
        return argv.to_vec();
    }
    let mut out = vec!["env".to_string()];
    out.extend(env);
    out.extend(argv.iter().cloned());
    out
}

/// `rex run`: argv in a new tab, labelled label, in cwd; its block.
pub fn rex_run(session: &str, cwd: &Path, label: &str, argv: &[String], focus: bool) -> Res<String> {
    let focus = format!("--focus={}", if focus { "true" } else { "false" });
    let cwd = cwd.display().to_string();
    let mut cmd = vec![
        rex(),
        "run".into(),
        "--json".into(),
        focus,
        "--label".into(),
        label.into(),
    ];
    cmd.extend(["-c".into(), cwd]);
    if !session.is_empty() {
        cmd.extend(["-s".into(), session.into()]);
    }
    cmd.push("--".into());
    cmd.extend(with_state(argv));
    first_block(&run(&cmd, true)?.stdout)
}

/// The first of block_ids in what `rex run` or `rex split` printed.
pub fn first_block(out: &str) -> Res<String> {
    let v: Value = match serde_json::from_str(out) {
        Ok(v) => v,
        Err(e) => return bullpen_core::fail(format!("rex said what isn't JSON: {e}")),
    };
    match v.get("block_ids").and_then(|b| b.get(0)) {
        Some(b) => Ok(py_str(b)),
        None => bullpen_core::fail("rex said no block_ids"),
    }
}

pub fn rex_focus(session: &str, block: &str) -> Res<()> {
    run(&rex_argv(session, &["focus"], &[block]), true)?;
    Ok(())
}

/// Have the Rex app show a session (on this window, when given): `rex do session.select`.
/// Focusing a pane changes only its own session, not the session the app is showing.
/// The app is REX_CLIENT's, else the one connected. None if it went, else why not.
pub fn rex_select(session: &str, window: Option<&str>) -> Res<Option<String>> {
    let mut argv = vec![
        rex(),
        "do".into(),
        "session.select".into(),
        format!("session_id={session}"),
    ];
    if let Some(w) = window.filter(|w| !w.is_empty()) {
        argv.push(format!("window_id={w}"));
    }
    let p = run(&argv, false)?;
    if p.ok() {
        return Ok(None);
    }
    let said = bullpen_core::text::squash(if p.stderr.is_empty() { &p.stdout } else { &p.stderr });
    Ok(Some(if said.is_empty() {
        "Rex didn't say why".into()
    } else {
        said
    }))
}

/// Close a pane this command may run in: last, after the work it set off (run_later),
/// since closing it may end this process.
pub fn close_last(session: &str, block: &str) -> Res<()> {
    crate::saves::run_later();
    let _ = std::io::Write::flush(&mut std::io::stdout());
    rex_close(session, block)
}

pub fn rex_close(session: &str, block: &str) -> Res<()> {
    if !block.is_empty() {
        let mut argv = vec![rex(), "block".into(), "close".into(), block.into()];
        if !session.is_empty() {
            argv.extend(["-s".into(), session.into()]);
        }
        run(&argv, false)?;
    }
    Ok(())
}

/// A worker's or reviewer's floating pane says how to put it away (lua/keys.lua's
/// default key). Its label is then "bullpen: BP-3 · ctrl+esc hides"; it's found by
/// "bullpen: BP-3" all the same, as are panes started before the hint.
pub const HIDE_HINT: &str = " · ctrl+esc hides";

/// Does block b (as the view lists it) have this label, with or without the hint?
pub fn is_label(b: &Obj, label: &str) -> bool {
    let Some(Value::String(l)) = b.get("label") else {
        return false;
    };
    !label.is_empty() && (l == label || l.strip_suffix(HIDE_HINT) == Some(label))
}

/// Where a pane is, as find_pane gives it: its window (none for a detached one), its
/// layer (as the view has it; {"kind": "detached"} for a detached one) and its block.
#[derive(Debug, Clone, PartialEq)]
pub struct Where {
    pub window: Option<String>,
    pub layer: Obj,
    pub block: String,
}

impl Where {
    /// The layer's kind ("tiled", "floating", "detached"), "" if it has none.
    pub fn kind(&self) -> &str {
        s(&self.layer, "kind")
    }

    pub fn layer_id(&self) -> String {
        self.layer.get("layer_id").map_or(String::new(), py_str)
    }
}

/// The view's windows, layers and blocks, each with its window ID and layer.
fn placed(view: Option<&Obj>) -> Vec<(String, &Obj, &Obj)> {
    let mut out = Vec::new();
    let windows = view.and_then(|v| v.get("windows")).and_then(Value::as_array);
    for w in windows.into_iter().flatten().filter_map(Value::as_object) {
        let wid = w.get("window_id").map_or(String::new(), py_str);
        let layers = w.get("layers").and_then(Value::as_array);
        for layer in layers.into_iter().flatten().filter_map(Value::as_object) {
            let blocks = layer.get("blocks").and_then(Value::as_array);
            for b in blocks.into_iter().flatten().filter_map(Value::as_object) {
                out.push((wid.clone(), layer, b));
            }
        }
    }
    out
}

fn where_of(wid: String, layer: &Obj, b: &Obj) -> Where {
    let block = b.get("block_id").map_or(String::new(), py_str);
    Where {
        window: Some(wid),
        layer: layer.clone(),
        block,
    }
}

/// Where a placed block is, by ID, else (no ID given) by label.
pub fn find_pane(view: Option<&Obj>, block: &str, label: &str) -> Option<Where> {
    placed(view)
        .into_iter()
        .find(|(_, _, b)| {
            if block.is_empty() {
                is_label(b, label)
            } else {
                b.get("block_id").and_then(Value::as_str) == Some(block)
            }
        })
        .map(|(w, l, b)| where_of(w, l, b))
}

/// Where every placed block with this label is.
pub fn find_panes(view: Option<&Obj>, label: &str) -> Vec<Where> {
    placed(view)
        .into_iter()
        .filter(|(_, _, b)| is_label(b, label))
        .map(|(w, l, b)| where_of(w, l, b))
        .collect()
}

/// Set a floating layer's state ("normal", "minimized"); did Rex take it?
pub fn set_layer(session: &str, layer_id: &str, state: &str, focus: bool) -> Res<bool> {
    let mut args = json!({"layer_id": layer_id, "state": state});
    if focus {
        args["focus"] = json!(true);
    }
    Ok(rex_api(session, "session.set_layer_state", &args, false)?.is_some())
}

pub fn coordinator_label(repo: &Repo) -> String {
    format!("bullpen: {}", repo.name())
}

pub fn board_label(repo: &Repo) -> String {
    format!("bullpen: {} board", repo.name())
}

pub fn review_label(t: &Obj) -> String {
    format!("bullpen: {} review", s(t, "id"))
}

/// The last n lines of a pane's screen (Python's lines[-n:]), without the blank ones at
/// the end. vt keeps its colours, as escape codes; a gap the screen skips over (cursor
/// forward) is spaces, as a reader of the codes would otherwise run its words together.
pub fn screen_end(session: &str, block: &str, n: i64, vt: bool) -> Res<Vec<String>> {
    let before: &[&str] = if vt { &["capture", "--format=vt"] } else { &["capture"] };
    let mut out = run(&rex_argv(session, before, &["-b", block]), true)?.stdout;
    if vt {
        out = forward_as_spaces(&out);
    }
    let mut lines: Vec<String> = out.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
    while lines
        .last()
        .is_some_and(|l| bullpen_core::text::strip(&without_sgr(l)).is_empty())
    {
        lines.pop();
    }
    let len = lines.len() as i64;
    let start = if n > 0 { len - n } else { -n }.clamp(0, len);
    Ok(lines.split_off(start as usize))
}

/// The escape sequences ESC [ <digits> C (cursor forward) as that many spaces (one for none).
pub fn forward_as_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("\x1b[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if after[digits..].starts_with('C') {
            let n = if digits == 0 {
                1
            } else {
                after[..digits].parse().unwrap_or(usize::MAX)
            };
            out.push_str(&" ".repeat(n));
            rest = &after[digits + 1..];
        } else {
            out.push_str("\x1b[");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The text without its colour codes (ESC [ <digits ; :> m).
pub fn without_sgr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("\x1b[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let params = after.len()
            - after
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == ';' || c == ':')
                .len();
        if after[params..].starts_with('m') {
            rest = &after[params + 1..];
        } else {
            out.push_str("\x1b[");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The JSON a pane's program is, as `rex block call … process` says (its "child"), or
/// None when Rex can't say.
pub fn pane_process(session: &str, block: &str) -> Res<Option<Value>> {
    let p = run(&rex_argv(session, &["block", "call"], &["-b", block, "process"]), false)?;
    if !p.ok() {
        return Ok(None);
    }
    Ok(serde_json::from_str(&p.stdout).ok())
}

/// The text on a pane's screen, or "" if Rex can't say.
pub fn screen(session: &str, block: &str) -> Res<String> {
    let p = run(&rex_argv(session, &["capture"], &["-b", block]), false)?;
    Ok(if p.ok() { p.stdout } else { String::new() })
}

/// The program's own (root) status record in the pane, as Rex keeps it, or None.
pub fn program_status(session: &str, block: &str) -> Res<Option<Obj>> {
    let p = run(
        &rex_argv(session, &["block", "call"], &["-b", block, "program_status"]),
        false,
    )?;
    if !p.ok() {
        return Ok(None);
    }
    let Ok(v) = serde_json::from_str::<Value>(&p.stdout) else {
        return Ok(None);
    };
    let records = v.as_object().and_then(|o| o.get("records")).and_then(Value::as_array);
    Ok(records
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|r| !truthy(r.get("id")))
        .cloned())
}

#[cfg(test)]
#[path = "test_rex.rs"]
mod tests;
