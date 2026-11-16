//! `doctor --all-repos`: the checks about the setup (Rex, the builds, the keys, the
//! watcher, the state folder) once, then each known repo's own, for every repo bullpen
//! has state for. A repo whose checkout is gone is left out, as `ls --all-repos` does.

use super::{checks_json, doctor_checks, failed, print_lines, summary, Line, Place, Run};
use bullpen_core::json::{dumps, read, s};
use bullpen_core::paths::homes;
use bullpen_core::repo::config_files;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::path::Path;

/// A known repo: its name, its root (None when its config.json doesn't parse), and its checks.
struct Known {
    name: String,
    root: Option<String>,
    checks: Vec<Line>,
}

/// Every repo with state whose checkout is still there, each with its own checks.
fn known() -> Res<Vec<Known>> {
    let mut out = Vec::new();
    for cfg in config_files()? {
        let Some(dir) = cfg.parent().map(Path::to_path_buf) else {
            continue;
        };
        let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        let (name, root, place) = match read(&cfg) {
            Ok(Some(Value::Object(o))) => {
                let root = s(&o, "root").to_string();
                if root.is_empty() || !Path::new(&root).is_dir() {
                    continue;
                }
                let repo = Repo::new(&root).ok();
                let name = Path::new(&root).file_name().map(|n| n.to_string_lossy().into_owned());
                (name, Some(root), Place::of(repo, dir))
            }
            // it doesn't parse: which checkout it's for can't be told, so it's named by its folder
            _ => (folder, None, Place::of(None, dir)),
        };
        let checks = doctor_checks(&place, Run::EachRepo);
        out.push(Known {
            name: name.unwrap_or_default(),
            root,
            checks,
        });
    }
    Ok(out)
}

/// The failed checks' names: a shared one's alone, a repo's as "repo/check".
fn problems(shared: &[Line], repos: &[Known]) -> Vec<String> {
    let mut out: Vec<String> = failed(shared).map(String::from).collect();
    for r in repos {
        out.extend(failed(&r.checks).map(|c| format!("{}/{c}", r.name)));
    }
    out
}

/// `doctor --all-repos`: the shared checks, then each repo's under its name.
pub fn doctor_all(as_json: bool) -> Res<i32> {
    let shared = doctor_checks(&Place::at(None), Run::Shared);
    let repos = known()?;
    let problems = problems(&shared, &repos);
    if as_json {
        let rs: Vec<Value> = repos
            .iter()
            .map(|r| json!({"repo": r.root, "name": r.name, "checks": checks_json(&r.checks)}))
            .collect();
        let v = json!({"shared": checks_json(&shared), "repos": rs, "problems": problems});
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
                Some(root) => println!("{}  ({})", r.name, homes(root)),
                None => println!("{}  (its config.json doesn't parse)", r.name),
            }
            print_lines(&r.checks, "  ");
        }
        println!();
        println!("{}", summary(&problems));
    }
    Ok(if problems.is_empty() { 0 } else { 1 })
}
