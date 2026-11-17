//! doctor: is everything bullpen needs in place?
//!
//! Each check (checks.rs) returns (state, what it found): "ok", "fail" (with the fix in
//! the text), "warn" (something to see to, that stops nothing) or "skip" (it doesn't
//! apply, or can't be told). `doctor` runs them all; `up`
//! runs the cheap ones (no network, nothing `up` itself just fixed) and only counts problems.
//! `doctor --all-repos` (all_repos.rs) runs those about the setup once, then each repo's.

mod all_repos;
mod checks;
mod news;

use crate::args::{flag, Arg, Args};
use crate::push_remote::{check_push, check_push_seen};
use bullpen_core::config::read_global_file;
use bullpen_core::json::dumps;
use bullpen_core::paths::{homes, safe, state};
use bullpen_core::repo::repo_root;
use bullpen_core::settings::defaults;
use bullpen_core::{Obj, Repo, Res};
use checks::*;
use serde_json::{json, Value};
use std::path::PathBuf;

/// The repo a run of the checks is about: its Repo, its state folder, and why there's
/// no Repo when there's a folder (its config.json, or the global one, doesn't parse).
struct Place {
    repo: Option<Repo>,
    dir: Option<PathBuf>,
    why_not: Option<&'static str>,
}

impl Place {
    /// The repo at root (None: outside a git repository) and its state folder.
    fn at(root: Option<PathBuf>) -> Place {
        let Some(root) = root else {
            return Place {
                repo: None,
                dir: None,
                why_not: None,
            };
        };
        let name = root.display().to_string().trim_matches('/').replace('/', "-");
        let dir = state().join("repos").join(safe(&name));
        // Err: its config.json, or the global one, doesn't parse
        Place::of(Repo::new(&root).ok(), dir)
    }

    fn of(repo: Option<Repo>, dir: PathBuf) -> Place {
        let why_not = match &repo {
            Some(_) => None,
            None if read_global_file().is_ok() => Some("config.json doesn't parse"),
            None => Some("the global config.json doesn't parse"),
        };
        Place {
            repo,
            dir: Some(dir),
            why_not,
        }
    }
}

/// Which of the checks a run makes.
#[derive(Clone, Copy, PartialEq)]
enum Run {
    /// `doctor` in one repo: all of them.
    Doctor,
    /// `up`: the quick ones, with no network and nothing `up` itself sees to.
    Up,
    /// `doctor --all-repos`, once: those about the setup rather than a repo.
    Shared,
    /// `doctor --all-repos`, for each repo: the rest, with the push remote from local refs.
    EachRepo,
}

/// A check's name, state and text.
pub type Line = (&'static str, &'static str, String);

/// [(name, state, text)], in doctor's order.
fn doctor_checks(place: &Place, run: Run) -> Vec<Line> {
    let (repo, why_not) = (place.repo.as_ref(), place.why_not);
    let cfg: Obj = repo.map_or_else(defaults, |r| r.cfg.clone());
    type Do<'a> = Box<dyn Fn() -> Check + 'a>;
    // (name, shared by every repo, left out by `up`, the check)
    let all: Vec<(&'static str, bool, bool, Do)> = vec![
        ("rex", true, false, Box::new(check_rex)),
        ("gh", false, true, Box::new(|| check_gh(repo))),
        ("agents", false, false, Box::new(|| check_agents(&cfg))),
        ("bullpen", true, false, Box::new(check_cli)),
        ("board", true, false, Box::new(check_board)),
        ("cargo", true, true, Box::new(check_cargo)),
        ("keys", true, false, Box::new(check_keys)),
        ("watcher", true, true, Box::new(check_watcher)),
        ("state", true, false, Box::new(check_state)),
        ("temp", true, true, Box::new(|| Ok(crate::claude_tmp::check()))),
        ("global", true, false, Box::new(check_global)),
        (
            "config",
            false,
            false,
            Box::new(|| place.dir.as_deref().map_or_else(outside, check_config)),
        ),
        ("setup", false, false, Box::new(|| check_hook(repo, "setup", why_not))),
        (
            "teardown",
            false,
            false,
            Box::new(|| check_hook(repo, "teardown", why_not)),
        ),
        (
            "coordinators",
            false,
            true,
            Box::new(|| check_coordinators(repo, why_not)),
        ),
        (
            "copy",
            false,
            false,
            Box::new(|| crate::copies::check_copy(repo, why_not)),
        ),
        (
            "risk",
            false,
            true,
            Box::new(|| crate::risk_paths::check_risk(repo, why_not)),
        ),
        ("news", false, true, Box::new(|| news::check_news(repo, why_not))),
        (
            "push",
            false,
            true,
            Box::new(|| match run {
                Run::EachRepo => check_push_seen(repo, why_not),
                _ => check_push(repo, why_not),
            }),
        ),
    ];
    all.into_iter()
        .filter(|(name, ..)| *name != "config" || place.dir.is_some()) // outside a repository
        .filter(|(_, shared, slow, _)| match run {
            Run::Doctor => true,
            Run::Up => !slow,
            Run::Shared => *shared,
            Run::EachRepo => !shared,
        })
        .map(|(name, _, _, f)| match f() {
            Ok((st, text)) => (name, st, text),
            Err(e) => (name, "skip", format!("can't tell: {e}")),
        })
        .collect()
}

/// A repo's own check outside a repository.
fn outside() -> Check {
    Ok(("skip", "not in a git repository".into()))
}

fn problems_phrase(n: usize) -> String {
    format!("{n} problem{}", if n != 1 { "s" } else { "" })
}

const DOCTOR: &[Arg] = &[
    flag(
        "--all-repos",
        "every repository bullpen knows: the checks about the setup once, then each repo's \
         (its push remote as last fetched or pushed, without the network)",
    ),
    flag(
        "--json",
        "the checks as JSON: {name, state, text} each, and the problems' names",
    ),
    flag(
        "--fix",
        "first fix what bullpen can by itself: the Claude scratch folders closed tasks of any repository left in \
         the temp folder; and in this repository, the news hook in .claude/settings.local.json (written with \
         `config --news wake`, removed with any other news)",
    ),
];

/// `doctor`: one line per check, with the fix for what's wrong.
pub fn cmd_doctor(argv: &[String]) -> Res<i32> {
    let a = Args::parse("doctor", argv, DOCTOR);
    if a.flag("--all-repos") {
        if a.flag("--fix") {
            return bullpen_core::fail("--fix is for one repository: run `bullpen doctor --fix` in it");
        }
        return all_repos::doctor_all(a.flag("--json"));
    }
    let place = Place::at(repo_root().ok());
    if a.flag("--fix") {
        fix(&place, a.flag("--json"))?;
    }
    let res = doctor_checks(&place, Run::Doctor);
    let failed: Vec<String> = failed(&res).map(String::from).collect();
    if a.flag("--json") {
        let root = place.repo.as_ref().map(|r| r.root_str());
        let v = json!({"repo": root, "checks": checks_json(&res), "problems": failed});
        println!("{}", dumps(&v));
    } else {
        print_lines(&res, "");
        println!("{}", summary(&failed));
    }
    Ok(if failed.is_empty() { 0 } else { 1 })
}

/// `doctor --fix`: what bullpen can fix by itself, before the checks. Said on stderr
/// with --json, whose output is the checks alone.
fn fix(place: &Place, json: bool) -> Res<()> {
    let said = |name: &str, did: &str| match json {
        true => eprintln!("fixed  {name}: {did}"),
        false => println!("fixed  {name}: {did}"),
    };
    if let Some(did) = crate::claude_tmp::fix() {
        said("temp", &did);
    }
    let Some(repo) = place.repo.as_ref() else {
        return match place.why_not {
            Some(why) => bullpen_core::fail(format!("--fix can't tell this repository's settings: {why}")),
            None => Ok(()), // outside a repository: the temp folder alone
        };
    };
    match crate::news::hook::sync(repo) {
        Ok(None) => {}
        Ok(Some(did)) => said("news", &did),
        Err(e) => eprintln!("bullpen: --fix left the news hook as it was: {}", e.0),
    }
    Ok(())
}

/// The names of the checks that failed.
fn failed(res: &[Line]) -> impl Iterator<Item = &'static str> + '_ {
    res.iter().filter(|r| r.1 == "fail").map(|r| r.0)
}

/// "ok    rex: ...", one line per check, each after indent.
fn print_lines(res: &[Line], indent: &str) {
    for (name, st, text) in res {
        let mark = match *st {
            "ok" => "ok",
            "fail" => "FAIL",
            "warn" => "WARN",
            _ => "--",
        };
        println!("{indent}{mark:<4}  {name}: {}", homes(text));
    }
}

/// doctor's last line: "All good." or "2 problems: keys, watcher."
fn summary(failed: &[String]) -> String {
    if failed.is_empty() {
        return "All good.".into();
    }
    format!("{}: {}.", problems_phrase(failed.len()), failed.join(", "))
}

/// [{name, state, text}], the text with ~ for the home folder as doctor prints it.
fn checks_json(res: &[Line]) -> Value {
    res.iter()
        .map(|(name, st, text)| json!({"name": name, "state": st, "text": homes(text)}))
        .collect()
}

/// `up`'s one line about problems doctor would find, or None.
pub fn up_hint() -> Option<String> {
    // a hint must never break up: a panic is kept quiet and gives no hint
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let res = std::panic::catch_unwind(|| doctor_checks(&Place::at(repo_root().ok()), Run::Up));
    std::panic::set_hook(hook);
    let n = res.ok()?.iter().filter(|r| r.1 == "fail").count();
    (n > 0).then(|| format!("{}: run `bullpen doctor`", problems_phrase(n)))
}

#[cfg(test)]
#[path = "test_doctor.rs"]
mod tests;
