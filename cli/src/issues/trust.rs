//! Whether an issue is trusted (docs/issues.md, "Who is trusted"): bullpen decides it,
//! never the coordinator, and it alone lets an issue start by itself. Its author is the
//! gh account's own login (`gh api user`, cached a day in `gh-login.json`), or is named
//! in `issues_trusted`; or the issue has the label `issues_label` and that login added
//! it (the last `labeled` event's actor: in a repository with collaborators anyone can
//! add a label).

use super::repo::{gh_api_pages, gh_login};
use bullpen_core::json::{self, obj, s, Obj};
use bullpen_core::timeutil::{iso_now, seconds_since};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// How long the gh account's login is kept before gh is asked again.
const LOGIN_KEPT: f64 = 24.0 * 60.0 * 60.0;

/// Whether an issue is trusted, and why, in words: "ann is the gh account's own login".
#[derive(Debug, Clone, PartialEq)]
pub struct Trust {
    pub yes: bool,
    pub why: String,
}

/// The gh account's login: from `gh-login.json` if asked within a day, else from gh.
pub fn login(repo: &Repo) -> Res<String> {
    let file = repo.dir.join("gh-login.json");
    let kept = json::read_obj(&file).ok().flatten().unwrap_or_default();
    let (l, at) = (s(&kept, "login"), s(&kept, "at"));
    if !l.is_empty() && !at.is_empty() && seconds_since(at, None) < LOGIN_KEPT {
        return Ok(l.to_string());
    }
    let l = gh_login(repo)?;
    std::fs::create_dir_all(&repo.dir)?;
    let mut o = Obj::new();
    o.insert("login".into(), json!(l));
    o.insert("at".into(), json!(iso_now()));
    json::write_obj(&file, &o)?;
    Ok(l)
}

/// The logins `issues_trusted` names, separated by commas or spaces.
pub fn named(repo: &Repo) -> Vec<String> {
    let all = repo.cfg_str("issues_trusted");
    all.split([',', ' '])
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Is the issue (as `gh issue view` gives it, with its number, author and labels) trusted?
pub fn trusted(repo: &Repo, slug: &str, issue: &Obj) -> Res<Trust> {
    let author = s(obj(issue, "author"), "login").to_string();
    let shown = super::text::author(issue);
    let me = login(repo)?;
    let says = |yes: bool, why: String| Ok(Trust { yes, why });
    if !author.is_empty() && same(&author, &me) {
        return says(true, format!("{shown} is the gh account's own login"));
    }
    if !author.is_empty() && named(repo).iter().any(|l| same(l, &author)) {
        return says(true, format!("{shown} is in issues_trusted"));
    }
    let label = repo.cfg_str("issues_label").to_string();
    let labels = issue.get("labels").and_then(Value::as_array).into_iter().flatten();
    let has_label = !label.is_empty() && labels.filter_map(Value::as_object).any(|l| same(s(l, "name"), &label));
    if !has_label {
        return says(false, format!("{shown} isn't trusted"));
    }
    let n = issue.get("number").and_then(Value::as_i64).unwrap_or(0);
    let what = format!("list who labeled {slug}'s issue {n}");
    let pages = gh_api_pages(repo, slug, &format!("issues/{n}/events?per_page=100"), &what)?;
    let by = labeled_by(&pages, &label);
    let label = super::text::line(&label, 50);
    match by {
        Some(by) if same(&by, &me) => says(true, format!("the gh account's own login added the label {label}")),
        Some(by) => says(
            false,
            format!(
                "{shown} isn't trusted, and the label {label} was added by {}, not the gh account's login",
                super::text::line(&by, 40)
            ),
        ),
        None => says(
            false,
            format!("{shown} isn't trusted, and who added the label {label} can't be told"),
        ),
    }
}

/// Who added the label last, from the issue's events (pages of them, in order).
pub fn labeled_by(pages: &[Value], label: &str) -> Option<String> {
    let events = pages
        .iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(Value::as_object);
    let added = events.filter(|e| s(e, "event") == "labeled" && same(s(obj(e, "label"), "name"), label));
    added
        .last()
        .map(|e| s(obj(e, "actor"), "login").to_string())
        .filter(|l| !l.is_empty())
}

#[cfg(test)]
#[path = "tests/test_trust.rs"]
mod tests;
