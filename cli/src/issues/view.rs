//! `bullpen issue N`: one GitHub issue, printed for reading. Its text (title, labels,
//! body, comments) is inside a fence that says whose it is, clipped; what's outside
//! the fence is bullpen's: the number, the repository, the item on the backlog.

use super::{repo, store, text};
use crate::args::{pos, Arg, Args, Nargs};
use bullpen_core::backlog;
use bullpen_core::json::{s, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::{fail, Repo, Res};
use serde_json::Value;

const ISSUE: &[Arg] = &[pos("number", Nargs::One, "the issue's number, like 12 or #12")];

const FIELDS: &str = "number,title,author,labels,body,comments,state,url";

/// `issue`: print a GitHub issue for reading, its text fenced.
pub fn cmd_issue(argv: &[String]) -> Res<i32> {
    let a = Args::parse("issue", argv, ISSUE);
    let repo = Repo::new(repo_root()?)?;
    let word = a.arg("number").unwrap_or("");
    let digits = word.strip_prefix('#').unwrap_or(word);
    let Some(n) = digits
        .parse::<u32>()
        .ok()
        .filter(|_| digits.bytes().all(|c| c.is_ascii_digit()))
    else {
        return fail(format!("{word} isn't an issue's number: `bullpen issue 12`"));
    };
    if !super::on(&repo) {
        return Ok(0);
    }
    let slug = repo::issues_repo(&repo)?;
    let args = ["issue", "view", &n.to_string(), "--json", FIELDS];
    let got = repo::gh_json(&repo, &slug, &args, &format!("show {slug}'s issue {n}"))?;
    let Some(issue) = got.as_object() else {
        return fail(format!(
            "gh couldn't show {slug}'s issue {n}: what it printed isn't an issue"
        ));
    };
    println!("{}", head(&repo, &slug, n, issue)?);
    println!("{}", text::fenced(issue));
    Ok(0)
}

/// bullpen's own lines about the issue: where it is, whether it's open, and its item on
/// the backlog with what can be done about it.
fn head(repo: &Repo, slug: &str, n: u32, issue: &Obj) -> Res<String> {
    let state = match s(issue, "state").to_ascii_lowercase().as_str() {
        "open" => "open",
        "closed" => "closed",
        _ => "state unknown",
    };
    let mut out = vec![format!("#{n} in {slug} ({state})")];
    let seen = store::load(repo)?;
    let b = backlog::load(repo)?;
    let item = seen.get(&n.to_string()).and_then(|r| b.item(store::item_of(r)).ok());
    out.push(match item {
        None => "  not on the backlog: `bullpen issues --now` reads the open issues".into(),
        Some(i) => on_backlog(i),
    });
    Ok(out.join("\n"))
}

fn on_backlog(item: &Obj) -> String {
    let id = s(item, "id");
    match s(item, "state") {
        "asks" => format!(
            "  {id} on the backlog, asking: `bullpen later {id} --keep` says yes, `bullpen later {id} --drop \"why\"` no"
        ),
        "kept" => format!("  {id} on the backlog: the person said yes"),
        "started" => format!(
            "  {id}, which became {}",
            item.get("task").and_then(Value::as_str).unwrap_or("a task")
        ),
        "dropped" => format!("  {id}: the person said no"),
        state => format!("  {id}, {state}"),
    }
}
