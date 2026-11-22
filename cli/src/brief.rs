//! `brief`: print a task's brief, or change it: `--file` replaces it, `--add` appends a
//! dated section. The brief is briefs/<ID>.md in the state folder; a started task's
//! worker reads briefs/<ID>.full.md, written again here. Each change is kept
//! (briefs/<ID>.brief-<n>.md: the added section, or the diff) and listed on the task
//! (`brief_changes`), so a worker is told only what changed and a reviewer's follow-up
//! round is told the brief changed since the last one (`changed_since`). What the
//! worker is told is `message`, typed by `tell`.

use crate::args::{pos, value, Arg, Args, Nargs};
use crate::guide::unified_diff;
use crate::host::Agent;
use crate::news::kind::Kind;
use crate::review::close_reviewer;
use crate::rules::task_rules;
use crate::saves::save;
use crate::start::launch::{hand_back, write_full};
use crate::start::read_text;
use crate::stops::stop;
use bullpen_core::json::{self, obj, py_str, s, truthy, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{at_prompt, back_to_work, clip, is_closed, is_status, is_waiting, whose_turn};
use bullpen_core::text::{rstrip, splitlines, strip};
use bullpen_core::timeutil::{iso_now, now};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::path::Path;

const BRIEF: &[Arg] = &[
    pos("id", Nargs::One, "the task, like BP-3"),
    value("--file", "FILE", "replace its brief with this file's text"),
    value("--add", "TEXT", "add this to the end of its brief, as a dated section"),
];

/// A change of more lines than this is read from its file, not typed to the worker.
const TYPED_LINES: usize = 40;

/// What a change did to the brief: the brief as it is now, and what changed (the added
/// section, or the diff).
#[derive(Debug, PartialEq)]
pub struct Change {
    pub how: &'static str,
    pub brief: String,
    pub what: String,
}

/// How telling the worker went.
enum Told {
    /// the task hasn't started: its worker will read the new brief
    NotStarted,
    Yes,
    NotRunning,
    AtPrompt,
    Failed(String),
    /// it didn't reach the worker of a task in review: the review is stopped and the task
    /// is its coordinator's (saved)
    ReviewStopped(String),
}

/// `brief`: print a task's brief, or change it and tell its worker what changed.
pub fn cmd_brief(argv: &[String]) -> Res<i32> {
    let a = Args::parse("brief", argv, BRIEF);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    let briefs = repo.dir.join("briefs");
    let path = briefs.join(format!("{id}.md"));
    let old = read_text(&path.display().to_string())?;
    let (file, add) = (a.opt("--file"), a.opt("--add"));
    let change = match (file, add) {
        (None, None) => {
            print!("{old}");
            return Ok(0);
        }
        (Some(_), Some(_)) => return fail("give --file or --add, not both"),
        (Some(f), None) => {
            refuse(&t)?;
            let text = read_text(f)?;
            if strip(&text).is_empty() {
                return fail(format!("{f} is empty: a brief says what a worker would do"));
            }
            replaced(&old, &text)
        }
        (None, Some(text)) => {
            refuse(&t)?;
            if strip(text).is_empty() {
                return fail("give the text to add");
            }
            Some(added(&old, text, &now().format("%Y-%m-%d %H:%M").to_string()))
        }
    };
    let Some(change) = change else {
        println!("{id}'s brief is as it was: nothing changed");
        return Ok(0);
    };
    let n = count(&t) + 1;
    let kept = briefs.join(format!("{id}.brief-{n}.md"));
    std::fs::write(&kept, format!("{}\n", change.what))?;
    std::fs::write(&path, &change.brief)?;
    let entry = json!({"n": n, "at": iso_now(), "how": change.how, "file": kept.display().to_string()});
    match t.get_mut("brief_changes").and_then(Value::as_array_mut) {
        Some(list) => list.push(entry),
        None => {
            t.insert("brief_changes".into(), json!([entry]));
        }
    }
    let full = briefs.join(format!("{id}.full.md"));
    if full.is_file() {
        write_full(&repo, &t, &task_rules(&repo, &t)?)?;
    }
    let told = tell(&repo, &mut t, &message(&change, &full, &kept))?;
    if !matches!(told, Told::Yes | Told::ReviewStopped(_)) {
        json::write_obj(&repo.task_path(&id), &t)?; // not a save: its status didn't change
    }
    let later = shell_quoted(&pointer(&full, &kept));
    match told {
        Told::NotStarted => println!("{id}'s brief changed: its worker starts with it as it is now"),
        Told::Yes => println!("{id}'s brief changed, and its worker is told what changed"),
        Told::NotRunning => {
            println!("{id}'s brief changed. Its worker isn't running, so it isn't told: `bullpen resume {id} {later}`")
        }
        Told::AtPrompt => println!(
            "{id}'s brief changed. Its worker is at a prompt, so it isn't told: once that's answered, \
             `bullpen tell {id} {later}`"
        ),
        Told::ReviewStopped(why) => {
            let later = shell_quoted(&format!("{} {REVIEW_STOPPED}", pointer(&full, &kept)));
            return fail(format!(
                "{id}'s brief changed and its review is stopped, but the message didn't reach its worker ({why}): \
                 `bullpen tell {id} {later}`"
            ));
        }
        Told::Failed(why) => {
            return fail(format!(
                "{id}'s brief changed, but the message didn't reach its worker ({why}): `bullpen tell {id} {later}`"
            ))
        }
    }
    Ok(0)
}

/// A brief changes only under work that isn't done: not a ready, finished or closed task's.
fn refuse(t: &Obj) -> Res<()> {
    let id = s(t, "id");
    let status = s(t, "status");
    if is_closed(status) {
        return fail(format!(
            "{}: add a follow-up task for what's still wanted (`bullpen add`)",
            is_status(t)
        ));
    }
    // a ready task the person was asked about is still ready
    let asked_from = s(obj(t, "asked_from"), "status");
    if [status, asked_from]
        .iter()
        .any(|st| matches!(*st, "ready" | "finished"))
    {
        return fail(format!(
            "{}: its brief can't change under work that's done. Send it back first (`bullpen tell {id} \"...\"`, \
             then `bullpen brief` again), or add a follow-up task (`bullpen add`)",
            is_status(t)
        ));
    }
    Ok(())
}

/// The brief with text in place of it, or None when that changes nothing.
pub fn replaced(old: &str, text: &str) -> Option<Change> {
    let brief = format!("{}\n", rstrip(text));
    if brief == old {
        return None;
    }
    let diff = unified_diff(
        &splitlines(old),
        &splitlines(&brief),
        "brief (before)",
        "brief (now)",
        3,
    );
    Some(Change {
        how: "replaced",
        brief,
        what: diff.join("\n"),
    })
}

/// The brief with text added at its end, as a section dated stamp.
pub fn added(old: &str, text: &str, stamp: &str) -> Change {
    let section = format!("## Added {stamp}\n\n{}", strip(text));
    Change {
        how: "added",
        brief: format!("{}\n\n{section}\n", rstrip(old)),
        what: section,
    }
}

/// What the worker is told: the change itself when it's short, so it needn't read the
/// brief again, else where to read it.
pub fn message(change: &Change, full: &Path, kept: &Path) -> String {
    let lines = splitlines(&change.what).len();
    if lines > TYPED_LINES {
        return pointer(full, kept);
    }
    let said = if change.how == "added" {
        "This was added to it"
    } else {
        "This is what changed in it"
    };
    format!(
        "Your brief changed: follow it as it is now. {said} (the whole brief, {}, has it too: you needn't read \
         the rest again):\n\n{}",
        full.display(),
        change.what
    )
}

/// One line that says where the change is, for a long change or a worker told later.
fn pointer(full: &Path, kept: &Path) -> String {
    format!(
        "Your brief changed: follow it as it is now. What changed is in {} (the whole brief, {}, has it too: you \
         needn't read the rest again).",
        kept.display(),
        full.display()
    )
}

fn shell_quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\\\""))
}

/// What a worker whose review was stopped is told after the change.
const REVIEW_STOPPED: &str = "The review of your work that was under way has been stopped: when you've done what \
                              changed, run your ready command again.";

/// Is a reviewer on the task? In review, stopped on its reviewer (a prompt, no verdict),
/// asked about during its review, or with a reviewer's pane and no verdict yet.
fn in_review(t: &Obj) -> bool {
    let r = obj(t, "review");
    whose_turn(t) == Some("reviewer")
        || s(obj(t, "asked_from"), "status") == "review"
        || (truthy(r.get("block")) && !truthy(r.get("verdict")))
}

/// Tell the task's worker msg, the way `tell` would. A review under way is of work on
/// the brief as it was: its reviewer is closed and the task goes back to its worker
/// (resumed, if its pane is gone), which runs `ready` again; that round isn't counted.
fn tell(repo: &Repo, t: &mut Obj, msg: &str) -> Res<Told> {
    let status = s(t, "status").to_string();
    if status == "todo" || truthy(t.get("needs_setup")) {
        return Ok(Told::NotStarted);
    }
    if in_review(t) {
        close_reviewer(t)?;
        if let Some(Value::Object(r)) = t.get_mut("review") {
            r.insert("void".into(), json!(true));
        }
        let failed = match hand_back(repo, t, &format!("{msg}\n\n{REVIEW_STOPPED}"), "person") {
            Ok(failed) => failed,
            Err(e) => Some(clip(&e.0, 160)),
        };
        let Some(why) = failed else {
            return Ok(Told::Yes);
        };
        // nobody is on it now: its coordinator's, not a review with no reviewer
        let stopped = "Its brief changed, but that didn't reach its worker";
        stop(repo, t, "worker", "not_sent", stopped, &[], Kind::NotSent)?;
        return Ok(Told::ReviewStopped(why));
    }
    if !Agent::of(t).alive()? {
        return Ok(Told::NotRunning);
    }
    if at_prompt(t) {
        return Ok(Told::AtPrompt);
    }
    if let Some(why) = Agent::of(t).tell(msg)? {
        return Ok(Told::Failed(why));
    }
    if is_waiting(&status) {
        back_to_work(t);
    }
    t.insert("last".into(), Value::Null);
    save(repo, t)?;
    Ok(Told::Yes)
}

/// How many times the task's brief has changed.
pub fn count(t: &Obj) -> i64 {
    t.get("brief_changes")
        .and_then(Value::as_array)
        .map_or(0, |l| l.len() as i64)
}

/// For a reviewer's prompt, after the brief's path: that the brief changed since the
/// round this one follows (prev, which saw `brief` changes), and where each change is.
/// "" for a first round, or when it hasn't.
pub fn changed_since(t: &Obj, prev: Option<&Obj>) -> String {
    let Some(prev) = prev else {
        return String::new();
    };
    let seen = prev.get("brief").and_then(Value::as_i64).unwrap_or(0);
    let files: Vec<String> = t
        .get("brief_changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| c.get("n").and_then(Value::as_i64).unwrap_or(0) > seen)
        .map(|c| format!("`{}`", py_str(c.get("file").unwrap_or(&Value::Null))))
        .collect();
    if files.is_empty() {
        return String::new();
    }
    format!(
        "\n\nThe brief changed since round {} looked (what changed: {}). Check the work against the brief as it \
         is now: what was added to it is asked for too, and what was taken out no longer is.",
        py_str(prev.get("round").unwrap_or(&Value::Null)),
        files.join(", ")
    )
}

#[cfg(test)]
#[path = "tests/test_brief.rs"]
mod tests;
