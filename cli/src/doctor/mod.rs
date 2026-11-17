//! doctor: is everything bullpen needs in place?
//!
//! A check is a `DoctorCheck` value in the module for its kind (tools.rs, builds.rs,
//! in_rex.rs, files.rs, hooks.rs, repo.rs, news.rs, workers.rs, remote.rs), which
//! `MODULES` lists in doctor's order. Each returns (state, what it found): "ok", "fail"
//! (with the fix in the text), "warn" (something to see to, that stops nothing) or
//! "skip" (it doesn't apply, or can't be told). `doctor` runs them all; `up` runs those
//! that aren't `slow` and only counts problems. `doctor --all-repos` (all_repos.rs) runs
//! the `shared` ones once, then each repo's. `--json` prints the checks alone; `up`'s
//! hint is `up_hint`; `doctor --fix` (`fix`) runs each check's own fix, if it has one.

mod all_repos;
mod builds;
mod files;
mod hooks;
mod in_rex;
mod news;
mod remote;
mod repo;
mod tools;
mod workers;

use crate::args::{flag, Arg, Args};
use bullpen_core::config::read_global_file;
use bullpen_core::json::dumps;
use bullpen_core::paths::{homes, safe, state};
use bullpen_core::repo::repo_root;
use bullpen_core::settings::defaults;
use bullpen_core::{Obj, Repo, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The modules of checks, in doctor's order: each lists its own, in order.
const MODULES: &[&[DoctorCheck]] = &[
    tools::CHECKS,
    builds::CHECKS,
    in_rex::CHECKS,
    files::CHECKS,
    hooks::CHECKS,
    repo::CHECKS,
    news::CHECKS,
    workers::CHECKS,
    remote::CHECKS,
];

/// Every check, in doctor's order.
fn all() -> impl Iterator<Item = &'static DoctorCheck> {
    MODULES.iter().flat_map(|m| m.iter())
}

/// One of doctor's checks: all doctor knows of it.
struct DoctorCheck {
    /// its line's name
    name: &'static str,
    /// about the setup rather than a repo, the same for every repo: `--all-repos` runs it once
    shared: bool,
    /// left out by `up`: it needs the network, sizes folders, or is something `up` sees to itself
    slow: bool,
    /// left out where there's no repository's state folder (outside a git repository)
    needs_dir: bool,
    run: fn(&Ctx) -> Check,
    /// what `doctor --fix` does for it, if anything
    fix: Option<Fix>,
}

impl DoctorCheck {
    /// Each value starts from this: a repo's check, that `up` runs too, with no fix.
    const NEW: DoctorCheck = DoctorCheck {
        name: "",
        shared: false,
        slow: false,
        needs_dir: false,
        run: |_| Ok(("skip", String::new())),
        fix: None,
    };
}

/// What a check is asked about: the repo (None outside one, or when its config doesn't
/// parse, then why_not says why), its state folder, its settings (the defaults outside a
/// repository) and the run.
struct Ctx<'a> {
    repo: Option<&'a Repo>,
    dir: Option<&'a Path>,
    why_not: Option<&'static str>,
    cfg: &'a Obj,
    run: Run,
}

/// A check's fix for `doctor --fix`: its words in the flag's help, and the fix.
struct Fix {
    help: &'static str,
    run: Fixer,
}

enum Fixer {
    /// In any folder (outside a repository too), first: what it did, if anything.
    Anywhere(fn() -> Option<String>),
    /// In this repository: what it did, each in words. `left` is what an error leaves as
    /// it was ("the news hook").
    InRepo {
        left: &'static str,
        run: fn(&Repo) -> Res<Vec<String>>,
    },
}

/// A check's result.
pub type Check = Res<(&'static str, String)>;

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
    let cfg: Obj = place.repo.as_ref().map_or_else(defaults, |r| r.cfg.clone());
    let ctx = Ctx {
        repo: place.repo.as_ref(),
        dir: place.dir.as_deref(),
        why_not: place.why_not,
        cfg: &cfg,
        run,
    };
    all()
        .filter(|c| !c.needs_dir || place.dir.is_some())
        .filter(|c| match run {
            Run::Doctor => true,
            Run::Up => !c.slow,
            Run::Shared => c.shared,
            Run::EachRepo => !c.shared,
        })
        .map(|c| match (c.run)(&ctx) {
            Ok((st, text)) => (c.name, st, text),
            Err(e) => (c.name, "skip", format!("can't tell: {e}")),
        })
        .collect()
}

fn problems_phrase(n: usize) -> String {
    format!("{n} problem{}", if n != 1 { "s" } else { "" })
}

/// doctor's flags, `--fix`'s help from the checks' fixes.
fn doctor_args() -> [Arg; 3] {
    static FIX_HELP: OnceLock<String> = OnceLock::new();
    [
        flag(
            "--all-repos",
            "every repository bullpen knows: the checks about the setup once, then each repo's \
             (its push remote as last fetched or pushed, without the network)",
        ),
        flag(
            "--json",
            "the checks as JSON: {name, state, text} each, and the problems' names",
        ),
        flag("--fix", FIX_HELP.get_or_init(fix_help)),
    ]
}

/// "first fix what bullpen can by itself: <each fix that runs anywhere>; and in this
/// repository, <each of this repository's>".
fn fix_help() -> String {
    let (anywhere, here): (Vec<&Fix>, Vec<&Fix>) = all()
        .filter_map(|c| c.fix.as_ref())
        .partition(|f| matches!(f.run, Fixer::Anywhere(_)));
    let mut parts: Vec<String> = anywhere.iter().map(|f| f.help.to_string()).collect();
    let here: Vec<&str> = here.iter().map(|f| f.help).collect();
    if !here.is_empty() {
        let and = if parts.is_empty() { "" } else { "and " };
        parts.push(format!("{and}in this repository, {}", listed(&here)));
    }
    format!("first fix what bullpen can by itself: {}", parts.join("; "))
}

/// "a", "a, and b", "a, b, and c".
fn listed(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// `doctor`: one line per check, with the fix for what's wrong.
pub fn cmd_doctor(argv: &[String]) -> Res<i32> {
    let a = Args::parse("doctor", argv, &doctor_args());
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
    for c in all() {
        if let Some(Fix {
            run: Fixer::Anywhere(run),
            ..
        }) = &c.fix
        {
            run().iter().for_each(|did| said(c.name, did));
        }
    }
    let Some(repo) = place.repo.as_ref() else {
        return match place.why_not {
            Some(why) => bullpen_core::fail(format!("--fix can't tell this repository's settings: {why}")),
            None => Ok(()), // outside a repository: the fixes that run anywhere alone
        };
    };
    for c in all() {
        if let Some(Fix {
            run: Fixer::InRepo { left, run },
            ..
        }) = &c.fix
        {
            match run(repo) {
                Ok(did) => did.iter().for_each(|d| said(c.name, d)),
                Err(e) => eprintln!("bullpen: --fix left {left} as it was: {}", e.0),
            }
        }
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
