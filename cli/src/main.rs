//! bullpen's command line in Rust (an experiment, BP-95). It runs the commands
//! ported so far and hands any other (and any it can't do as Python would) to the
//! Python bullpen, bin/bullpen, so it can stand in for it.

mod add;
mod agents;
mod args;
mod check;
mod coordinators;
mod core;
mod diff;
mod events;
mod json;
mod modes;
mod news;
mod peek;
mod rex;
mod show;
mod stops;
mod task_line;
mod text;
mod timeutil;
mod triage;
mod widths;

use crate::args::{parse, Spec};
use crate::core::{Fail, Res};
use std::os::unix::process::CommandExt;

/// Run a ported command. Ok(None): not one.
fn dispatch(argv: &[String]) -> Res<Option<i32>> {
    let rest = &argv[1..];
    let spec = |flags, opts, min, max| Spec { flags, opts, min, max };
    match argv[0].as_str() {
        "_nop" => {} // does nothing: the benchmark times start-up with it
        "ls" => show::cmd_ls(&parse(rest, &spec(&["--all", "--all-repos", "--json"], &[], 0, 0))?)?,
        "show" => show::cmd_show(&parse(rest, &spec(&["--json"], &[], 1, 1))?)?,
        "news" => news::cmd_news(&parse(rest, &spec(&["--follow"], &["--every"], 0, 0))?)?,
        "peek" => peek::cmd_peek(&parse(rest, &spec(&["--vt"], &["--lines"], 1, 1))?)?,
        "diff" => diff::cmd_diff(&parse(rest, &spec(&["--stat", "--pane", "--back"], &["--session"], 1, 1))?)?,
        "check" => {
            let flags = &["--fix", "--quiet", "--all-repos"];
            return Ok(Some(check::cmd_check(&parse(rest, &spec(flags, &[], 0, usize::MAX))?)?));
        }
        "add" => add::cmd_add(&parse(
            rest,
            &spec(&["--auto-accept", "--no-auto-accept"], &["--brief", "--brief-file", "--base", "--after", "--model", "--review"], 1, 1),
        )?)?,
        "after" => add::cmd_after(&parse(rest, &spec(&["--none"], &[], 1, usize::MAX))?)?,
        "model" => add::cmd_model(&parse(rest, &spec(&["--clear"], &[], 1, 2))?)?,
        "review-mode" => add::cmd_review_mode(&parse(rest, &spec(&[], &[], 2, 2))?)?,
        "ok" => triage::cmd_ok(&parse(rest, &spec(&[], &[], 1, 1))?)?,
        "note" => triage::cmd_note(&parse(rest, &spec(&[], &[], 2, usize::MAX))?)?,
        _ => return Ok(None),
    }
    Ok(Some(0))
}

/// Exec the Python bullpen with the same arguments; returns only if that fails.
/// BULLPEN_RUST_STRICT=1 (check/compare.sh) fails instead, so a match is the port's.
fn hand_to_python(argv: &[String]) -> i32 {
    if std::env::var_os("BULLPEN_RUST_STRICT").is_some_and(|v| !v.is_empty()) {
        eprintln!("bullpen-rust: not ported: {}", argv.join(" "));
        return 3;
    }
    let py = core::python_self();
    let err = std::process::Command::new(&py).args(argv).exec();
    eprintln!("bullpen: couldn't run {}: {err}", py.display());
    1
}

fn main() {
    // die quietly on a closed pipe, as a shell tool does, rather than panic
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() || argv[0].starts_with('-') {
        std::process::exit(hand_to_python(&argv));
    }
    let code = match dispatch(&argv) {
        Ok(Some(code)) => code,
        Ok(None) | Err(Fail::Python) => hand_to_python(&argv),
        Err(Fail::Msg(m)) => {
            eprintln!("bullpen: {m}");
            1
        }
    };
    std::process::exit(code);
}
