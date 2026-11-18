//! `stats`: where tasks' time went, from their timelines (bullpen_core::timeline). With
//! an ID, one task's timeline and totals; without, the totals across closed tasks, with
//! medians, the slowest tasks in each phase and how often tasks went back. In words, or
//! with `--json` as JSON (docs/stats.md).

mod old;
pub mod totals;

use crate::args::{flag, pos, value, Arg, Args, Nargs};
use crate::review::review_kind;
use bullpen_core::json::{dumps, obj, s, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{is_closed, task_num};
use bullpen_core::timeline;
use bullpen_core::timeutil::{now, parse, Stamp};
use bullpen_core::{fail, Repo, Res};
use chrono::Local;
use serde_json::{json, Value};
use totals::{closed_at, lines, outcome, totals, PHASES};

const STATS: &[Arg] = &[
    pos(
        "id",
        Nargs::Maybe,
        "a task, like BP-3: its timeline and totals (none: closed tasks')",
    ),
    value(
        "--since",
        "WHEN",
        "closed tasks closed since then: a date (2026-11-01), a stamp, or days back (7d)",
    ),
    value("--last", "N", "the N closed tasks closed last"),
    flag("--json", "as JSON"),
];

/// `stats [ID] [--since WHEN] [--last N] [--json]`.
pub fn cmd_stats(argv: &[String]) -> Res<i32> {
    let a = Args::parse("stats", argv, STATS);
    let repo = Repo::new(repo_root()?)?;
    let at = now();
    let news = old::news_by_task(&repo.dir);
    if let Some(id) = a.arg("id").filter(|i| !i.is_empty()) {
        let t = repo.task(id)?;
        let st = task_stats(&repo, &t, news.get(s(&t, "id")).map_or(&[], Vec::as_slice), at, true);
        print(&if a.flag("--json") {
            dumps(&Value::Object(st))
        } else {
            task_text(&st)
        });
        return Ok(0);
    }
    let since = match a.opt("--since") {
        Some(w) => Some(since(w, at).ok_or_else(|| {
            bullpen_core::Fail(format!(
                "--since {w}: give a date (2026-11-01), a stamp, or days back (7d)"
            ))
        })?),
        None => None,
    };
    let last = match a.opt("--last") {
        Some(n) => Some(
            n.trim()
                .parse::<usize>()
                .or_else(|_| fail(format!("--last {n}: give a number")))?,
        ),
        None => None,
    };
    let mut closed: Vec<(Stamp, Obj)> = repo
        .tasks()?
        .into_iter()
        .filter(|t| is_closed(s(t, "status")))
        .filter_map(|t| closed_at(&t).map(|c| (c, t)))
        .filter(|(c, _)| since.is_none_or(|w| *c >= w))
        .collect();
    closed.sort_by_key(|(c, t)| (*c, task_num(t)));
    if let Some(n) = last {
        closed.drain(..closed.len().saturating_sub(n));
    }
    let tasks: Vec<Obj> = closed
        .iter()
        .map(|(_, t)| task_stats(&repo, t, news.get(s(t, "id")).map_or(&[], Vec::as_slice), at, false))
        .collect();
    let all = across(tasks);
    let what = match (a.opt("--since"), last) {
        (Some(w), Some(n)) => format!(" (the last {n} since {w})"),
        (Some(w), None) => format!(" (since {w})"),
        (None, Some(n)) => format!(" (the last {n})"),
        (None, None) => String::new(),
    };
    print(&if a.flag("--json") {
        dumps(&Value::Object(all))
    } else {
        across_text(&all, &what)
    });
    Ok(0)
}

fn print(text: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout().lock(), "{text}");
}

/// --since: "2026-11-01", a stamp, or "7d" (days back from at).
fn since(w: &str, at: Stamp) -> Option<Stamp> {
    let w = w.trim();
    if let Some(days) = w.strip_suffix('d').and_then(|n| n.parse::<i64>().ok()) {
        return Some(at - chrono::Duration::days(days));
    }
    parse(w).or_else(|| parse(&format!("{w}T00:00")))
}

// ---------- one task ----------

/// A task's stats: what it is, its times, its totals, and (with events) its timeline.
/// One with no timeline of its own (from before them) has its news for one, and its
/// phases unknown (null).
pub fn task_stats(repo: &Repo, t: &Obj, news: &[Obj], at: Stamp, events: bool) -> Obj {
    let id = s(t, "id");
    let recorded = timeline::read(&repo.dir, id).unwrap_or_default();
    let whole = recorded.first().is_some_and(|e| s(e, "event") == "added");
    let kind = match (recorded.is_empty(), whole) {
        (true, _) => "none",
        (false, true) => "whole",
        (false, false) => "partial",
    };
    // a whole timeline says when the task was added and closed; else the task does
    let ended = recorded
        .iter()
        .rev()
        .find(|e| s(e, "event") == "status" && is_closed(s(e, "to")))
        .and_then(|e| parse(s(e, "at")));
    let (added, closed) = match recorded.first().and_then(|e| parse(s(e, "at"))) {
        Some(first) if whole && (ended.is_some() || !is_closed(s(t, "status"))) => (Some(first), ended),
        _ => (parse(s(t, "created")), closed_at(t)),
    };
    let mut st = Obj::new();
    st.insert("id".into(), json!(id));
    st.insert("title".into(), json!(s(t, "title")));
    st.insert("status".into(), json!(s(t, "status")));
    st.insert("outcome".into(), json!(outcome(t)));
    st.insert("timeline".into(), json!(kind));
    st.insert("added".into(), json!(added.map(|a| a.to_rfc3339())));
    st.insert("closed".into(), json!(closed.map(|c| c.to_rfc3339())));
    let end = closed.unwrap_or(at);
    st.insert("elapsed".into(), json!(added.map(|a| (end - a).num_seconds().max(0))));
    if recorded.is_empty() {
        let mut time = Obj::new();
        for (k, _) in PHASES {
            time.insert(k.into(), Value::Null);
        }
        st.insert("time".into(), Value::Object(time));
        for k in ["turns", "questions"] {
            st.insert(k.into(), Value::Null);
        }
        let round = obj(t, "review").get("round").and_then(Value::as_i64);
        st.insert("review_rounds".into(), json!(round)); // the task says how many rounds it had
        for k in ["reviews", "trips_back", "stalls", "accept_refused"] {
            st.insert(k.into(), Value::Null);
        }
    } else {
        st.extend(totals(&recorded, at));
    }
    st.insert("commits".into(), json!(t.get("commits").and_then(Value::as_i64)));
    st.insert(
        "lines".into(),
        json!(lines(t).map(|(a, r)| json!({"added": a, "removed": r}))),
    );
    if events {
        // the news from before the timeline was recorded, then the timeline
        let first = recorded.first().and_then(|e| parse(s(e, "at")));
        let before = news
            .iter()
            .filter(|e| first.is_none_or(|f| parse(s(e, "at")).is_some_and(|a| a < f)));
        let all: Vec<Value> = before.chain(recorded.iter()).cloned().map(Value::Object).collect();
        st.insert("events".into(), Value::Array(all));
    }
    st
}

/// A span of time in words: "2h 05m", "14m", "40s", "3d 2h".
pub fn span(secs: i64) -> String {
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{secs}s")
    }
}

fn span_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_i64).map_or("unknown".into(), span)
}

fn clock(stamp: &str) -> String {
    parse(stamp).map_or(stamp.to_string(), |t| {
        t.with_timezone(&Local).format("%b %d %H:%M").to_string()
    })
}

/// "review 2, rebase 1" from counts; "none" for none.
fn counts(v: Option<&Value>) -> String {
    match v.and_then(Value::as_object) {
        None => "unknown".into(),
        Some(o) if o.is_empty() => "none".into(),
        Some(o) => o.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", "),
    }
}

fn num(v: Option<&Value>) -> String {
    v.and_then(Value::as_i64).map_or("unknown".into(), |n| n.to_string())
}

/// What a timeline entry says, in words.
pub fn describe(e: &Obj) -> String {
    let why = |e: &Obj| match s(e, "why") {
        "" => String::new(),
        w => format!(": {w}"),
    };
    let risk = |e: &Obj| {
        let r = obj(e, "risk");
        match s(r, "level") {
            "" => String::new(),
            level => format!(" ({})", bullpen_core::risk::label(level, s(r, "why"))),
        }
    };
    let kind = |e: &Obj| review_kind(&[("kind".to_string(), json!(s(e, "kind")))].into_iter().collect());
    match s(e, "event") {
        "added" => "added".into(),
        "turn_ended" => "its worker ended a turn".into(),
        "review_started" => format!("{} started (round {})", kind(e), num(e.get("round"))),
        "review_finished" => match s(e, "verdict") {
            "approved" => format!("{} approved it", kind(e)),
            _ => format!("{} asked for changes", kind(e)),
        },
        "accept_refused" => format!("auto-accept refused ({}){}", s(e, "reason"), why(e)),
        "news" => format!("news, {}: {}", s(e, "kind"), s(e, "text")),
        "status" => {
            let (from, to) = (s(e, "from"), s(e, "to"));
            match to {
                "working" if from == "todo" => "started".into(),
                "working" => match s(e, "back") {
                    "" if matches!(from, "needs") => "working again (answered)".into(),
                    "" => format!("working again (was {from})"),
                    "limit" => "carried on after the usage limit".into(),
                    back => format!("sent back to work ({back})"),
                },
                "review" if from == "working" => "ready: in review".into(),
                "review" => format!("in review again (was {from})"),
                "ready" => format!("ready to accept{}{}", why(e), risk(e)),
                "needs" => format!("needs the person{}", why(e)),
                "triage" => format!("coordinator on it{}", why(e)),
                "done" => outcome_of(e).into(),
                other => format!("{other}{}", why(e)),
            }
        }
        other => other.into(),
    }
}

fn outcome_of(e: &Obj) -> &'static str {
    match s(e, "outcome") {
        "kept" => "closed, branch kept",
        "closed" => "closed (nothing to merge)",
        _ => "accepted",
    }
}

/// One task's stats as stats prints them.
fn task_text(st: &Obj) -> String {
    let mut out = vec![format!("{} {}", s(st, "id"), s(st, "title"))];
    let state = st
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or_else(|| s(st, "status"));
    let to = if st.get("outcome").is_some_and(|o| !o.is_null()) {
        "to closed"
    } else {
        "so far"
    };
    out.push(format!("{state}, {} from added {to}", span_of(st.get("elapsed"))));
    match s(st, "timeline") {
        "none" => out.push("No timeline: it's from before they were recorded (its news below).".into()),
        "partial" => out.push("Its timeline starts partway: the time before it is unknown.".into()),
        _ => {}
    }
    out.push(String::new());
    out.push("Timeline".into());
    for e in st.get("events").and_then(Value::as_array).into_iter().flatten() {
        if let Value::Object(e) = e {
            out.push(format!("  {}  {}", clock(s(e, "at")), describe(e)));
        }
    }
    out.push(String::new());
    out.push("Time".into());
    let time = obj(st, "time");
    for (k, words) in PHASES {
        out.push(format!("  {words:<28} {:>8}", span_of(time.get(k))));
    }
    out.push(String::new());
    out.push(format!(
        "Review rounds {} (reviews: {}); trips back: {}; stalls {}; questions {}; turns {}",
        num(st.get("review_rounds")),
        counts(st.get("reviews")),
        counts(st.get("trips_back")),
        num(st.get("stalls")),
        num(st.get("questions")),
        num(st.get("turns")),
    ));
    if obj(st, "accept_refused").len() > 0 {
        out.push(format!("Auto-accept refused: {}", counts(st.get("accept_refused"))));
    }
    out.push(format!("Changes: {}", changes(st)));
    out.join("\n")
}

fn changes(st: &Obj) -> String {
    let lines = obj(st, "lines");
    let commits = st
        .get("commits")
        .and_then(Value::as_i64)
        .map_or("commits unknown".into(), bullpen_core::task::commits);
    match (lines.get("added"), lines.get("removed")) {
        (Some(a), Some(r)) => format!("{commits}, +{a} −{r}"),
        _ => format!("{commits}, lines unknown"),
    }
}

// ---------- across tasks ----------

fn median(mut v: Vec<i64>) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    })
}

fn ints<'a>(tasks: impl Iterator<Item = &'a Obj>, f: impl Fn(&Obj) -> Option<i64>) -> Vec<i64> {
    tasks.filter_map(|t| f(t)).collect()
}

/// Add counts into sum.
fn add_counts(sum: &mut Obj, v: Option<&Value>) {
    for (k, n) in v.and_then(Value::as_object).into_iter().flatten() {
        let was = sum.get(k).and_then(Value::as_i64).unwrap_or(0);
        sum.insert(k.clone(), json!(was + n.as_i64().unwrap_or(0)));
    }
}

/// The totals across tasks (task_stats without events, closed ones), and the tasks.
pub fn across(tasks: Vec<Obj>) -> Obj {
    let timed: Vec<&Obj> = tasks.iter().filter(|t| s(t, "timeline") == "whole").collect();
    let mut outcomes = Obj::new();
    for t in &tasks {
        let o = t.get("outcome").and_then(Value::as_str).unwrap_or("open");
        let n = outcomes.get(o).and_then(Value::as_i64).unwrap_or(0);
        outcomes.insert(o.into(), json!(n + 1));
    }
    let mut phases = Obj::new();
    let all_time: i64 = timed
        .iter()
        .flat_map(|t| obj(t, "time").values().filter_map(Value::as_i64))
        .sum();
    for (k, _) in PHASES {
        let secs = |t: &Obj| obj(t, "time").get(k).and_then(Value::as_i64);
        let v = ints(timed.iter().copied(), secs);
        let total: i64 = v.iter().sum();
        let mut slow: Vec<(i64, &str)> = timed
            .iter()
            .filter_map(|t| secs(t).filter(|n| *n > 0).map(|n| (n, s(t, "id"))))
            .collect();
        slow.sort_by(|a, b| b.0.cmp(&a.0));
        let slowest: Vec<Value> = slow
            .iter()
            .take(3)
            .map(|(n, id)| json!({"id": id, "seconds": n}))
            .collect();
        let share = if all_time > 0 { total * 100 / all_time } else { 0 };
        phases.insert(
            k.into(),
            json!({"median": median(v), "total": total, "share": share, "slowest": slowest}),
        );
    }
    let (mut trips, mut refused, mut reviews) = (Obj::new(), Obj::new(), Obj::new());
    for t in &timed {
        add_counts(&mut trips, t.get("trips_back"));
        add_counts(&mut refused, t.get("accept_refused"));
        add_counts(&mut reviews, t.get("reviews"));
    }
    let sent_back = timed.iter().filter(|t| !obj(t, "trips_back").is_empty()).count();
    let stat = |v: Vec<i64>| json!({"median": median(v.clone()), "total": v.iter().sum::<i64>()});
    let get = |k: &'static str| move |t: &Obj| t.get(k).and_then(Value::as_i64);
    let line = |k: &'static str| move |t: &Obj| obj(t, "lines").get(k).and_then(Value::as_i64);
    let mut out = Obj::new();
    out.insert("count".into(), json!(tasks.len()));
    out.insert("outcomes".into(), Value::Object(outcomes));
    out.insert("with_timeline".into(), json!(timed.len()));
    out.insert("phases".into(), Value::Object(phases));
    out.insert("elapsed".into(), stat(ints(tasks.iter(), get("elapsed"))));
    out.insert("review_rounds".into(), stat(ints(tasks.iter(), get("review_rounds"))));
    out.insert("reviews".into(), Value::Object(reviews));
    out.insert("trips_back".into(), Value::Object(trips));
    out.insert("tasks_sent_back".into(), json!(sent_back));
    out.insert("stalls".into(), stat(ints(timed.iter().copied(), get("stalls"))));
    out.insert("questions".into(), stat(ints(timed.iter().copied(), get("questions"))));
    out.insert("accept_refused".into(), Value::Object(refused));
    out.insert("commits".into(), stat(ints(tasks.iter(), get("commits"))));
    out.insert(
        "lines".into(),
        json!({"added": stat(ints(tasks.iter(), line("added"))), "removed": stat(ints(tasks.iter(), line("removed")))}),
    );
    let mut list = tasks;
    for t in &mut list {
        t.shift_remove("events");
    }
    out.insert(
        "tasks".into(),
        Value::Array(list.into_iter().map(Value::Object).collect()),
    );
    out
}

/// "median 14m, 2h 05m in all".
fn med_total(v: &Value, f: fn(i64) -> String) -> String {
    let m = v.get("median").and_then(Value::as_i64).map_or("unknown".into(), f);
    let t = v.get("total").and_then(Value::as_i64).unwrap_or(0);
    format!("median {m}, {} in all", f(t))
}

/// The totals across tasks as stats prints them. what: the filter, in words.
fn across_text(all: &Obj, what: &str) -> String {
    let n = all["count"].as_i64().unwrap_or(0);
    if n == 0 {
        return format!("No closed tasks{what}.");
    }
    let timed = all["with_timeline"].as_i64().unwrap_or(0);
    let mut out = vec![format!(
        "{n} closed task{}{what}: {}.",
        if n == 1 { "" } else { "s" },
        counts(all.get("outcomes"))
    )];
    let without: Vec<&str> = all["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|t| s(t, "timeline") != "whole")
        .map(|t| s(t, "id"))
        .collect();
    if !without.is_empty() {
        out.push(format!(
            "{timed} with a whole timeline; {} without one (from before they were recorded, or started before): {}. \
             Their phases are unknown and left out.",
            without.len(),
            without.join(", ")
        ));
    }
    let span_of = |v: i64| span(v);
    out.push(String::new());
    if timed > 0 {
        out.push(format!(
            "Time per task, over the {timed} with a timeline (share: of all their time)"
        ));
        out.push(format!(
            "  {:<28} {:>8} {:>8} {:>6}  slowest",
            "", "median", "total", "share"
        ));
        let phases = obj(all, "phases");
        for (k, words) in PHASES {
            let p = obj(phases, k);
            let slowest: Vec<String> = p
                .get("slowest")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .map(|x| format!("{} {}", s(x, "id"), span(x["seconds"].as_i64().unwrap_or(0))))
                .collect();
            out.push(format!(
                "  {words:<28} {:>8} {:>8} {:>5}%  {}",
                span_of_opt(p.get("median")),
                span(p.get("total").and_then(Value::as_i64).unwrap_or(0)),
                p.get("share").and_then(Value::as_i64).unwrap_or(0),
                slowest.join(", ")
            ));
        }
        out.push(String::new());
    }
    out.push(format!("From added to closed: {}", med_total(&all["elapsed"], span_of)));
    let rounds = med_total(&all["review_rounds"], |n| n.to_string());
    out.push(match timed {
        0 => format!("Review rounds: {rounds}"),
        _ => format!("Review rounds: {rounds} (reviews: {})", counts(all.get("reviews"))),
    });
    if timed > 0 {
        out.push(format!(
            "Trips back: {} ({} of {timed} tasks went back at least once)",
            counts(all.get("trips_back")),
            all["tasks_sent_back"]
        ));
        out.push(format!("Stalls: {}", med_total(&all["stalls"], |n| n.to_string())));
        out.push(format!(
            "Questions to the person: {}",
            med_total(&all["questions"], |n| n.to_string())
        ));
        out.push(format!("Auto-accept refused: {}", counts(all.get("accept_refused"))));
    }
    let lines = obj(all, "lines");
    let total = |k: &str| obj(lines, k).get("total").and_then(Value::as_i64).unwrap_or(0);
    out.push(format!(
        "Changes: commits {}; lines +{} −{} in all",
        med_total(&all["commits"], |n| n.to_string()),
        total("added"),
        total("removed")
    ));
    out.join("\n")
}

fn span_of_opt(v: Option<&Value>) -> String {
    v.and_then(Value::as_i64).map_or("-".into(), span)
}

#[cfg(test)]
#[path = "tests/test_stats.rs"]
mod tests;
