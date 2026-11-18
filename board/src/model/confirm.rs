//! The keys each task gets, and the y/n questions of an accept or a drop.

use super::*;

/// The keys that make sense for this task, for the footer: (key, what it does).
pub fn task_keys(t: Option<&Task>) -> Vec<(&'static str, &'static str)> {
    let Some(t) = t else { return Vec::new() };
    // ⏎ opens the pane the task is stopped on: its reviewer's, else its worker's
    let enter = if t.on_reviewer() { "reviewer" } else { "open" };
    let mut keys: Vec<(&str, &str)> = match t.status.as_str() {
        "needs" if t.question().is_some() => vec![("i", "answer"), ("⏎", enter), ("t", "tell"), ("p", "peek")],
        "needs" => vec![("⏎", enter), ("t", "tell"), ("p", "peek")],
        "triage" => vec![("⏎", enter), ("p", "peek"), ("t", "tell")],
        "stopped" => vec![("r", "resume"), ("p", "peek"), ("d", "drop")],
        "review" => vec![("⏎", "reviewer"), ("g", "tour"), ("p", "peek")],
        "ready" => vec![("a", "accept"), ("g", "tour"), ("d", "drop"), ("⏎", "open")],
        "finished" => vec![("a", "close"), ("d", "drop"), ("⏎", "open")],
        "working" => vec![("⏎", "open"), ("t", "tell"), ("p", "peek")],
        "todo" => vec![("s", "start"), ("d", "drop")],
        _ => Vec::new(),
    };
    if t.worktree.as_deref().is_some_and(|w| !w.is_empty()) && !t.closed() {
        keys.push(("v", "diff"));
    }
    keys
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Close,
    Drop,
    DropForce,
    /// an item of the Later group: its question is `item_confirm`'s
    DropItem,
}

/// What could go wrong with an accept or a drop, worst first, each in full and in
/// short: failing or running checks, changes asked for, no review; a worker or
/// reviewer still running.
pub fn confirm_risks(action: Action, t: &Task, cfg: &Config) -> Vec<(&'static str, &'static str)> {
    let mut risks = Vec::new();
    match action {
        Action::Accept => {
            match t.gh.as_ref().and_then(|g| g.checks.as_deref()) {
                Some("failing") => risks.push(("Checks failing", "Checks failing")),
                Some("running") => risks.push(("Checks still running", "Checks running")),
                _ => {}
            }
            let nothing = t.nothing_to_merge();
            match t.review.as_ref().and_then(|r| r.verdict.as_deref()) {
                Some("approved") => {}
                Some("changes") => risks.push(("Review asks for changes", "Changes asked")),
                _ if cfg.review && !nothing => risks.push(("Not reviewed", "Not reviewed")),
                _ => {}
            }
        }
        Action::Drop => {
            // only while its pane is open: bullpen clears the block when the pane goes
            let open = |b: Option<&str>| b.is_some_and(|b| !b.is_empty());
            let reviewer = t.review.as_ref().and_then(|r| r.block.as_deref());
            if t.reviewers_turn() {
                if open(reviewer) {
                    risks.push(("Its reviewer is still running", "Reviewer running"));
                }
            } else if matches!(t.status.as_str(), "working" | "needs" | "triage") && open(t.block.as_deref()) {
                risks.push(("Its worker is still running", "Worker running"));
            }
        }
        Action::Close | Action::DropForce | Action::DropItem => {}
    }
    risks
}

/// Does the confirm turn red: a drop, or an accept with a risk.
pub fn confirm_risky(action: Action, t: &Task, cfg: &Config) -> bool {
    match action {
        Action::Accept | Action::Close => !confirm_risks(action, t, cfg).is_empty(),
        Action::Drop | Action::DropForce | Action::DropItem => true,
    }
}

/// The question a task's own row turns into while the board waits for y or n, in
/// shorter and shorter words: the row shows the first that fits. A risk comes first,
/// so a cut never loses it, and the shortest is the risk alone: the key row still
/// says what y does.
pub fn confirm_texts(action: Action, t: &Task, cfg: &Config) -> Vec<String> {
    let pr = pr_short(t.pr.as_deref());
    let verb = match cfg.merge.as_str() {
        "rebase" => "rebase-merge",
        "squash" => "squash-merge",
        other => other,
    };
    let nothing = t.nothing_to_merge();
    let risks = confirm_risks(action, t, cfg);
    let ask = match action {
        Action::Accept if pr.is_empty() && t.is_local() && !nothing => {
            format!("Accept and {verb} into {}", t.base.as_deref().unwrap_or("its base"))
        }
        Action::Accept if pr.is_empty() => "Accept and clean up".into(),
        Action::Accept => format!("Accept and {verb}"),
        Action::Close if bullpen_core::task::keeps_branch(t.obj()) => "Close it and keep its branch".into(),
        Action::Close => "Close it and remove its worktree".into(),
        Action::Drop | Action::DropItem if pr.is_empty() => "Drop it".into(),
        Action::Drop | Action::DropItem => format!("Drop it and close {pr}"),
        Action::DropForce => "Discard uncommitted changes".into(),
    };
    let Some(&(worst, short)) = risks.first() else { return vec![format!("{ask}?")] };
    let all = risks.iter().enumerate().map(|(i, r)| if i == 0 { r.0.to_string() } else { r.0.to_lowercase() });
    let all = all.collect::<Vec<_>>().join(", ");
    let mut texts = match action {
        Action::Accept => vec![
            format!("{all}. {ask} anyway?"),
            format!("{worst}. {ask} anyway?"),
            format!("{worst}. Accept anyway?"),
            format!("{short}. Accept anyway?"),
            format!("{short}. Accept?"),
            format!("{short}."),
        ],
        _ => vec![
            format!("{worst}. {ask}?"),
            format!("{short}. {ask}?"),
            format!("{short}. Drop it?"),
            format!("{short}. Drop?"),
            format!("{short}."),
        ],
    };
    texts.dedup();
    texts
}

/// The longest question, as the row says it with all the room it needs.
#[cfg(test)]
pub fn confirm_text(action: Action, t: &Task, cfg: &Config) -> String {
    confirm_texts(action, t, cfg).remove(0)
}

#[cfg(test)]
#[path = "tests/test_confirm.rs"]
mod tests;
