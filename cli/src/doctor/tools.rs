//! doctor's checks of the programs bullpen runs: Rex (its server and version), gh, and
//! the agents' programs.

use super::{Check, DoctorCheck};
use bullpen_core::json::{py_str, strs, truthy, Obj};
use bullpen_core::paths::rex;
use bullpen_core::proc::{run, which};
use bullpen_core::text::{splitlines, strip};
use bullpen_core::Repo;
use serde_json::Value;
use std::path::Path;

pub(super) const CHECKS: &[DoctorCheck] = &[REX, GH, AGENTS];

/// preview.13: minimized floating panes. A release sorts after its previews (u64::MAX).
pub const REX_MIN: (u64, u64, u64, u64) = (0, 1, 0, 13);
/// (major, minor, patch, preview) from "0.1.0-preview.13"; a release sorts after its
/// previews. None if it isn't a version (`v?(\d+)\.(\d+)\.(\d+)(?:-preview\.(\d+))?$`).
pub fn rex_version_key(text: &str) -> Option<(u64, u64, u64, u64)> {
    let t = strip(text);
    let t = t.strip_prefix('v').unwrap_or(t);
    let (nums, pre) = match t.split_once("-preview.") {
        Some((n, p)) => (n, Some(p)),
        None => (t, None),
    };
    let num = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let parts: Vec<&str> = nums.split('.').collect();
    let [a, b, c] = parts[..] else {
        return None;
    };
    let pre = match pre {
        Some(p) => num(p)?,
        None => u64::MAX,
    };
    Some((num(a)?, num(b)?, num(c)?, pre))
}

const REX: DoctorCheck = DoctorCheck {
    name: "rex",
    shared: true,
    run: |_| check_rex(),
    ..DoctorCheck::NEW
};

pub fn check_rex() -> Check {
    let r = rex();
    let Some(exe) = which(&r) else {
        return Ok((
            "fail",
            format!("{r} isn't on PATH: install Rex (https://www.superlogical.com/rex)"),
        ));
    };
    let p = run(&[r.as_str(), "server", "status", "--json"], false)?;
    if !p.ok() {
        return Ok((
            "fail",
            format!("{exe}, but its server doesn't answer: start Rex, or run `rex server start`"),
        ));
    }
    let version = match serde_json::from_str::<Value>(&p.stdout) {
        Ok(Value::Object(o)) => o.get("rex_version").filter(|v| !v.is_null()).map(py_str),
        _ => None,
    };
    let key = version.as_deref().and_then(rex_version_key);
    let Some(key) = key else {
        let shown = version.filter(|v| !v.is_empty()).unwrap_or_else(|| "none given".into());
        return Ok(("skip", format!("{exe}; can't tell the server's version ({shown})")));
    };
    let version = version.unwrap_or_default();
    if key < REX_MIN {
        return Ok((
            "fail",
            format!(
                "the server is {version}; bullpen needs 0.1.0-preview.13 or later \
                 (minimized floating panes): update Rex and restart its server"
            ),
        ));
    }
    Ok(("ok", format!("server {version}, {exe}")))
}

const GH: DoctorCheck = DoctorCheck {
    name: "gh",
    slow: true,
    run: |c| check_gh(c.repo),
    ..DoctorCheck::NEW
};

/// gh: installed and signed in, for a repo with a remote (pull requests). With `issues`
/// on it is checked in local mode too, and for the `repo` scope, which reading a
/// private repository's issues needs.
pub fn check_gh(repo: Option<&Repo>) -> Check {
    let issues = repo.is_some_and(|r| truthy(r.cfg.get("issues")));
    let (repo, remote) = match repo {
        Some(r) if r.has_origin()? => (r, true),
        Some(r) if issues => (r, false),
        _ => return Ok(("skip", "local mode (no origin remote): gh isn't needed".into())),
    };
    let gh = repo.gh();
    let Some(exe) = which(&gh) else {
        let why = if remote {
            "this repo has a remote"
        } else {
            "`issues` is on"
        };
        return Ok((
            "fail",
            format!("{gh} isn't on PATH, and {why}: install gh (https://cli.github.com)"),
        ));
    };
    let status = run(&[exe.as_str(), "auth", "status"], false)?;
    if !status.ok() {
        return Ok(("fail", format!("{exe} isn't signed in: run `gh auth login`")));
    }
    if !issues {
        return Ok(("ok", format!("signed in, {exe}")));
    }
    match repo_scope(&format!("{}\n{}", status.stdout, status.stderr)) {
        Some(false) => Ok((
            "fail",
            format!(
                "{exe} is signed in without the `repo` scope, which reading issues needs: run `gh auth refresh -s repo`"
            ),
        )),
        Some(true) => Ok(("ok", format!("signed in, with the repo scope (for issues), {exe}"))),
        None => Ok(("ok", format!("signed in, {exe} (it doesn't say its token's scopes)"))),
    }
}

/// Does `gh auth status` say the token in use has the `repo` scope? From its "Token
/// scopes: 'gist', 'repo'" lines, those of an account that isn't the active one left
/// out. None when it names no scopes (a fine-grained token has none): can't tell.
pub fn repo_scope(said: &str) -> Option<bool> {
    let mut active = true;
    let mut found = None;
    for l in splitlines(said) {
        if l.contains("Logged in to") {
            active = true;
        } else if l.contains("Active account: false") {
            active = false;
        }
        let Some((_, scopes)) = l.split_once("Token scopes:").filter(|_| active) else {
            continue;
        };
        let mut scopes = scopes.split(',').map(|w| w.trim().trim_matches(['\'', '"']));
        let scopes: Vec<&str> = scopes.by_ref().filter(|w| !w.is_empty() && *w != "none").collect();
        if !scopes.is_empty() {
            found = Some(found == Some(true) || scopes.contains(&"repo"));
        }
    }
    found
}

const AGENTS: DoctorCheck = DoctorCheck {
    name: "agents",
    run: |c| check_agents(c.cfg),
    ..DoctorCheck::NEW
};

/// The programs the coordinator, workers and reviewers run (claude, unless changed).
pub fn check_agents(cfg: &Obj) -> Check {
    let mut progs: Vec<(String, Vec<&str>)> = Vec::new();
    for key in ["coordinator_cmd", "worker", "reviewer"] {
        if !truthy(cfg.get(key)) {
            continue;
        }
        let Some(p) = strs(cfg, key).into_iter().next() else {
            continue;
        };
        match progs.iter_mut().find(|(q, _)| *q == p) {
            Some((_, keys)) => keys.push(key),
            None => progs.push((p, vec![key])),
        }
    }
    let missing: Vec<String> = progs
        .iter()
        .filter(|(p, _)| which(p).is_none())
        .map(|(p, keys)| {
            let what = if Path::new(p).file_name().is_some_and(|n| n == "claude") {
                format!("{p} isn't on PATH: install Claude Code (https://claude.com/claude-code)")
            } else {
                format!("{p} isn't on PATH")
            };
            format!("{what} (config.json's {})", keys.join(", "))
        })
        .collect();
    if !missing.is_empty() {
        return Ok(("fail", missing.join("; ")));
    }
    let found: Vec<String> = progs.iter().filter_map(|(p, _)| which(p)).collect();
    Ok(("ok", found.join(", ")))
}
