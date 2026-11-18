//! The kinds of news line: for each, its event (the word news.log keeps for it), the
//! row of the coordinator guide's news table it belongs to (or that it says what to do
//! itself), and so whether it wakes a coordinator (wake.rs): a line whose row asks the
//! coordinator to act wakes it; one that needs nothing rides along with the next line
//! that does.
//!
//! Every call that writes news (`news`, `say`, `tell_coordinator`, `stop`) passes a
//! kind, so a new line can't be written without one. An event no kind has (an old
//! log's) still wakes when read back, and record.rs reads an old line of an event since
//! split by its wording.

/// A kind of news line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `BP-3 is ready: ...` (ready_news)
    Ready,
    /// `BP-3 not auto-accepted: ... (nothing to do: the news about that follows)` (prs.rs)
    NotAutoAccepted,
    /// `BP-3 finished (...)`
    Finished,
    /// `BP-3 accepted (...)`
    Accepted,
    /// auto-accept's `BP-3 approved by review and auto-accepted (...)`
    AutoAccepted,
    /// `BP-3 closed (...)`
    Closed,
    /// auto-accept's `BP-3 finished (nothing to merge) and was closed by auto-accept: ...`
    AutoClosed,
    /// `BP-3 closed without merging. ...` (its branch kept)
    Kept,
    /// `BP-3 dropped`
    Dropped,
    /// `BP-3 started (...)`
    Started,
    /// `BP-3 is working again (was ready): ...`
    Working,
    /// `BP-3 sent back by the person after the tour: ...`
    TourSent,
    /// `After BP-3: ...` where every task was brought up to date (or left in review)
    Rebased,
    /// `After BP-3: ...` where a task was `not rebased`, or its push failed
    NotRebased,
    /// auto-accept's `... Sent back to its worker to rebase (trip 1 of 3)`
    RebaseSent,
    /// `Keys changed: tell the person to run `rex config reload` ...`
    Keys,
    /// `... The accept still counts: tell the person`
    AcceptFailed,
    /// the queue's `BP-4 can start (...), but the usage limit is reached ...`, and
    /// revert's `BP-9 was added to revert BP-3 but isn't started (...)`
    Queue,
    /// the queue's `BP-4 couldn't start (...): ... `bullpen start BP-4` tries again`
    QueueFailed,
    /// pickup's `Pickup started its 4 for today ...: ..., nothing to do`
    Pickup,
    /// pickup's asks: `Pickup paused: ... Tell the person`, `Room for one from the
    /// backlog: ... Write its brief`, `Pickup waits for a brief ...`, `... but it didn't start`
    PickupAsk,
    /// `BP-3 added L4 to the backlog: ... (`bullpen later L4 --keep` ...)`
    Later,
    /// `BP-3 needs the person: ...`, or a worker or reviewer at a prompt
    NeedsYou,
    /// `ask`'s `BP-3 needs the person: <question>`
    Ask,
    /// `BP-3: the person answers ...`
    Answer,
    /// a worker or reviewer that ended its turn without `ready` or a verdict (stop())
    Quiet,
    /// a worker's failed turn (stop())
    Failed,
    /// a worker or reviewer that exited, or whose pane is gone (stop())
    Stopped,
    /// a message that didn't reach its worker (stop())
    NotSent,
    /// `BP-3 has shown no progress for 20 min: ...`
    Stuck,
    /// an agent stopped at the usage limit
    Limit,
    /// `the usage limit has reset: continued BP-4, BP-6's reviewer: nothing to do`
    LimitContinued,
    /// `the usage limit has reset: continued ...` where one wasn't: `BP-4 (not continued,
    /// ...; `bullpen resume BP-4` continues it)`, or `(not continued: ...; tell the person)`
    LimitReset,
    /// `BP-3 became ready without a risk label, so here it is: ... Tell the person`
    Risk,
    /// `BP-4 and BP-6 both changed ...`
    Overlap,
    /// new comments or failed checks on a task's PR: `... Sent back to its worker.`
    PrSentBack,
    /// the same, kept for a worker on a prompt or stopped mid-turn: `... Not sent to its
    /// worker, which is on a prompt: `bullpen tell BP-3` it when it's free` (one that
    /// didn't reach it is a stop: NotSent)
    PrFeedback,
    /// `BP-3's PR was merged outside bullpen: tell the person; ...`
    PrMerged,
    /// `BP-3's PR was closed outside bullpen: tell the person; ...`
    PrClosed,
    /// a background job that failed
    Job,
    /// `<base> wasn't pushed to <remote>: ...`
    Push,
    /// `Your coordinator guide changed ...`
    Guide,
    /// new GitHub issues, or reading them failed
    Issues,
    /// a judged issue that asks the person (`... It asks the person (...): ask them ...`),
    /// or the person's yes to one with no brief (`... it needs a brief ...`)
    IssueAsk,
    /// a write to a GitHub issue that failed (`#12 (BP-3): bullpen couldn't ...`, `BP-3 was
    /// accepted, but #12 wasn't closed ...`), or a revert that leaves its issue closed
    IssueWrite,
    /// `BP-5 waits on BP-3, which was dropped, so it stays queued. Ask the person ...`
    DroppedDependency,
    /// `bullpen wasn't rebuilt: ...`
    Build,
    /// `the board wasn't rebuilt: ...`
    Board,
}

/// What the coordinator guide's news table (prompts/coordinator.md, "News") says of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// `starts For the coordinator:`: see Triage (stop() words its lines so; with no
    /// coordinator running, `needs the person` instead)
    Triage,
    /// `says a task needs the person, or is at a prompt`: tell them
    NeedsPerson,
    /// `has the person's answer to your ask`: act on it
    Answer,
    /// `is ready, finished or auto-accepted`: report it
    Report,
    /// `accepted, closed, dropped, started, working again, sent back, After BP-3: ...`:
    /// nothing to do
    Done,
    /// the same row's `In an After line, a task not rebased or whose push failed: see to it`
    NotRebased,
    /// `two tasks both changed a file`: tell the person in a line
    BothChanged,
    /// none: the line says what to do itself (a command to run, who to tell), or that
    /// there is nothing to do (asks: false)
    Own { asks: bool },
}

/// The rows of the guide's table, in its order (but its last, `anything else`: a line
/// that fits none has to say what to do itself). NotRebased shares Done's row.
pub const ROWS: &[Row] = &[
    Row::Triage,
    Row::NeedsPerson,
    Row::Answer,
    Row::Report,
    Row::Done,
    Row::BothChanged,
];

impl Row {
    /// The first cell of its row in the guide's table, as the guide words it (a test
    /// holds the guide to these), or None for a line that says what to do itself.
    pub fn cell(self) -> Option<&'static str> {
        Some(match self {
            Row::Triage => "starts `For the coordinator:`",
            Row::NeedsPerson => "says a task `needs the person`, or is at a prompt",
            Row::Answer => "has the person's answer to your `ask` (`BP-3: ...`)",
            Row::Report => "`is ready`, `finished` or auto-accepted",
            Row::Done | Row::NotRebased => {
                "`accepted`, `closed`, `dropped`, `started`, `working again`, `sent back`, `After BP-3: ...`"
            }
            Row::BothChanged => "two tasks `both changed` a file",
            Row::Own { .. } => return None,
        })
    }

    /// Does a line in this row ask the coordinator to do something?
    pub fn asks(self) -> bool {
        !matches!(self, Row::Done | Row::Own { asks: false })
    }
}

/// Every kind, for the tests and for reading an event back.
pub const ALL: &[Kind] = &[
    Kind::Ready,
    Kind::NotAutoAccepted,
    Kind::Finished,
    Kind::Accepted,
    Kind::AutoAccepted,
    Kind::Closed,
    Kind::AutoClosed,
    Kind::Kept,
    Kind::Dropped,
    Kind::Started,
    Kind::Working,
    Kind::TourSent,
    Kind::Rebased,
    Kind::NotRebased,
    Kind::RebaseSent,
    Kind::Keys,
    Kind::AcceptFailed,
    Kind::Queue,
    Kind::QueueFailed,
    Kind::Pickup,
    Kind::PickupAsk,
    Kind::Later,
    Kind::NeedsYou,
    Kind::Ask,
    Kind::Answer,
    Kind::Quiet,
    Kind::Failed,
    Kind::Stopped,
    Kind::NotSent,
    Kind::Stuck,
    Kind::Limit,
    Kind::LimitContinued,
    Kind::LimitReset,
    Kind::Risk,
    Kind::Overlap,
    Kind::PrSentBack,
    Kind::PrFeedback,
    Kind::PrMerged,
    Kind::PrClosed,
    Kind::Job,
    Kind::Push,
    Kind::Guide,
    Kind::Issues,
    Kind::IssueAsk,
    Kind::IssueWrite,
    Kind::DroppedDependency,
    Kind::Build,
    Kind::Board,
];

impl Kind {
    /// The word news.log keeps for it (`<stamp>[ [to]] <task> <event>: <line>`). An
    /// event that meant several things kept its word for one of them, and record.rs
    /// reads an old line of it by its wording.
    pub fn event(self) -> &'static str {
        match self {
            Kind::Ready => "ready",
            Kind::NotAutoAccepted => "not-auto-accepted",
            Kind::Finished => "finished",
            Kind::Accepted => "accepted",
            Kind::AutoAccepted => "auto-accepted",
            Kind::Closed => "closed",
            Kind::AutoClosed => "auto-closed",
            Kind::Kept => "kept",
            Kind::Dropped => "dropped",
            Kind::Started => "started",
            Kind::Working => "working",
            Kind::TourSent => "tour-sent",
            Kind::Rebased => "rebased",
            Kind::NotRebased => "not-rebased",
            Kind::RebaseSent => "rebase_sent",
            Kind::Keys => "keys",
            Kind::AcceptFailed => "accept_failed",
            Kind::Queue => "queue",
            Kind::QueueFailed => "queue-failed",
            Kind::Pickup => "pickup",
            Kind::PickupAsk => "pickup-ask",
            Kind::Later => "later",
            Kind::NeedsYou => "needs-you",
            Kind::Ask => "ask",
            Kind::Answer => "answer",
            Kind::Quiet => "quiet",
            Kind::Failed => "failed",
            Kind::Stopped => "stopped",
            Kind::NotSent => "not-sent",
            Kind::Stuck => "stuck",
            Kind::Limit => "limit",
            Kind::LimitContinued => "limit-continued",
            Kind::LimitReset => "limit-reset",
            Kind::Risk => "risk",
            Kind::Overlap => "overlap",
            Kind::PrSentBack => "pr-sent-back",
            Kind::PrFeedback => "pr-feedback",
            Kind::PrMerged => "pr-merged",
            Kind::PrClosed => "pr-closed",
            Kind::Job => "job",
            Kind::Push => "push",
            Kind::Guide => "guide",
            Kind::Issues => "issues",
            Kind::IssueAsk => "issue-ask",
            Kind::IssueWrite => "issue-write",
            Kind::DroppedDependency => "dropped-dependency",
            Kind::Build => "build",
            Kind::Board => "board",
        }
    }

    /// The kind a logged event names, if any.
    pub fn of_event(event: &str) -> Option<Kind> {
        ALL.iter().copied().find(|k| k.event() == event)
    }

    /// Its row in the guide's news table.
    pub fn row(self) -> Row {
        match self {
            Kind::Ready | Kind::Finished | Kind::AutoAccepted | Kind::AutoClosed => Row::Report,
            Kind::Accepted
            | Kind::Closed
            | Kind::Kept
            | Kind::Dropped
            | Kind::Started
            | Kind::Working
            | Kind::TourSent
            | Kind::Rebased
            | Kind::RebaseSent
            | Kind::PrSentBack => Row::Done,
            Kind::NotRebased => Row::NotRebased,
            Kind::NeedsYou | Kind::Ask => Row::NeedsPerson,
            Kind::Answer => Row::Answer,
            Kind::Quiet | Kind::Failed | Kind::Stopped | Kind::NotSent => Row::Triage,
            Kind::Overlap => Row::BothChanged,
            // a command for later or for whoever decides, or nothing to do: nothing to do now
            Kind::NotAutoAccepted | Kind::Pickup | Kind::Queue | Kind::Later | Kind::LimitContinued => {
                Row::Own { asks: false }
            }
            Kind::Keys
            | Kind::AcceptFailed
            | Kind::QueueFailed
            | Kind::PickupAsk
            | Kind::Stuck
            | Kind::Limit
            | Kind::LimitReset
            | Kind::Risk
            | Kind::PrFeedback
            | Kind::PrMerged
            | Kind::PrClosed
            | Kind::Job
            | Kind::Push
            | Kind::Guide
            | Kind::Issues
            | Kind::IssueAsk
            | Kind::IssueWrite
            | Kind::DroppedDependency
            | Kind::Build
            | Kind::Board => Row::Own { asks: true },
        }
    }

    /// Does a line of this kind wake a coordinator by itself? Only when its row asks
    /// something of it.
    pub fn wakes(self) -> bool {
        self.row().asks()
    }
}

/// `_news_kinds`: the table as JSON, for the tests (support.py's `check_news` looks a
/// logged line's row up by its event; test_guide.py holds the guide's table to `rows`).
pub fn cmd_news_kinds(argv: &[String]) -> bullpen_core::Res<i32> {
    crate::args::Args::parse("_news_kinds", argv, &[]);
    let kinds = ALL
        .iter()
        .map(|k| {
            serde_json::json!({
                "event": k.event(),
                "row": k.row().cell(),
                "asks": k.row().asks(),
                "wakes": k.wakes(),
            })
        })
        .collect();
    let rows = ROWS.iter().map(|r| r.cell().into()).collect();
    let table = serde_json::json!({"rows": serde_json::Value::Array(rows), "kinds": serde_json::Value::Array(kinds)});
    println!("{}", bullpen_core::json::dumps(&table));
    Ok(0)
}

impl From<Kind> for &'static str {
    fn from(k: Kind) -> &'static str {
        k.event()
    }
}

#[cfg(test)]
#[path = "tests/test_kind.rs"]
mod tests;
