//! Which GitHub repository a repo's issues are read from (`issues_repo`, `slug`), and the
//! one way gh is asked about them (`gh_json`): always with `-R <repo>`, so gh's own guess
//! among remotes never matters; and `gh api`, which takes the repository in its path
//! (`gh_api_pages`), and the signed-in account's login (`gh_login`). gh runs one way for
//! reads and writes (`gh_quiet`): with no terminal, no prompt and a timeout.

use bullpen_core::proc::{git, quiet, Done};
use bullpen_core::task::clip;
use bullpen_core::text::{splitlines, strip};
use bullpen_core::{fail, Repo, Res};
use serde_json::Value;
use std::process::Command;

/// The GitHub repository a remote's URL names, as gh's `-R` takes it (`owner/repo`):
/// `git@github.com:owner/repo.git`, `https://github.com/owner/repo`, `ssh://...`. None
/// for a URL that isn't github.com's (a path, another host).
pub fn slug(url: &str) -> Option<String> {
    let u = strip(url).trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    let (host, path) = match u.split_once("://") {
        Some((_, rest)) => {
            let (at, path) = rest.split_once('/')?;
            (at.rsplit('@').next()?.split(':').next()?, path)
        }
        None => {
            // scp's form: [user@]host:path
            let (at, path) = u.split_once(':')?;
            (at.rsplit('@').next()?, path)
        }
    };
    let host = host.to_ascii_lowercase();
    if !["github.com", "www.github.com", "ssh.github.com"].contains(&host.as_str()) {
        return None;
    }
    let slug = path.trim_matches('/').to_string();
    (slug.split('/').count() == 2 && well_formed(&slug)).then_some(slug)
}

/// Is this what `-R` takes: `[host/]owner/repo`, each part of letters, digits, `.`, `_`
/// and `-`, and not starting with a dash (gh would read it as an option)?
pub fn well_formed(slug: &str) -> bool {
    let parts: Vec<&str> = slug.split('/').collect();
    let part = |p: &&str| {
        !p.is_empty() && !p.starts_with('-') && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    (2..=3).contains(&parts.len()) && parts.iter().all(part)
}

/// The repository to read issues from: the `issues_repo` setting, else the one origin's
/// URL names, or in local mode the push remote's.
pub fn issues_repo(repo: &Repo) -> Res<String> {
    const SET: &str = "`bullpen config --issues-repo OWNER/REPO` names it";
    let named = strip(repo.cfg_str("issues_repo"));
    if !named.is_empty() {
        if !well_formed(named) {
            return fail(format!(
                "issues_repo is {named}, which isn't a GitHub repository: {SET}"
            ));
        }
        return Ok(named.to_string());
    }
    let remote = if repo.has_origin()? {
        "origin"
    } else {
        repo.cfg_str("push_remote")
    };
    if remote.is_empty() {
        return fail(format!(
            "no GitHub repository to read issues from (no origin remote and no push remote): {SET}"
        ));
    }
    let url = git(&repo.root, &["remote", "get-url", remote], false)?;
    match slug(&url.stdout).filter(|_| url.ok()) {
        Some(slug) => Ok(slug),
        None => fail(format!(
            "can't tell a GitHub repository from the {remote} remote's URL: {SET}"
        )),
    }
}

/// Run `gh <args> -R <slug>` and read its JSON. `what` says what it was for, when it fails.
pub fn gh_json(repo: &Repo, slug: &str, args: &[&str], what: &str) -> Res<Value> {
    let mut argv: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    argv.extend(["-R".into(), slug.into()]);
    let out = gh_out(repo, &argv, what)?;
    match serde_json::from_str(&out) {
        Ok(v) => Ok(v),
        Err(_) => fail(format!("gh couldn't {what}: what it printed isn't JSON")),
    }
}

/// `gh api` on a path of the repository's (`repos/<owner>/<repo>/<path>`), every page:
/// what each page printed, in order. `gh api` takes no `-R`, so the repository is in the
/// path, and its host (a slug of three parts) is `--hostname`.
pub fn gh_api_pages(repo: &Repo, slug: &str, path: &str, what: &str) -> Res<Vec<Value>> {
    let parts: Vec<&str> = slug.split('/').collect();
    let (host, owner_repo) = match parts.as_slice() {
        [host, o, r] => (Some(*host), format!("{o}/{r}")),
        _ => (None, slug.to_string()),
    };
    let mut argv = vec!["api".to_string(), "--paginate".into()];
    if let Some(h) = host {
        argv.extend(["--hostname".into(), h.into()]);
    }
    argv.push(format!("repos/{owner_repo}/{path}"));
    let out = gh_out(repo, &argv, what)?;
    let pages: Result<Vec<Value>, _> = serde_json::Deserializer::from_str(&out).into_iter().collect();
    match pages {
        Ok(p) => Ok(p),
        Err(_) => fail(format!("gh couldn't {what}: what it printed isn't JSON")),
    }
}

/// `gh api user`'s login: the account gh is signed in as.
pub fn gh_login(repo: &Repo) -> Res<String> {
    let what = "say which account it is signed in as";
    let out = gh_out(repo, &["api".into(), "user".into()], what)?;
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    match v.get("login").and_then(Value::as_str).filter(|l| !l.is_empty()) {
        Some(l) => Ok(l.to_string()),
        None => fail(format!("gh couldn't {what}: what it printed has no login")),
    }
}

/// How long one gh read may take (every page of it) before it is given up.
const READ_TIMEOUT: u64 = 120;

/// The one way bullpen runs gh about issues, to read (`gh_out`) or to write (write.rs's
/// `gh_write`): in the repo, away from any terminal and with gh's prompts off, so nothing
/// can wait for the person; killed after timeout seconds.
pub fn gh_quiet<S: AsRef<std::ffi::OsStr>>(repo: &Repo, args: &[S], timeout: u64) -> Res<Done> {
    let mut cmd = Command::new(repo.gh());
    cmd.args(args).current_dir(&repo.root);
    cmd.env("GH_PROMPT_DISABLED", "1").env("GH_NO_UPDATE_NOTIFIER", "1");
    quiet(cmd, timeout)
}

/// What a gh that failed said, in a line (None: nothing).
pub fn gh_said(done: &Done) -> Option<&str> {
    let out = if strip(&done.stderr).is_empty() {
        &done.stdout
    } else {
        &done.stderr
    };
    splitlines(out).into_iter().map(strip).find(|l| !l.is_empty())
}

/// Run gh with args in the repo; what it printed. `what` says what it was for, when it fails.
fn gh_out(repo: &Repo, args: &[String], what: &str) -> Res<String> {
    let done = match gh_quiet(repo, args, READ_TIMEOUT) {
        Ok(d) => d,
        Err(e) => return fail(format!("gh couldn't {what}: it didn't run ({})", e.0)),
    };
    if !done.ok() {
        let said = gh_said(&done).unwrap_or("it failed");
        return fail(format!("gh couldn't {what}: {}", clip(said, 200)));
    }
    Ok(done.stdout)
}

#[cfg(test)]
#[path = "tests/test_repo.rs"]
mod tests;
