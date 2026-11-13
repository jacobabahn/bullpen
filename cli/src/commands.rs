//! Every command bullpen has, in `bullpen --help`'s order: its name, its group in the
//! help, its help line, and which side runs it (Rust, or the Python it is handed over
//! to). A command lives in exactly one place: test_cli checks that the Python-owned
//! names are the commands Python still declares (`bullpen _commands` prints this table).
//! Keep one line per command, so tasks that add rows merge without thinking.

use crate::args::Args;
use bullpen_core::fail::Res;

/// What runs a command.
#[derive(Clone, Copy)]
pub enum Owner {
    /// Rust: the command's function, given its arguments (after the name).
    Rust(fn(&[String]) -> Res<i32>),
    /// The Python bullpen (bin/bullpen-py), handed the whole call before anything is read or written.
    Python,
    /// Another name for the command named here, which runs instead (with its own name).
    Alias(&'static str),
}

pub use Owner::{Alias, Python, Rust};

pub struct Command {
    pub name: &'static str,
    /// The group `bullpen --help` lists it under (a title in help::GROUPS); None: internal, not listed.
    pub group: Option<&'static str>,
    pub help: &'static str,
    pub owner: Owner,
}

const YOU: Option<&str> = Some("You");
const COORDINATOR: Option<&str> = Some("Coordinator");
const WORKER: Option<&str> = Some("Worker and reviewer");
const SETUP: Option<&str> = Some("Setup");
const INTERNAL: Option<&str> = None;

const fn c(name: &'static str, group: Option<&'static str>, help: &'static str, owner: Owner) -> Command {
    Command { name, group, help, owner }
}

/// A hidden other name: not in the help, never offered as "Did you mean".
const fn alias(name: &'static str, of: &'static str) -> Command {
    Command { name, group: None, help: "", owner: Alias(of) }
}

#[rustfmt::skip]
pub const COMMANDS: &[Command] = &[
    c("go", YOU, "focus a task's pane (or the coordinator's, or the board's)", Python),
    c("hide", YOU, "put the worker pane you're in away (into its capsule, or the background)", Python),
    c("adopt", COORDINATOR, "(coordinator) make tasks yours: their news comes to this pane", Python),
    c("board", YOU, "a live list of tasks to switch between", Python),
    c("ls", YOU, "list tasks", Python),
    c("show", YOU, "one task", Python),
    c("diff", YOU, "a task's changes against its base, in a pager (delta if installed)", Python),
    c("answer", YOU, "press an option of the prompt a task's agent is at", Python),
    c("reply", YOU, "answer what the coordinator asked about a task: it goes to the coordinator", Python),
    c("prompt", COORDINATOR, "the prompt a task's agent is at, and its screen, as JSON (for the board)", Python),
    c("accept", YOU, "(the person's call) merge a ready task's PR, or its local branch without a remote (config --merge), and clean up; on a finished task (nothing to merge), close it", Python),
    c("close", YOU, "(the person's call) close a finished task: remove its worktree, branch and pane (the same as accept on it)", Python),
    c("drop", YOU, "stop a task, close its PR, remove its worktree", Python),
    c("tour", YOU, "a guided tour of a ready task's change: step through it (its path, off a terminal), or start writing one", Python),
    c("_tour", INTERNAL, "", Python),
    c("tour-comment", YOU, "draft comments on a tour's steps: add, list (no STEP) or remove", Python),
    c("pick", YOU, "type part of any open task, in any repository, and go to it", Python),
    c("add", COORDINATOR, "add a task; prints its ID", Python),
    c("after", COORDINATOR, "set the tasks a task waits on (--none: nothing)", Python),
    c("model", COORDINATOR, "set the worker model a task starts and resumes with (--clear: the config's)", Python),
    c("review-mode", COORDINATOR, "set how a task's first review goes: full, light or auto", Python),
    c("start", COORDINATOR, "make a task's worktree and start its worker", Python),
    c("resume", COORDINATOR, "restart a stopped worker, continuing its conversation", Python),
    c("tell", COORDINATOR, "type a message to a task's worker", Python),
    c("peek", COORDINATOR, "print the end of a worker's screen", Python),
    c("ask", COORDINATOR, "(coordinator) the task needs the person: the question shows on the board", Python),
    c("ok", COORDINATOR, "(coordinator) a task waiting on you is seen to: back to work, nothing typed", Python),
    c("review", COORDINATOR, "(coordinator) a new reviewer in place of one that stopped without a verdict", Python),
    c("note", COORDINATOR, "an informational line on the board about a task", Python),
    c("check", COORDINATOR, "compare each task's status with what its agent is doing now", Python),
    c("news", COORDINATOR, "print news logged since the last time; --follow keeps printing", Python),
    c("guide", COORDINATOR, "print the coordinator's guide", Python),
    c("ready", WORKER, "(worker) push the branch, open the PR (none without a remote), tell the coordinator", Python),
    c("finish", WORKER, "(worker) the task is done with nothing to merge: no PR, no review", Python),
    c("verdict", WORKER, "(reviewer) approve a task or send it back with findings", Python),
    c("_auto_accept", INTERNAL, "", Python),
    c("up", SETUP, "start (or register) this repo's coordinator and the watcher", Python),
    c("config", SETUP, "show or change this repo's settings (--global: every repo's)", Python),
    c("rules", SETUP, "print the rules every worker and reviewer gets, and where they're from", Python),
    c("_push", INTERNAL, "", Python),
    c("build", SETUP, "build bullpen and the board now and put them in place (install.sh runs it)", Python),
    alias("build-board", "build"), // its old name, until T13
    c("_build_board", INTERNAL, "", Python),
    c("keys", SETUP, "print the Rex config line for bullpen's key bindings", Python),
    c("doctor", SETUP, "check bullpen's setup: one line per check, with the fix for what's wrong", Python),
    c("_coordinator_gone", INTERNAL, "", Python),
    c("_flush", INTERNAL, "", Python),
    c("_prs", INTERNAL, "", Python),
    c("_event", INTERNAL, "", Python),
    c("_stuck", INTERNAL, "", Python),
    c("_limit", INTERNAL, "", Python),
    c("_gone", INTERNAL, "", Python),
    c("_nop", INTERNAL, "", Rust(cmd_nop)),
    c("_commands", INTERNAL, "", Rust(cmd_commands)),
];

/// The command a name runs, if bullpen has one by that name.
pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// `_nop`: does nothing (the benchmark times start-up with it).
fn cmd_nop(argv: &[String]) -> Res<i32> {
    Args::parse("_nop", argv, &[])?;
    Ok(0)
}

/// `_commands`: this table as JSON, for test_cli.
fn cmd_commands(argv: &[String]) -> Res<i32> {
    Args::parse("_commands", argv, &[])?;
    let rows: Vec<serde_json::Value> = COMMANDS
        .iter()
        .map(|c| {
            let owner = match c.owner {
                Rust(_) => "rust".to_string(),
                Python => "python".to_string(),
                Alias(of) => format!("alias of {of}"),
            };
            serde_json::json!({"name": c.name, "group": c.group, "help": c.help, "owner": owner})
        })
        .collect();
    println!("{}", bullpen_core::json::dumps(&serde_json::Value::Array(rows)));
    Ok(0)
}
