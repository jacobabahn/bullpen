//! The step into `ready` (`make_ready`): what every path into it does, the way the
//! task became ready (`How`) deciding the parts that differ. The risk label, the
//! status, the save, then auto-accept or the news that it is ready. A branch with no
//! commits is recorded here as nothing to merge ("nothing_to_merge", which core's
//! `nothing_to_merge` reads). Its callers keep only what is theirs: publishing the
//! branch, the review record, the findings file. An approval by a verdict is checked
//! against its base first when accepts left it in review (`left_in_review`).

use crate::accept::left_in_review;
use crate::auto_accept::start_auto_accept;
use crate::news::ready_news;
use crate::risk::{assess, Said};
use crate::saves::save;
use bullpen_core::json::{s, Obj};
use bullpen_core::task::{auto_accepts, commits, set_status, where_};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// How a task became ready.
pub enum How<'a> {
    /// `ready` with review off: the rules alone label it, and it waits for the person
    /// (auto-accept takes only an approval)
    Plain,
    /// `ready` with review on, an approval kept as the code is as approved: what kept it
    /// ("the same commit")
    Kept(&'a str),
    /// A ready task whose worker took a turn and left the code as approved (`ready_again`):
    /// what kept its approval. It auto-accepts as a kept approval at `ready` does.
    Again(&'a str),
    /// A ready task rebased after an accept with its approval kept (`rebase_task`).
    /// Its news is the accept's `After BP-3: ...` line, and it doesn't auto-accept
    /// (it was ready before the rebase).
    Rebased,
    /// `ready` on a branch with no commits: no label, and nothing to merge
    Nothing,
    /// A reviewer's approve verdict: its text (`text`, which the label reads), what the
    /// news says of it (`said`), and the accepts that left it in review (`left_by`)
    Approved {
        text: &'a str,
        said: String,
        left_by: &'a [String],
    },
    /// A verdict that still asks for changes after the last round: the rounds, and where
    /// its findings are. It waits for the person.
    OutOfRounds { rounds: i64, findings: &'a str },
}

/// The task is ready now, by `how`: its risk label, its status, saved, then auto-accept
/// (an approval with auto-accept on) or the ready news.
pub fn make_ready(repo: &Repo, t: &mut Obj, base_ref: &str, how: How) -> Res<()> {
    match &how {
        How::Plain | How::OutOfRounds { .. } => assess(repo, t, base_ref, Said::Nothing)?,
        How::Kept(_) | How::Again(_) | How::Rebased => assess(repo, t, base_ref, Said::Kept)?,
        How::Approved { text, .. } => assess(repo, t, base_ref, Said::Approval(text))?,
        How::Nothing => {
            t.insert("risk".into(), Value::Null);
        }
    }
    let why = match &how {
        How::Nothing => Some("Nothing to merge (no commits)".to_string()),
        How::OutOfRounds { rounds, .. } => Some(format!("Review still asks for changes after {rounds} rounds")),
        _ => None,
    };
    set_status(t, "ready", why.as_deref(), None);
    if matches!(how, How::Nothing) {
        t.insert("nothing_to_merge".into(), json!(true)); // what core's nothing_to_merge reads
    } else {
        t.shift_remove("nothing_to_merge");
    }
    save(repo, t)?;
    if let How::Approved { left_by, .. } = &how {
        if !left_in_review::after_approval(repo, t, left_by)? {
            return Ok(()); // sent back to rebase, or reviewed again: its news said so
        }
    }
    if let Some(wait) = auto_accept_wait(&how) {
        if auto_accepts(repo, t) {
            let id = s(t, "id").to_string();
            start_auto_accept(repo, &id, wait);
            return Ok(());
        }
    }
    let n = t.get("commits").and_then(Value::as_i64).unwrap_or(0);
    let again = matches!(how, How::Again(_));
    let (facts, then) = match how {
        How::Plain => (vec![commits(n), where_(t)], String::new()),
        How::Kept(kept) => (
            vec![commits(n), where_(t), format!("its approval carried over: {kept}")],
            String::new(),
        ),
        How::Again(kept) => (
            vec![format!("its worker's turn left the code as approved: {kept}")],
            format!(". `bullpen peek {}` if it said something", s(t, "id")),
        ),
        How::Nothing => (vec!["nothing to merge".into()], String::new()),
        How::Rebased => return Ok(()), // the accept's `After BP-3: ...` line says it
        How::Approved { said, .. } => (vec![commits(n), where_(t), said], String::new()),
        How::OutOfRounds { rounds, findings } => (
            vec![where_(t)],
            format!(", but review still asks for changes after {rounds} rounds: findings in {findings}"),
        ),
    };
    ready_news(repo, t, &facts, &then, again)
}

/// Whether this way into ready can auto-accept (auto-accept takes only an approval), and
/// if so whether to wait for the job: the verdict does, as it closes the reviewer's pane
/// only after; a `ready` doesn't, as it runs in the worker's pane, which accepting closes,
/// and nor does a turn's end (`ready_again`), which the watcher's event runs.
fn auto_accept_wait(how: &How) -> Option<bool> {
    match how {
        How::Approved { .. } => Some(true),
        How::Kept(_) | How::Again(_) => Some(false),
        How::Plain | How::Nothing | How::OutOfRounds { .. } | How::Rebased => None,
    }
}
