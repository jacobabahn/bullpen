//! `bullpen issue N`: one GitHub issue, printed for reading. Its text (title, labels,
//! body, comments) is inside a fence that says whose it is, clipped; what's outside
//! the fence is bullpen's: the number, the repository, whether it's trusted, the item
//! on the backlog and the coordinator's judgement. With --score or --brief-file it
//! records a judgement instead (judge.rs).

use super::{judge, repo, store, text, trust};
use crate::args::{pos, value, Arg, Args, Nargs};
use bullpen_core::backlog;
use bullpen_core::json::{obj, s, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::{fail, Repo, Res};
use serde_json::Value;

const ISSUE: &[Arg] = &[
    pos("number", Nargs::One, "the issue's number, like 12 or #12"),
    value(
        "--score",
        "C,S,R,W",
        "judge it: its clarity (clear, vague), size (small, medium, large), risk (low, high) and whether it's worth \
         doing (yes, unsure, no)",
    ),
    value(
        "--why",
        "LINE",
        "with --score: one line on what it is and why you scored it so",
    ),
    value("--area", "PATHS", "with --score: the files or folders it touches"),
    value(
        "--brief-file",
        "FILE",
        "its brief, in your own words, read from a file: the issue is quoted at its end",
    ),
];

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
    if a.opt("--score").is_some() || ["--why", "--area", "--brief-file"].iter().any(|f| a.opt(f).is_some()) {
        return judge::judge(&repo, n, &a);
    }
    let slug = repo::issues_repo(&repo)?;
    let issue = fetch(&repo, &slug, n)?;
    println!("{}", head(&repo, &slug, n, &issue)?);
    println!("{}", text::fenced(&issue));
    Ok(0)
}

/// The issue numbered n, as gh views it.
pub fn fetch(repo: &Repo, slug: &str, n: u32) -> Res<Obj> {
    let args = ["issue", "view", &n.to_string(), "--json", FIELDS];
    let got = repo::gh_json(repo, slug, &args, &format!("show {slug}'s issue {n}"))?;
    match got {
        Value::Object(o) => Ok(o),
        _ => fail(format!(
            "gh couldn't show {slug}'s issue {n}: what it printed isn't an issue"
        )),
    }
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
    out.push(match trust::trusted(repo, slug, issue) {
        Ok(t) if t.yes => format!("  trusted: {}", t.why),
        Ok(t) => format!("  not trusted: {}, so it never starts by itself", t.why),
        Err(e) => format!("  whether it's trusted can't be told: {}", e.0),
    });
    let seen = store::load(repo)?;
    let b = backlog::load(repo)?;
    let item = seen.get(&n.to_string()).and_then(|r| b.item(store::item_of(r)).ok());
    out.push(match item {
        None => "  not on the backlog: `bullpen issues --now` reads the open issues".into(),
        Some(i) => on_backlog(i),
    });
    if let Some(j) = item.map(|i| obj(i, "judged")).filter(|j| !j.is_empty()) {
        let area = match s(j, "area") {
            "" => String::new(),
            a => format!(" · {a}"),
        };
        out.push(format!(
            "  judged: {}{area} · {}",
            super::judge::score_of(j),
            s(j, "why")
        ));
    }
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
