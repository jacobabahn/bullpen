//! doctor: is everything bullpen needs in place? (bullpen/doctor.py)
//!
//! Each check (checks.rs) returns (state, what it found): "ok", "fail" (with the fix in
//! the text) or "skip" (it doesn't apply, or can't be told). `doctor` runs them all; `up`
//! runs the cheap ones (no network, nothing `up` itself just fixed) and only counts problems.

mod checks;

use crate::args::Args;
use crate::push_remote::check_push;
use bullpen_core::config::read_global_file;
use bullpen_core::paths::{homes, safe, state};
use bullpen_core::repo::repo_root;
use bullpen_core::settings::defaults;
use bullpen_core::{Obj, Repo, Res};
use checks::*;
use std::path::PathBuf;

/// This repo's Repo and its state folder, or (None, folder or None): outside a git
/// repository, or when its config.json doesn't parse.
fn doctor_repo() -> (Option<Repo>, Option<PathBuf>) {
    let Ok(root) = repo_root() else {
        return (None, None);
    };
    let name = root.display().to_string().trim_matches('/').replace('/', "-");
    let d = state().join("repos").join(safe(&name));
    // Err: its config.json, or the global one, doesn't parse
    (Repo::new(&root).ok(), Some(d))
}

/// [(name, state, text)]. cheap leaves out what needs the network or what `up` sees to.
fn doctor_checks(cheap: bool) -> Vec<(&'static str, &'static str, String)> {
    let (repo, d) = doctor_repo();
    let repo = repo.as_ref();
    let cfg: Obj = repo.map_or_else(defaults, |r| r.cfg.clone());
    let why_not = match (repo, &d) {
        (None, Some(_)) if read_global_file().is_ok() => Some("config.json doesn't parse"),
        (None, Some(_)) => Some("the global config.json doesn't parse"),
        _ => None,
    };
    type Run<'a> = Box<dyn Fn() -> Check + 'a>;
    let mut out: Vec<(&'static str, Run)> = vec![("rex", Box::new(check_rex))];
    if !cheap {
        out.push(("gh", Box::new(|| check_gh(repo))));
    }
    out.push(("agents", Box::new(|| check_agents(&cfg))));
    out.push(("bullpen", Box::new(check_cli)));
    out.push(("board", Box::new(check_board)));
    out.push(("keys", Box::new(check_keys)));
    if !cheap {
        out.push(("watcher", Box::new(check_watcher)));
    }
    out.push(("state", Box::new(check_state)));
    out.push(("global", Box::new(check_global)));
    if let Some(d) = &d {
        out.push(("config", Box::new(move || check_config(d))));
    }
    out.push(("setup", Box::new(|| check_hook(repo, "setup", why_not))));
    out.push(("teardown", Box::new(|| check_hook(repo, "teardown", why_not))));
    if !cheap {
        out.push(("coordinators", Box::new(|| check_coordinators(repo, why_not))));
        out.push(("push", Box::new(|| check_push(repo, why_not))));
    }
    out.into_iter()
        .map(|(name, f)| match f() {
            Ok((st, text)) => (name, st, text),
            Err(e) => (name, "skip", format!("can't tell: {e}")),
        })
        .collect()
}

fn problems_phrase(n: usize) -> String {
    format!("{n} problem{}", if n != 1 { "s" } else { "" })
}

/// `doctor`: one line per check, with the fix for what's wrong.
pub fn cmd_doctor(argv: &[String]) -> Res<i32> {
    Args::parse("doctor", argv, &[]);
    let res = doctor_checks(false);
    for (name, st, text) in &res {
        let mark = match *st {
            "ok" => "ok",
            "fail" => "FAIL",
            _ => "--",
        };
        println!("{mark:<4}  {name}: {}", homes(text));
    }
    let failed: Vec<&str> = res.iter().filter(|r| r.1 == "fail").map(|r| r.0).collect();
    if failed.is_empty() {
        println!("All good.");
        return Ok(0);
    }
    println!("{}: {}.", problems_phrase(failed.len()), failed.join(", "));
    Ok(1)
}

/// `up`'s one line about problems doctor would find, or None.
pub fn up_hint() -> Option<String> {
    // a hint must never break up: a panic is kept quiet and gives no hint
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let res = std::panic::catch_unwind(|| doctor_checks(true));
    std::panic::set_hook(hook);
    let n = res.ok()?.iter().filter(|r| r.1 == "fail").count();
    (n > 0).then(|| format!("{}: run `bullpen doctor`", problems_phrase(n)))
}

#[cfg(test)]
#[path = "test_doctor.rs"]
mod tests;
