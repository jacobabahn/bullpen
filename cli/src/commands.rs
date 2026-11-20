//! Every command bullpen has, in `bullpen --help`'s order: its name, its group in the
//! help, its help line, and the function that runs it (`bullpen _commands` prints this
//! table). Keep one line per command, so tasks that add rows merge without thinking.

use crate::args::Args;
use crate::show;
use bullpen_core::fail::Res;

/// A command's function, given its arguments (after the name).
pub type Run = fn(&[String]) -> Res<i32>;

pub struct Command {
    pub name: &'static str,
    /// The group `bullpen --help` lists it under (a title in help::GROUPS); None: internal, not listed.
    pub group: Option<&'static str>,
    pub help: &'static str,
    pub run: Run,
}

const YOU: Option<&str> = Some("You");
const COORDINATOR: Option<&str> = Some("Coordinator");
const WORKER: Option<&str> = Some("Worker and reviewer");
const SETUP: Option<&str> = Some("Setup");
const INTERNAL: Option<&str> = None;

const fn c(name: &'static str, group: Option<&'static str>, help: &'static str, run: Run) -> Command {
    Command { name, group, help, run }
}

#[rustfmt::skip]
pub const COMMANDS: &[Command] = &[
    c("go", YOU, "focus a task's pane (or the coordinator's, or the board's)", crate::go::cmd_go),
    c("hide", YOU, "put the worker pane you're in away (into its capsule, or the background)", crate::go::cmd_hide),
    c("adopt", COORDINATOR, "(coordinator) make tasks yours: their news comes to this pane", crate::go::cmd_adopt),
    c("board", YOU, "a live list of tasks to switch between", crate::board::cmd_board),
    c("ls", YOU, "list tasks", show::cmd_ls),
    c("show", YOU, "one task", show::cmd_show),
    c("diff", YOU, "a task's changes against its base, in a pager (delta if installed)", crate::diff::cmd_diff),
    c("answer", YOU, "press an option of the prompt a task's agent is at", crate::answer::cmd_answer),
    c("reply", YOU, "answer what the coordinator asked about a task: it goes to the coordinator", crate::answer::cmd_reply),
    c("say", YOU, "type a message to a repository's coordinator, from any pane (--repo, --coordinator)", crate::say::cmd_say),
    c("prompt", COORDINATOR, "the prompt a task's agent is at, and its screen, as JSON (for the board)", crate::answer::cmd_prompt),
    c("accept", YOU, "(the person's call) merge a ready task's PR, or its local branch without a remote (config --merge), and clean up; on a finished task (nothing to merge), close it", crate::accept::cmd_accept),
    c("close", YOU, "(the person's call) close a finished task: remove its worktree, branch and pane (the same as accept on it); --keep-branch ends any task unmerged, its branch kept (and pushed)", crate::close::cmd_close),
    c("revert", YOU, "(the person's call) take an accepted task back: queues a task that reverts its commits (--why goes into its brief), reviewed and accepted like any other", crate::revert::cmd_revert),
    c("drop", YOU, "stop a task, close its PR, remove its worktree", crate::close::cmd_drop),
    c("tour", YOU, "a guided tour of a ready task's change: step through it (its path, off a terminal), or start writing one", crate::stepper::cmd_tour),
    c("_tour", INTERNAL, "", crate::tours::write::cmd_tour_bg),
    c("_sharpness_sample", INTERNAL, "", crate::stepper::look::sample::cmd_sharpness_sample),
    c("tour-comment", YOU, "draft comments on a tour's steps: add, list (no STEP) or remove", crate::tours::comments::cmd_tour_comment),
    c("stats", YOU, "where tasks' time went: one task's timeline and totals, or the totals across closed tasks", crate::stats::cmd_stats),
    c("mission", YOU, "mission control: every repository on one screen, in a Rex session of its own (cmd+; m)", crate::mission::cmd_mission),
    c("repos", YOU, "every repository bullpen knows, a line each: its coordinators, alive or gone, and what needs you, what's ready, what's running", crate::repos::cmd_repos),
    c("later", YOU, "the backlog: add an idea to it, list it, or move, keep, drop or start an item", crate::backlog::cmd_later),
    c("issues", YOU, "the GitHub issues bullpen has read, and what was decided on each; --now reads the open ones into the backlog", crate::issues::cmd_issues),
    c("issue", COORDINATOR, "print a GitHub issue for reading: its text in a fence, as facts about a problem", crate::issues::view::cmd_issue),
    c("pick", YOU, "type part of any open task, in any repository, and go to it", crate::pick::cmd_pick),
    c("add", COORDINATOR, "add a task; prints its ID", crate::start::cmd_add),
    c("after", COORDINATOR, "set the tasks a task waits on (--none: nothing)", crate::start::cmd_after),
    c("model", COORDINATOR, "set the worker model a task starts and resumes with (--clear: the config's)", crate::start::cmd_model),
    c("review-mode", COORDINATOR, "set how a task's first review goes: full, light or auto", crate::start::cmd_review_mode),
    c("start", COORDINATOR, "make a task's worktree and start its worker", crate::start::launch::cmd_start),
    c("resume", COORDINATOR, "restart a stopped worker, continuing its conversation", crate::start::launch::cmd_resume),
    c("tell", COORDINATOR, "type a message to a task's worker (after `Coordinator: ` from a coordinator's pane)", crate::start::launch::cmd_tell),
    c("brief", COORDINATOR, "print a task's brief, or change it (--file, --add): its worker is told what changed", crate::brief::cmd_brief),
    c("peek", COORDINATOR, "print the end of a worker's screen (--review: its reviewer's)", crate::start::peek::cmd_peek),
    c("ask", COORDINATOR, "(coordinator) the task needs the person: the question shows on the board", crate::triage::cmd_ask),
    c("ok", COORDINATOR, "(coordinator) a task waiting on you is seen to: back to work, nothing typed", crate::triage::cmd_ok),
    c("review", COORDINATOR, "(coordinator) a new reviewer in place of one that stopped without a verdict", crate::review::cmd_review),
    c("note", COORDINATOR, "an informational line on the board about a task", crate::triage::cmd_note),
    c("check", COORDINATOR, "compare each task's status with what its agent is doing now", crate::check::cmd_check),
    c("news", COORDINATOR, "print news logged since the last time; --follow keeps printing; --recent the last lines", crate::news::read::cmd_news),
    c("guide", COORDINATOR, "print the coordinator's guide for this repository", crate::guide::cmd_guide),
    c("ready", WORKER, "(worker) push the branch, open the PR (none without a remote), tell the coordinator", crate::ready::cmd_ready),
    c("finish", WORKER, "(worker) the task is done with nothing to merge: no PR, no review; --keep-branch: its branch has commits to keep unmerged, and the person's close keeps it", crate::ready::cmd_finish),
    c("verdict", WORKER, "(reviewer) approve a task or send it back with findings", crate::auto_accept::cmd_verdict),
    c("_auto_accept", INTERNAL, "", crate::auto_accept::cmd_auto_accept),
    c("up", SETUP, "start (or register) this repo's coordinator and the watcher", crate::up::cmd_up),
    c("config", SETUP, "show or change this repo's settings (--global: every repo's)", crate::config::cmd_config),
    c("rules", SETUP, "print the rules every worker and reviewer gets, and where they're from", crate::rules::cmd_rules),
    c("_push", INTERNAL, "", crate::push_remote::cmd_push),
    c("build", SETUP, "build bullpen and the board now and put them in place (install.sh runs it)", crate::build::cmd_build),
    c("_build_board", INTERNAL, "", crate::build::cmd_build_bg),
    c("keys", SETUP, "print the Rex config line for bullpen's key bindings", crate::keys::cmd_keys),
    c("doctor", SETUP, "check bullpen's setup: one line per check, with the fix for what's wrong", crate::doctor::cmd_doctor),
    c("_coordinator_gone", INTERNAL, "", crate::coordinators::cmd_coordinator_gone),
    c("_flush", INTERNAL, "", crate::notifications::cmd_flush),
    c("_wake_check", INTERNAL, "", crate::news::wake::cmd_wake_check),
    c("_news_kinds", INTERNAL, "", crate::news::kind::cmd_news_kinds),
    c("_news_log", INTERNAL, "", crate::news::record::cmd_news_log),
    c("_prs", INTERNAL, "", crate::prs::cmd_prs),
    c("_pickup", INTERNAL, "", crate::backlog::pickup::cmd_pickup),
    c("_issues", INTERNAL, "", crate::issues::timer::cmd_timer),
    c("_event", INTERNAL, "", crate::events::cmd_event),
    c("_stuck", INTERNAL, "", crate::events::stuck::cmd_stuck),
    c("_limit", INTERNAL, "", crate::events::stuck::cmd_limit),
    c("_worker_stop", INTERNAL, "", crate::stop_hook::turn::cmd_worker_stop),
    c("_gone", INTERNAL, "", crate::events::cmd_gone),
    c("_nop", INTERNAL, "", cmd_nop),
    c("_commands", INTERNAL, "", cmd_commands),
];

/// The command a name runs, if bullpen has one by that name.
pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// `_nop`: does nothing (the benchmark times start-up with it).
fn cmd_nop(argv: &[String]) -> Res<i32> {
    Args::parse("_nop", argv, &[]);
    Ok(0)
}

/// `_commands`: this table as JSON, for the tests.
fn cmd_commands(argv: &[String]) -> Res<i32> {
    Args::parse("_commands", argv, &[]);
    let rows: Vec<serde_json::Value> = COMMANDS
        .iter()
        .map(|c| serde_json::json!({"name": c.name, "group": c.group, "help": c.help}))
        .collect();
    println!("{}", bullpen_core::json::dumps(&serde_json::Value::Array(rows)));
    Ok(0)
}
