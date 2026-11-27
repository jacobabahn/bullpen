//! `peek`: the end of a worker's screen (start.py's cmd_peek), or of its reviewer's
//! (`peek --review`), or of a coordinator's (`peek --coordinator c1`).

use crate::args::{flag, pos, usage_error, value, Arg, Args, Nargs};
use crate::whose_pane::{named, Pane, COORDINATOR, REPO};
use bullpen_core::json::s;
use bullpen_core::{fail, Res};

const PEEK: &[Arg] = &[
    pos("id", Nargs::Maybe, "the task, like BP-3 (none with --coordinator)"),
    value("--lines", "LINES", "how many lines (default 40)"),
    flag("--vt", "keep the screen's colours, as terminal escape codes"),
    flag("--review", "the task's reviewer, not its worker"),
    COORDINATOR,
    REPO,
];

/// Python's int() of a str: digits (single underscores between them), a sign, and
/// whitespace around; None when it isn't one.
pub fn py_int(text: &str) -> Option<i64> {
    let t = text.trim();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let ok = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    if !ok {
        return None;
    }
    let n: i64 = digits.replace('_', "").parse().ok()?;
    Some(if neg { -n } else { n })
}

/// `peek`: print the end of a worker's screen, or with --review of its reviewer's, or
/// with --coordinator of a coordinator's.
pub fn cmd_peek(argv: &[String]) -> Res<i32> {
    let a = Args::parse("peek", argv, PEEK);
    let n = match a.opt("--lines") {
        None => 40,
        Some(v) => match py_int(v) {
            Some(n) => n,
            None => usage_error("peek", PEEK, &format!("argument --lines: invalid int value: '{v}'")),
        },
    };
    let review = a.flag("--review");
    if review && a.opt("--coordinator").is_some() {
        usage_error("peek", PEEK, "--review is a task's reviewer: not with --coordinator");
    }
    let t = match named("peek", PEEK, &a)? {
        (_, Pane::Task(t)) => t,
        // running: named found it so
        (_, Pane::Coordinator(_, agent)) => return show(&a, &agent, n),
    };
    let agent = crate::agents::agent(&t, if review { "reviewer" } else { "worker" });
    if review && agent.id.is_empty() {
        return fail(format!("{} has no reviewer running", s(&t, "id")));
    }
    if !agent.alive()? {
        let whose = if review { "reviewer" } else { "agent" };
        return fail(format!("{}'s {whose} isn't running", s(&t, "id")));
    }
    show(&a, &agent, n)
}

/// Print the last n lines of agent's screen.
fn show(a: &Args, agent: &crate::host::Agent, n: i64) -> Res<i32> {
    println!("{}", agent.read_end(n, a.flag("--vt"))?.join("\n"));
    Ok(0)
}
