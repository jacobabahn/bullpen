//! What each key does: a mode's keys while one is open, else the board's and the
//! selected task's (or the selected backlog item's: later.rs).

use super::jobs::{last_line, run};
use super::{App, Job, Kind, Mode, Note};
use crate::edit::LineEdit;
use crate::model::{Action, Layout, Question, Task, TourShow};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

/// Does this key do something to a task (or leave the board for the coordinator)?
/// Esc counts too: it puts a failure away.
fn acts_on_task(code: KeyCode, ch: Option<char>) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Esc) || ch.is_some_and(|c| "adsrpvtigcyJK".contains(c))
}

/// A key in a peek: None stays in it, Some(false) goes back to the board, Some(true)
/// goes back and does what the key does there.
fn peek_key(code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => Some(false),
        KeyCode::Enter | KeyCode::Char('t' | 'i' | 'a' | 'd' | 'v') => Some(true),
        _ => None,
    }
}

impl App {
    /// Open what `i` answers on the task: its agent's prompt, or a line for the
    /// coordinator's question.
    fn answer(&mut self, t: &Task) {
        match t.question() {
            Some(Question::Prompt) => {
                self.mode = Mode::Answer { id: t.id.clone(), title: format!("{}  {}", t.id, t.title), prompt: None };
                self.peek_due = Instant::now();
                self.peek();
            }
            Some(Question::Ask) => self.mode = Mode::Tell { id: t.id.clone(), text: LineEdit::default(), reply: true },
            None if t.needs_you() => self.msg = Note::info(format!("{} asks nothing here: ⏎ opens it.", t.id)),
            None => self.msg = Note::info(format!("{} doesn't need you.", t.id)),
        }
    }

    /// A key while a prompt is shown: an option's key presses it (if the prompt is
    /// still on its screen), ⏎ opens the pane, esc or q goes back.
    fn answer_key(&mut self, code: KeyCode) {
        let Mode::Answer { id, prompt, .. } = &self.mode else { return };
        let (id, prompt) = (id.clone(), prompt.clone());
        let reviewer = prompt.as_ref().is_some_and(|p| p.who == "reviewer");
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                let (ok, out) = self.go(&id, reviewer);
                self.msg = Note::of(ok, format!("Opened {id}."), out);
            }
            KeyCode::Char(c) => {
                let Some(p) = prompt else { return };
                let (Some((key, label)), Some(pid)) = (p.option(c), p.id.as_deref()) else { return };
                self.mode = Mode::Normal;
                let (ok, out) = self.bp(&["answer", &id, key, "--prompt", pid]);
                self.msg = Note::of(ok, format!("Answered {id}: {label}."), out);
            }
            _ => {}
        }
    }

    /// g: the task's guided tour. A current one opens; with none, one is written and
    /// opens when it's done; a stale one asks first.
    fn tour(&mut self, t: &Task) {
        if !t.tourable() {
            self.msg = Note::info(match t.status.as_str() {
                "finished" => format!("{} has no code to tour.", t.id),
                _ => format!("{} gets a tour once it's in review.", t.id),
            });
            return;
        }
        if !t.tour_writing() {
            let (bullpen, root) = (self.bullpen.clone(), self.root.clone());
            let (ok, out) = run(&bullpen, &root, &["tour".into(), t.id.clone(), "--show".into(), "--json".into()]);
            if !ok {
                self.msg = Note::failed(last_line(&out));
                return;
            }
            if TourShow::parse(&out).is_some_and(|s| s.state == "stale") {
                self.mode = Mode::Stale { id: t.id.clone() };
                self.msg = Note::info(format!("{}'s tour is stale: the code changed.", t.id));
                return;
            }
        }
        self.open_tour(&t.id, false);
    }

    /// `bullpen go <id>`: the task's pane (its reviewer's), and with go_switch the Rex
    /// app goes to its session.
    fn go(&self, id: &str, reviewer: bool) -> (bool, String) {
        let mut args = vec!["go", id];
        if reviewer {
            args.push("--review");
        }
        if self.go_switch {
            args.push("--switch");
        }
        self.bp(&args)
    }

    /// `bullpen tour <id> --pane`: open the stepper, or start writing a tour (new) that opens when written.
    fn open_tour(&mut self, id: &str, new: bool) {
        let mut args = vec!["tour", id, "--pane"];
        if new {
            args.push("--new");
        }
        let (ok, out) = self.bp(&args);
        self.msg = Note::of(ok, format!("{}.", bullpen_core::text::upper_first(&out)), out);
    }

    /// A key while g asks about a stale tour.
    fn stale_key(&mut self, ch: Option<char>) {
        let Mode::Stale { id } = &self.mode else { return };
        let id = id.clone();
        self.mode = Mode::Normal;
        match ch {
            Some('n') => self.open_tour(&id, true),
            Some('o') => self.open_tour(&id, false),
            _ => self.msg = Note::info("Cancelled."),
        }
    }

    /// Handle a key. False when the board should quit.
    pub(crate) fn key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return false;
        }
        match &self.mode {
            Mode::Normal => {}
            Mode::Overlay { .. } => {
                if !self.overlay_key(k.code) {
                    return true;
                }
            }
            Mode::Quit => return self.quit_key(k.code),
            Mode::Help { .. } => {
                self.help_key(k.code);
                return true;
            }
            Mode::Answer { .. } => {
                self.answer_key(k.code);
                return true;
            }
            Mode::Stale { .. } => {
                self.stale_key(if let KeyCode::Char(c) = k.code { Some(c) } else { None });
                return true;
            }
            Mode::Tell { .. } => {
                self.tell_key(k);
                return true;
            }
        }
        self.board_key(k)
    }

    /// A key on a full-screen text. True when it goes on to act on the board: a key
    /// that leaves a peek for the board acts there on the peeked task.
    fn overlay_key(&mut self, code: KeyCode) -> bool {
        let Mode::Overlay { peek, .. } = &self.mode else { return false };
        let Some(id) = peek.clone() else {
            self.mode = Mode::Normal;
            return false;
        };
        let Some(back) = peek_key(code) else { return false };
        self.mode = Mode::Normal;
        if !back {
            return false;
        }
        // back on the board with the peeked task selected, the key acts on it
        self.sel = Some(id);
        true
    }

    /// A key while q asks to be sure: y quits (false), any other stays.
    fn quit_key(&mut self, code: KeyCode) -> bool {
        if matches!(code, KeyCode::Char('y' | 'Y')) {
            return false;
        }
        self.mode = Mode::Normal;
        self.msg = Note::info("Cancelled.");
        true
    }

    /// A key in the `?` list: scroll keys scroll, any other closes it.
    fn help_key(&mut self, code: KeyCode) {
        // the draw keeps it in range: it knows how many lines the list wraps to
        let page = usize::from(self.height.saturating_sub(4)).max(1);
        let Mode::Help { top } = &mut self.mode else { return };
        match code {
            KeyCode::Down | KeyCode::Char('j') => *top += 1,
            KeyCode::Up | KeyCode::Char('k') => *top = top.saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(' ') => *top += page,
            KeyCode::PageUp | KeyCode::Char('b') => *top = top.saturating_sub(page),
            _ => self.mode = Mode::Normal,
        }
    }

    /// A key while typing a line to tell a worker or answer its coordinator: ⏎ sends
    /// it, esc drops it.
    fn tell_key(&mut self, k: KeyEvent) {
        let Mode::Tell { id, text, reply } = &mut self.mode else { return };
        if text.key(k) {
            return;
        }
        match k.code {
            KeyCode::Enter => {
                let (id, text, reply) = (id.clone(), text.text().trim().to_string(), *reply);
                self.mode = Mode::Normal;
                if !text.is_empty() {
                    let (ok, out) = self.bp(&[if reply { "reply" } else { "tell" }, &id, &text]);
                    let sent = if reply { format!("Answer sent to {id}'s coordinator.") } else { format!("Sent to {id}.") };
                    self.msg = Note::of(ok, sent, out);
                }
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
    }

    /// A key on the board itself: moving, the board's keys, then the selected task's.
    fn board_key(&mut self, k: KeyEvent) -> bool {
        let ch = match k.code {
            KeyCode::Char(c) => Some(c),
            _ => None,
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // a failure stays while you look around, until the next action on a task (or esc)
        if self.msg.kind != Kind::Failed || self.pending.is_some() || acts_on_task(k.code, ch.filter(|_| !ctrl)) {
            self.msg.clear();
        }

        if let Some((action, tid)) = self.pending.take() {
            if !matches!(ch, Some('y' | 'Y')) {
                self.msg = Note::info("Cancelled.");
                return true;
            }
            let job = match action {
                Action::Accept => Job::Accept,
                Action::Close => Job::Close,
                Action::Drop => Job::Drop,
                Action::DropForce => Job::DropForce,
                Action::DropItem => {
                    self.drop_item(&tid);
                    return true;
                }
            };
            self.spawn(&tid, job);
            return true;
        }

        if ch == Some('L') && !ctrl {
            self.toggle_later();
            return true;
        }
        // an item of the Later group selected: its keys first (its n drops, its s starts)
        if !ctrl && self.item_key(k.code, ch) {
            return true;
        }

        let ids = self.nav_ids();
        let i = ids.iter().position(|x| Some(x) == self.sel.as_ref()).unwrap_or(0);
        // the selection moves; the list scrolls to keep it in view
        let page = self.page.max(1);
        let to = |n: usize| ids.get(n.min(ids.len().saturating_sub(1))).cloned();
        match (k.code, ch) {
            // before the letters: ctrl+d isn't d (drop)
            (_, Some('d')) if ctrl => self.sel = to(i + (page / 2).max(1)),
            (_, Some('u')) if ctrl => self.sel = to(i.saturating_sub((page / 2).max(1))),
            (_, Some(_)) if ctrl => {}
            (_, Some('q')) => self.mode = Mode::Quit,
            (KeyCode::Down, _) | (_, Some('j')) => self.sel = to(i + 1),
            (KeyCode::Up, _) | (_, Some('k')) => self.sel = to(i.saturating_sub(1)),
            (KeyCode::PageDown, _) => self.sel = to(i + page),
            (KeyCode::PageUp, _) => self.sel = to(i.saturating_sub(page)),
            (KeyCode::End, _) | (_, Some('G')) => self.sel = to(usize::MAX),
            (KeyCode::Home, _) => self.sel = to(0),
            (_, Some('/')) => {
                // a floating pane over this window; it goes where you choose, or esc
                let (ok, out) = self.bp(&["pick", "--pane"]);
                if !ok {
                    self.msg = Note::failed(out);
                }
            }
            (_, Some('?')) => {
                self.mode = Mode::Help { top: 0 }
            }
            (_, Some('l')) => {
                let layout = self.cfg.layout.other();
                self.cfg.layout = layout;
                self.top = 0;
                let (ok, out) = self.bp(&["config", "--board-layout", layout.name()]);
                let said = match layout {
                    Layout::Grouped => "Grouped layout.",
                    Layout::Compact => "Compact layout.",
                };
                self.msg = if ok { Note::info(said) } else { Note::failed(out) };
            }
            (_, Some('I')) => self.switch_look(),
            (_, Some('f')) => {
                self.closed = !self.closed;
                self.top = 0;
                // a failure stays while you look around
                if self.msg.kind != Kind::Failed {
                    self.msg = Note::info(if self.closed { "Showing closed tasks too." } else { "Hiding closed tasks." });
                }
            }
            (_, Some('o')) => self.toggle_others(),
            (_, Some('c')) => {
                let (ok, out) = self.bp(&["go"]);
                self.msg = Note::of(ok, "Opened the coordinator.", out);
            }
            (_, Some('n')) => match self.next_up(i, None) {
                Some((id, true)) => self.sel = Some(id),
                Some((id, false)) => {
                    let st = self.tasks.iter().find(|t| t.id == id).map_or("", |t| t.status.as_str());
                    self.msg = Note::info(format!("Nothing needs you: {id} is {}.", if st == "ready" { "ready" } else { "finished" }));
                    self.sel = Some(id);
                }
                None => self.msg = Note::info("Nothing needs you."),
            },
            _ => self.task_key(k.code, ch),
        }
        true
    }

    /// Keys about the selected task.
    fn task_key(&mut self, code: KeyCode, ch: Option<char>) {
        let Some(cur) = self.current().cloned() else { return };
        let sel = cur.id.as_str();
        match (code, ch) {
            (KeyCode::Enter, _) => {
                let reviewer = cur.reviewers_turn();
                // a pane that opens in another tab shows nothing here: say it worked
                self.msg = if reviewer {
                    let (ok, out) = self.go(sel, true);
                    Note::of(ok, format!("Opened {sel}'s reviewer."), out)
                } else if cur.block.is_some() {
                    let (ok, out) = self.go(sel, false);
                    Note::of(ok, format!("Opened {sel}."), out)
                } else {
                    Note::info(match cur.status.as_str() {
                        "todo" => "Not started yet: s starts it.".into(),
                        "stopped" => "Its agent exited: r resumes it.".into(),
                        _ => format!("{sel} has no pane open."),
                    })
                };
            }
            (_, Some('a')) => {
                let action = match cur.status.as_str() {
                    "ready" => Action::Accept,
                    "finished" => Action::Close,
                    _ => {
                        self.msg = Note::info(if cur.closed() { format!("{sel} is closed already.") } else { format!("{sel} isn't ready yet.") });
                        return;
                    }
                };
                if !self.still(sel) {
                    self.pending = Some((action, sel.into()));
                }
            }
            (_, Some('d')) if !cur.closed() => {
                if !self.still(sel) {
                    self.pending = Some((Action::Drop, sel.into()));
                }
            }
            (_, Some('s')) => self.spawn(sel, Job::Start),
            (_, Some('r')) => self.spawn(sel, Job::Resume),
            (_, Some('p')) => {
                // in review, the reviewer's screen: what ⏎ opens
                self.peek_review = cur.reviewers_turn();
                let whose = if self.peek_review { " · its reviewer" } else { "" };
                self.mode = Mode::Overlay {
                    title: format!("{sel}  {}{whose}", cur.title),
                    lines: vec!["…".into()],
                    peek: Some(sel.into()),
                };
                self.peek_failed = false;
                self.peek_due = Instant::now();
                self.peek();
            }
            (_, Some('v')) => {
                let mut args = vec!["diff", sel, "--pane"];
                if let Some(session) = self.pane_session.as_deref() {
                    args.extend(["--session", session]);
                }
                let (ok, out) = self.bp(&args);
                self.msg = Note::of(ok, format!("Opened {sel}'s diff."), out);
            }
            (_, Some('t')) => self.mode = Mode::Tell { id: sel.into(), text: LineEdit::default(), reply: false },
            (_, Some('i')) => self.answer(&cur),
            (_, Some('g')) => self.tour(&cur),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "tests/test_keys.rs"]
mod tests;
