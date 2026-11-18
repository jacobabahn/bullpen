//! `bullpen later`: the backlog, where ideas go before they're tasks (docs/backlog.md).
//! The list itself is core's (bullpen_core::backlog); here are its command (adding,
//! keep, drop, skip and the moves), who is
//! calling (the person, a coordinator, or a worker, whose items ask for a yes), and
//! the line `show` says about a task that came from it. The list's lines are in
//! list.rs, making an item a task in start.rs.

pub mod list;
pub mod pickup;
pub mod start;

use crate::args::{flag, pos, value, Arg, Args, Nargs};
use crate::coordinators::calling_coordinator;
use crate::news::kind::Kind;
use bullpen_core::backlog::{self, is_item_id, is_open, Move};
use bullpen_core::json::{s, Obj};
use bullpen_core::repo::{lookup_block, repo_root};
use bullpen_core::text::{rstrip, strip};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

const LATER: &[Arg] = &[
    pos(
        "what",
        Nargs::Maybe,
        "an idea's title, to add it; or an item, like L7, to change it; none: the list",
    ),
    pos("why", Nargs::Maybe, "with --drop: why not"),
    flag("--top", "add it at the top, or move the item there"),
    flag("--up", "move the item up one"),
    flag("--down", "move the item down one"),
    flag("--bottom", "move the item to the bottom"),
    value(
        "--brief-file",
        "FILE",
        "its brief, read from a file: what a worker would need to do it",
    ),
    flag("--keep", "yes: an item that asks goes on the list"),
    flag("--drop", "no: the item leaves the list"),
    value("--skip", "WHY", "pass over the item this time, and why"),
    flag(
        "--start",
        "make the item a task now and start its worker (it needs a brief)",
    ),
    value(
        "--from",
        "ID",
        "the task whose worker adds it (filled in from a worker's pane): it asks for a yes",
    ),
    flag("--json", "the list as JSON"),
    flag("--all", "dropped items too, and started ones whose task closed"),
];

/// Who runs the command.
pub enum Caller {
    Person,
    /// a coordinator, by ID
    Coordinator(String),
    /// a task's worker (or reviewer), by its task
    Worker(Obj),
}

impl Caller {
    /// The item's `by`: person, the coordinator's ID, or the task's.
    fn by(&self) -> String {
        match self {
            Caller::Person => "person".into(),
            Caller::Coordinator(c) => c.clone(),
            Caller::Worker(t) => s(t, "id").into(),
        }
    }
}

/// Who is calling: the task --from names, else the pane this runs in (a worker's or
/// reviewer's, a coordinator's), else the person.
pub fn caller(repo: &Repo, from: Option<&str>) -> Res<Caller> {
    if let Some(f) = from.filter(|f| !f.is_empty()) {
        return Ok(Caller::Worker(repo.task(f)?));
    }
    let block = std::env::var("REX_BLOCK").unwrap_or_default();
    if let Some((owner, role)) = lookup_block(Some(&block))? {
        let tid = role.split(' ').next().unwrap_or("");
        if owner.root == repo.root && tid.contains('-') {
            if let Ok(t) = repo.task(tid) {
                return Ok(Caller::Worker(t));
            }
        }
    }
    Ok(match calling_coordinator(repo) {
        Some(c) => Caller::Coordinator(s(c, "id").into()),
        None => Caller::Person,
    })
}

/// `later`: add an idea to the backlog, list it, or change an item.
pub fn cmd_later(argv: &[String]) -> Res<i32> {
    let a = Args::parse("later", argv, LATER);
    let repo = Repo::new(repo_root()?)?;
    let what = a.arg("what").unwrap_or("");
    if what.is_empty() {
        if let Some(f) = item_flag(&a) {
            return fail(format!("{f} needs an item: `bullpen later L7 {f}`"));
        }
        return list::print(&repo, a.flag("--all"), a.flag("--json"));
    }
    let who = caller(&repo, a.opt("--from"))?;
    if is_item_id(what) {
        let id = backlog::norm_id(what).unwrap_or_default();
        return change(&repo, &id, &a, &who);
    }
    add(&repo, what, &a, &who)
}

/// The first flag given that is about one item, if any: with no item named it can't be
/// done, and listing instead would hide the mistake.
fn item_flag(a: &Args) -> Option<&'static str> {
    let flags = ["--top", "--up", "--down", "--bottom", "--keep", "--drop", "--start"];
    if let Some(f) = flags.into_iter().find(|f| a.flag(f)) {
        return Some(f);
    }
    ["--skip", "--brief-file"].into_iter().find(|f| a.opt(f).is_some())
}

/// Add an item: kept for the person and a coordinator, asking for a yes for a worker,
/// whose coordinator hears of it.
fn add(repo: &Repo, title: &str, a: &Args, who: &Caller) -> Res<i32> {
    let title = strip(title).to_string();
    let doing = ["--up", "--down", "--bottom", "--keep", "--drop", "--start"];
    if let Some(f) = doing.iter().find(|f| a.flag(f)) {
        return fail(format!("{f} is for an item already on the list, like `later L7 {f}`"));
    }
    if a.opt("--skip").is_some() || a.arg("why").is_some() {
        return fail("a new item's title is one argument: put it in quotes");
    }
    let brief = brief_text(a)?;
    let state = if matches!(who, Caller::Worker(_)) {
        "asks"
    } else {
        "kept"
    };
    let id = backlog::change(repo, |b| {
        let id = b.add(&title, &who.by(), state, a.flag("--top"));
        if let Some(text) = &brief {
            let name = write_brief(repo, &id, text)?;
            b.item_mut(&id)?.insert("brief".into(), json!(name));
        }
        Ok(id)
    })?;
    println!("{id}");
    if let Caller::Worker(t) = who {
        let text = format!(
            "{} added {id} to the backlog: {title} (`bullpen later {id} --keep` puts it on the list, `--drop` says no)",
            s(t, "id")
        );
        crate::news::say(repo, t, Kind::Later, &text)?;
    }
    pickup::soon(repo);
    Ok(0)
}

/// --brief-file's text, if given.
fn brief_text(a: &Args) -> Res<Option<String>> {
    match a.opt("--brief-file").filter(|f| !f.is_empty()) {
        Some(f) => Ok(Some(brief_text_of(f)?)),
        None => Ok(None),
    }
}

/// A brief file's text, which mustn't be empty.
pub fn brief_text_of(f: &str) -> Res<String> {
    let text = crate::start::read_text(f)?;
    if strip(&text).is_empty() {
        return fail(format!("{f} is empty: a brief says what a worker would do"));
    }
    Ok(text)
}

/// Write an item's brief to briefs/<ID>.md; its file name there (the item's `brief`).
pub fn write_brief(repo: &Repo, id: &str, text: &str) -> Res<String> {
    let briefs = repo.dir.join("briefs");
    std::fs::create_dir_all(&briefs)?;
    let name = format!("{id}.md");
    std::fs::write(briefs.join(&name), format!("{}\n", rstrip(text)))?;
    Ok(name)
}

/// What to do with an item, from the flags: one of them, and maybe a brief.
fn change(repo: &Repo, id: &str, a: &Args, who: &Caller) -> Res<i32> {
    let viewing = item_flag(a).is_none() && a.arg("why").is_none();
    if let (Caller::Worker(t), false) = (who, viewing) {
        return fail(format!(
            "{}'s worker only adds to the backlog: the coordinator or the person decides on its items",
            s(t, "id")
        ));
    }
    let moves = [
        ("--top", Move::Top),
        ("--up", Move::Up),
        ("--down", Move::Down),
        ("--bottom", Move::Bottom),
    ];
    let mut acts: Vec<&str> = ["--top", "--up", "--down", "--bottom", "--keep", "--drop", "--start"]
        .into_iter()
        .filter(|f| a.flag(f))
        .collect();
    if a.opt("--skip").is_some() {
        acts.push("--skip");
    }
    if acts.len() > 1 {
        return fail(format!("one of {} at a time", acts.join(", ")));
    }
    if a.arg("why").is_some() && acts != ["--drop"] {
        return fail("a second argument is only for --drop: why not");
    }
    let brief = match brief_text(a)? {
        // an issue's item: the issue quoted at the brief's end
        Some(text) => Some(crate::issues::judge::brief_for(
            repo,
            backlog::load(repo)?.item(id)?,
            &text,
        )?),
        None => None,
    };
    let act = acts.first().copied();
    if act == Some("--start") {
        return start::start(repo, id, brief.as_deref(), who);
    }
    let said = backlog::change(repo, |b| {
        let mut said = Vec::new();
        if let Some(text) = &brief {
            let name = write_brief(repo, id, text)?;
            b.item_mut(id)?.insert("brief".into(), json!(name));
            said.push(format!(
                "{id} has a brief: {}",
                repo.dir.join("briefs").join(&name).display()
            ));
        }
        let to = moves.iter().find(|(m, _)| Some(*m) == act).map(|(_, to)| *to);
        match act {
            _ if let Some(to) = to => said.push(move_to(b, id, to)?),
            Some("--keep") => said.push(keep(b.item_mut(id)?)?),
            Some("--drop") => said.push(drop(b.item_mut(id)?, a.arg("why"))?),
            Some("--skip") => said.push(skip(b.item_mut(id)?, a.opt("--skip").unwrap_or(""))?),
            _ if said.is_empty() => said.push(list::detail(repo, b.item(id)?)?),
            _ => {}
        }
        Ok(said)
    })?;
    println!("{}", said.join("\n"));
    if matches!(act, Some("--keep" | "--drop")) {
        crate::issues::decided(repo, id)?; // an issue's item: the yes or no is remembered
    }
    if act == Some("--drop") {
        crate::issues::write::declined(repo, backlog::load(repo)?.item(id)?, a.arg("why"))?;
    }
    if act == Some("--keep") && matches!(who, Caller::Person) {
        crate::issues::judge::yes_needs_brief(repo, backlog::load(repo)?.item(id)?)?;
    }
    if !viewing {
        pickup::soon(repo); // a brief, a yes, a skip or a move can make the next item another
    }
    Ok(0)
}

/// --top, --up, --down, --bottom: where the item is now.
fn move_to(b: &mut backlog::Backlog, id: &str, to: Move) -> Res<String> {
    b.move_item(id, to)?;
    let k = b.open().position(|i| s(i, "id") == id).unwrap_or(0);
    Ok(format!("{id} is {} of {} on the list", k + 1, b.open().count()))
}

/// --keep: an item that asks (or was dropped) goes on the list.
fn keep(item: &mut Obj) -> Res<String> {
    let id = s(item, "id").to_string();
    match s(item, "state") {
        "kept" => Ok(format!("{id} is on the list already")),
        "asks" | "dropped" | "done" => {
            item.insert("state".into(), json!("kept"));
            let n = bullpen_core::json::obj(item, "issue").get("number").cloned();
            let needs = match n {
                Some(n) if s(item, "by") == "issue" && !bullpen_core::json::truthy(item.get("brief")) => {
                    format!(". It needs a brief before it can start: `bullpen issue {n} --brief-file FILE`")
                }
                _ => String::new(),
            };
            Ok(format!("{id} is on the list: {}{needs}", s(item, "title")))
        }
        _ => fail(not_open(item)),
    }
}

/// --drop: the item leaves the list, with why if given.
fn drop(item: &mut Obj, why: Option<&str>) -> Res<String> {
    if !is_open(item) {
        return fail(not_open(item));
    }
    item.insert("state".into(), json!("dropped"));
    if let Some(w) = why.map(strip).filter(|w| !w.is_empty()) {
        item.insert("note".into(), json!(w));
    }
    Ok(format!("{} dropped: {}", s(item, "id"), s(item, "title")))
}

/// --skip: pass over the item this time; the reason shows on it.
fn skip(item: &mut Obj, why: &str) -> Res<String> {
    if !is_open(item) {
        return fail(not_open(item));
    }
    let why = strip(why);
    if why.is_empty() {
        return fail("say why it's skipped: `--skip \"overlaps BP-3\"`");
    }
    item.insert("skipped".into(), json!(why));
    Ok(format!("{} skipped: {why}", s(item, "id")))
}

/// Why a started (or dropped) item can't be changed.
pub fn not_open(item: &Obj) -> String {
    let id = s(item, "id");
    match s(item, "state") {
        "started" => format!(
            "{id} is a task already ({}): see to it there",
            item.get("task").map_or("None".into(), bullpen_core::json::py_str)
        ),
        "dropped" => format!("{id} was dropped: `bullpen later {id} --keep` puts it back first"),
        state => format!("{id} is {state}, not on the list"),
    }
}

/// For `show`: where a task came from, if the backlog: "     from: the backlog (L7), started by c3".
pub fn from_line(t: &Obj) -> Option<String> {
    let from = t.get("from").and_then(Value::as_str).filter(|f| !f.is_empty())?;
    let whence = match s(t, "origin") {
        "" | "backlog" => format!("the backlog ({from})"),
        origin => format!("{from} ({origin})"),
    };
    let by = match s(t, "started_by") {
        "" => String::new(),
        "person" => ", started by you".into(),
        "itself" => ", started by itself".into(),
        c => format!(", started by {c}"),
    };
    Some(format!("     from: {whence}{by}"))
}

#[cfg(test)]
#[path = "tests/test_backlog.rs"]
mod tests;
