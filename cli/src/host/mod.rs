//! The host a task's agents run on: what bullpen asks of it, in bullpen's own terms.
//! `Host` is the interface (start an agent, tell it something, is it alive, read it,
//! what state it's in and what it's asking, answer that, show, hide or close it, set
//! its title); `Agent` is one agent, as a record keeps it (its host, where on the host
//! it lives and its ID there), and calls its host. Rex is the one host now (rex.rs).
//!
//! Host-neutral: everything here, and the code that reaches an agent through an
//! `Agent` (a task's worker and reviewer, a coordinator). Rex-only, and left on
//! `crate::rex`: sessions, blocks, layers, floats and capsules (`panes/`), the workers
//! tab, boards, diff panes, the watcher (lua/watch.lua) and its pane, mission control's
//! session, `go` and the hide key (they move between Rex panes, boards included),
//! finding panes again after a Rex restart (`go/relink.rs`), starting a coordinator
//! (`up`), notifications sent through a pane's terminal, `rex_view` and kitty graphics.
//!
//! A record says which host its agent is on by its "host" (a host's `name`); one
//! without it is on Rex, so task files from before hosts read as they did. Adding a
//! host: implement `Host` in a file of its own here and add it to `named`.

pub mod rex;

use crate::screen::prompt::Prompt;
use bullpen_core::json::{py_str, truthy, Obj};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// Seconds a new idle, blocked or error state is left alone: its event may still be on the way.
pub const SETTLE: f64 = 60.0;

/// What an agent is doing now. state: gone, working, blocked, error, idle or shell (its
/// turn ended, but a background shell of its own runs, so it takes another). kind and
/// msg: its host's words for it ("" when it has none). age: seconds in that state, None
/// if unknown.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub state: &'static str,
    pub kind: String,
    pub msg: String,
    pub age: Option<f64>,
}

/// What bullpen asks of the host its agents run on. Each call names the agent by an
/// `Agent`; nothing assumes a pane, a terminal screen or a tty (Rex reads its screens
/// to answer what another host might report itself).
pub trait Host: Sync {
    /// Its name, as a record's "host" keeps it.
    fn name(&self) -> &'static str;

    /// Start argv as task t's worker or reviewer (role), labelled label, in t's
    /// worktree, within where (a Rex session; "" for the host's default). The agent,
    /// and how the host placed it (Rex's placement name, recorded as the pane's "pane").
    fn start(
        &self,
        repo: &Repo,
        t: &Obj,
        within: &str,
        label: &str,
        argv: &[String],
        role: &str,
    ) -> Res<(Agent, &'static str)>;

    /// Is it running?
    fn alive(&self, a: &Agent) -> Res<bool>;

    /// Give it a message (a line, or several) to act on. None if it went through, else
    /// why not, in a few words.
    fn tell(&self, a: &Agent, msg: &str) -> Res<Option<String>>;

    /// tell, failing when it doesn't go through.
    fn send(&self, a: &Agent, msg: &str) -> Res<()>;

    /// The latest of what it shows (a screen, or messages), as text; "" if the host can't say.
    fn read(&self, a: &Agent) -> Res<String>;

    /// The last n lines of what it shows, without blank ones at the end; colour keeps
    /// the colours, as escape codes.
    fn read_end(&self, a: &Agent, n: i64, colour: bool) -> Res<Vec<String>>;

    /// Its own status record, as the host keeps it: "state" (working, blocked, error,
    /// idle or done), "kind", "msg", "updated_at". None if the host can't say.
    fn status(&self, a: &Agent) -> Res<Option<Obj>>;

    /// What it's doing now (gone included). None: unsure, or it changed a moment ago.
    fn state(&self, a: &Agent) -> Res<Option<State>>;

    /// Is it compacting its conversation? msg: its status message, if it has one.
    fn compacting(&self, a: &Agent, msg: &str) -> Res<bool>;

    /// Has its turn ended with a background shell of its own still running?
    fn shell_running(&self, a: &Agent) -> Res<bool>;

    /// The question it's waiting on an answer to, with its options, or None.
    fn question(&self, a: &Agent) -> Res<Option<Prompt>>;

    /// Answer its question with one of the options' keys (`question`'s).
    fn answer(&self, a: &Agent, key: &str) -> Res<()>;

    /// Is something half-typed in its input, which a message given now would send along?
    fn has_draft(&self, a: &Agent) -> Res<bool>;

    /// Show it to the person, and put away whatever of bullpen's it shows instead.
    #[allow(dead_code)] // `go` and the hide key show and hide Rex panes (boards too) on Rex
    fn show(&self, repo: &Repo, a: &Agent) -> Res<()>;

    /// Put it away. False if the host has nowhere to put it away to.
    #[allow(dead_code)] // as show
    fn hide(&self, repo: &Repo, a: &Agent) -> Res<bool>;

    /// Close it. last: this command may run in it, so close it after the command's
    /// other work, since that may end this process.
    fn close(&self, a: &Agent, last: bool) -> Res<()>;

    /// Show title (bullpen's state of its task, mark first) where the host lists it.
    #[allow(dead_code)] // no caller until a host lists its agents by title (T3's sidebar)
    fn set_title(&self, a: &Agent, title: &str) -> Res<()>;
}

/// The host by its name, or None.
pub fn named(name: &str) -> Option<&'static dyn Host> {
    [&rex::REX as &'static dyn Host].into_iter().find(|h| h.name() == name)
}

/// The host a new agent of task t's starts on.
pub fn for_new(_repo: &Repo, _t: &Obj) -> &'static dyn Host {
    &rex::REX
}

/// One agent: the host it's on, where there it lives (a Rex session; "" for the
/// host's default) and its ID there (a Rex block; "" for none).
#[derive(Clone)]
pub struct Agent {
    pub host: &'static dyn Host,
    pub within: String,
    pub id: String,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Agent({} {:?} {:?})", self.host.name(), self.within, self.id)
    }
}

impl PartialEq for Agent {
    fn eq(&self, other: &Agent) -> bool {
        self.host.name() == other.host.name() && self.within == other.within && self.id == other.id
    }
}

/// A record's value as text: "" when it's falsy.
fn text(v: Option<&Value>) -> String {
    v.filter(|v| truthy(Some(v))).map_or(String::new(), py_str)
}

impl Agent {
    /// An agent on Rex, in session, at block.
    pub fn on_rex(session: &str, block: &str) -> Agent {
        Agent {
            host: &rex::REX,
            within: session.into(),
            id: block.into(),
        }
    }

    /// The agent a record keeps (a task's, its review's, a coordinator's): its "host"
    /// (Rex when it has none, or one this build doesn't know), "session" and "block".
    pub fn of(rec: &Obj) -> Agent {
        Agent {
            host: named(&text(rec.get("host"))).unwrap_or(&rex::REX),
            within: text(rec.get("session")),
            id: text(rec.get("block")),
        }
    }

    /// Keep it in rec, as `of` reads it.
    pub fn record(&self, rec: &mut Obj) {
        rec.insert("block".into(), json!(self.id));
        rec.insert("session".into(), json!(self.within));
        rec.insert("host".into(), json!(self.host.name()));
    }

    pub fn alive(&self) -> Res<bool> {
        self.host.alive(self)
    }

    pub fn tell(&self, msg: &str) -> Res<Option<String>> {
        self.host.tell(self, msg)
    }

    pub fn send(&self, msg: &str) -> Res<()> {
        self.host.send(self, msg)
    }

    pub fn read_end(&self, n: i64, colour: bool) -> Res<Vec<String>> {
        self.host.read_end(self, n, colour)
    }

    pub fn status(&self) -> Res<Option<Obj>> {
        self.host.status(self)
    }

    pub fn state(&self) -> Res<Option<State>> {
        self.host.state(self)
    }

    pub fn compacting(&self, msg: &str) -> Res<bool> {
        self.host.compacting(self, msg)
    }

    pub fn shell_running(&self) -> Res<bool> {
        self.host.shell_running(self)
    }

    pub fn question(&self) -> Res<Option<Prompt>> {
        self.host.question(self)
    }

    pub fn answer(&self, key: &str) -> Res<()> {
        self.host.answer(self, key)
    }

    pub fn has_draft(&self) -> Res<bool> {
        self.host.has_draft(self)
    }

    #[allow(dead_code)] // as Host::show
    pub fn show(&self, repo: &Repo) -> Res<()> {
        self.host.show(repo, self)
    }

    #[allow(dead_code)] // as Host::show
    pub fn hide(&self, repo: &Repo) -> Res<bool> {
        self.host.hide(repo, self)
    }

    pub fn close(&self) -> Res<()> {
        self.host.close(self, false)
    }

    /// close, last: after this command's other work (it may run in this agent).
    pub fn close_last(&self) -> Res<()> {
        self.host.close(self, true)
    }

    #[allow(dead_code)] // as Host::set_title
    pub fn set_title(&self, title: &str) -> Res<()> {
        self.host.set_title(self, title)
    }
}

#[cfg(test)]
#[path = "tests/test_host.rs"]
mod tests;
