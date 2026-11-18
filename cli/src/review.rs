//! Reviews: what a review covers, starting a reviewer, what `ready`
//! does with review on, and the coordinator's `review` (a new reviewer in place of one
//! that stopped without a verdict). What a review covers: `patch_id`, `own_lines`,
//! `rebase_check`, `keep_approval`; the kind of a first review; `start_review` and
//! `close_reviewer`; what `ready` does: `review_at_ready`.
//!
//! A review records what it looked at: the commit, the base it was on (merge-base), a
//! patch-id of the task's diff against that base, and the task's own lines (own_lines:
//! what it added and removed, file by file, without context or line numbers). An
//! approval holds while the code doesn't change: the same commit, the same patch-id
//! (reworded commits), or the same own lines after a rebase (only the base moved, even
//! where it changed the context, or where a conflict was resolved by keeping both
//! sides). After a rebase that conflicted, own lines that changed only in the files that
//! conflicted get a rebase check (a short review of the resolution, which doesn't count
//! as a round); a change anywhere else gets a review after a rebase (rebased: only that
//! the change still fits, as there are no findings to check).

use crate::agents::{pane, running};
use crate::args::{pos, Arg, Args, Nargs};
use crate::coordinators::session_for;
use crate::git_names::{unquote, Changed};
use crate::modes::task_base_ref;
use crate::rex::{review_label, rex_close};
use crate::rules::task_rules;
use crate::saves::save;
use crate::start::launch::py_format;
use bullpen_core::json::{get, obj, py_str, s, truthy, Obj};
use bullpen_core::paths::{home, prompts, self_bin};
use bullpen_core::proc::{git, head, merge_base, resolve, run_input};
use bullpen_core::repo::{index_block, repo_root, unindex_block};
use bullpen_core::task::{is_closed, is_status, set_status, whose_turn, with_model};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

const REVIEW: &[Arg] = &[pos("id", Nargs::One, "the task, like BP-3")];

/// `review` (coordinator): a new reviewer on the task's current commit, in place of one
/// that stopped or exited without a verdict.
pub fn cmd_review(argv: &[String]) -> Res<i32> {
    let a = Args::parse("review", argv, REVIEW);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    let r = obj(&t, "review").clone();
    let has_wt = truthy(t.get("worktree"));
    if is_closed(s(&t, "status")) || !has_wt {
        let more = if has_wt { "" } else { ", with no worktree" };
        return fail(format!("{}{more}", is_status(&t)));
    }
    if r.is_empty() {
        return fail(format!("{id} has had no review: its worker starts one with `ready`"));
    }
    if truthy(r.get("verdict")) || whose_turn(&t) != Some("reviewer") {
        return fail(format!("{}, not waiting on a reviewer", is_status(&t)));
    }
    let rounds = rounds(&repo);
    if round_of(&r) >= rounds && s(&r, "kind") != "tour" {
        return fail(format!(
            "{id} has used its {rounds} review rounds: `bullpen ask {id}` the person"
        ));
    }
    let base_ref = task_base_ref(&repo, &t)?;
    start_review(&repo, &mut t, &base_ref, None)?;
    println!(
        "{id}: a new reviewer started (round {} of {rounds})",
        round_of(obj(&t, "review"))
    );
    Ok(0)
}

/// The repo's review_rounds.
pub fn rounds(repo: &Repo) -> i64 {
    repo.cfg.get("review_rounds").and_then(Value::as_i64).unwrap_or(0)
}

/// A review's round (0 for none).
fn round_of(r: &Obj) -> i64 {
    r.get("round").and_then(Value::as_i64).unwrap_or(0)
}

/// ready with review on: keep an approval that still covers the code, leave a reviewer
/// that is on this very commit, or start a review. Returns what to tell the worker.
pub fn review_at_ready(repo: &Repo, t: &mut Obj, base_ref: &str) -> Res<String> {
    let r = obj(t, "review").clone();
    if truthy(t.get("tour_sent")) {
        // the person sent comments back after a tour: a review checks them, whatever the code
        start_review(repo, t, base_ref, None)?;
        return Ok(
            "A reviewer checks that the person's comments are addressed now: end your turn, and \
                   you'll hear back if it asks for changes."
                .into(),
        );
    }
    let check = rebase_check(t, base_ref)?;
    if let Some(Check::Kept(kept)) = &check {
        keep_approval(t, base_ref)?;
        crate::become_ready::make_ready(repo, t, base_ref, crate::become_ready::How::Kept(kept))?;
        return Ok(format!(
            "Its review approval carries over ({kept}), so there's no new review: end your turn."
        ));
    }
    if s(t, "status") == "review"
        && truthy(r.get("block"))
        && r.get("commit").and_then(Value::as_str) == Some(head(s(t, "worktree"))?.as_str())
        && running(t, Some("reviewer"))?
    {
        save(repo, t)?;
        return Ok(
            "A reviewer is already checking this commit: end your turn, and you'll hear back \
                   if it asks for changes."
                .into(),
        );
    }
    let rebase = match check {
        Some(Check::Rebase(files)) => Some(files),
        _ => None,
    };
    start_review(repo, t, base_ref, rebase)?;
    Ok(format!(
        "A reviewer checks it now ({}): end your turn, and you'll hear back if it asks for changes.",
        review_kind(obj(t, "review"))
    ))
}

// ---------- what a review covers ----------

/// The stable patch-id of the branch's change against its base, or None if there's none.
pub fn patch_id(wt: &str, base_ref: &str) -> Res<Option<String>> {
    let diff = git(wt, &["diff", "--binary", &format!("{base_ref}...HEAD")], true)?.stdout;
    if diff.trim().is_empty() {
        return Ok(None);
    }
    let out = run_input(&["git", "-C", wt, "patch-id", "--stable"], &diff, true)?.stdout;
    Ok(out.split_whitespace().next().map(String::from))
}

/// The branch's own added and removed lines, file by file ({path: hash}), without
/// context or line numbers: what stays the same when only the base moved under it.
pub fn own_lines(wt: &str, base_ref: &str) -> Res<Obj> {
    let range = format!("{base_ref}...HEAD");
    let args = [
        "diff",
        "-U0",
        "--binary",
        "--no-color",
        "--no-ext-diff",
        "--no-renames",
        &range,
    ];
    let out = git(wt, &args, true)?.stdout;
    Ok(own_lines_of(&out))
}

/// own_lines from the diff's text.
pub fn own_lines_of(diff: &str) -> Obj {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut at: Option<usize> = None;
    for line in diff.split('\n') {
        if let Some(pair) = line.strip_prefix("diff --git ") {
            let path = header_path(pair);
            // a path seen again starts afresh, in its first place (a dict's key)
            at = Some(match files.iter().position(|(p, _)| *p == path) {
                Some(i) => {
                    files[i].1.clear();
                    i
                }
                None => {
                    files.push((path, Vec::new()));
                    files.len() - 1
                }
            });
        } else if let Some(i) = at {
            if !line.starts_with("index ") && !line.starts_with("@@") {
                files[i].1.extend_from_slice(line.as_bytes());
                files[i].1.push(b'\n');
            }
        }
    }
    files
        .into_iter()
        .map(|(p, bytes)| (p, json!(bullpen_core::sha1::hex(&bytes))))
        .collect()
}

/// The path of a patch's `diff --git` header: "a/<path> b/<path>", the same path twice,
/// or both quoted when git quotes the name (`"a/caf\303\251.md" "b/caf\303\251.md"`).
fn header_path(pair: &str) -> String {
    if pair.starts_with('"') {
        let (a, _) = unquote(pair);
        return a.strip_prefix("a/").unwrap_or(&a).to_string();
    }
    let chars: Vec<char> = pair.chars().collect();
    let n = chars.len().saturating_sub(5) / 2;
    chars.iter().skip(2).take(n).collect()
}

/// What the task's approval needs now.
#[derive(Debug, PartialEq)]
pub enum Check {
    /// it still covers the code: why, in a few words
    Kept(String),
    /// its own lines changed only in the files a rebase conflicted in: a rebase check
    /// looks at the resolution
    Rebase(Vec<String>),
}

/// What the task's approval needs now, or None (a normal review).
pub fn rebase_check(t: &Obj, base_ref: &str) -> Res<Option<Check>> {
    let r = obj(t, "review");
    let wt = s(t, "worktree");
    if s(r, "verdict") != "approved" || !truthy(r.get("patch")) || !truthy(t.get("worktree")) {
        return Ok(None);
    }
    if r.get("commit").and_then(Value::as_str) == Some(head(wt)?.as_str()) {
        return Ok(Some(Check::Kept("the same commit".into())));
    }
    let moved = merge_base(wt, base_ref)?.as_deref() != get(r, "base").and_then(Value::as_str);
    if !truthy(r.get("conflict")) && patch_id(wt, base_ref)?.as_deref() == r.get("patch").and_then(Value::as_str) {
        return Ok(Some(Check::Kept(if moved {
            format!("the same patch after a clean rebase onto {base_ref}")
        } else {
            "the same patch, in other commits".into()
        })));
    }
    let Some(Value::Object(before)) = get(r, "own") else {
        return Ok(None); // reviewed before bullpen kept own lines
    };
    let own = own_lines(wt, base_ref)?;
    let changed: BTreeSet<&String> = own
        .keys()
        .chain(before.keys())
        .filter(|p| own.get(*p) != before.get(*p))
        .collect();
    if changed.is_empty() {
        return Ok(Some(Check::Kept(
            if moved {
                "only the base moved"
            } else {
                "the same lines, in other commits"
            }
            .into(),
        )));
    }
    let conflicted: Vec<&str> = match r.get("conflict") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if moved && changed.iter().all(|p| conflicted.contains(&p.as_str())) {
        return Ok(Some(Check::Rebase(changed.into_iter().cloned().collect())));
    }
    Ok(None)
}

/// The approval now covers the task's current commit.
pub fn keep_approval(t: &mut Obj, base_ref: &str) -> Res<()> {
    let wt = s(t, "worktree").to_string();
    let (commit, base) = (head(&wt)?, merge_base(&wt, base_ref)?);
    let (patch, own) = (patch_id(&wt, base_ref)?, own_lines(&wt, base_ref)?);
    let Some(Value::Object(r)) = t.get_mut("review") else {
        return Ok(());
    };
    r.insert("commit".into(), json!(commit));
    r.insert("base".into(), json!(base));
    r.insert("patch".into(), json!(patch));
    r.insert("own".into(), Value::Object(own));
    r.shift_remove("conflict");
    r.shift_remove("conflict_diff");
    let kept = r.get("kept").and_then(Value::as_i64).unwrap_or(0);
    r.insert("kept".into(), json!(kept + 1));
    Ok(())
}

pub fn is_doc(path: &str) -> bool {
    path.starts_with("docs/") || path.starts_with("prompts/") || path.ends_with(".md")
}

/// Why a change to this path gets a full review however small: a prompt or the
/// repository's rules is what agents are told to do, so a wrong one changes behaviour.
fn full_reason(path: &str) -> Option<&'static str> {
    if path.starts_with("prompts/") {
        Some("changes a prompt")
    } else if path == crate::rules::RULES_FILE {
        Some("changes the rules")
    } else {
        None
    }
}

/// full_reason of the first such path among the changed files.
fn full_reason_of(files: &[Changed]) -> Option<&'static str> {
    files.iter().find_map(|f| full_reason(&f.path))
}

/// Why a later round is a full review, or None: the first prompt or rules path in the
/// task's own lines (`own`, {path: hash}) that no full review has read as it is now
/// (`covered`: those paths' lines as the last full review with a verdict read them).
pub fn uncovered_reason(own: &Obj, covered: &Obj) -> Option<&'static str> {
    own.iter()
        .filter(|(path, lines)| covered.get(*path) != Some(*lines))
        .find_map(|(path, _)| full_reason(path))
}

/// What a full review has read of the task's prompts and rules once the round `old` has
/// its verdict: `old`'s own lines in those paths when it was a full review, else what
/// the round before it knew. Null when bullpen can't tell (a review from before it kept
/// this), and then no later round is made full for it.
pub fn covered_after(old: &Obj) -> Value {
    if s(old, "kind") == "full" {
        return match old.get("own") {
            Some(Value::Object(own)) => {
                let read = own.iter().filter(|(path, _)| full_reason(path).is_some());
                Value::Object(read.map(|(p, h)| (p.clone(), h.clone())).collect())
            }
            _ => Value::Null,
        };
    }
    match old.get("prev") {
        Some(Value::Object(prev)) => prev.get("covered").cloned().unwrap_or(Value::Null),
        _ => json!({}),
    }
}

/// uncovered_reason for the task's branch now, after the round `prev`. A light review
/// set for the task (`--review light`) still wins: None.
fn later_full_reason(t: &Obj, base_ref: &str, prev: &Obj) -> Res<Option<&'static str>> {
    let Some(Value::Object(covered)) = prev.get("covered") else {
        return Ok(None);
    };
    if s(t, "review_mode") == "light" {
        return Ok(None);
    }
    Ok(uncovered_reason(&own_lines(s(t, "worktree"), base_ref)?, covered))
}

/// The first review's kind and why for a change, from its changed files
/// (git_names::numstat): full if it touches a prompt or the rules, else by its size.
pub fn kind_of_change(changed: &[Changed], limit: i64) -> (String, Option<String>) {
    if let Some(why) = full_reason_of(changed) {
        return ("full".into(), Some(why.into()));
    }
    let (files, lines, docs) = change_size_of(changed);
    kind_by_size(files, lines, docs, limit)
}

/// change_size from the changed files (git_names::numstat).
pub fn change_size_of(changed: &[Changed]) -> (i64, Option<i64>, bool) {
    let (mut files, mut lines, mut docs) = (0, Some(0), true);
    for f in changed {
        files += 1;
        lines = lines.zip(f.lines).map(|(n, m)| n + m);
        docs = docs && is_doc(&f.path);
    }
    (files, lines, docs && files > 0)
}

/// (kind, why) for a task's first review: "full" or "light", by its own review mode,
/// else by what the change touches and its size. A task made from an issue: full, always.
fn first_kind(repo: &Repo, t: &Obj, base_ref: &str) -> Res<(String, Option<String>)> {
    if s(t, "origin") == "issue" {
        return Ok(("full".into(), Some("from an issue".into())));
    }
    let mode = match s(t, "review_mode") {
        "" => "auto",
        m => m,
    };
    if mode != "auto" {
        return Ok((mode.into(), Some(format!("set for {}", s(t, "id")))));
    }
    let range = format!("{base_ref}...HEAD");
    let changed = crate::git_names::numstat(s(t, "worktree"), &range)?;
    let limit = repo.cfg.get("light_review_lines").and_then(Value::as_i64).unwrap_or(0);
    Ok(kind_of_change(&changed, limit))
}

/// The first review's kind for a change of this size.
pub fn kind_by_size(files: i64, lines: Option<i64>, docs: bool, limit: i64) -> (String, Option<String>) {
    if docs {
        return ("light".into(), Some("docs only".into()));
    }
    match lines {
        Some(n) if n <= limit && files <= 2 => {
            let (ls, fs) = (plural(n), plural(files));
            ("light".into(), Some(format!("{n} line{ls} in {files} file{fs}")))
        }
        _ => ("full".into(), None),
    }
}

fn plural(n: i64) -> &'static str {
    if n != 1 {
        "s"
    } else {
        ""
    }
}

/// "full review", "full review (changes a prompt)", "light review (docs only)",
/// "follow-up review".
pub fn review_kind(r: &Obj) -> String {
    let word = match s(r, "kind") {
        "" => return "review".into(),
        "full" => "full review",
        "light" => "light review",
        "followup" => "follow-up review",
        "tour" => "review of the person's comments",
        "rebase" => "rebase check",
        "rebased" => "review after a rebase",
        other => other,
    };
    match r.get("why").filter(|w| truthy(Some(w))) {
        Some(why) => format!("{word} ({})", py_str(why)),
        None => word.into(),
    }
}

/// The reviewer's model for a review of this kind, from the config.
fn review_model(repo: &Repo, kind: &str) -> Option<String> {
    let full = get(&repo.cfg, "reviewer_model");
    let m = if kind == "full" {
        full
    } else {
        repo.cfg
            .get("reviewer_followup_model")
            .filter(|v| truthy(Some(v)))
            .or(full)
    };
    m.map(py_str)
}

/// Longer, and a follow-up's brief gives the command instead.
const MAX_DIFF_LINES: usize = 800;

/// (command, diff) for what changed since the reviewed commit prev["commit"]: a plain
/// diff when it's still in the branch's history, else (a rebase) a range-diff.
fn since_last(wt: &str, base_ref: &str, prev: &Obj) -> Res<(String, String)> {
    let c = py_str(prev.get("commit").unwrap_or(&Value::Null));
    let short = |x: &str| x.chars().take(12).collect::<String>();
    let args: Vec<String> = if git(wt, &["merge-base", "--is-ancestor", &c, "HEAD"], false)?.ok() {
        vec!["diff".into(), format!("{}..HEAD", short(&c))]
    } else {
        let base = prev.get("base").filter(|v| truthy(Some(v))).map_or(c.clone(), py_str);
        vec![
            "range-diff".into(),
            format!("{}..{}", short(&base), short(&c)),
            format!("{base_ref}..HEAD"),
        ]
    };
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let p = git(wt, &argv, false)?;
    let text = if p.ok() { p.stdout.as_str() } else { "" };
    let cmd = format!("git -C {wt} {}", args.join(" "));
    if text.trim().is_empty() {
        let said = if p.ok() {
            "(no changes)"
        } else {
            "(git couldn't show it: run the command)"
        };
        return Ok((cmd, said.into()));
    }
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    if lines.len() > MAX_DIFF_LINES {
        let said = format!("({} lines, too long to include: run the command)", lines.len());
        return Ok((cmd, said));
    }
    Ok((cmd, lines.join("\n")))
}

/// A diff for a reviewer's brief, or what to run when it's empty or too long.
pub fn clipped_diff(text: &str, cmd: &str) -> String {
    if text.trim().is_empty() {
        return "(no changes)".into();
    }
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    if lines.len() > MAX_DIFF_LINES {
        return format!("({} lines, too long to include: run `{cmd}`)", lines.len());
    }
    lines.join("\n")
}

/// The conflicts a rebase met, as saved when it stopped, for a rebase check.
fn conflict_text(path: &str) -> Res<String> {
    let f = Path::new(path);
    if path.is_empty() || !f.is_file() {
        return Ok("(not saved: the range-diff below shows how they were resolved)".into());
    }
    Ok(clipped_diff(&read_text(f)?, &format!("cat {path}")))
}

/// A file's text, as Path.read_text() reads it (universal newlines).
fn read_text(path: &Path) -> Res<String> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// The branch's change against its base in a few words: "3 files, +120 −14".
pub fn shortstat(wt: &str, base_ref: &str) -> Res<Option<String>> {
    let range = format!("{base_ref}...HEAD");
    Ok(shortstat_of(&git(wt, &["diff", "--shortstat", &range], true)?.stdout))
}

/// shortstat from `git diff --shortstat`'s line: each "<n> file", "<n> insertion" and
/// "<n> deletion" in it (the last of each, as a dict keeps it).
pub fn shortstat_of(out: &str) -> Option<String> {
    let (mut files, mut ins, mut del) = (None, 0, 0);
    let words: Vec<&str> = out.split(' ').collect();
    for pair in words.windows(2) {
        let digits: String = pair[0].chars().rev().take_while(char::is_ascii_digit).collect();
        let Ok(n) = digits.chars().rev().collect::<String>().parse::<i64>() else {
            continue;
        };
        if pair[1].starts_with("file") {
            files = Some(n);
        } else if pair[1].starts_with("insertion") {
            ins = n;
        } else if pair[1].starts_with("deletion") {
            del = n;
        }
    }
    let files = files.filter(|n| *n != 0)?;
    Some(format!("{files} file{}, +{ins} −{del}", plural(files)))
}

/// Start a reviewer agent on the task's branch, in its own capsule. The first round is
/// a full review, or a light one for a small change; later rounds follow up on the last
/// verdict: its findings and what changed since, or are a full review when the change
/// now touches a prompt or the rules in a way no full review has read. Comments the person sent back after a
/// tour (tour_sent) get a review of their own, and an approved change whose own lines
/// changed only where a rebase conflicted (rebase: those files) a rebase check of the
/// resolution. Both keep the round they follow: they don't count toward review_rounds.
/// A round after an approval that only a rebase sent back (rebased) checks only the
/// fit onto the new base; it counts as a round.
pub fn start_review(repo: &Repo, t: &mut Obj, base_ref: &str, rebase: Option<Vec<String>>) -> Res<()> {
    let old = obj(t, "review").clone();
    close_reviewer(t)?;
    let sent = t
        .get("tour_sent")
        .filter(|v| truthy(Some(v)))
        .and_then(Value::as_object)
        .cloned();
    let rebase = if sent.is_some() { None } else { rebase };
    // a round a change of the brief stopped (brief.rs) is run again, not counted
    let again = sent.is_some() || rebase.is_some() || truthy(old.get("void"));
    let n = round_of(&old) + if again { 0 } else { 1 };
    let id = s(t, "id").to_string();
    let briefs = repo.dir.join("briefs");
    let wt = s(t, "worktree").to_string();
    let scratch = scratch_dir(repo, &id);
    std::fs::create_dir_all(&scratch)?;
    // the round the next one follows up on: the last one with a verdict
    let mut prev: Option<Obj> = if truthy(old.get("verdict")) {
        let keys = ["round", "commit", "base", "findings", "verdict", "brief"];
        let mut p: Obj = keys
            .iter()
            .map(|k| (k.to_string(), old.get(*k).cloned().unwrap_or(Value::Null)))
            .collect();
        p.insert("covered".into(), covered_after(&old));
        Some(p)
    } else {
        old.get("prev").and_then(Value::as_object).cloned()
    };
    let (kind, why) = if sent.is_some() {
        ("tour".to_string(), None)
    } else if rebase.is_some() {
        ("rebase".to_string(), None)
    } else if let Some(p) = prev.as_ref().filter(|p| truthy(p.get("commit"))) {
        match later_full_reason(t, base_ref, p)? {
            Some(why) => ("full".to_string(), Some(why.to_string())),
            None => (followup_kind(repo, &id, &wt, base_ref, p)?.to_string(), None),
        }
    } else {
        prev = None;
        first_kind(repo, t, base_ref)?
    };
    let summary = t
        .get("summary")
        .filter(|v| truthy(Some(v)))
        .map_or("(none)".into(), py_str);
    let mut vals: Vec<(&str, String)> = vec![
        ("id", id.clone()),
        ("title", s(t, "title").into()),
        ("round", n.to_string()),
        ("rounds", rounds(repo).to_string()),
        ("of_rounds", of_rounds(n, rounds(repo), &kind)),
        ("brief", briefs.join(format!("{id}.md")).display().to_string()),
        ("worktree", wt.clone()),
        ("branch", py_str(t.get("branch").unwrap_or(&Value::Null))),
        ("base_ref", base_ref.into()),
        ("bin", self_bin().display().to_string()),
        ("scratch", scratch.display().to_string()),
        ("root", home().display().to_string()),
        ("summary", summary),
        ("rules", review_rules(repo, t)?),
        ("why", why.clone().unwrap_or_default()),
        ("brief_changed", crate::brief::changed_since(t, prev.as_ref())),
    ];
    if let Some(sent) = &sent {
        let (since, diff) = since_last(&wt, base_ref, sent)?;
        let file = py_str(sent.get("file").unwrap_or(&Value::Null));
        let f = Path::new(&file);
        let comments = if f.is_file() {
            read_text(f)?.trim().to_string()
        } else {
            "(the file is gone)".into()
        };
        vals.extend([
            ("since", since),
            ("diff", diff),
            ("comments_file", file.clone()),
            ("count", py_str(sent.get("count").unwrap_or(&Value::Null))),
            ("comments", comments),
        ]);
    } else if let Some(prev) = &prev {
        let (since, diff) = since_last(&wt, base_ref, prev)?;
        let file = prev.get("findings").filter(|v| truthy(Some(v))).map(py_str);
        let findings = match &file {
            Some(f) if Path::new(f).is_file() => read_text(Path::new(f))?.trim().to_string(),
            _ => "(none)".into(),
        };
        let approved = s(prev, "verdict") == "approved";
        // a round that asked for changes: its notes went to the coordinator, not to be fixed
        let findings = if approved {
            findings
        } else {
            crate::notes::without_notes(&findings)
        };
        let said = if approved {
            "approved the change. Its notes"
        } else {
            "asked for changes. Its findings"
        };
        vals.extend([
            ("prev_round", py_str(prev.get("round").unwrap_or(&Value::Null))),
            ("since", since),
            ("diff", diff),
            ("findings_file", file.unwrap_or_else(|| "(none)".into())),
            ("findings", findings),
            ("prev_said", said.into()),
        ]);
    }
    if let Some(files) = &rebase {
        let range = format!("{base_ref}...HEAD");
        let mut args = vec!["diff", range.as_str(), "--"];
        args.extend(files.iter().map(String::as_str));
        let now = git(&wt, &args, false)?.stdout;
        let cmd = format!("git -C {wt} diff {range} -- {}", bullpen_core::text::shell_join(files));
        let quoted: Vec<String> = files
            .iter()
            .map(|f| format!("`{}`", crate::git_names::shown(f)))
            .collect();
        vals.extend([
            ("conflict_files", quoted.join(", ")),
            ("conflicts", conflict_text(s(&old, "conflict_diff"))?),
            ("now", clipped_diff(&now, &cmd)),
        ]);
    }
    let mut look = py_format(&read_text(&prompts().join(format!("review-{kind}.md")))?, &vals);
    if kind == "full" && prev.is_some() {
        // a later round made full: the last round's findings still want checking
        look += "\n";
        look += &py_format(&read_text(&prompts().join("review-full-later.md"))?, &vals);
    }
    let name = match (&sent, &rebase) {
        (Some(sent), _) => format!("{id}.tour-{}.review.md", py_str(sent.get("n").unwrap_or(&Value::Null))),
        (None, Some(_)) => format!("{id}.review-{n}.rebase.md"),
        _ => format!("{id}.review-{n}.md"),
    };
    let prompt = briefs.join(name);
    let mut kind_vals = vals;
    let mut said_kind = Obj::new();
    said_kind.insert("kind".into(), json!(kind));
    said_kind.insert("why".into(), json!(why));
    kind_vals.extend([("kind", review_kind(&said_kind)), ("look", look)]);
    let text = py_format(&read_text(&prompts().join("reviewer.md"))?, &kind_vals);
    std::fs::write(&prompt, text)?;
    let first = format!(
        "You are the reviewer for bullpen task {id}. Read {} and follow it.",
        prompt.display()
    );
    let model = review_model(repo, &kind);
    let reviewer = bullpen_core::json::strs(&repo.cfg, "reviewer");
    let mut argv = resolve(&with_model(&reviewer, model.as_deref()));
    argv.extend(["-n".into(), format!("{id} review"), first]);
    let session = match s(t, "session") {
        "" => session_for(repo, None)?,
        own => own.to_string(),
    };
    let (block, at) = crate::panes::place(repo, t, &session, &review_label(t), &argv, "reviewer", true, false)?;
    index_block(&block, repo, &format!("{id} review"))?;
    set_status(t, "review", None, None);
    let mut r = Obj::new();
    r.insert("round".into(), json!(n));
    r.insert("block".into(), json!(block));
    r.insert("session".into(), json!(session));
    r.insert("pane".into(), json!(at));
    r.insert("verdict".into(), Value::Null);
    r.insert("findings".into(), Value::Null);
    r.insert("kind".into(), json!(kind));
    r.insert("why".into(), json!(why));
    r.insert("model".into(), json!(model));
    r.insert("prev".into(), prev.map_or(Value::Null, Value::Object));
    r.insert("scratch".into(), json!(scratch.display().to_string()));
    r.insert("brief".into(), json!(crate::brief::count(t))); // the changes of the brief this round sees
    if let Some(sent) = &sent {
        r.insert("tour".into(), sent.get("n").cloned().unwrap_or(Value::Null));
    }
    if let Some(files) = &rebase {
        r.insert("rebase".into(), json!(files));
    }
    r.insert("commit".into(), json!(head(&wt)?));
    r.insert("base".into(), json!(merge_base(&wt, base_ref)?));
    r.insert("patch".into(), json!(patch_id(&wt, base_ref)?));
    r.insert("own".into(), Value::Object(own_lines(&wt, base_ref)?));
    t.insert("review".into(), Value::Object(r));
    save(repo, t)
}

/// The kind of a round after one with a verdict: "rebased" when that round approved,
/// the branch has been rebased since (its base moved) and nothing but a rebase sent it
/// back to work, so there are no findings to check, only the fit onto the new base;
/// "followup" otherwise (the person or the PR asked for more, say).
fn followup_kind(repo: &Repo, id: &str, wt: &str, base_ref: &str, prev: &Obj) -> Res<&'static str> {
    let base = s(prev, "base");
    let moved = !base.is_empty() && merge_base(wt, base_ref)?.as_deref() != Some(base);
    let only_rebased =
        || bullpen_core::timeline::read(&repo.dir, id).is_some_and(|events| only_rebase_since_approval(&events));
    Ok(if s(prev, "verdict") == "approved" && moved && only_rebased() {
        "rebased"
    } else {
        "followup"
    })
}

/// Whether every send-back to work since the task's last approval was for a rebase (or
/// the usage limit, which only continues the same work), as its timeline says.
pub fn only_rebase_since_approval(events: &[Obj]) -> bool {
    let approved = |e: &Obj| s(e, "event") == "review_finished" && s(e, "verdict") == "approved";
    let Some(at) = events.iter().rposition(approved) else {
        return false;
    };
    events[at + 1..]
        .iter()
        .filter(|e| s(e, "event") == "status")
        .all(|e| matches!(s(e, "back"), "" | "rebase" | "limit"))
}

/// " of 2" for the header of a reviewer's brief, or "" when the round is past the
/// limit or doesn't count toward it (a rebase check, a tour's review).
pub fn of_rounds(n: i64, rounds: i64, kind: &str) -> String {
    if matches!(kind, "rebase" | "tour") || n > rounds {
        return String::new();
    }
    format!(" of {rounds}")
}

/// The rules section of a reviewer's brief, or "".
fn review_rules(repo: &Repo, t: &Obj) -> Res<String> {
    let rules = task_rules(repo, t)?;
    if rules.is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "## Rules for this repository\n\nThe worker was given these rules too. Check that the work follows \
         them.\n\n{rules}\n\n"
    ))
}

/// The folder a task's reviewer keeps its own files in, outside the worktree.
pub fn scratch_dir(repo: &Repo, id: &str) -> std::path::PathBuf {
    repo.dir.join("scratch").join(format!("{id}-review"))
}

/// Remove the review's scratch folder, if it made one (only a folder in a `scratch` folder).
pub fn remove_scratch(r: &Obj) -> Res<()> {
    let dir = Path::new(s(r, "scratch"));
    let ours = dir.parent().and_then(Path::file_name).is_some_and(|n| n == "scratch");
    if ours && dir.is_dir() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Close the task's reviewer pane, if it has one, and remove its scratch folder.
pub fn close_reviewer(t: &mut Obj) -> Res<()> {
    remove_scratch(obj(t, "review"))?;
    let block = s(obj(t, "review"), "block").to_string();
    if !truthy(obj(t, "review").get("block")) {
        return Ok(());
    }
    unindex_block(&block)?;
    let (session, block) = pane(t, "reviewer");
    rex_close(&session, &block)?;
    if let Some(Value::Object(r)) = t.get_mut("review") {
        r.insert("block".into(), Value::Null);
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/test_review.rs"]
mod tests;
