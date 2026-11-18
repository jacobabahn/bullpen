//! Guided review tours: a tour's files, the diff it is written
//! on (`diff_files`, `covers`), its state (`tour_state`, `tour_stale`), a step's patch
//! (`step_diff`), and the person's draft comments on its steps. The tour agent that
//! writes one is in write.rs, `tour-comment` in comments.rs. The stepper (`tour`, which
//! starts `_tour`, and reads and sends a tour) is in stepper/.
//!
//! On request (`bullpen tour BP-3`, the board's g), a cheap headless agent writes a
//! tour of a ready or in-review task's change: its steps in reading order, each a
//! title, a note and the hunks it covers (file and line ranges). The agent only reads:
//! it prints the tour as JSON and bullpen writes the file (briefs/<ID>.tour.json),
//! adding a last step, "Other changes", for any hunk no step covers. Code is always
//! shown from git, never from the agent's text. A tour is tied to the commit it was
//! written on: it stays current while the patch is the same (as approvals do), and is
//! stale otherwise. The person's comments on its steps are drafts
//! (briefs/<ID>.tour-comments.json) until `tour --send` sends them to the worker as a
//! findings-style file. t["tour"] says whether a tour is being written ("writing", with
//! the pid writing it), was written, or failed (with why).

pub mod comments;
pub mod write;

use crate::modes::task_base_ref;
use crate::review::patch_id;
use bullpen_core::json::{self, obj, read_obj, s, Obj};
use bullpen_core::proc::{git, head, merge_base};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The last step's title: the hunks no step of the agent's covers.
pub const OTHER: &str = "Other changes";

pub fn tour_path(repo: &Repo, t: &Obj) -> PathBuf {
    repo.dir.join("briefs").join(format!("{}.tour.json", s(t, "id")))
}

pub fn tour_log(repo: &Repo, t: &Obj) -> PathBuf {
    repo.dir.join("briefs").join(format!("{}.tour.log", s(t, "id")))
}

pub fn comments_path(repo: &Repo, t: &Obj) -> PathBuf {
    repo.dir
        .join("briefs")
        .join(format!("{}.tour-comments.json", s(t, "id")))
}

/// Append text to a log, making it if need be.
pub fn append(log: &Path, text: &str) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?
        .write_all(text.as_bytes())
}

/// Write a task's file as it is: not a save, since its status didn't change.
pub fn write_task(repo: &Repo, t: &Obj) -> Res<()> {
    Ok(json::write_obj(&repo.task_path(s(t, "id")), t)?)
}

/// Change task tid's file as it is now, under the tours' lock (tours.lock): read it
/// again, change it, and write it if change says so. The command that starts a writer
/// and the writer both write t["tour"], so neither writes over the other with an older
/// copy. Returns the task as it is.
pub fn update_task(repo: &Repo, tid: &str, change: impl FnOnce(&mut Obj) -> bool) -> Res<Obj> {
    let _held = bullpen_core::lock::lock(&repo.dir.join("tours.lock"))?;
    let mut t = repo.task(tid)?;
    if change(&mut t) {
        write_task(repo, &t)?;
    }
    Ok(t)
}

// ---------- the diff ----------

/// One hunk of a diff: its old and new (start, count), and its text (the @@ line on).
#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    pub old: (i64, i64),
    pub new: (i64, i64),
    pub text: String,
}

/// One file of a diff: its name, its header lines, its hunks (none: a binary file),
/// and whether the change deletes it.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffFile {
    pub file: String,
    pub head: Vec<String>,
    pub hunks: Vec<Hunk>,
    pub deleted: bool,
}

/// The diff from base to commit, as files (diff_of).
pub fn diff_files(wt: &str, base: &str, commit: &str) -> Res<Vec<DiffFile>> {
    let args = [
        "-c",
        "core.quotePath=false",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        base,
        commit,
    ];
    Ok(diff_of(&git(wt, &args, true)?.stdout))
}

/// A diff's text as files, each with its header lines and hunks.
pub fn diff_of(text: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut texts: Vec<Vec<Vec<&str>>> = Vec::new(); // each file's hunks' lines
    for line in text.split('\n') {
        if line.starts_with("diff --git ") {
            files.push(DiffFile {
                file: pair_name(line),
                head: vec![line.into()],
                hunks: Vec::new(),
                deleted: false,
            });
            texts.push(Vec::new());
            continue;
        }
        let (Some(f), Some(lines)) = (files.last_mut(), texts.last_mut()) else {
            continue;
        };
        if let Some((old, new)) = hunk_head(line) {
            f.hunks.push(Hunk {
                old,
                new,
                text: String::new(),
            });
            lines.push(vec![line]);
        } else if let Some(h) = lines.last_mut() {
            h.push(line);
        } else {
            f.head.push(line.into());
            if line.starts_with("+++ ") {
                if let Some(p) = git_path(line, "b/").filter(|p| !p.is_empty()) {
                    f.file = p;
                }
            } else if let Some(p) = line.strip_prefix("rename to ") {
                f.file = p.into();
            } else if line.starts_with("deleted file") {
                f.deleted = true;
            }
        }
    }
    for (f, lines) in files.iter_mut().zip(texts) {
        for (h, mut lines) in f.hunks.iter_mut().zip(lines) {
            while lines.last() == Some(&"") {
                lines.pop(); // the diff's last newline
            }
            h.text = lines.join("\n");
        }
    }
    files
}

/// The name in a `diff --git a/x b/x` line: x when both halves match, else all after a/.
fn pair_name(line: &str) -> String {
    let s: Vec<char> = line.chars().skip("diff --git a/".len()).collect();
    if s.len() >= 3 {
        let half = (s.len() - 3) / 2;
        if s[half..half + 3] == [' ', 'b', '/'] {
            return s[..half].iter().collect();
        }
    }
    s.into_iter().collect()
}

/// A hunk's `@@ -o,oc +n,nc @@` line: its old and new (start, count), a count of 1 when
/// it isn't given. None for any other line.
pub fn hunk_head(line: &str) -> Option<((i64, i64), (i64, i64))> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = range(rest)?;
    let rest = rest.strip_prefix(" +")?;
    let (new, rest) = range(rest)?;
    rest.starts_with(" @@").then_some((old, new))
}

/// "12,3" or "12" at the start of text: (start, count), and the rest.
fn range(text: &str) -> Option<((i64, i64), &str)> {
    let (start, rest) = digits(text)?;
    match rest.strip_prefix(',') {
        Some(after) => {
            let (count, rest) = digits(after)?;
            Some(((start, count), rest))
        }
        None => Some(((start, 1), rest)),
    }
}

fn digits(text: &str) -> Option<(i64, &str)> {
    let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let n = text[..end].parse().ok()?;
    Some((n, &text[end..]))
}

/// The path in a `--- a/x` or `+++ b/x` line, or None for /dev/null.
fn git_path(line: &str, prefix: &str) -> Option<String> {
    let p = line.get(4..).unwrap_or("").trim_end_matches('\t');
    p.strip_prefix(prefix).map(String::from)
}

/// A (start, count) as the lines it spans, first and last (a count of 0 spans one).
pub fn span((start, count): (i64, i64)) -> (i64, i64) {
    let start = start.max(1);
    (start, start + count.max(1) - 1)
}

/// The lines a hunk covers, as the tour names them: in the new file, or in the old
/// one for a deleted file.
pub fn lines_of(f: &DiffFile, h: &Hunk) -> (i64, i64) {
    span(if f.deleted { h.old } else { h.new })
}

/// A tour's hunk reference's lines, [start, end], or None for the whole file.
pub fn ref_range(r: &Obj) -> Option<(i64, i64)> {
    let lines = r.get("lines")?.as_array().filter(|l| !l.is_empty())?;
    let n = |i: usize| lines.get(i).and_then(Value::as_i64).unwrap_or(0);
    Some((n(0), n(1)))
}

/// Does a tour's hunk reference {"file", "lines"} cover hunk h of file f?
pub fn covers(r: &Obj, f: &DiffFile, h: &Hunk) -> bool {
    if s(r, "file") != f.file {
        return false;
    }
    let Some((a, b)) = ref_range(r) else {
        return true; // the whole file
    };
    let mut sides = Vec::new();
    if !f.deleted {
        sides.push(h.new);
    }
    if f.deleted || h.new.1 == 0 {
        sides.push(h.old); // lines it only removes are named by their old numbers
    }
    sides.into_iter().map(span).any(|(lo, hi)| a <= hi && lo <= b)
}

// ---------- its state ----------

/// The tour wasn't written: a line on the board and in the log. No news, no triage.
pub fn tour_failed(repo: &Repo, tid: &str, why: &str) -> Res<()> {
    let t = update_task(repo, tid, |t| {
        t.insert("tour".into(), json!({"state": "failed", "at": iso_now(), "why": why}));
        t.insert("note".into(), json!(format!("tour failed: {why}")));
        true
    })?;
    append(&tour_log(repo, &t), &format!("{} failed: {why}\n", iso_now()))?;
    Ok(())
}

/// Is a process with this pid running (or there, but not ours to signal)?
fn pid_alive(pid: Option<&Value>) -> bool {
    let Some(pid) = pid.and_then(Value::as_i64).and_then(|p| i32::try_from(p).ok()) else {
        return false;
    };
    // SAFETY: signal 0 only checks that the process exists
    unsafe { libc::kill(pid, 0) == 0 }
}

/// What the tour is: "writing", "failed" (why), "current", "stale" (why) or "none",
/// with the tour when there is one. A tour being written whose writer is gone has failed.
pub fn tour_state(repo: &Repo, t: &mut Obj) -> Res<(&'static str, Option<Obj>, Option<String>)> {
    if s(obj(t, "tour"), "state") == "writing" {
        if pid_alive(obj(t, "tour").get("pid")) {
            return Ok(("writing", None, None));
        }
        tour_failed(repo, s(t, "id"), "its writer stopped before the tour was written")?;
        let fresh = repo.task(s(t, "id"))?;
        t.extend(fresh);
    }
    let run = obj(t, "tour");
    if s(run, "state") == "failed" {
        let why = run.get("why").map(json::py_str);
        return Ok(("failed", read_obj(&tour_path(repo, t))?, why));
    }
    let Some(tour) = read_obj(&tour_path(repo, t))?.filter(|o| !o.is_empty()) else {
        return Ok(("none", None, None));
    };
    Ok(match tour_stale(repo, t, &tour)? {
        Some(why) => ("stale", Some(tour), Some(why)),
        None => ("current", Some(tour), None),
    })
}

/// Why the tour no longer shows the task's code, or None while it does: the same
/// commit, or the same patch (after a clean rebase, as approvals carry over).
pub fn tour_stale(repo: &Repo, t: &Obj, tour: &Obj) -> Res<Option<String>> {
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).is_dir() {
        return Ok(None); // nothing to compare with: the tour is all there is
    }
    let commit = s(tour, "commit");
    if head(wt)? == commit {
        return Ok(None);
    }
    let patch = patch_id(wt, &task_base_ref(repo, t)?)?;
    if patch.map_or(Value::Null, Value::from) == tour.get("patch").cloned().unwrap_or(Value::Null) {
        return Ok(None);
    }
    let short: String = commit.chars().take(12).collect();
    Ok(Some(format!(
        "the code changed since the tour was written (on {short})"
    )))
}

fn has_commit(wt: &str, c: &str) -> Res<bool> {
    Ok(!c.is_empty() && git(wt, &["cat-file", "-e", &format!("{c}^{{commit}}")], false)?.ok())
}

/// The diff the tour was written on: from its own commits, else (they're gone after a
/// rebase) from the task's code now.
pub fn tour_diff(repo: &Repo, t: &Obj, tour: &Obj) -> Res<Vec<DiffFile>> {
    let wt = s(t, "worktree");
    let (commit, base) = (s(tour, "commit"), s(tour, "base"));
    if has_commit(wt, commit)? && has_commit(wt, base)? {
        return diff_files(wt, base, commit);
    }
    let base_ref = task_base_ref(repo, t)?;
    let base = merge_base(wt, &base_ref)?.unwrap_or_default();
    diff_files(wt, &base, &head(wt)?)
}

/// A step's hunks from git, as a patch: each file the step names, in the step's order,
/// with its header and the hunks the step covers.
pub fn step_diff(files: &[DiffFile], step: &Obj) -> String {
    let refs: Vec<&Obj> = refs_of(step);
    let mut names: Vec<&str> = Vec::new();
    for r in &refs {
        if !names.contains(&s(r, "file")) {
            names.push(s(r, "file"));
        }
    }
    let mut out = Vec::new();
    for name in names {
        for f in files.iter().filter(|f| f.file == name) {
            let mine: Vec<&&Obj> = refs.iter().filter(|r| s(r, "file") == name).collect();
            let hunks: Vec<&str> = f
                .hunks
                .iter()
                .filter(|h| mine.iter().any(|r| covers(r, f, h)))
                .map(|h| h.text.as_str())
                .collect();
            if !hunks.is_empty() || f.hunks.is_empty() {
                let mut part: Vec<&str> = f.head.iter().map(String::as_str).collect();
                part.extend(hunks);
                out.push(part.join("\n"));
            }
        }
    }
    let text = out.join("\n");
    if out.is_empty() {
        text
    } else {
        text + "\n"
    }
}

/// A step's (or a comment's) hunk references.
fn refs_of(step: &Obj) -> Vec<&Obj> {
    let hunks = step.get("hunks").and_then(Value::as_array);
    hunks.into_iter().flatten().filter_map(Value::as_object).collect()
}

/// "bin/bullpen 120-140, README.md": a step's files and lines, for people.
pub fn step_files(step: &Obj) -> String {
    let named: Vec<String> = refs_of(step)
        .into_iter()
        .map(|r| match ref_range(r) {
            Some((a, b)) => format!("{} {a}-{b}", s(r, "file")),
            None => s(r, "file").to_string(),
        })
        .collect();
    if named.is_empty() {
        "(no code)".into()
    } else {
        named.join(", ")
    }
}

#[cfg(test)]
#[path = "tests/test_tours.rs"]
pub mod tests;
