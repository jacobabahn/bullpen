//! What the ported commands ask Rex (bullpen/rex.py).

use crate::core::{rex_program, run, Res};
use crate::json::{truthy, Obj};
use serde_json::Value;

/// Is the pane there? (`rex block inspect`, in its session when given.)
pub fn alive(session: &str, block: &str) -> Res<bool> {
    if block.is_empty() {
        return Ok(false);
    }
    let rex = rex_program();
    let mut argv = vec![rex.as_str(), "block", "inspect", block];
    if !session.is_empty() {
        argv.extend(["-s", session]);
    }
    Ok(run(&argv, false)?.code == 0)
}

/// `-s session`, when there is one.
fn session_args(session: &str) -> Vec<&str> {
    if session.is_empty() {
        vec![]
    } else {
        vec!["-s", session]
    }
}

/// Output as Python's text mode reads it: "\r\n" and a lone "\r" become "\n".
fn universal_newlines(out: &str) -> String {
    out.replace("\r\n", "\n").replace('\r', "\n")
}

/// The text on a pane's screen, or "" if Rex can't say.
pub fn screen(session: &str, block: &str) -> Res<String> {
    let rex = rex_program();
    let mut argv = vec![rex.as_str(), "capture"];
    argv.extend(session_args(session));
    argv.extend(["-b", block]);
    let p = run(&argv, false)?;
    Ok(if p.code == 0 { universal_newlines(&p.stdout) } else { String::new() })
}

/// `\x1b[<n>C` (cursor forward) as n spaces (one when n is missing).
fn forward_as_spaces(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("\x1b[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if after[digits..].starts_with('C') {
            let n = if digits == 0 { 1 } else { after[..digits].parse().unwrap_or(0) };
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

/// Text without its colour codes (`\x1b[...m`).
fn without_sgr(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("\x1b[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let n = after.bytes().take_while(|b| b.is_ascii_digit() || *b == b';' || *b == b':').count();
        if after[n..].starts_with('m') {
            rest = &after[n + 1..];
        } else {
            out.push_str("\x1b[");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The last n lines of a pane's screen (as Python's lines[-n:] slices them), without
/// the blank ones at the end. vt keeps its colours, with a cursor-forward gap as spaces.
pub fn screen_end(session: &str, block: &str, n: i64, vt: bool) -> Res<Vec<String>> {
    let rex = rex_program();
    let mut argv = vec![rex.as_str(), "capture"];
    if vt {
        argv.push("--format=vt");
    }
    argv.extend(session_args(session));
    argv.extend(["-b", block]);
    let mut out = universal_newlines(&run(&argv, true)?.stdout);
    if vt {
        out = forward_as_spaces(&out);
    }
    let mut lines: Vec<String> = out.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
    while lines.last().is_some_and(|l| crate::text::strip(&without_sgr(l)).is_empty()) {
        lines.pop();
    }
    let len = lines.len() as i64;
    let start = if n > 0 { len - n } else { -n }.clamp(0, len);
    Ok(lines.split_off(start as usize))
}

/// The program's own (root) status record in the pane, as Rex keeps it, or None.
pub fn program_status(session: &str, block: &str) -> Res<Option<Obj>> {
    let rex = rex_program();
    let mut argv = vec![rex.as_str(), "block", "call"];
    argv.extend(session_args(session));
    argv.extend(["-b", block, "program_status"]);
    let p = run(&argv, false)?;
    if p.code != 0 {
        return Ok(None);
    }
    // (JSON that isn't an object: Python fails on it; here it is no record)
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&p.stdout) else { return Ok(None) };
    let records = o.get("records").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(records.into_iter().filter_map(|r| r.as_object().cloned()).find(|r| !truthy(r.get("id"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_as_python_reads_them() {
        assert_eq!(forward_as_spaces("a\x1b[3Cb\x1b[Cc\x1b[0Cd\x1b[2m"), "a   b cd\x1b[2m");
        assert_eq!(without_sgr("\x1b[32;1m ok\x1b[0m\x1b[K"), " ok\x1b[K");
        assert_eq!(universal_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
    }
}
