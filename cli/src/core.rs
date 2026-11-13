//! The base of the port (bullpen/core.py): paths, running programs, a repo's config
//! and tasks, and a task's status. A task is its JSON object as the file has it
//! (`Obj`), so what the port doesn't know about is written back untouched.

use crate::json::{self, get, s, truthy, Obj};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Why a command stopped. `Python`: the port can't do this one as Python would (an
/// old config to migrate, a change that notifies the person): main hands the whole
/// command line to the Python bullpen instead. Only before anything is written or printed.
#[derive(Debug)]
pub enum Fail {
    Msg(String),
    Python,
}

pub type Res<T> = Result<T, Fail>;

pub fn fail<T>(msg: impl Into<String>) -> Res<T> {
    Err(Fail::Msg(msg.into()))
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Fail {
        Fail::Msg(e.to_string())
    }
}

// ---------- paths ----------

/// Path.home(): $HOME.
pub fn user_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

/// <state>: $BULLPEN_STATE, else ~/.local/state/bullpen.
pub fn state_dir() -> PathBuf {
    match std::env::var("BULLPEN_STATE") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(user_home()).join(".local/state/bullpen"),
    }
}

pub fn rex_program() -> String {
    std::env::var("BULLPEN_REX").unwrap_or_else(|_| "rex".into())
}

/// bullpen's checkout: the folder above this program's that has bin/bullpen.
pub fn checkout() -> PathBuf {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).unwrap_or_default();
    exe.ancestors()
        .find(|d| d.join("bin/bullpen").is_file() && d.join("bullpen/core.py").is_file())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
}

/// The Python entry script (core.SELF).
pub fn python_self() -> PathBuf {
    checkout().join("bin/bullpen")
}

pub fn safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect()
}

/// A path with $HOME as ~, for people to read.
pub fn home(path: &str) -> String {
    let h = user_home();
    if h != "/" && (path == h || path.starts_with(&format!("{h}/"))) {
        format!("~{}", &path[h.len()..])
    } else {
        path.into()
    }
}

/// Python's `\w`: a letter, digit or underscore (in any script).
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// ---------- running programs ----------

pub struct Done {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// core.run: the program's exit code and output; stdin is /dev/null. With check, a
/// failure is a Fail that quotes the command and what it said.
pub fn run(argv: &[&str], check: bool) -> Res<Done> {
    let out = Command::new(argv[0]).args(&argv[1..]).stdin(Stdio::null()).output();
    let done = match out {
        Ok(o) => Done {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return fail(format!("[Errno 2] No such file or directory: '{}'", argv[0]))
        }
        Err(e) => return Err(e.into()),
    };
    if check && done.code != 0 {
        let msg = if done.stderr.is_empty() { &done.stdout } else { &done.stderr };
        return fail(format!("`{}` failed: {}", argv.join(" "), crate::text::strip(msg)));
    }
    Ok(done)
}

pub fn git(repo: &str, args: &[&str], check: bool) -> Res<Done> {
    let mut argv = vec!["git", "-C", repo];
    argv.extend_from_slice(args);
    run(&argv, check)
}

// ---------- config ----------

/// Every setting's default, in core.SETTINGS's order.
pub fn defaults() -> Obj {
    let agent = json!(["claude", "--permission-mode", "auto"]);
    let v = json!({
        "prefix": "BP", "next": 1, "base": null, "max_running": 3, "merge": "rebase",
        "rebase_others": true, "push_remote": null, "auto_accept": false, "setup": null,
        "teardown": null, "gh": null, "worker": agent, "coordinator_cmd": agent, "reviewer": agent,
        "tour": ["claude"], "workers": "float", "worker_model": null, "reviewer_model": null,
        "reviewer_followup_model": "sonnet", "tour_model": "sonnet", "review": true, "review_rounds": 2,
        "light_review_lines": 40, "board_layout": "grouped", "board_bottom_gap": 1, "board_repos": true,
        "news": "type", "notify": "needs", "stuck_after": 20, "triage_minutes": 10, "coordinators": [],
        "next_coordinator": 1,
    });
    match v {
        Value::Object(o) => o,
        _ => unreachable!(),
    }
}

/// Settings only a repository can have.
pub const REPO_ONLY: [&str; 7] = ["prefix", "next", "base", "push_remote", "news", "coordinators", "next_coordinator"];
/// What bullpen notes for itself in a repo's config.json.
pub const INTERNAL: [&str; 4] = ["root", "triage_migrated", "stops_migrated", "layered"];

pub fn global_config_path() -> PathBuf {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(user_home()).join(".config"),
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
        // Python's message names the parse error its own way: let it say it
        Err(_) => Err(Fail::Python),
    }
}

pub fn read_global() -> Res<Obj> {
    Ok(read_global_file()?
        .into_iter()
        .filter(|(k, _)| !REPO_ONLY.contains(&k.as_str()) && !INTERNAL.contains(&k.as_str()))
        .collect())
}

/// The global settings over the built-in defaults.
pub fn inherited_config() -> Res<Obj> {
    let mut cfg = defaults();
    cfg.extend(read_global()?);
    Ok(cfg)
}

/// A JSON file's object: {} when it is missing. A file that doesn't parse goes to Python.
fn read_obj(path: &Path) -> Res<Obj> {
    match json::read(path) {
        Ok(None) => Ok(Obj::new()),
        Ok(Some(Value::Object(o))) => Ok(o),
        _ => Err(Fail::Python),
    }
}

/// A repo's settings: its own config.json over the global one, over the defaults.
pub fn load_config(dir: &Path) -> Res<Obj> {
    let mut cfg = inherited_config()?;
    cfg.extend(read_obj(&dir.join("config.json"))?);
    if cfg.contains_key("coordinator") || cfg.contains_key("board") {
        let old = cfg.shift_remove("coordinator").unwrap_or(Value::Null);
        let board = cfg.shift_remove("board").unwrap_or(Value::Null);
        let none = cfg.get("coordinators").and_then(Value::as_array).is_none_or(|a| a.is_empty());
        if truthy(Some(&old)) && none {
            let o = old.as_object().cloned().unwrap_or_default();
            let c = json!({"id": "c1", "block": o.get("block"), "session": o.get("session"), "board": board});
            cfg.insert("coordinators".into(), json!([c]));
            cfg.insert("next_coordinator".into(), json!(2));
        }
    }
    Ok(cfg)
}

// ---------- a repo and its tasks ----------

pub struct Repo {
    pub root: String,
    pub dir: PathBuf,
    pub cfg: Obj,
    /// the keys this repo's config.json sets itself
    pub own: Vec<String>,
}

impl Repo {
    pub fn new(root: &str) -> Res<Repo> {
        let dir = state_dir().join("repos").join(safe(&root.trim_matches('/').replace('/', "-")));
        let cfg = load_config(&dir)?;
        let own: Vec<String> = read_obj(&dir.join("config.json"))?.keys().cloned().collect();
        if dir.join("config.json").exists()
            && !["triage_migrated", "stops_migrated", "layered"].iter().all(|k| truthy(cfg.get(*k)))
        {
            return Err(Fail::Python); // a config from before a migration: Python migrates it
        }
        Ok(Repo { root: root.into(), dir, cfg, own })
    }

    /// The repo folder's name.
    pub fn name(&self) -> &str {
        self.root.rsplit('/').next().unwrap_or("")
    }

    pub fn cfg_str(&self, key: &str) -> &str {
        s(&self.cfg, key)
    }

    /// The coordinators, as config.json lists them.
    pub fn coordinators(&self) -> Vec<&Obj> {
        self.cfg.get("coordinators").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object).collect()
    }

    /// Write config.json: only what this repo sets itself (Repo.save).
    pub fn save(&mut self) -> Res<()> {
        self.cfg.insert("root".into(), json!(self.root));
        for k in ["triage_migrated", "stops_migrated", "layered"] {
            self.cfg.insert(k.into(), json!(true));
        }
        let inherited = inherited_config()?;
        let own: Obj = self
            .cfg
            .iter()
            .filter(|(k, _)| {
                let k = k.as_str();
                self.own.iter().any(|o| o == k) || REPO_ONLY.contains(&k) || INTERNAL.contains(&k) || !inherited.contains_key(k)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.own = own.keys().cloned().collect();
        json::write(&self.dir.join("config.json"), &Value::Object(own))?;
        Ok(())
    }

    pub fn task_path(&self, tid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{tid}.json"))
    }

    pub fn norm(&self, tid: &str) -> String {
        let t = tid.to_uppercase();
        if t.contains('-') {
            t
        } else {
            format!("{}-{t}", self.cfg_str("prefix"))
        }
    }

    pub fn task(&self, tid: &str) -> Res<Obj> {
        let tid = self.norm(tid);
        match read_task(&self.task_path(&tid))? {
            Some(t) => Ok(t),
            None => fail(format!("no task {tid} in {}: `bullpen ls --all` lists them", home(&self.root))),
        }
    }

    /// Every task, by number.
    pub fn tasks(&self) -> Res<Vec<Obj>> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.dir.join("tasks")) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".json") && !name.starts_with('.') {
                    out.extend(read_task(&e.path())?);
                }
            }
        }
        out.sort_by_key(task_num);
        Ok(out)
    }

    /// Repo.save_task: write a task with when it was updated and when it entered its
    /// status. What Python's on_save hooks would do beyond that (a notification, the run
    /// queue) the port doesn't: such a change goes to Python. A PR to follow is marked.
    pub fn save_task(&self, t: &mut Obj) -> Res<()> {
        let path = self.task_path(s(t, "id"));
        let old = read_task(&path)?.unwrap_or_default();
        let changed = get(&old, "status") != get(t, "status");
        if notifies(&self.cfg, t, &old) || slot_freed(&old, t) {
            return Err(Fail::Python);
        }
        t.insert("updated".into(), json!(crate::timeutil::iso_now()));
        if changed {
            t.insert("note".into(), Value::Null);
            t.shift_remove("stuck");
            t.shift_remove("auto_wait");
        }
        if changed || !truthy(t.get("since")) {
            t.insert("since".into(), json!(crate::timeutil::iso_now_micro()));
        }
        json::write(&path, &Value::Object(t.clone()))?;
        if followed(t) {
            let st = state_dir();
            std::fs::create_dir_all(&st)?;
            std::fs::OpenOptions::new().create(true).append(true).open(st.join("prs"))?;
        }
        Ok(())
    }

    /// The branch tasks start from: the config's, else origin's default, else the
    /// checked-out one, else main.
    pub fn base(&self) -> Res<String> {
        if truthy(self.cfg.get("base")) {
            return Ok(self.cfg_str("base").into());
        }
        let p = git(&self.root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], false)?;
        if p.code == 0 {
            let b = crate::text::strip(&p.stdout);
            return Ok(b.split_once('/').map_or(b, |(_, r)| r).into());
        }
        let cur = git(&self.root, &["branch", "--show-current"], true)?;
        let cur = crate::text::strip(&cur.stdout);
        Ok(if cur.is_empty() { "main".into() } else { cur.into() })
    }

    pub fn has_origin(&self) -> Res<bool> {
        Ok(git(&self.root, &["remote", "get-url", "origin"], false)?.code == 0)
    }
}

/// A task's file; None when it is missing. One that doesn't parse goes to Python, whose
/// error says so its own way.
pub fn read_task(path: &Path) -> Res<Option<Obj>> {
    match json::read(path) {
        Ok(None) => Ok(None),
        Ok(Some(Value::Object(o))) => Ok(Some(o)),
        _ => Err(Fail::Python),
    }
}

pub fn task_num(t: &Obj) -> i64 {
    s(t, "id").rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// notifications.notice, as far as whether one goes out under the repo's notify level.
fn notifies(cfg: &Obj, t: &Obj, old: &Obj) -> bool {
    let (st, was) = (s(t, "status"), s(old, "status"));
    let asked = truthy(t.get("ask")) && !(truthy(old.get("ask")) && get(old, "why") == get(t, "why"));
    let kind = if st == "needs" && (was != "needs" || asked) {
        "needs"
    } else if st == was {
        return false;
    } else if st == "ready" || st == "finished" || (st == "done" && truthy(t.get("auto_accepted"))) {
        "news"
    } else {
        return false;
    };
    let level = s(cfg, "notify");
    !(level == "off" || (level == "needs" && kind != "needs"))
}

/// start.slot_freed: does the save move the run queue?
fn slot_freed(old: &Obj, t: &Obj) -> bool {
    let closed = |o: &Obj| matches!(s(o, "status"), "done" | "dropped");
    (closed(t) && !closed(old)) || (s(old, "status") == "working" && matches!(s(t, "status"), "review" | "ready" | "finished"))
}

/// prs.followed: an open PR of a task that waits on review or the person.
pub fn followed(t: &Obj) -> bool {
    let open = match json::obj(t, "gh").get("state") {
        None => true,
        Some(v) => v.as_str() == Some("open"),
    };
    truthy(t.get("pr")) && !truthy(t.get("local")) && matches!(s(t, "status"), "review" | "ready" | "needs" | "triage") && open
}

/// The repo this command runs in: the main checkout of the git repo around the
/// current folder (a worktree's too).
pub fn repo_root() -> Res<String> {
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".into());
    let p = run(&["git", "-C", &cwd, "rev-parse", "--path-format=absolute", "--git-common-dir"], false)?;
    if p.code != 0 {
        return fail("not in a git repository");
    }
    let common = crate::text::strip(&p.stdout).to_string();
    let path = Path::new(&common);
    Ok(if path.file_name().is_some_and(|n| n == ".git") {
        path.parent().map(|p| p.display().to_string()).unwrap_or(common)
    } else {
        common
    })
}

/// Every repository bullpen has state for whose checkout is still there, by state folder.
pub fn known_repos() -> Res<Vec<Repo>> {
    let mut cfgs: Vec<PathBuf> = std::fs::read_dir(state_dir().join("repos"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("config.json"))
        .filter(|p| p.is_file())
        .collect();
    cfgs.sort();
    let mut out = Vec::new();
    for cfg in cfgs {
        let Ok(Some(Value::Object(o))) = json::read(&cfg) else { continue };
        let root = s(&o, "root");
        if !root.is_empty() && Path::new(root).is_dir() {
            out.push(Repo::new(root)?);
        }
    }
    Ok(out)
}

/// The tasks `ls` lists: the open ones, or all with --all.
pub fn listed(repo: &Repo, all: bool) -> Res<Vec<Obj>> {
    Ok(repo.tasks()?.into_iter().filter(|t| all || !is_closed(s(t, "status"))).collect())
}

// ---------- a task in words, and its status ----------

pub fn is_closed(status: &str) -> bool {
    matches!(status, "done" | "dropped")
}

pub fn is_stopped(status: &str) -> bool {
    matches!(status, "needs" | "triage" | "stopped")
}

pub fn pr_short(url: &str) -> String {
    if let Some(i) = url.find("/pull/") {
        let n: String = url[i + 6..].chars().take_while(char::is_ascii_digit).collect();
        if !n.is_empty() {
            return format!("PR #{n}");
        }
    }
    url.into()
}

/// The worker model a task chose for itself, if it isn't the config's.
pub fn own_model<'a>(repo: &Repo, t: &'a Obj) -> Option<&'a str> {
    let m = t.get("worker_model").filter(|v| truthy(Some(v)))?;
    (Some(m) != repo.cfg.get("worker_model")).then(|| m.as_str().unwrap_or(""))
}

/// When it entered its current status.
pub fn stamp(t: &Obj) -> &str {
    let since = s(t, "since");
    if since.is_empty() {
        s(t, "updated")
    } else {
        since
    }
}

fn board_words(status: &str) -> &str {
    match status {
        "needs" | "stopped" => "needs you",
        "triage" => "coordinator on it",
        "ready" => "ready to accept",
        "finished" => "finished",
        "review" => "in review",
        "working" => "working",
        "todo" => "queued",
        "done" | "dropped" => "closed",
        other => other,
    }
}

pub fn status_words(status: &str) -> String {
    let words = board_words(status);
    if words.contains(status) {
        words.into()
    } else {
        format!("{words} ({status})")
    }
}

/// "BP-3 is queued (todo)", "BP-3 needs you (stopped)": for errors.
pub fn is_status(t: &Obj) -> String {
    let words = status_words(s(t, "status"));
    let id = s(t, "id");
    if words.starts_with("coordinator") {
        format!("{id}'s {}", words.replacen(" on it", " is on it", 1))
    } else if words.starts_with("needs") {
        format!("{id} {words}")
    } else {
        format!("{id} is {words}")
    }
}

/// core.set_status: the one place a task's status changes; a new status ends ask and ok.
pub fn set_status(t: &mut Obj, status: &str, why: Option<&str>, stop: Option<Value>) {
    if t.get("status").and_then(Value::as_str) != Some(status) {
        for k in ["ok", "ask", "asked_from"] {
            t.shift_remove(k);
        }
    }
    t.insert("status".into(), json!(status));
    t.insert("why".into(), why.map_or(Value::Null, |w| json!(w)));
    if let Some(stop) = stop {
        t.insert("stop".into(), stop);
    } else if !is_stopped(status) {
        t.shift_remove("stop");
    }
}

/// Is the task's stop its reviewer's? (Not one that couldn't be continued after the limit.)
pub fn reviewers_stop(t: &Obj) -> bool {
    let st = json::obj(t, "stop");
    s(st, "by") == "reviewer" && s(st, "kind") != "limit"
}
