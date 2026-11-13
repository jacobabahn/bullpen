//! `diff`: a task's changes against its base (bullpen/show.py's cmd_diff), through a
//! pager, as `--stat` just the files, or in a pane of its own (`--pane`, the board's v).

use crate::args::{flag, pos, value, Arg, Args, Nargs, SUPPRESS};
use crate::coordinators::{home_of, session_for};
use crate::modes::task_base_ref;
use crate::panes::{place, show};
use crate::rex::{find_pane, rex_close, rex_focus, rex_view};
use bullpen_core::json::{obj, s, truthy, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::{git, which};
use bullpen_core::repo::repo_root;
use bullpen_core::task::is_status;
use bullpen_core::text::{rstrip, splitlines};
use bullpen_core::{fail, Repo, Res};
use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};

const DIFF: &[Arg] = &[
    pos("id", Nargs::One, "the task, like BP-3"),
    flag("--stat", "just the files changed, no pager"),
    flag("--pane", "show it in a floating pane (the board's v)"),
    flag("--back", SUPPRESS), // focus the board after
    value(
        "--session",
        "SESSION",
        "the Rex session for its pane (default: the coordinator's)",
    ),
];

const UNCOMMITTED: &str = "Uncommitted (not in any commit yet):";

fn diff_label(t: &Obj) -> String {
    format!("bullpen: {} diff", s(t, "id"))
}

/// `diff`: a task's changes against its base, through a pager; --stat just the files.
pub fn cmd_diff(argv: &[String]) -> Res<i32> {
    let a = Args::parse("diff", argv, DIFF);
    let repo = Repo::new(repo_root()?)?;
    let t = repo.task(a.arg("id").unwrap_or(""))?;
    let wt = s(&t, "worktree").to_string();
    if !truthy(t.get("worktree")) || !std::path::Path::new(&wt).is_dir() {
        return fail(format!(
            "{}, with no worktree, so there's nothing to diff",
            is_status(&t)
        ));
    }
    if a.flag("--pane") {
        diff_pane(&repo, &t, a.opt("--session"))?;
        return Ok(0);
    }
    let base_ref = task_base_ref(&repo, &t)?;
    if a.flag("--stat") {
        println!("{}", diff_stat(&wt, &base_ref, s(&t, "id"))?);
        return Ok(0);
    }
    let delta = which("delta");
    let tty = std::io::stdout().is_terminal();
    let color = if tty && delta.is_none() {
        "--color=always"
    } else {
        "--no-color"
    };
    let mut text = git(&wt, &["diff", color, &format!("{base_ref}...HEAD")], true)?.stdout;
    let loose = git(&wt, &["diff", color, "HEAD"], true)?.stdout;
    let untracked = git(&wt, &["ls-files", "--others", "--exclude-standard"], true)?.stdout;
    let untracked = splitlines(&untracked);
    if !loose.is_empty() || !untracked.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("{UNCOMMITTED}\n\n{loose}"));
        for f in untracked {
            text.push_str(&format!("untracked: {f}\n"));
        }
    }
    if text.is_empty() {
        text = format!("{} has no changes against {base_ref}.\n", s(&t, "id"));
    }
    page(&text, if tty { delta } else { None }, tty)?;
    if a.flag("--back") {
        let c = home_of(&repo, &t)?;
        let b = obj(&c, "board");
        if crate::rex::alive(s(b, "session"), s(b, "block"))? {
            rex_focus(s(b, "session"), s(b, "block"))?;
        }
    }
    Ok(0)
}

/// The files the branch changed since its base, then any not committed yet.
fn diff_stat(wt: &str, base_ref: &str, tid: &str) -> Res<String> {
    let committed = git(wt, &["diff", "--stat", &format!("{base_ref}...HEAD")], true)?.stdout;
    let mut parts = vec![match rstrip(&committed) {
        "" => format!("{tid} has no commits against {base_ref}."),
        c => c.to_string(),
    }];
    let loose = git(wt, &["diff", "--stat", "HEAD"], true)?.stdout;
    let loose = rstrip(&loose);
    let untracked = git(wt, &["ls-files", "--others", "--exclude-standard"], true)?.stdout;
    let untracked = splitlines(&untracked);
    if !loose.is_empty() || !untracked.is_empty() {
        let mut lines = vec![UNCOMMITTED.to_string()];
        if !loose.is_empty() {
            lines.push(loose.to_string());
        }
        lines.extend(untracked.iter().map(|f| format!(" untracked: {f}")));
        parts.push(lines.join("\n"));
    }
    Ok(parts.join("\n\n"))
}

/// Show text in delta, else `less -R`; straight to stdout when that's no terminal. The
/// pager never quits by itself on a short diff: in a pane, that would close it at once.
fn page(text: &str, delta: Option<String>, tty: bool) -> Res<()> {
    let mut out = std::io::stdout();
    if !tty {
        out.write_all(text.as_bytes())?;
        return Ok(());
    }
    let mut pager = match &delta {
        Some(d) => {
            let mut c = Command::new(d);
            c.arg("--paging=always");
            c
        }
        None => {
            let mut c = Command::new("less");
            c.args(["-R", "-+F"]);
            c
        }
    };
    // a pager quit before it read everything is EPIPE here, not the end of `diff --back`
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    match pager.stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes()); // a pager quit before reading it all
            }
            child.wait()?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => out.write_all(text.as_bytes())?,
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// Show the diff in a floating pane over the board (a tab with --workers tabs);
/// quitting the pager closes it and focuses the board again.
fn diff_pane(repo: &Repo, t: &Obj, session: Option<&str>) -> Res<()> {
    let mut session = session_for(repo, session)?;
    if session.is_empty() {
        session = s(t, "session").into();
    }
    if let Some(old) = find_pane(rex_view(&session)?.as_ref(), "", &diff_label(t)) {
        rex_close(&session, &old.block)?; // one diff pane per task, showing the latest
    }
    let argv = [
        self_bin().display().to_string(),
        "diff".into(),
        s(t, "id").into(),
        "--back".into(),
    ];
    let (block, _) = place(repo, t, &session, &diff_label(t), &argv, "diff", false, true)?;
    show(repo, &session, &block)
}
