//! After an accept: bringing the other open tasks on the same base up to date
//! (`rebase_others`; `rebase_task` for one task). A working agent is told to rebase when the
//! accepted change meets its work, one between turns is rebased, and a running review
//! is left be. One news line sums it up. Off with `config --rebase-others off`.
//! A trivial conflict in a doc or test file is resolved keeping both sides. What a
//! worker is told is `told`; `show`'s line for a ready task that no longer rebases is
//! `stale_conflicts`, by `conflicts_with`.

use crate::become_ready::{make_ready, How};
use crate::git_names::{self, listed};
use crate::modes::{is_local, quiet_git, refresh, task_base_ref};
use crate::news::kind::Kind;
use crate::news::{news, Opts};
use crate::review::{is_doc, keep_approval, rebase_check, shortstat, start_review, Check};
use crate::rex::{alive, rex_type};
use crate::saves::save;
use crate::start::launch::hand_back;
use bullpen_core::json::{obj, py_str, s, truthy, Obj};
use bullpen_core::proc::{git, text};
use bullpen_core::task::{is_closed, worker_idle};
use bullpen_core::text::{split, splitlines, strip};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

/// Bring the other open tasks on the accepted task's base up to date. touched: the
/// files the accepted change touched (None: not known).
pub fn rebase_others(repo: &Repo, accepted: &Obj, touched: Option<&BTreeSet<String>>) -> Res<()> {
    if !truthy(repo.cfg.get("rebase_others")) {
        return Ok(());
    }
    let (aid, base) = (s(accepted, "id"), accepted.get("base"));
    let mut others: Vec<Obj> = repo
        .tasks()?
        .into_iter()
        .filter(|o| {
            let status = s(o, "status");
            s(o, "id") != aid
                && !is_closed(status)
                && status != "finished"
                && o.get("base") == base
                && truthy(o.get("worktree"))
                && Path::new(s(o, "worktree")).is_dir()
        })
        .collect();
    let mut said = Vec::new();
    for o in &mut others {
        let line = match rebase_other(repo, o, accepted, touched) {
            Ok(line) => line,
            Err(e) => Some(format!("{} not rebased ({e})", s(o, "id"))),
        };
        said.extend(line);
    }
    if said.is_empty() {
        return Ok(());
    }
    let everyone = Opts {
        everyone: true,
        ..Opts::default()
    };
    news(
        repo,
        accepted,
        after_kind(&said),
        &format!("After {aid}: {}.", said.join("; ")),
        everyone,
    )
}

/// The kind of an `After BP-3: ...` line, from what it says of each task: NotRebased
/// when one needs seeing to (not rebased, its push failed, or the message to rebase
/// didn't reach its worker), else Rebased.
pub fn after_kind(said: &[String]) -> Kind {
    let asks = |l: &String| {
        ["not rebased", "the push failed", "didn't reach its worker"]
            .iter()
            .any(|p| l.contains(p))
    };
    if said.iter().any(asks) {
        Kind::NotRebased
    } else {
        Kind::Rebased
    }
}

/// Bring one task up to date with its base; say how in a few words, or None when there
/// was nothing to do.
fn rebase_other(repo: &Repo, t: &mut Obj, accepted: &Obj, touched: Option<&BTreeSet<String>>) -> Res<Option<String>> {
    let (id, wt) = (s(t, "id").to_string(), s(t, "worktree").to_string());
    let base_ref = task_base_ref(repo, t)?;
    if git(&wt, &["merge-base", "--is-ancestor", &base_ref, "HEAD"], false)?.ok() {
        return Ok(None); // already on top of it
    }
    let status = s(t, "status");
    if super::left_in_review::in_review(t) {
        // a reviewer is on it: its approval checks it against the base (left_in_review.rs)
        super::left_in_review::mark(repo, &id, s(accepted, "id"))?;
        return Ok(Some(format!("{id} left in review")));
    }
    if status == "working" && alive(s(t, "session"), s(t, "block"))? {
        return tell_to_rebase(t, accepted, &base_ref, touched).map(Some);
    }
    if !worker_idle(t) {
        // mid-turn: on a prompt, or failed
        let who = if status == "needs" {
            "the person"
        } else {
            "its coordinator"
        };
        return Ok(Some(format!("{id} waiting on {who}, not rebased")));
    }
    if !strip(&git(&wt, &["status", "--porcelain"], true)?.stdout).is_empty() {
        return Ok(Some(format!("{id} has uncommitted changes, not rebased")));
    }
    rebase_task(repo, t, s(accepted, "id"), &base_ref).map(Some)
}

/// Rebase a task between turns onto base_ref, keeping both sides of a trivial conflict;
/// one that conflicts otherwise goes back to its worker. by: the accepts that moved the
/// base, in words ("BP-3", "BP-3 and BP-5"). Says how in a few words.
pub fn rebase_task(repo: &Repo, t: &mut Obj, by: &str, base_ref: &str) -> Res<String> {
    let (id, wt) = (s(t, "id").to_string(), s(t, "worktree").to_string());
    let mut done = git(&wt, &["rebase", "-q", &base_ref], false)?.ok();
    let mut both: Vec<String> = Vec::new(); // files where bullpen kept both sides of a conflict
    while !done {
        let files = unmerged(&wt)?;
        let shown = git(&wt, &["diff"], false)?.stdout; // the conflicts, before anything is resolved
        if files.is_empty() || !files.iter().all(|f| keep_both_sides(&wt, f)) {
            git(&wt, &["rebase", "--abort"], false)?;
            return rebase_conflict(repo, t, by, base_ref, &files, &shown);
        }
        let mut add = vec!["add", "--"];
        add.extend(files.iter().map(String::as_str));
        git(&wt, &add, true)?;
        for f in &files {
            if !both.contains(f) {
                both.push(f.clone());
            }
        }
        save_conflicts(repo, t, &files, &shown)?;
        done = Command::new("git")
            .args(["-C", &wt, "rebase", "--continue"])
            .env("GIT_EDITOR", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?
            .status
            .success();
    }
    if let Some(failed) = refresh(repo, t)? {
        // its PR gets the rebased branch
        return Ok(format!("{id} rebased, but the push failed: {failed}"));
    }
    let base = t.get("base").cloned().unwrap_or(Value::Null);
    t.insert("rebased".into(), base);
    if s(t, "status") == "ready" && truthy(t.get("diffstat")) {
        t.insert("diffstat".into(), json!(shortstat(&wt, base_ref)?));
    }
    let mut rebased = format!("{id} rebased");
    if !both.is_empty() {
        rebased += &format!(", keeping both sides of a conflict in {}", listed(&both));
    }
    if s(obj(t, "review"), "verdict") == "approved" {
        let check = rebase_check(t, base_ref)?;
        if let Some(Check::Kept(why)) = &check {
            keep_approval(t, base_ref)?;
            if s(t, "status") == "ready" {
                make_ready(repo, t, base_ref, How::Rebased)?; // labelled for its code now
            } else {
                save(repo, t)?; // same status: it stays where it was
            }
            return Ok(format!("{rebased}; its approval carried over ({why})"));
        }
        if s(t, "status") == "ready" && truthy(repo.cfg.get("review")) {
            let rebase = match check {
                Some(Check::Rebase(files)) => Some(files),
                _ => None,
            };
            let checked = rebase.is_some();
            start_review(repo, t, base_ref, rebase)?;
            return Ok(if checked {
                format!("{rebased}; a rebase check looks at the resolution")
            } else {
                format!("{rebased}, but its patch changed: a reviewer checks it again")
            });
        }
    }
    save(repo, t)?; // same status: it stays where it was
    Ok(rebased)
}

/// What a working task's work meets in the accepted change.
#[derive(Debug, PartialEq)]
pub enum Need {
    /// committed work that doesn't merge cleanly: the files
    Conflict(Vec<String>),
    /// uncommitted changes in files the change touched
    Uncommitted(Vec<String>),
    /// nothing: leave it alone, and why
    Alone(&'static str),
}

/// The names a git command lists (git_names::names); none when git fails.
fn names(wt: &str, args: &[&str]) -> Res<BTreeSet<String>> {
    Ok(git_names::names(wt, args)?.unwrap_or_default().into_iter().collect())
}

/// The files a rebase stopped with unmerged, exactly as named.
fn unmerged(wt: &str) -> Res<Vec<String>> {
    Ok(git_names::names(wt, &["diff", "--name-only", "--diff-filter=U"])?.unwrap_or_default())
}

/// Whether a working task's work meets the accepted change (the files in touched;
/// None: whatever its base gained since the task branched off).
fn rebase_need(wt: &str, base_ref: &str, touched: Option<&BTreeSet<String>>) -> Res<Need> {
    let touched = match touched {
        Some(t) => t.clone(),
        None => names(
            wt,
            &["diff", "--name-only", "--no-renames", &format!("HEAD...{base_ref}")],
        )?,
    };
    let committed = names(
        wt,
        &["diff", "--name-only", "--no-renames", &format!("{base_ref}...HEAD")],
    )?;
    let mut uncommitted = names(wt, &["diff", "--name-only", "--no-renames", "HEAD"])?;
    uncommitted.extend(names(wt, &["ls-files", "--others", "--exclude-standard"])?);
    let mut conflicts = BTreeSet::new();
    let in_committed: BTreeSet<String> = touched.intersection(&committed).cloned().collect();
    if !in_committed.is_empty() {
        conflicts = match merge_conflicts(wt, base_ref, "HEAD")? {
            Some(files) => files.into_iter().collect(),
            None => in_committed.clone(), // it couldn't tell: say so to the worker rather than guess
        };
    }
    let in_uncommitted: Vec<String> = touched.intersection(&uncommitted).cloned().collect();
    Ok(conflict_or(
        conflicts,
        in_committed.is_empty() && in_uncommitted.is_empty(),
        in_uncommitted,
    ))
}

/// The files a merge of head into base_ref would conflict in (a trial merge, `git
/// merge-tree`, which writes nothing); empty when it merges cleanly, None when git
/// couldn't tell.
fn merge_conflicts(dir: &str, base_ref: &str, head: &str) -> Res<Option<Vec<String>>> {
    let args = [
        "merge-tree",
        "--write-tree",
        "--name-only",
        "--no-messages",
        "-z",
        base_ref,
        head,
    ];
    let p = git(dir, &args, false)?;
    Ok(match p.code {
        0 => Some(Vec::new()),
        // the tree, then the conflicted files
        1 => Some(git_names::names_of(&p.stdout).into_iter().skip(1).collect()),
        _ => None,
    })
}

/// The need, from what was found: conflicts first, then uncommitted changes.
pub fn conflict_or(conflicts: BTreeSet<String>, no_overlap: bool, uncommitted: Vec<String>) -> Need {
    if no_overlap {
        Need::Alone("no overlap")
    } else if !conflicts.is_empty() {
        Need::Conflict(conflicts.into_iter().collect())
    } else if !uncommitted.is_empty() {
        Need::Uncommitted(uncommitted)
    } else {
        Need::Alone("no conflict")
    }
}

/// A working task: type into its pane that the base moved, only when that meets its
/// work (a conflict, or uncommitted changes in the files the accepted change touched).
fn tell_to_rebase(t: &Obj, accepted: &Obj, base_ref: &str, touched: Option<&BTreeSet<String>>) -> Res<String> {
    let id = s(t, "id");
    let need = rebase_need(s(t, "worktree"), base_ref, touched)?;
    let said = match &need {
        Need::Alone(why) => return Ok(format!("{id} left alone ({why})")),
        Need::Conflict(files) => format!("conflicts in {}", listed(files)),
        Need::Uncommitted(files) => format!("uncommitted changes in {}", listed(files)),
    };
    let base = py_str(t.get("base").unwrap_or(&Value::Null));
    let msg = told(&base, accepted, &need, base_ref);
    rex_type(s(t, "session"), s(t, "block"), &msg)?;
    Ok(format!("{id} told to rebase ({said})"))
}

/// What a working task is told when the accepted change meets its work: what landed,
/// where it meets the work, and to rebase at a good point (committing first, when the
/// work is uncommitted). Nothing for a task that is left alone.
pub fn told(base: &str, accepted: &Obj, need: &Need, base_ref: &str) -> String {
    let title = match s(accepted, "title") {
        "" => String::new(),
        title => format!(" ({title})"),
    };
    let landed = format!("{base} moved: {}{title} was just accepted", s(accepted, "id"));
    let onto = onto(base_ref);
    match need {
        Need::Alone(_) => String::new(),
        Need::Conflict(files) => format!(
            "{landed}, and your branch conflicts with it in {}. At a good point, {onto}; {KEEP_BOTH}.",
            listed(files)
        ),
        Need::Uncommitted(files) => format!(
            "{landed}, and it changed {}, where you have uncommitted changes. Commit them at a good point, then {onto}.",
            listed(files)
        ),
    }
}

/// How a worker settles a conflict, in words for its prompt.
const KEEP_BOTH: &str = "where both sides changed the same place, keep both";

/// How a worker rebases onto base_ref, in words for its prompt.
pub fn onto(base_ref: &str) -> String {
    if base_ref.starts_with("origin/") {
        return format!("run `git fetch origin`, then rebase your branch onto {base_ref}");
    }
    format!("rebase your branch onto {base_ref}")
}

/// Note on the task's review the files a rebase conflicted in (adding to any noted
/// before), and keep the conflicts as git showed them, for a rebase check.
fn save_conflicts(repo: &Repo, t: &mut Obj, files: &[String], shown: &str) -> Res<()> {
    let f = repo
        .dir
        .join("briefs")
        .join(format!("{}.rebase-conflicts.diff", s(t, "id")));
    let Some(Value::Object(r)) = t.get_mut("review").filter(|r| truthy(Some(r))) else {
        return Ok(());
    };
    let mut conflict: BTreeSet<String> = match r.get("conflict") {
        Some(Value::Array(a)) => a.iter().map(py_str).collect(),
        _ => BTreeSet::new(),
    };
    conflict.extend(files.iter().cloned());
    r.insert("conflict".into(), json!(conflict.into_iter().collect::<Vec<_>>()));
    let old = if truthy(r.get("conflict_diff")) && f.is_file() {
        std::fs::read_to_string(&f)?
    } else {
        String::new()
    };
    std::fs::write(&f, old + shown)?;
    r.insert("conflict_diff".into(), json!(f.display().to_string()));
    Ok(())
}

/// A conflict marker line (`^(<{7}|\|{7}|={7}|>{7})( |$)`): its first character.
fn marker(line: &str) -> Option<char> {
    let c = line.chars().next().filter(|c| "<|=>".contains(*c))?;
    if line.chars().take_while(|&x| x == c).count() != 7 {
        return None;
    }
    let rest = &line[7..]; // after seven ASCII characters: a char boundary
    (rest.is_empty() || rest.starts_with(' ')).then_some(c)
}

/// str.splitlines(keepends=True).
fn lines_with_ends(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut at = 0;
    for line in splitlines(text) {
        let end = line.as_ptr() as usize - text.as_ptr() as usize + line.len();
        let rest = &text[end..];
        let brk = if rest.starts_with("\r\n") {
            2
        } else {
            rest.chars().next().map_or(0, char::len_utf8)
        };
        out.push(&text[at..end + brk]);
        at = end + brk;
    }
    out
}

/// Resolve a trivial conflict mid-rebase: in a doc or test file that both sides changed,
/// every conflict is lines both sides added at the same place (nothing in the base
/// between them). Writes the file with the base's lines, then the task's, and returns
/// true; returns false, leaving it be, for anything else.
fn keep_both_sides(wt: &str, path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
    if !(is_doc(path) || path.starts_with("tests/") || path.contains("/tests/") || name.starts_with("test_")) {
        return false;
    }
    let Ok(staged) = git(wt, &["ls-files", "-u", "-z", "--", path], true) else {
        return false;
    };
    let stages: BTreeSet<String> = git_names::names_of(&staged.stdout)
        .iter()
        .filter_map(|row| split(row).get(2).map(|s| s.to_string()))
        .collect();
    if stages != BTreeSet::from(["1".into(), "2".into(), "3".into()]) {
        return false; // added on both sides, deleted on one, ...
    }
    if !git(wt, &["checkout", "--conflict=diff3", "--", path], false).is_ok_and(|p| p.ok()) {
        return false;
    }
    let file = Path::new(wt).join(path);
    let Ok(bytes) = std::fs::read(&file) else {
        return false;
    };
    if std::str::from_utf8(&bytes).is_err() {
        return false;
    }
    match both_sides(&text(&bytes)) {
        // read as Python's text mode does: \r\n and \r are \n
        Some(out) => std::fs::write(&file, out).is_ok(),
        None => false,
    }
}

/// The file with every conflict replaced by the base's added lines, then the task's;
/// None when a conflict isn't lines both sides added at one place, or there is none.
pub fn both_sides(content: &str) -> Option<String> {
    let mut out = String::new();
    let (mut part, mut found) = (None, false);
    let (mut ours, mut base, mut theirs) = (String::new(), String::new(), String::new());
    for line in lines_with_ends(content) {
        let mark = marker(line.trim_end_matches(['\r', '\n']));
        match (part, mark) {
            (None, Some('<')) => {
                (part, found) = (Some("ours"), true);
                (ours, base, theirs) = (String::new(), String::new(), String::new());
            }
            (None, _) => out.push_str(line), // outside a conflict, "=======" is just a line
            (Some(p), None) => match p {
                "ours" => ours.push_str(line),
                "base" => base.push_str(line),
                _ => theirs.push_str(line),
            },
            (Some("ours"), Some('|')) => part = Some("base"),
            (Some("base"), Some('=')) => part = Some("theirs"),
            (Some("theirs"), Some('>')) => {
                if !base.is_empty() {
                    return None; // one side changed lines the other kept or changed
                }
                out.push_str(&ours);
                out.push_str(&theirs);
                part = None;
            }
            _ => return None, // a marker out of place
        }
    }
    (part.is_none() && found).then_some(out)
}

/// The task's branch doesn't rebase onto the moved base: it's left as it was, and the
/// conflict goes to its worker, like review findings do.
fn rebase_conflict(repo: &Repo, t: &mut Obj, by: &str, base_ref: &str, files: &[String], shown: &str) -> Res<String> {
    let id = s(t, "id").to_string();
    let were = if by.contains(" and ") { "were" } else { "was" };
    let why = format!(
        "{} moved ({by} {were} accepted) and your branch doesn't rebase cleanly onto it",
        py_str(t.get("base").unwrap_or(&Value::Null)),
    );
    let names = names_of(files);
    if let Some(failed) = send_back_conflict(repo, t, &why, base_ref, files, shown)? {
        // left where it was
        return Ok(format!(
            "{id} conflicts in {names}, but the message didn't reach its worker ({failed}): \
             `bullpen tell {id}` it to rebase onto {base_ref} and fix them"
        ));
    }
    Ok(format!("{id} conflicts in {names}, sent back"))
}

/// The conflicting files in words.
pub fn names_of(files: &[String]) -> String {
    if files.is_empty() {
        "some files".to_string()
    } else {
        listed(files)
    }
}

/// Send a task whose branch doesn't rebase onto base_ref back to its worker (resumed if
/// its pane is gone), why first: the files it conflicts in, and to rebase and fix them.
/// The conflicts are noted on its review: its next ready keeps the approval if its own
/// lines are the same, gets a rebase check if they changed only in these files, else a
/// follow-up review. None, or why the message didn't reach the worker (left as it was).
pub fn send_back_conflict(
    repo: &Repo,
    t: &mut Obj,
    why: &str,
    base_ref: &str,
    files: &[String],
    shown: &str,
) -> Res<Option<String>> {
    save_conflicts(repo, t, files, shown)?;
    let msg = format!(
        "{why}: conflicts in {}. Your branch is as it was. Now {}; {KEEP_BOTH}. Run the full suite again, commit, \
         and run your ready command again.",
        names_of(files),
        onto(base_ref)
    );
    hand_back(repo, t, &msg, "rebase")
}

/// Whether the task's committed branch conflicts with its base as it is now (fetched
/// first, with a remote): (base_ref, the files, the conflicts as git shows them), from a
/// rebase tried in its worktree and undone. None when it rebases cleanly, or that can't
/// be told (no worktree, uncommitted changes).
pub fn base_conflict(repo: &Repo, t: &Obj) -> Res<Option<(String, Vec<String>, String)>> {
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).is_dir() {
        return Ok(None);
    }
    if !is_local(repo, t)? {
        let base = py_str(t.get("base").unwrap_or(&Value::Null));
        quiet_git(repo, &["fetch", "-q", "origin", &base], 60)?; // unfetched: it goes by what it has
    }
    let base_ref = task_base_ref(repo, t)?;
    if git(wt, &["merge-base", "--is-ancestor", &base_ref, "HEAD"], false)?.ok()
        || !strip(&git(wt, &["status", "--porcelain"], true)?.stdout).is_empty()
    {
        return Ok(None);
    }
    let old = strip(&git(wt, &["rev-parse", "HEAD"], true)?.stdout).to_string();
    if git(wt, &["rebase", "-q", &base_ref], false)?.ok() {
        git(wt, &["reset", "-q", "--hard", &old], true)?; // only a trial: the branch stays as it was
        return Ok(None);
    }
    let files = unmerged(wt)?;
    let shown = git(wt, &["diff"], false)?.stdout;
    git(wt, &["rebase", "--abort"], false)?;
    Ok((!files.is_empty()).then_some((base_ref, files, shown)))
}

/// For `show`: the files a ready task's branch conflicts with its base in, as the base
/// is here now (nothing is fetched or written: a trial merge). None when it still
/// rebases, isn't ready, or that can't be told.
pub fn stale_conflicts(repo: &Repo, t: &Obj) -> Res<Option<Vec<String>>> {
    let branch = s(t, "branch");
    if s(t, "status") != "ready" || branch.is_empty() {
        return Ok(None);
    }
    let base_ref = task_base_ref(repo, t)?;
    conflicts_with(
        &repo.root.display().to_string(),
        &base_ref,
        &format!("refs/heads/{branch}"),
    )
}

/// The files head conflicts with base_ref in (a trial merge in dir, nothing written):
/// the one test of a conflict for `show`'s rebase line and an approval after an accept
/// (left_in_review.rs). None when head is on top of base_ref, merges cleanly, or that
/// can't be told.
pub fn conflicts_with(dir: &str, base_ref: &str, head: &str) -> Res<Option<Vec<String>>> {
    if git(dir, &["merge-base", "--is-ancestor", base_ref, head], false)?.ok() {
        return Ok(None); // on top of it (or the branch is gone, which merge-tree says below)
    }
    Ok(merge_conflicts(dir, base_ref, head)?.filter(|files| !files.is_empty()))
}

/// `show`'s line for a ready task that no longer rebases onto its base.
pub fn stale_line(t: &Obj, files: &[String]) -> String {
    let (id, base) = (s(t, "id"), py_str(t.get("base").unwrap_or(&Value::Null)));
    format!(
        "   rebase: no longer rebases onto {base}: conflicts in {}. `bullpen tell {id}` it to rebase and run ready again",
        listed(files)
    )
}

#[cfg(test)]
#[path = "tests/test_rebase.rs"]
mod tests;
