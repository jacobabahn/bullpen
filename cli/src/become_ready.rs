//! The step into `ready` (`make_ready`): what every path into it does, the way the
//! task became ready (`How`) deciding the parts that differ. The risk label, the
//! status, the save, then auto-accept or the news that it is ready. Its callers keep
//! only what is theirs: publishing the branch, the review record.

use crate::auto_accept::start_auto_accept;
use crate::news::ready_news;
use crate::risk::{assess, Said};
use crate::saves::save;
use bullpen_core::json::{s, Obj};
use bullpen_core::task::{auto_accepts, commits, set_status, where_};
use bullpen_core::{Repo, Res};
use serde_json::Value;

/// How a task became ready.
pub enum How<'a> {
    /// `ready` with review off: the rules alone label it, and it waits for the person
    /// (auto-accept takes only an approval)
    Plain,
    /// `ready` with review on, an approval kept as the code is as approved: what kept it
    /// ("the same commit")
    Kept(&'a str),
    /// `ready` on a branch with no commits: no label, and nothing to merge
    Nothing,
}

/// The task is ready now, by `how`: its risk label, its status, saved, then auto-accept
/// (an approval with auto-accept on) or the ready news.
pub fn make_ready(repo: &Repo, t: &mut Obj, base_ref: &str, how: How) -> Res<()> {
    match how {
        How::Plain => assess(repo, t, base_ref, Said::Nothing)?,
        How::Kept(_) => assess(repo, t, base_ref, Said::Kept)?,
        How::Nothing => {
            t.insert("risk".into(), Value::Null);
        }
    }
    let why = matches!(how, How::Nothing).then_some("Nothing to merge (no commits)");
    set_status(t, "ready", why, None);
    save(repo, t)?;
    if matches!(how, How::Kept(_)) && auto_accepts(repo, t) {
        // not waited for: a `ready` runs in the worker's pane, which accepting closes
        let id = s(t, "id").to_string();
        start_auto_accept(repo, &id, false);
        return Ok(());
    }
    let n = t.get("commits").and_then(Value::as_i64).unwrap_or(0);
    let facts = match how {
        How::Plain => vec![commits(n), where_(t)],
        How::Kept(kept) => vec![commits(n), where_(t), format!("its approval carried over: {kept}")],
        How::Nothing => vec!["nothing to merge".into()],
    };
    ready_news(repo, t, &facts, "", false)
}
