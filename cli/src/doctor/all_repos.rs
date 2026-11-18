//! `doctor --all-repos`: the checks about the setup (Rex, the builds, the keys, the
//! watcher, the state folder) once, then each known repo's own, for every repo bullpen
//! has state for: the snapshot's (core/src/fleet.rs), and those it can't read. A repo
//! whose checkout is gone is left out, as `ls --all-repos` does.

use super::{checks_json, doctor_checks, print_lines, summary, Line, Place, Run};
use bullpen_core::fleet::{self, Unread};
use bullpen_core::json::dumps;
use bullpen_core::paths::{homes, state};
use bullpen_core::Res;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A known repo: its name, its root (None when its config.json doesn't parse), its
/// name in the problems (its folder's, with as many of the folders above it as tell it
/// from another repo's), and its checks.
struct Known {
    name: String,
    root: Option<PathBuf>,
    label: String,
    checks: Vec<Line>,
}

/// Every repo with state whose checkout is still there, by state folder, each with its
/// own checks.
fn known() -> Vec<Known> {
    let state = state();
    // the global config.json doesn't parse: then every repo is among the unread
    let repos = fleet::repos(&state).unwrap_or_default();
    let mut places: Vec<(Option<PathBuf>, Place)> = repos
        .into_iter()
        .map(|r| (Some(r.root.clone()), Place::of(Some(r.clone()), r.dir)))
        .collect();
    for Unread { dir, root } in fleet::unread(&state) {
        places.push((root, Place::of(None, dir)));
    }
    places.sort_by(|a, b| a.1.dir.cmp(&b.1.dir));
    // a config.json that doesn't parse can't tell its checkout: it's named by its folder
    let paths: Vec<PathBuf> = places
        .iter()
        .map(|(root, p)| match root {
            Some(r) => r.clone(),
            None => PathBuf::from(p.dir.as_deref().and_then(Path::file_name).unwrap_or_default()),
        })
        .collect();
    let labels = labels(&paths);
    places
        .into_iter()
        .zip(paths.iter().zip(labels))
        .map(|((root, place), (path, label))| Known {
            name: name(path),
            root,
            label,
            checks: doctor_checks(&place, Run::EachRepo),
        })
        .collect()
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Each path's last folder, with as many of the folders above it as tell it from the
/// others': ~/a/shop and ~/b/shop are "a/shop" and "b/shop".
fn labels(paths: &[PathBuf]) -> Vec<String> {
    let parts: Vec<Vec<String>> = paths
        .iter()
        .map(|p| p.iter().rev().map(|c| c.to_string_lossy().into_owned()).collect())
        .collect();
    let tail = |p: &[String], n: usize| -> Vec<String> { p.iter().take(n).cloned().collect() };
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut n = 1;
            while n < p.len()
                && parts
                    .iter()
                    .enumerate()
                    .any(|(j, q)| j != i && tail(q, n) == tail(p, n))
            {
                n += 1;
            }
            let mut t = tail(p, n);
            t.reverse();
            t.join("/").trim_start_matches('/').to_string()
        })
        .collect()
}

/// The failed checks: a shared one's by its name alone, a repo's as "repo/check".
struct Problem<'a> {
    name: String,
    repo: Option<&'a Known>,
    check: &'a Line,
}

fn problems<'a>(shared: &'a [Line], repos: &'a [Known]) -> Vec<Problem<'a>> {
    let failed = |ls: &'a [Line]| ls.iter().filter(|l| l.1 == "fail");
    let mut out: Vec<Problem> = failed(shared)
        .map(|l| Problem {
            name: l.0.into(),
            repo: None,
            check: l,
        })
        .collect();
    for r in repos {
        out.extend(failed(&r.checks).map(|l| Problem {
            name: format!("{}/{}", r.label, l.0),
            repo: Some(r),
            check: l,
        }));
    }
    out
}

/// A problem as JSON: its name, its check's, the folder to fix it in (null for one
/// about the setup, or a repo whose config.json doesn't parse) and the check's text.
fn problem_json(p: &Problem) -> Value {
    let root = p.repo.and_then(|r| r.root.as_ref()).map(|r| r.display().to_string());
    json!({"name": p.name, "check": p.check.0, "repo": root, "text": homes(&p.check.2)})
}

/// `doctor --all-repos`: the shared checks, then each repo's under its name.
pub fn doctor_all(as_json: bool) -> Res<i32> {
    let shared = doctor_checks(&Place::at(None), Run::Shared);
    let repos = known();
    let problems = problems(&shared, &repos);
    if as_json {
        let rs: Vec<Value> = repos
            .iter()
            .map(|r| {
                let root = r.root.as_ref().map(|r| r.display().to_string());
                json!({"repo": root, "name": r.name, "checks": checks_json(&r.checks)})
            })
            .collect();
        let ps: Vec<Value> = problems.iter().map(problem_json).collect();
        let v = json!({"shared": checks_json(&shared), "repos": rs, "problems": ps});
        println!("{}", dumps(&v));
    } else {
        println!("Setup, for every repo");
        print_lines(&shared, "  ");
        if repos.is_empty() {
            println!();
            println!("No repositories yet: `bullpen up` in one starts its bullpen.");
        }
        for r in &repos {
            println!();
            match &r.root {
                Some(root) => println!("{}  ({})", r.name, homes(&root.display().to_string())),
                None => println!("{}  (its config.json doesn't parse)", r.name),
            }
            print_lines(&r.checks, "  ");
        }
        println!();
        let names: Vec<String> = problems.iter().map(|p| p.name.clone()).collect();
        println!("{}", summary(&names));
    }
    Ok(if problems.is_empty() { 0 } else { 1 })
}

#[cfg(test)]
#[path = "tests/test_all_repos.rs"]
mod tests;
