//! Idle pickup (docs/backlog.md): with `pickup` on, bullpen starts the backlog's next
//! kept item by itself when there's room, under caps. It runs after a save that frees
//! a slot (after the run queue, saves.rs), after `later` changes the list, and from the
//! watcher's timer (`_pickup`, while <state>/pickup exists). One start a run at most.
//!
//! An item with a brief starts as a task `started_by: itself`, kept for the person to
//! accept (unless pickup_auto_accept), with pickup_model if set. One without a brief
//! asks the coordinators for one, once a room; with none running, the next item that
//! has a brief starts instead. An ask nobody answers in BRIEF_WAIT is told to the person,
//! once; the list's order still holds. pickup.json in the repo's state folder holds what
//! a room has asked for (and when), what pickup waits for, whether the person was told
//! and whether pickup is paused, so each is said once.

use crate::coordinators::{sees_to, session_for};
use crate::limit::limit_active;
use crate::news::kind::Kind;
use crate::news::{news, touch, Opts};
use bullpen_core::backlog::{self, Backlog};
use bullpen_core::json::{self, s, truthy, Obj};
use bullpen_core::paths::state;
use bullpen_core::repo::known_repos;
use bullpen_core::task::{clip, is_closed, needs_you};
use bullpen_core::timeutil::{clock, iso_now, parse, seconds_since};
use bullpen_core::{Fail, Repo, Res};
use chrono::{Local, NaiveDate};
use serde_json::{json, Value};
use std::path::PathBuf;

/// What `started_by` says on a task pickup started.
pub const ITSELF: &str = "itself";

/// A self-started task waiting for the person: pickup pauses at pickup_waiting of them.
const WAITS_FOR_YOU: [&str; 4] = ["ready", "finished", "needs", "stopped"];

/// How long pickup waits for a coordinator's brief before it tells the person: long
/// enough to read the item and write one, short of a forgotten afternoon.
const BRIEF_WAIT: f64 = 30.0 * 60.0;

/// <state>/pickup: exists while some repo has pickup on and a kept item, so the
/// watcher's timer only calls `_pickup` then.
pub fn flag() -> PathBuf {
    state().join("pickup")
}

fn state_path(repo: &Repo) -> PathBuf {
    repo.dir.join("pickup.json")
}

fn on(repo: &Repo) -> bool {
    truthy(repo.cfg.get("pickup"))
}

fn num(repo: &Repo, key: &str) -> i64 {
    let d = bullpen_core::settings::setting(key).map(|s| s.default_value());
    repo.cfg
        .get(key)
        .and_then(Value::as_i64)
        .or_else(|| d.and_then(|d| d.as_i64()))
        .unwrap_or(0)
}

/// The kept items pickup may start, in list order.
fn kept(b: &Backlog) -> Vec<Obj> {
    b.open().filter(|i| s(i, "state") == "kept").cloned().collect()
}

/// The tasks pickup started that aren't closed.
fn self_started(tasks: &[Obj]) -> Vec<&Obj> {
    tasks
        .iter()
        .filter(|t| s(t, "started_by") == ITSELF && !is_closed(s(t, "status")))
        .collect()
}

/// How many tasks pickup made on day (local), closed ones too: the budget counts them all.
pub fn started_on(tasks: &[Obj], day: NaiveDate) -> i64 {
    let made = |t: &&Obj| parse(s(t, "created")).is_some_and(|c| c.with_timezone(&Local).date_naive() == day);
    tasks
        .iter()
        .filter(|t| s(t, "started_by") == ITSELF)
        .filter(made)
        .count() as i64
}

/// What holds pickup back now, if anything.
#[derive(Debug, PartialEq)]
pub enum Held {
    /// a task needs the person (needs or stopped): new work waits for their answer
    Needed,
    /// pickup_waiting self-started tasks wait for the person (their IDs)
    Paused(Vec<String>),
    /// no room: max_running tasks work, or pickup_running self-started ones work or are in review
    NoRoom,
    /// pickup_per_day tasks were started today
    Spent,
}

/// The caps, from the tasks as they are: what holds pickup back, or None (it may start one).
pub fn held(tasks: &[Obj], caps: [i64; 4], today: NaiveDate) -> Option<Held> {
    let [max_running, running, waiting, per_day] = caps;
    if tasks.iter().any(|t| needs_you(s(t, "status"))) {
        return Some(Held::Needed);
    }
    if let Some(ids) = pile(tasks, waiting) {
        return Some(Held::Paused(ids));
    }
    let mine = self_started(tasks);
    let working = tasks.iter().filter(|t| s(t, "status") == "working").count() as i64;
    let busy = mine
        .iter()
        .filter(|t| matches!(s(t, "status"), "working" | "review"))
        .count() as i64;
    if working >= max_running || busy >= running {
        return Some(Held::NoRoom);
    }
    if started_on(tasks, today) >= per_day {
        return Some(Held::Spent);
    }
    None
}

/// The self-started tasks that wait for the person, if there are pickup_waiting of them:
/// whatever else holds pickup back, the pause lasts until this pile is gone.
fn pile(tasks: &[Obj], waiting: i64) -> Option<Vec<String>> {
    let waits: Vec<String> = self_started(tasks)
        .iter()
        .filter(|t| WAITS_FOR_YOU.contains(&s(t, "status")))
        .map(|t| s(t, "id").to_string())
        .collect();
    (waits.len() as i64 >= waiting).then_some(waits)
}

fn caps(repo: &Repo) -> [i64; 4] {
    ["max_running", "pickup_running", "pickup_waiting", "pickup_per_day"].map(|k| num(repo, k))
}

/// "BP-1", "BP-1 and BP-2", "BP-1, BP-2 and BP-3".
fn and_list(ids: &[String]) -> String {
    match ids {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Run f on pickup.json, under its lock, and write it back. Two pickups at once would
/// both see room: while one runs (a worktree's setup can take minutes) the other does
/// nothing, as the room it would see is the one being taken.
fn with_state(repo: &Repo, f: impl FnOnce(&mut Obj) -> Res<()>) -> Res<()> {
    std::fs::create_dir_all(&repo.dir)?;
    let Some(_held) = bullpen_core::lock::try_lock(&repo.dir.join("pickup.lock"))? else {
        return Ok(());
    };
    let mut st = json::read_obj(&state_path(repo))?.unwrap_or_default();
    let out = f(&mut st);
    json::write_obj(&state_path(repo), &st)?; // what was said is kept even if a start failed
    out
}

/// The news a pickup line goes out as: about the repo, not one task (the task's ID in
/// the log is the item's or the task's).
fn say(repo: &Repo, about: &Obj, kind: Kind, text: &str) -> Res<()> {
    news(
        repo,
        about,
        kind,
        text,
        Opts {
            everyone: true,
            ..Opts::default()
        },
    )
}

/// Pickup, once, in this repo: start the next item if there's room. Whether pickup still
/// has something to look at here (on, with a kept item): the timer's flag stays then.
pub fn pickup(repo: &Repo) -> Res<bool> {
    if !on(repo) {
        return Ok(false);
    }
    let items = kept(&backlog::load(repo)?);
    if items.is_empty() {
        return Ok(false);
    }
    std::fs::create_dir_all(state())?;
    touch(&flag())?;
    with_state(repo, |st| run(repo, st, &items))?;
    Ok(true)
}

fn run(repo: &Repo, st: &mut Obj, items: &[Obj]) -> Res<()> {
    let tasks = repo.tasks()?;
    let today = Local::now().date_naive();
    let caps = caps(repo);
    let held = held(&tasks, caps, today);
    st.shift_remove("waits"); // set again below while pickup waits for a brief
    match &held {
        Some(Held::Paused(ids)) if !truthy(st.get("paused")) => {
            st.insert("paused".into(), json!(true));
            let text = format!("Pickup paused: {} wait for you", and_list(ids));
            let first = repo.task(&ids[0])?;
            let told = format!(
                "Pickup paused: {} wait for a look. Tell the person: it goes on once they have looked",
                and_list(ids)
            );
            say(repo, &first, Kind::PickupAsk, &told)?;
            crate::notifications::tell_person(repo, &first, &text);
        }
        // Needed is checked first: the pause ends with the pile, not when something else holds it
        _ if pile(&tasks, caps[2]).is_none() => {
            st.shift_remove("paused");
        }
        _ => {}
    }
    if held == Some(Held::Spent) && s(st, "spent") != today.to_string() {
        st.insert("spent".into(), json!(today.to_string()));
        let text = format!(
            "Pickup started its {} for today (pickup_per_day): the backlog waits for tomorrow, nothing to do",
            num(repo, "pickup_per_day")
        );
        say(repo, &item_as_task(&items[0]), Kind::Pickup, &text)?;
    }
    if held == Some(Held::NoRoom) {
        forget_asks(st); // the room was taken: the next one asks again
    }
    if held.is_some() || limit_active(None)?.is_some() {
        return Ok(());
    }
    let session = session_for(repo, None)?;
    if session.is_empty() {
        return Ok(()); // no Rex session to start a worker in: `bullpen up` makes one
    }
    for item in items.iter().filter(|i| !truthy(i.get("skipped"))) {
        let id = s(item, "id").to_string();
        if truthy(item.get("brief")) {
            forget_asks(st);
            clear_skips(repo)?;
            return start(repo, item, &session);
        }
        let about = item_as_task(item);
        if asked_at(st, &id).is_none() {
            note_ask(st, &id);
            let text = format!(
                "Room for one from the backlog: {id} {}. Write its brief and give it with \
                 `bullpen later {id} --brief-file <f>` (pickup then starts it), or \
                 `bullpen later {id} --skip \"why\"`",
                s(item, "title")
            );
            say(repo, &about, Kind::PickupAsk, &text)?;
        }
        if !sees_to(repo, &about)?.is_empty() {
            st.insert("waits".into(), json!(id));
            return unanswered(repo, st, item); // a coordinator will write it: the list's order holds
        }
    }
    Ok(())
}

/// When the room asked for item's brief, if it did.
fn asked_at(st: &Obj, id: &str) -> Option<String> {
    let at = st.get("asked")?.as_object()?.get(id)?.as_str()?;
    Some(at.to_string())
}

/// Note that the room asked for id's brief, now.
fn note_ask(st: &mut Obj, id: &str) {
    let mut asked = st.get("asked").and_then(Value::as_object).cloned().unwrap_or_default();
    asked.insert(id.into(), json!(iso_now()));
    st.insert("asked".into(), Value::Object(asked));
}

/// A room's asks end with it (a start, or the room taken): the next room asks again.
fn forget_asks(st: &mut Obj) {
    st.shift_remove("asked");
    st.shift_remove("told");
}

/// An ask unanswered for BRIEF_WAIT: tell the person, once, that the item waits for a
/// brief. Pickup doesn't go past it: the list's order is the person's, so they (or the
/// coordinator) brief it, skip it or move it.
fn unanswered(repo: &Repo, st: &mut Obj, item: &Obj) -> Res<()> {
    let id = s(item, "id");
    let Some(at) = asked_at(st, id) else { return Ok(()) };
    let mut told = json::strs(st, "told");
    if seconds_since(&at, None) < BRIEF_WAIT || told.iter().any(|t| t == id) {
        return Ok(());
    }
    told.push(id.to_string());
    st.insert("told".into(), json!(told));
    let when = parse(&at).map_or(at.clone(), |t| clock(t, None));
    let text = format!(
        "Pickup waits for a brief for {id} (asked at {when}): give it with \
         `bullpen later {id} --brief-file <f>`, or `bullpen later {id} --skip \"why\"` to pass over it"
    );
    let about = item_as_task(item);
    say(repo, &about, Kind::PickupAsk, &text)?;
    crate::notifications::tell_person(repo, &about, &text);
    Ok(())
}

/// For news about an item: its ID, and its coordinator if one added it.
fn item_as_task(item: &Obj) -> Obj {
    let mut t = Obj::new();
    t.insert("id".into(), json!(s(item, "id")));
    let by = s(item, "by");
    if by.starts_with('c') && by[1..].bytes().all(|b| b.is_ascii_digit()) {
        t.insert("coordinator".into(), json!(by));
    }
    t
}

/// A skip lasts until pickup starts the next item.
fn clear_skips(repo: &Repo) -> Res<()> {
    backlog::change(repo, |b| {
        for i in b.items.iter_mut() {
            i.shift_remove("skipped");
        }
        Ok(())
    })
}

/// The add flags a self-started task gets: kept for the person (unless
/// pickup_auto_accept), and pickup_model.
fn flags(repo: &Repo) -> Vec<String> {
    let mut out = vec![if truthy(repo.cfg.get("pickup_auto_accept")) {
        "--auto-accept".to_string()
    } else {
        "--no-auto-accept".to_string()
    }];
    if let Some(m) = repo
        .cfg
        .get("pickup_model")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
    {
        out.extend(["--model".into(), m.to_string()]);
    }
    out
}

/// Make the item a task and start its worker, with one news line either way.
fn start(repo: &Repo, item: &Obj, session: &str) -> Res<()> {
    let id = s(item, "id");
    let tid = super::start::make_task(repo, id, None, ITSELF, &flags(repo))?;
    let mut t = repo.task(&tid)?;
    let started = crate::start::launch::start_task(repo, &mut t, session, None);
    match started {
        Ok(false) => Ok(()), // its setup failed: that has its own news
        Ok(true) => say(
            repo,
            &t,
            Kind::Started,
            &format!("{tid} started by itself from the backlog ({id}: {})", s(item, "title")),
        ),
        Err(Fail(e)) => say(
            repo,
            &t,
            Kind::PickupAsk,
            &format!(
                "{id} is {tid} now, but it didn't start: {}. `bullpen start {tid}` tries again",
                clip(&e, 200)
            ),
        ),
    }
}

/// The line `later` ends with while pickup is on: what it waits for, if anything.
pub fn status_line(repo: &Repo) -> Res<Option<String>> {
    if !on(repo) {
        return Ok(None);
    }
    let tasks = repo.tasks()?;
    let today = Local::now().date_naive();
    let made = started_on(&tasks, today);
    let per_day = num(repo, "pickup_per_day");
    let waits = waits_for(repo)?;
    let why = match held(&tasks, caps(repo), today) {
        Some(Held::Paused(ids)) => format!("paused: {} wait for you", and_list(&ids)),
        Some(Held::Needed) => "waits: a task needs you".into(),
        Some(Held::NoRoom) => "waits for room".into(),
        Some(Held::Spent) => format!("done for today ({made} of {per_day} started)"),
        None if limit_active(None)?.is_some() => "waits: the usage limit is reached".into(),
        None if let Some(w) = waits => w,
        None => format!("on ({made} of {per_day} started today)"),
    };
    Ok(Some(format!("Pickup {why}")))
}

/// What pickup waits for a brief for, if it does: "waits for a brief for L7 (asked at
/// 3:00 PM)". Only while the item is still the kept one with no brief.
fn waits_for(repo: &Repo) -> Res<Option<String>> {
    let st = json::read_obj(&state_path(repo))?.unwrap_or_default();
    let id = s(&st, "waits");
    let b = backlog::load(repo)?;
    let still = b
        .open()
        .any(|i| s(i, "id") == id && s(i, "state") == "kept" && !truthy(i.get("brief")) && !truthy(i.get("skipped")));
    if !still {
        return Ok(None);
    }
    let when = asked_at(&st, id).and_then(|a| parse(&a)).map(|t| clock(t, None));
    Ok(Some(match when {
        Some(w) => format!("waits for a brief for {id} (asked at {w})"),
        None => format!("waits for a brief for {id}"),
    }))
}

/// Run pickup once the command's own work is done (saves::later).
pub fn soon(repo: &Repo) {
    if !on(repo) {
        return;
    }
    let root = repo.root_str();
    crate::saves::later(&format!("pickup {root}"), move || {
        if let Err(e) = Repo::new(&root).and_then(|r| pickup(&r)) {
            eprintln!("bullpen: {e}");
        }
    });
}

/// `_pickup`: the watcher's timer. Pickup in every repo; the flag goes once none has
/// anything for it.
pub fn cmd_pickup(argv: &[String]) -> Res<i32> {
    crate::args::Args::parse("_pickup", argv, &[]);
    let mut any = false;
    for repo in known_repos()? {
        match pickup(&repo) {
            Ok(wants) => any |= wants,
            Err(e) => eprintln!("bullpen: {}: {e}", repo.name()),
        }
    }
    if !any {
        match std::fs::remove_file(flag()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(0)
}

#[cfg(test)]
#[path = "test_pickup.rs"]
mod tests;
