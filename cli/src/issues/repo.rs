//! Which GitHub repository a repo's issues are read from, and the one way gh is asked
//! about them: always with `-R <repo>`, so gh's own guess among remotes never matters.

use bullpen_core::proc::{git, run_in};
use bullpen_core::task::clip;
use bullpen_core::text::{splitlines, strip};
use bullpen_core::{fail, Repo, Res};
use serde_json::Value;

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
    let mut argv = vec![repo.gh()];
    argv.extend(args.iter().map(|a| a.to_string()));
    argv.extend(["-R".into(), slug.into()]);
    let done = match run_in(&argv, &repo.root, false) {
        Ok(d) => d,
        Err(e) => return fail(format!("gh couldn't {what}: it didn't run ({})", e.0)),
    };
    if !done.ok() {
        let out = if strip(&done.stderr).is_empty() {
            &done.stdout
        } else {
            &done.stderr
        };
        let said = splitlines(out).into_iter().map(strip).find(|l| !l.is_empty());
        return fail(format!(
            "gh couldn't {what}: {}",
            clip(said.unwrap_or("it failed"), 200)
        ));
    }
    match serde_json::from_str(&done.stdout) {
        Ok(v) => Ok(v),
        Err(_) => fail(format!("gh couldn't {what}: what it printed isn't JSON")),
    }
}

#[cfg(test)]
#[path = "test_repo.rs"]
mod tests;
