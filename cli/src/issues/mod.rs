//! GitHub issues, read into the backlog (docs/issues.md). `bullpen issues --now` asks gh
//! for the repository's open issues once and compares them with `issues.json` (`sync`): one not
//! seen before becomes a backlog item that asks for a yes, an undecided one that was
//! edited is read again, a closed one's item is done. `bullpen issues` lists what's
//! recorded, and `bullpen issue N` (view.rs) prints one for reading. The coordinator
//! judges each (`issue N --score`, judge.rs): a trusted (trust.rs), clear, small,
//! low-risk one goes on the list by itself, and the rest ask the person. Nothing is
//! written to GitHub. A keep or drop of an issue's item is remembered (`decided`), so it
//! is never asked again.
//!
//! The watcher reads them too, on a timer, and tells the coordinators of new ones in
//! one news line (timer.rs).
//!
//! An issue's text is whoever filed it's: only its title, clipped and without control
//! characters (text.rs), its number and its author reach the backlog and the news.

pub mod judge;
pub mod repo;
pub mod store;
pub mod text;
pub mod timer;
pub mod trust;
pub mod view;

use crate::args::{flag, Arg, Args};
use bullpen_core::backlog::{self, is_open, Backlog};
use bullpen_core::json::{s, truthy, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::timeutil::iso_now;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

/// What gh is asked for, about each open issue.
const FIELDS: &str = "number,title,author,labels,updatedAt,url";
/// The most issues one read takes. With this many back, there may be more: an issue
/// that isn't among them can't be taken for closed.
const LIMIT: usize = 100;

const ISSUES: &[Arg] = &[flag(
    "--now",
    "read the repository's open issues from GitHub now: new ones go on the backlog, asking for a yes",
)];

/// Are issues on for the repo? Says so when not: every issues command does nothing then.
pub fn on(repo: &Repo) -> bool {
    let on = truthy(repo.cfg.get("issues"));
    if !on {
        println!("issues are off: `bullpen config --issues on` turns them on");
    }
    on
}

/// `issues`: what's recorded of the repo's GitHub issues; --now reads them first.
pub fn cmd_issues(argv: &[String]) -> Res<i32> {
    let a = Args::parse("issues", argv, ISSUES);
    let repo = Repo::new(repo_root()?)?;
    if !a.flag("--now") {
        return listed(&repo);
    }
    if !on(&repo) {
        return Ok(0);
    }
    let Read { slug, open: n, changes } = read(&repo)?;
    for c in &changes {
        println!("{}", c.line);
    }
    let count = format!("{n} open issue{} in {slug}", if n == 1 { "" } else { "s" });
    match changes.iter().find_map(|c| c.asks.as_deref()) {
        _ if changes.is_empty() => println!("nothing new: {count}"),
        Some(id) => println!(
            "{count}. `bullpen issue N` shows one; `bullpen later {id} --keep` says yes to its item, `--drop \"why\"` no"
        ),
        None => println!("{count}. `bullpen issue N` shows one"),
    }
    Ok(0)
}

/// One thing a read changed: the line `issues --now` prints for it, and, for an issue
/// not seen before, its number and what the news calls it (`#12 Title (author)`).
pub struct Change {
    pub line: String,
    pub new: Option<(i64, String)>,
    /// the item, when it asks for a yes now (new, edited or open again)
    pub asks: Option<String>,
}

/// What a read found: the repository, how many issues are open there, what changed.
pub struct Read {
    pub slug: String,
    pub open: usize,
    pub changes: Vec<Change>,
}

fn changed(line: String, asks: Option<&str>) -> Option<Change> {
    Some(Change {
        line,
        new: None,
        asks: asks.map(str::to_string),
    })
}

/// One read (`issues --now`, and the watcher's timer): ask gh for the repository's open
/// issues and bring the records and the backlog up to date with them.
pub fn read(repo: &Repo) -> Res<Read> {
    let slug = repo::issues_repo(repo)?;
    let limit = LIMIT.to_string();
    let args = ["issue", "list", "--state", "open", "--json", FIELDS, "--limit", &limit];
    let got = repo::gh_json(repo, &slug, &args, &format!("list {slug}'s issues"))?;
    let Some(open) = got.as_array() else {
        return fail(format!(
            "gh couldn't list {slug}'s issues: what it printed isn't a list"
        ));
    };
    let open: Vec<&Obj> = open.iter().filter_map(Value::as_object).collect();
    let changes = sync(repo, &open)?;
    Ok(Read {
        slug,
        open: open.len(),
        changes,
    })
}

/// Compare the open issues with the records, and change the backlog to match: each
/// thing that changed.
fn sync(repo: &Repo, open: &[&Obj]) -> Res<Vec<Change>> {
    store::change(repo, |seen| {
        backlog::change(repo, |b| {
            let mut said = Vec::new();
            for issue in open {
                if let Some(line) = read_one(seen, b, issue) {
                    said.push(line);
                }
            }
            if open.len() < LIMIT {
                said.extend(close_gone(seen, b, open));
            }
            Ok(said)
        })
    })
}

fn number(issue: &Obj) -> Option<i64> {
    issue.get("number").and_then(Value::as_i64)
}

/// One open issue: new (an item that asks, at the bottom), decided (left alone), or
/// undecided and edited or reopened (read again; its item stays where it is).
fn read_one(seen: &mut Obj, b: &mut Backlog, issue: &Obj) -> Option<Change> {
    let n = number(issue)?;
    let key = n.to_string();
    let (title, updated) = (text::item_title(issue), s(issue, "updatedAt"));
    let Some(rec) = seen.get_mut(&key).and_then(Value::as_object_mut) else {
        let id = b.add(&title, "issue", "asks", false);
        let about = json!({"number": n, "url": text::line(s(issue, "url"), 200),
            "author": text::author(issue), "updated": updated});
        b.item_mut(&id).ok()?.insert("issue".into(), about);
        seen.insert(key, store::record(&id, updated));
        return Some(Change {
            line: format!("new: {id}  {title}"),
            new: Some((n, text::told(issue))),
            asks: Some(id),
        });
    };
    let id = s(rec, "item").to_string();
    let item = b.item_mut(&id).ok()?;
    let reopened = rec.shift_remove("closed").is_some();
    if store::settle(rec, item) {
        return None;
    }
    if reopened && s(item, "state") == "done" {
        item.insert("state".into(), json!("asks"));
    }
    let edited = s(rec, "updated") != updated;
    if edited {
        rec.insert("updated".into(), json!(updated));
        item.insert("title".into(), json!(title));
        if let Some(about) = item.get_mut("issue").and_then(Value::as_object_mut) {
            about.insert("updated".into(), json!(updated));
        }
    }
    let asks = (s(item, "state") == "asks").then_some(id.as_str());
    match (reopened, edited) {
        (true, _) => changed(format!("open again: {id}  {title}"), asks),
        (_, true) => changed(format!("edited: {id}  {title}"), asks),
        _ => None,
    }
}

/// The recorded issues that aren't open any more: each is marked closed, and its item,
/// if still on the list, is done.
fn close_gone(seen: &mut Obj, b: &mut Backlog, open: &[&Obj]) -> Vec<Change> {
    let still: Vec<String> = open.iter().filter_map(|i| number(i)).map(|n| n.to_string()).collect();
    let mut said = Vec::new();
    for (key, rec) in seen.iter_mut().filter(|(k, _)| !still.contains(k)) {
        let Some(rec) = rec.as_object_mut().filter(|r| !r.contains_key("closed")) else {
            continue;
        };
        rec.insert("closed".into(), json!(iso_now()));
        let Ok(item) = b.item_mut(s(rec, "item")) else { continue };
        store::settle(rec, item);
        if is_open(item) {
            item.insert("state".into(), json!("done"));
            let line = format!(
                "closed: {}  {} (#{key} is closed on GitHub)",
                s(item, "id"),
                s(item, "title")
            );
            said.extend(changed(line, None));
        }
    }
    said
}

/// `later` kept, dropped or started item id: if it is an issue's, its record says so
/// from now on, and the issue is never asked about again.
pub fn decided(repo: &Repo, id: &str) -> Res<()> {
    let b = backlog::load(repo)?;
    let Some(item) = b.item(id).ok().filter(|i| s(i, "by") == "issue") else {
        return Ok(());
    };
    store::change(repo, |seen| {
        let rec = seen.values_mut().find(|r| store::item_of(r) == id);
        if let Some(rec) = rec.and_then(Value::as_object_mut) {
            store::settle(rec, item);
        }
        Ok(())
    })
}

/// An issue's item that a judgement took back off the list: its record is undecided
/// again, so the issue asks the person.
pub fn undecided(repo: &Repo, id: &str) -> Res<()> {
    store::change(repo, |seen| {
        let rec = seen.values_mut().find(|r| store::item_of(r) == id);
        if let Some(rec) = rec.and_then(Value::as_object_mut) {
            rec.insert("decided".into(), Value::Null);
            rec.insert("at".into(), json!(iso_now()));
        }
        Ok(())
    })
}

/// `issues` alone: every issue recorded, by number, with its item and what was decided.
fn listed(repo: &Repo) -> Res<i32> {
    let seen = store::load(repo)?;
    let b = backlog::load(repo)?;
    let mut rows: Vec<(i64, String, String, String)> = Vec::new();
    for (key, rec) in &seen {
        let Some(rec) = rec.as_object() else { continue };
        let id = s(rec, "item");
        let title = b.item(id).map(|i| s(i, "title").to_string()).unwrap_or_default();
        rows.push((key.parse().unwrap_or(0), id.to_string(), said(rec), title));
    }
    rows.sort();
    let (nw, iw, sw) = (
        rows.iter().map(|r| r.0.to_string().len()).max().unwrap_or(0) + 1,
        rows.iter().map(|r| r.1.len()).max().unwrap_or(0),
        rows.iter().map(|r| r.2.len()).max().unwrap_or(0),
    );
    for (n, id, said, title) in &rows {
        println!("{:<nw$}  {id:<iw$}  {said:<sw$}  {title}", format!("#{n}"));
    }
    if !truthy(repo.cfg.get("issues")) {
        on(repo);
    } else if rows.is_empty() {
        println!("no issues read yet: `bullpen issues --now` reads the open ones");
    }
    Ok(0)
}

/// What a record says of its issue: "asks", "yes", "no", and "closed".
fn said(rec: &Obj) -> String {
    let decided = match s(rec, "decided") {
        "" => "asks",
        d => d,
    };
    match (rec.contains_key("closed"), decided) {
        (true, "asks") => "closed".into(),
        (true, d) => format!("{d}, closed"),
        (false, d) => d.into(),
    }
}
