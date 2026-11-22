//! Rex as a host: an agent is a pane (a block, in a session), started where its placement
//! puts it (`panes/`). What it's doing and asking is read off its screen and Rex's
//! status record of its program (`screen/`); messages are typed into it. Rex's own
//! commands are `crate::rex`'s.

use super::{Agent, Host, State, SETTLE};
use crate::rex;
use crate::screen::prompt::{read_prompt, Prompt};
use crate::screen::{compacting_in, screen_state, shell_running};
use bullpen_core::json::{py_str, truthy, Obj};
use bullpen_core::timeutil::seconds_since;
use bullpen_core::{Repo, Res};

pub struct Rex;

pub static REX: Rex = Rex;

/// A record's value as text: "" when it's falsy.
fn text_of(r: &Obj, key: &str) -> String {
    r.get(key).filter(|v| truthy(Some(v))).map_or(String::new(), py_str)
}

/// A state with no kind, message or age.
fn bare(state: &'static str) -> State {
    State {
        state,
        kind: String::new(),
        msg: String::new(),
        age: None,
    }
}

impl Host for Rex {
    fn name(&self) -> &'static str {
        "rex"
    }

    fn start(
        &self,
        repo: &Repo,
        t: &Obj,
        within: &str,
        label: &str,
        argv: &[String],
        role: &str,
    ) -> Res<(Agent, &'static str)> {
        let (block, at) = crate::panes::place(repo, t, within, label, argv, role, true, false)?;
        Ok((Agent::on_rex(within, &block), at))
    }

    fn alive(&self, a: &Agent) -> Res<bool> {
        rex::alive(&a.within, &a.id)
    }

    fn tell(&self, a: &Agent, msg: &str) -> Res<Option<String>> {
        rex::type_into(&a.within, &a.id, msg)
    }

    fn send(&self, a: &Agent, msg: &str) -> Res<()> {
        rex::rex_type(&a.within, &a.id, msg)
    }

    fn read(&self, a: &Agent) -> Res<String> {
        rex::screen(&a.within, &a.id)
    }

    fn read_end(&self, a: &Agent, n: i64, colour: bool) -> Res<Vec<String>> {
        rex::screen_end(&a.within, &a.id, n, colour)
    }

    fn status(&self, a: &Agent) -> Res<Option<Obj>> {
        rex::program_status(&a.within, &a.id)
    }

    /// By its status record, else its screen.
    fn state(&self, a: &Agent) -> Res<Option<State>> {
        if !self.alive(a)? {
            return Ok(Some(bare("gone")));
        }
        let mut st = match self.status(a)?.filter(|r| !r.is_empty()) {
            Some(r) => {
                let age = truthy(r.get("updated_at")).then(|| seconds_since(&text_of(&r, "updated_at"), None));
                let state = match text_of(&r, "state").as_str() {
                    "working" => "working",
                    "blocked" => "blocked",
                    "error" => "error",
                    _ => "idle",
                };
                if state != "working" && age.is_some_and(|a| a < SETTLE) {
                    // its event may still be on the way (the watcher holds an end of turn back).
                    // Not working: the watcher forwards that at once, and a busy agent's record
                    // is new every few seconds, as its message changes.
                    return Ok(None);
                }
                State {
                    state,
                    kind: text_of(&r, "kind"),
                    msg: text_of(&r, "msg"),
                    age,
                }
            }
            None => match screen_state(&self.read(a)?) {
                Some(state) => bare(state),
                None => return Ok(None),
            },
        };
        if st.state == "idle" && self.shell_running(a)? {
            st.state = "shell";
        }
        Ok(Some(st))
    }

    /// Claude Code's status record says only that it's working, so this goes by its
    /// message, else its spinner line: "✽ Compacting conversation…" (or the PreCompact
    /// hooks it runs first).
    fn compacting(&self, a: &Agent, msg: &str) -> Res<bool> {
        if compacting_in(msg, "") {
            return Ok(true);
        }
        Ok(compacting_in("", &self.read(a)?))
    }

    fn shell_running(&self, a: &Agent) -> Res<bool> {
        Ok(shell_running(&self.read(a)?))
    }

    fn question(&self, a: &Agent) -> Res<Option<Prompt>> {
        Ok(read_prompt(&self.read(a)?))
    }

    /// Press the option's key, as the person would in its pane.
    fn answer(&self, a: &Agent, key: &str) -> Res<()> {
        rex::rex_send(&a.within, &a.id, key)
    }

    fn has_draft(&self, a: &Agent) -> Res<bool> {
        Ok(crate::news::draft_in(&rex::capture_vt(&a.within, &a.id)?))
    }

    fn show(&self, repo: &Repo, a: &Agent) -> Res<()> {
        crate::panes::show(repo, &a.within, &a.id)
    }

    fn hide(&self, repo: &Repo, a: &Agent) -> Res<bool> {
        crate::panes::hide(repo, &a.within, &a.id)
    }

    fn close(&self, a: &Agent, last: bool) -> Res<()> {
        if last {
            rex::close_last(&a.within, &a.id)
        } else {
            rex::rex_close(&a.within, &a.id)
        }
    }

    /// Nothing: a pane's label is how bullpen finds it again after a Rex restart, so it
    /// keeps its own.
    fn set_title(&self, _a: &Agent, _title: &str) -> Res<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/test_rex.rs"]
mod tests;
