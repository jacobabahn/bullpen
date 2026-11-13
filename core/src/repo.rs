//! A repo bullpen runs in (core.Repo): its state folder, its settings and its tasks;
//! finding the repo a command is about; and the panes bullpen knows (<state>/blocks).

use crate::config::{self, inherited_config, load_config, read_own};
use crate::fail::{fail, Res};
use crate::json::{self, s, truthy, Obj};
use crate::paths::{home_path, safe, state};
use crate::proc::{git, run};
use crate::settings::{is_internal, is_repo_only};
use crate::task::task_num;
use crate::text::strip;
use crate::timeutil::{iso_now, iso_now_micro};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A repository bullpen runs in.
#[derive(Debug, Clone)]
pub struct Repo {
    /// The main checkout.
    pub root: PathBuf,
    /// Its state folder: <state>/repos/<root as a name>.
    pub dir: PathBuf,
    /// Its settings: its own over the global ones, over the defaults.
    pub cfg: Obj,
    /// The keys this repo sets itself (its config.json's).
    pub own: BTreeSet<String>,
}

/// A path as Python's Path() writes it: no doubled or trailing slash, no "." parts.
fn py_path(p: &Path) -> PathBuf {
    p.components().collect()
}

impl Repo {
    /// The repo at root, its config read. A config.json from before a migration is
    /// migrated (triage, stops, layers) and saved, as core.Repo's __init__ does.
    pub fn new(root: impl AsRef<Path>) -> Res<Repo> {
        let root = py_path(root.as_ref());
        let name = root.display().to_string().trim_matches('/').replace('/', "-");
        let dir = state().join("repos").join(safe(&name));
        let cfg = load_config(&dir)?;
        let own = read_own(&dir)?.keys().cloned().collect();
        let mut repo = Repo { root, dir, cfg, own };
        let done = ["triage_migrated", "stops_migrated", "layered"]
            .iter()
            .all(|k| truthy(repo.cfg.get(*k)));
        if repo.dir.join("config.json").exists() && !done {
            if !truthy(repo.cfg.get("triage_migrated")) {
                config::migrate_triage(&repo)?;
            }
            if !truthy(repo.cfg.get("stops_migrated")) {
                config::migrate_stops(&repo)?;
            }
            if !truthy(repo.cfg.get("layered")) {
                config::migrate_layers(&mut repo)?;
            }
            repo.save()?;
        }
        Ok(repo)
    }

    /// The root as text.
    pub fn root_str(&self) -> String {
        self.root.display().to_string()
    }

    /// The repo folder's name.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// A setting's text ("" when it isn't text).
    pub fn cfg_str(&self, key: &str) -> &str {
        s(&self.cfg, key)
    }

    /// This repo's own value for a setting, over the global one.
    pub fn set(&mut self, key: &str, value: Value) {
        self.cfg.insert(key.into(), value);
        self.own.insert(key.into());
    }

    /// Drop this repo's own value for a setting: the global one (or the default) applies.
    pub fn unset(&mut self, key: &str) -> Res<()> {
        let inherited = inherited_config()?;
        self.unset_from(key, &inherited);
        Ok(())
    }

    /// unset, with what it inherits already read.
    pub fn unset_from(&mut self, key: &str, inherited: &Obj) {
        self.own.remove(key);
        match inherited.get(key) {
            Some(v) => {
                self.cfg.insert(key.into(), v.clone());
            }
            None => {
                self.cfg.shift_remove(key);
            }
        }
    }

    /// Write config.json: only what this repo sets itself; the rest comes from the
    /// global config.json and the defaults, as they are each time they're read.
    pub fn save(&mut self) -> Res<()> {
        self.cfg.insert("root".into(), json!(self.root_str()));
        self.cfg.insert("triage_migrated".into(), json!(true)); // tasks made from here on know triage
        self.cfg.insert("stops_migrated".into(), json!(true)); // and record their stops
        self.cfg.insert("layered".into(), json!(true)); // and keep only their own settings
        let inherited = inherited_config()?;
        let own: Obj = self
            .cfg
            .iter()
            .filter(|(k, _)| self.own.contains(*k) || is_repo_only(k) || is_internal(k) || !inherited.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.own = own.keys().cloned().collect();
        json::write_obj(&self.dir.join("config.json"), &own)?;
        Ok(())
    }

    /// The gh program: $BULLPEN_GH, else the gh setting, else "gh".
    pub fn gh(&self) -> String {
        match std::env::var("BULLPEN_GH") {
            Ok(v) if !v.is_empty() => v,
            _ if truthy(self.cfg.get("gh")) => self.cfg_str("gh").into(),
            _ => "gh".into(),
        }
    }

    /// No origin remote means local mode: tasks merge into the local base branch.
    pub fn has_origin(&self) -> Res<bool> {
        Ok(git(&self.root, &["remote", "get-url", "origin"], false)?.ok())
    }

    /// The branch tasks start from: the config's, else origin's default, else the
    /// checked-out one, else main.
    pub fn base(&self) -> Res<String> {
        if truthy(self.cfg.get("base")) {
            return Ok(json::py_str(&self.cfg["base"]));
        }
        let p = git(
            &self.root,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
            false,
        )?;
        if p.ok() {
            let b = strip(&p.stdout);
            return Ok(b.split_once('/').map_or(b, |(_, rest)| rest).into());
        }
        let cur = git(&self.root, &["branch", "--show-current"], true)?;
        let cur = strip(&cur.stdout);
        Ok(if cur.is_empty() { "main".into() } else { cur.into() })
    }

    pub fn task_path(&self, tid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{tid}.json"))
    }

    /// A task ID in full and in capitals: "3" is "BP-3", "bp-3" is "BP-3".
    pub fn norm(&self, tid: &str) -> String {
        let tid = tid.to_uppercase();
        if tid.contains('-') {
            tid
        } else {
            format!("{}-{tid}", json::py_str(self.cfg.get("prefix").unwrap_or(&Value::Null)))
        }
    }

    /// The task tid; a Fail when there is none.
    pub fn task(&self, tid: &str) -> Res<Obj> {
        let tid = self.norm(tid);
        match json::read_obj(&self.task_path(&tid))? {
            Some(t) => Ok(t),
            None => fail(format!(
                "no task {tid} in {}: `bullpen ls --all` lists them",
                home_path(&self.root)
            )),
        }
    }

    /// Every task, by number.
    pub fn tasks(&self) -> Res<Vec<Obj>> {
        let mut out = Vec::new();
        for p in config::task_files(&self.dir) {
            out.extend(json::read_obj(&p)?);
        }
        out.sort_by_key(task_num);
        Ok(out)
    }

    /// core.Repo.save_task, up to writing it: stamp when it was updated and, when its
    /// status changed (or it has no since), when it entered it; a new status drops
    /// its note, stuck and auto_wait. Returns the task as it was saved before ({} for
    /// a new one): what a save sets off (the run queue, a notification, a PR to
    /// follow) is the caller's to run, with both.
    pub fn save_task(&self, t: &mut Obj) -> Res<Obj> {
        if s(t, "id").is_empty() {
            return fail("a task to save has no id");
        }
        t.insert("updated".into(), json!(iso_now()));
        let path = self.task_path(s(t, "id"));
        let old = json::read_obj(&path)?.unwrap_or_default();
        let changed = old.get("status").unwrap_or(&Value::Null) != t.get("status").unwrap_or(&Value::Null);
        if changed {
            t.insert("note".into(), Value::Null); // what it needed then, not now
            t.shift_remove("stuck");
            t.shift_remove("auto_wait"); // only an approval that is still current accepts it
        }
        if changed || !truthy(t.get("since")) {
            // when it entered its current status; fine-grained so `go --next` can order it
            t.insert("since".into(), json!(iso_now_micro()));
        }
        json::write_obj(&path, t)?;
        Ok(old)
    }

    /// Where a task's worktree goes: <root>-bullpen/<ID> beside the checkout.
    pub fn worktree_dir(&self, t: &Obj) -> PathBuf {
        let parent = self.root.parent().unwrap_or(Path::new("/"));
        parent.join(format!("{}-bullpen", self.name())).join(s(t, "id"))
    }
}

/// The repo a command runs in: the main checkout of the git repo around the current
/// folder (a worktree's too).
pub fn repo_root() -> Res<PathBuf> {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    repo_root_of(Path::new(&cwd))
}

/// repo_root, for the git repo around path.
pub fn repo_root_of(path: &Path) -> Res<PathBuf> {
    let path = path.display().to_string();
    let p = run(
        &[
            "git",
            "-C",
            &path,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ],
        false,
    )?;
    if !p.ok() {
        return fail("not in a git repository");
    }
    let common = PathBuf::from(strip(&p.stdout));
    let is_git = common.file_name().is_some_and(|n| n == ".git");
    Ok(match common.parent() {
        Some(parent) if is_git => parent.to_path_buf(),
        _ => common,
    })
}

// ---------- panes bullpen knows (<state>/blocks), and finding the repo ----------

fn block_path(block: &str) -> PathBuf {
    state().join("blocks").join(format!("{}.json", safe(block)))
}

/// Note the pane block as the repo's, in role ("worker", "reviewer", ...).
pub fn index_block(block: &str, repo: &Repo, role: &str) -> Res<()> {
    // the watcher writes what the pane's agent is doing into doing/, for the board
    std::fs::create_dir_all(state().join("doing"))?;
    // stuck_after is a copy of the repo's setting, where the watcher can read it
    let entry = json!({"repo": repo.root_str(), "role": role, "stuck_after": repo.cfg.get("stuck_after")});
    json::write(&block_path(block), &entry)?;
    Ok(())
}

/// Give the repo's panes its settings again (after `config` changed them).
pub fn reindex(repo: &Repo) -> Res<()> {
    let root = repo.root_str();
    for p in json_files(&state().join("blocks")) {
        let mut entry = json::read_obj(&p)?.unwrap_or_default();
        if s(&entry, "repo") == root {
            entry.insert(
                "stuck_after".into(),
                repo.cfg.get("stuck_after").cloned().unwrap_or(Value::Null),
            );
            json::write_obj(&p, &entry)?;
        }
    }
    Ok(())
}

/// The *.json files in dir (dot-files too, as pathlib's glob).
fn json_files(dir: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    entries
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".json"))
        .collect()
}

/// Note in the pane's index entry that its agent is compacting (on), or no longer,
/// so the watcher passes on its next working event (the one that may start a real turn).
pub fn mark_compacting(block: &str, on: bool) -> Res<()> {
    if block.is_empty() {
        return Ok(());
    }
    let path = block_path(block);
    let Some(mut entry) = json::read_obj(&path)?.filter(|e| !e.is_empty()) else {
        return Ok(());
    };
    if truthy(entry.get("compacting")) != on {
        entry.shift_remove("compacting");
        if on {
            entry.insert("compacting".into(), json!(true));
        }
        json::write_obj(&path, &entry)?;
    }
    Ok(())
}

/// Forget the pane block: its index entry, and what the watcher noted it doing.
pub fn unindex_block(block: &str) -> Res<()> {
    if block.is_empty() {
        return Ok(());
    }
    for dir in ["blocks", "doing"] {
        match std::fs::remove_file(state().join(dir).join(format!("{}.json", safe(block)))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

/// The repo that owns the pane block, and the pane's role there; None for a pane
/// bullpen doesn't know.
pub fn lookup_block(block: Option<&str>) -> Res<Option<(Repo, String)>> {
    let Some(block) = block.filter(|b| !b.is_empty()) else {
        return Ok(None);
    };
    let Some(entry) = json::read_obj(&block_path(block))?.filter(|e| !e.is_empty()) else {
        return Ok(None);
    };
    Ok(Some((Repo::new(s(&entry, "repo"))?, s(&entry, "role").into())))
}

/// The repo a command is about: the one owning from_block, else the one we're in,
/// else (for key bindings, which run in the Rex server's directory) the bullpen repo
/// whose coordinator most recently started.
pub fn find_repo(from_block: Option<&str>) -> Res<Repo> {
    if let Some((repo, _)) = lookup_block(from_block)? {
        return Ok(repo);
    }
    let here = repo_root().and_then(Repo::new);
    if let Ok(repo) = here {
        if repo.dir.join("config.json").exists() {
            return Ok(repo);
        }
    }
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for cfg in config_files()? {
        let Some(dir) = cfg.parent() else { continue };
        let coordinators = load_config(dir)?.get("coordinators").cloned().unwrap_or(Value::Null);
        for c in coordinators
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
        {
            if !truthy(c.get("block")) {
                continue;
            }
            let idx = block_path(&json::py_str(&c["block"]));
            let Some(entry) = json::read_obj(&idx)?.filter(|e| !e.is_empty()) else {
                continue;
            };
            let Ok(mtime) = std::fs::metadata(&idx).and_then(|m| m.modified()) else {
                continue;
            };
            if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
                best = Some((mtime, s(&entry, "repo").into()));
            }
        }
    }
    match best {
        Some((_, root)) => Repo::new(root),
        None => fail("no bullpen here: run `bullpen up` in a repository first"),
    }
}

/// <state>/repos/*/config.json, sorted.
fn config_files() -> Res<Vec<PathBuf>> {
    let dirs = std::fs::read_dir(state().join("repos")).into_iter().flatten().flatten();
    let mut out: Vec<PathBuf> = dirs
        .map(|e| e.path().join("config.json"))
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    Ok(out)
}

/// Every repository bullpen has state for (<state>/repos/*) whose checkout is still
/// there, by state folder.
pub fn known_repos() -> Res<Vec<Repo>> {
    let mut out = Vec::new();
    for cfg in config_files()? {
        // a config.json caught half-written, or broken: that repo's own commands say so
        let Ok(Some(Value::Object(o))) = json::read(&cfg) else {
            continue;
        };
        let root = s(&o, "root");
        if !root.is_empty() && Path::new(root).is_dir() {
            out.push(Repo::new(root)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "test_repo.rs"]
mod tests;
