//! The stepper on a terminal (`tour ID --view`): raw mode, its own screen, a key at a
//! time; what a key asks for (save a comment, tell the worker, send, accept, open the
//! task's pane or diff) is done here.

use super::screen::{diff_lines, Act, Mode, Stepper};
use super::{save_place, send_comments};
use crate::coordinators::home_of;
use crate::line_edit::{read_keys, ready, term_size, Decoder, Raw, PASTE_OFF, PASTE_ON};
use crate::rex::{alive, rex_focus};
use crate::tours::comments::{add_comment, read_comments};
use crate::tours::{step_diff, tour_diff, tour_path, tour_stale, write_task};
use bullpen_core::json::{obj, read_obj, s, truthy, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::{run_with, which};
use bullpen_core::text::{splitlines, strip};
use bullpen_core::{fail, Fail, Repo, Res};
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// The drafts as objects.
fn drafts(repo: &Repo, t: &Obj) -> Res<Vec<Obj>> {
    Ok(read_comments(repo, t)?
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| v.as_object().cloned().unwrap_or_default())
        .collect())
}

/// Run bullpen: whether it worked, and its last line.
fn bp(repo: &Repo, args: &[&str]) -> (bool, String) {
    let mut argv = vec![self_bin().to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let p = match run_with(&argv, Some(&repo.root), None, false) {
        Ok(p) => p,
        Err(e) => return (false, e.0),
    };
    let said = if !p.ok() && !p.stderr.is_empty() {
        &p.stderr
    } else {
        &p.stdout
    };
    let last = splitlines(strip(said)).last().copied().unwrap_or("");
    (p.ok(), last.strip_prefix("bullpen: ").unwrap_or(last).to_string())
}

fn draw(out: &mut impl Write, st: &mut Stepper) {
    let (cols, rows) = term_size();
    let lines = st.screen(cols, rows);
    let cursor = if st.mode == Mode::Read {
        "\x1b[?25l".to_string()
    } else {
        let (r, c) = st.cursor(cols, lines.len() as i64);
        format!("\x1b[?25h\x1b[{r};{c}H")
    };
    let _ = write!(out, "\x1b[H\x1b[2J{}{cursor}", lines.join("\r\n"));
    let _ = out.flush();
}

/// Run the stepper on the terminal until act says it's done (true). act does what the
/// stepper can't: save a comment, send, accept, close. moved(at) is told of each step
/// the person goes to.
fn stepper_tui(
    st: &mut Stepper,
    mut act: impl FnMut(&mut Stepper, Act) -> Res<bool>,
    mut moved: impl FnMut(usize) -> Res<()>,
) -> Res<()> {
    let raw = Raw::new(0).map_err(|e| Fail(format!("couldn't set up the terminal: {e}")))?;
    let mut out = std::io::stdout();
    let mut decoder = Decoder::default();
    // its own screen; long lines are cut, not wrapped
    let _ = write!(out, "\x1b[?1049h\x1b[?7l{PASTE_ON}");
    let mut run = || -> Res<()> {
        loop {
            draw(&mut out, st);
            if !ready(0, 500) {
                continue; // redraw: the pane may have changed size
            }
            let rows = term_size().1;
            for k in read_keys(0, &mut decoder) {
                let at = st.at;
                if let Some(a) = st.key(&k, rows - 4) {
                    if act(st, a)? {
                        return Ok(());
                    }
                }
                if st.at != at {
                    moved(st.at)?;
                }
            }
        }
    };
    let done = run();
    drop(raw);
    let mut out = std::io::stdout();
    let _ = write!(out, "{PASTE_OFF}\x1b[?7h\x1b[?25h\x1b[?1049l");
    let _ = out.flush();
    done
}

/// Step through the task's tour on this terminal: the board's g opens it in a pane.
/// back: focus the task's board after, unless it closed for the task's pane or diff.
pub fn view_tour(repo: &Repo, t: &mut Obj, back: bool) -> Res<()> {
    let id = s(t, "id").to_string();
    let Some(tour) = read_obj(&tour_path(repo, t))?.filter(|o| !o.is_empty()) else {
        return fail(format!("{id} has no tour: `bullpen tour {id}` writes one"));
    };
    // SAFETY: isatty only looks at the fd
    if unsafe { libc::isatty(0) } != 1 {
        return fail("the tour needs a terminal (the board's g opens it in a pane)");
    }
    let commit = tour.get("commit").cloned().unwrap_or(Value::Null);
    let run = obj(t, "tour");
    if s(run, "state") == "written" && !truthy(run.get("stale")) && tour_stale(repo, t, &tour)?.is_none() {
        // the board marks it toured (not for the old code's tour)
        if let Some(Value::Object(run)) = t.get_mut("tour") {
            run.insert("opened".into(), commit.clone());
        }
        write_task(repo, t)?;
    }
    let comments = read_comments(repo, t)?;
    let place = obj(&comments, "at");
    let at = if place.get("commit").unwrap_or(&Value::Null) == &commit {
        place.get("step").and_then(Value::as_i64).unwrap_or(1) - 1
    } else {
        0
    };
    let wt = s(t, "worktree");
    let files = if !wt.is_empty() && Path::new(wt).is_dir() {
        tour_diff(repo, t, &tour)?
    } else {
        Vec::new()
    };
    let delta = which("delta");
    let steps: Vec<Obj> = tour
        .get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| v.as_object().cloned().unwrap_or_default())
        .collect();
    let diff = Box::new(move |i: usize, w: i64| diff_lines(&step_diff(&files, &steps[i]), w, delta.as_deref()));
    let title = t.get("title").map(bullpen_core::json::py_str).unwrap_or_default();
    let mut st = Stepper::new(&id, &title, &tour, drafts(repo, t)?, at, diff);
    let left = std::cell::Cell::new(false); // closed for the task's pane or diff: the board isn't focused after
    let act = |st: &mut Stepper, a: Act| -> Res<bool> {
        match a {
            Act::Close => Ok(true),
            Act::Open | Act::Diff => {
                // as the board's ⏎ and v: the worker's pane (the reviewer's while in review), or the diff
                let args: &[&str] = if a == Act::Diff {
                    &["diff", &id, "--pane"]
                } else if s(&repo.task(&id)?, "status") == "review" {
                    &["go", &id, "--review"]
                } else {
                    &["go", &id]
                };
                let (ok, said) = bp(repo, args);
                if ok {
                    left.set(true);
                    return Ok(true);
                }
                st.msg = if said.is_empty() {
                    format!("couldn't open {id}")
                } else {
                    said
                };
                Ok(false)
            }
            Act::Tell(text) => {
                let (ok, said) = bp(repo, &["tell", &id, &text]);
                st.msg = if ok { format!("Sent to {id}.") } else { said };
                Ok(false)
            }
            Act::Comment(text) => {
                let n = add_comment(repo, &repo.task(&id)?, st.at as i64 + 1, &text)?;
                st.comments = drafts(repo, &repo.task(&id)?)?;
                st.msg = format!("Saved comment {n} on step {}.", st.at + 1);
                Ok(false)
            }
            Act::Send => match send_comments(repo, &mut repo.task(&id)?) {
                Ok(_) => Ok(true),
                Err(e) => {
                    st.msg = e.0;
                    Ok(false)
                }
            },
            Act::Accept => {
                st.msg = format!("Accepting {id}…");
                let (cols, rows) = term_size();
                let mut out = std::io::stdout();
                let _ = write!(out, "\x1b[H\x1b[2J{}", st.screen(cols, rows).join("\r\n"));
                let _ = out.flush();
                let (ok, said) = bp(repo, &["accept", &id]);
                if ok {
                    return Ok(true);
                }
                st.msg = if said.is_empty() { "accept failed".into() } else { said };
                Ok(false)
            }
        }
    };
    let task = t.clone();
    stepper_tui(&mut st, act, |at| save_place(repo, &task, &commit, at))?;
    if back && !left.get() {
        let home = home_of(repo, t)?;
        let b = obj(&home, "board");
        if alive(s(b, "session"), s(b, "block"))? {
            rex_focus(s(b, "session"), s(b, "block"))?;
        }
    }
    Ok(())
}
