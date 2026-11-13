//! A repo's settings, in layers: the built-in defaults, then the global config.json,
//! then the repo's own. And the migrations a repo's state gets once, from before
//! triage, recorded stops and the layers (Repo::new runs them).

use crate::fail::{fail, Res};
use crate::json::{self, get, s, truthy, Obj};
use crate::repo::Repo;
use crate::settings::{defaults, is_internal, is_repo_only};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The config.json with every repository's defaults: $XDG_CONFIG_HOME/bullpen/config.json,
/// else ~/.config/bullpen/config.json.
pub fn global_config_path() -> PathBuf {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(crate::paths::user_home()).join(".config"),
    };
    base.join("bullpen").join("config.json")
}

/// The global config.json as it is ({} when there is none). Fail when it doesn't parse.
pub fn read_global_file() -> Res<Obj> {
    let path = global_config_path();
    let shown = path.display();
    if path.is_dir() {
        return fail(format!("{shown} is a folder, not a file: move it away"));
    }
    match json::read(&path) {
        Ok(None) => Ok(Obj::new()),
        Ok(Some(Value::Object(o))) => Ok(o),
        Ok(Some(_)) => fail(format!("{shown} isn't a JSON object ({{...}}): fix it, or move it away")),
        Err(e) => fail(format!("{shown} doesn't parse ({e}): fix it, or move it away")),
    }
}

/// The settings the global config.json gives every repository: what it holds,
/// without the keys only a repository can have.
pub fn read_global() -> Res<Obj> {
    Ok(read_global_file()?.into_iter().filter(|(k, _)| !is_repo_only(k) && !is_internal(k)).collect())
}

/// What a repository gets when its own config.json doesn't say: the global settings
/// over the built-in defaults.
pub fn inherited_config() -> Res<Obj> {
    let mut cfg = defaults();
    cfg.extend(read_global()?);
    Ok(cfg)
}

/// A repo's own config.json in its state folder ({} when there is none).
pub fn read_own(dir: &Path) -> Res<Obj> {
    Ok(json::read_obj(&dir.join("config.json"))?.unwrap_or_default())
}

/// A repo's settings: its own config.json over the global one, over the defaults.
/// A config from before several coordinators has one {"coordinator": {block, session}}
/// and one "board": it becomes coordinator c1.
pub fn load_config(dir: &Path) -> Res<Obj> {
    let mut cfg = inherited_config()?;
    cfg.extend(read_own(dir)?);
    if cfg.contains_key("coordinator") || cfg.contains_key("board") {
        let old = cfg.shift_remove("coordinator").unwrap_or(Value::Null);
        let board = cfg.shift_remove("board").unwrap_or(Value::Null);
        if truthy(Some(&old)) && !truthy(cfg.get("coordinators")) {
            let o = old.as_object().cloned().unwrap_or_default();
            let c = json!({"id": "c1", "block": get(&o, "block"), "session": get(&o, "session"), "board": board});
            cfg.insert("coordinators".into(), json!([c]));
            cfg.insert("next_coordinator".into(), json!(2));
        }
    }
    Ok(cfg)
}

/// Where a setting's value comes from: "repo", "global" or "default".
pub fn config_source(repo: &Repo, key: &str) -> Res<&'static str> {
    if is_repo_only(key) {
        // always written down: it is the repo's own once it isn't the default
        let default = defaults().get(key).cloned();
        let differs = repo.cfg.get(key).cloned() != default;
        return Ok(if repo.own.contains(key) && differs { "repo" } else { "default" });
    }
    if repo.own.contains(key) {
        return Ok("repo");
    }
    Ok(if read_global()?.contains_key(key) { "global" } else { "default" })
}

// ---------- the migrations ----------

/// A repo's task files (tasks/*.json, as pathlib's glob finds them: dot-files too), sorted.
pub fn task_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir.join("tasks"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json") || p.file_name().is_some_and(|n| n == ".json"))
        .collect();
    out.sort();
    out
}

/// A task file for a migration: its object, or {} (core's `read_json(p) or {}`).
fn read_task(path: &Path) -> Res<Obj> {
    Ok(json::read_obj(path)?.unwrap_or_default())
}

/// Before triage, every stop was "needs". Reasons the coordinator can see became
/// triage, once per repo; prompts stay needs.
pub const TRIAGE_WHY: [&str; 7] = [
    "Failed",
    "Ended its turn",
    "Review findings didn't reach",
    "PR ",
    "Couldn't be continued",
    "Reviewer stopped",
    "Reviewer exited",
];

/// A needs task whose why the coordinator can see to becomes triage. Written as it is,
/// not by save_task: when it stopped stays as it was.
pub fn migrate_triage(repo: &Repo) -> Res<()> {
    for p in task_files(&repo.dir) {
        let mut t = read_task(&p)?;
        if s(&t, "status") == "needs" && TRIAGE_WHY.iter().any(|w| s(&t, "why").starts_with(w)) {
            t.insert("status".into(), json!("triage"));
            json::write_obj(&p, &t)?;
        }
    }
    Ok(())
}

/// Before stops were recorded, a task's why said who stopped and how: (its start,
/// by, kind). Longer prefixes first.
pub const OLD_STOPS: [(&str, &str, &str); 11] = [
    ("Reviewer needs you", "reviewer", "prompt"),
    ("Reviewer stopped", "reviewer", "ended"),
    ("Reviewer exited", "reviewer", "exited"),
    ("Needs you", "worker", "prompt"),
    ("Failed", "worker", "failed"),
    ("Ended its turn", "worker", "ended"),
    ("Agent exited", "worker", "exited"),
    ("Its pane was gone", "worker", "pane_gone"),
    ("Review findings didn't reach", "worker", "not_sent"),
    ("PR ", "worker", "not_sent"),
    ("Couldn't be continued", "worker", "limit"),
];

/// The stop an old task's why says it had ({"by", "kind"}), or None.
pub fn stop_from_why(why: Option<&str>) -> Option<Value> {
    let why = why.filter(|w| !w.is_empty())?;
    let (by, kind) =
        OLD_STOPS.iter().find(|(prefix, _, _)| why.starts_with(prefix)).map_or(("worker", "other"), |s| (s.1, s.2));
    Some(json!({"by": by, "kind": kind}))
}

/// Once per repo, stopped tasks get the stop their why says.
pub fn migrate_stops(repo: &Repo) -> Res<()> {
    for p in task_files(&repo.dir) {
        let mut t = read_task(&p)?;
        if !crate::task::is_stopped(s(&t, "status")) || truthy(t.get("stop")) {
            continue;
        }
        // an asked task's why is the question: ask_of kept what stopped it
        let why = if truthy(t.get("ask")) { t.get("ask_of") } else { t.get("why") };
        let stop = stop_from_why(why.and_then(Value::as_str));
        t.shift_remove("ask_of");
        if let Some(stop) = stop {
            t.insert("stop".into(), stop);
        }
        json::write_obj(&p, &t)?;
    }
    Ok(())
}

/// A config.json from before the global one held every setting, the defaults too.
/// Its values that are just the built-in default are dropped, so the global ones apply.
pub fn migrate_layers(repo: &mut Repo) -> Res<()> {
    let inherited = inherited_config()?;
    let builtin = defaults();
    let own: Vec<String> = repo.own.iter().cloned().collect();
    for k in own {
        let is_default = builtin.get(&k).is_some_and(|d| repo.cfg.get(&k) == Some(d));
        if is_default && !is_repo_only(&k) {
            repo.unset_from(&k, &inherited);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "test_config.rs"]
mod tests;
