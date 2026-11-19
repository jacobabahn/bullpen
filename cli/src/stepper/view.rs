//! The stepper on a terminal (`tour ID --view`): raw mode, its own screen, a key at a
//! time; what a key asks for (save a comment, tell the worker, send, accept, open the
//! task's pane or diff) is done here. With `board_look image` the tour is drawn as
//! images (look/) when the terminal says it draws them, else it's text, saying why.

use super::look::ink::Type;
use super::look::{ask, Look, PatchFn};
use super::screen::{diff_lines, Act, Mode, Stepper};
use super::{save_place, send_comments};
use crate::coordinators::home_of;
use crate::line_edit::{read_keys, ready, split_keys, term_size, Decoder, Raw, PASTE_OFF, PASTE_ON};
use crate::rex::{alive, rex_focus};
use crate::tours::comments::{add_comment, read_comments};
use crate::tours::{step_diff, tour_diff, tour_path, tour_stale, update_task};
use bullpen_core::json::{obj, read_obj, s, truthy, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::{run_with, which};
use bullpen_core::text::{splitlines, strip};
use bullpen_core::{fail, Fail, Repo, Res};
use bullpen_draw::font::Fonts;
use bullpen_draw::theme::Theme;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

/// How the tour is shown: as text, or drawn (look/).
enum Show {
    Text,
    Image(Box<Look>),
}

/// The drawn look, wanted: the font `board_font` names, whether `board_text_size` is
/// large (small is medium here: the tour's prose is never under the board's medium),
/// and each step's patch.
pub struct Wanted {
    font: Option<PathBuf>,
    large: bool,
    patch: PatchFn,
}

fn draw(out: &mut impl Write, st: &mut Stepper, show: &mut Show) {
    let (cols, rows) = term_size();
    if let Show::Image(look) = show {
        let Err(e) = look.frame(st, (cols, rows), out) else {
            return;
        };
        // an image it can't make (or write): the rest of the tour is text
        let _ = look.close(out);
        *show = Show::Text;
        st.msg = format!("Drawing the tour as text: {e}.");
    }
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

/// The drawn look, when it's wanted and the terminal draws images; else text, and why
/// on the message line. The keys typed while the terminal was asked are kept.
fn image_look(st: &mut Stepper, wanted: Option<Wanted>, out: &mut impl Write, typed: &mut Vec<String>) -> Show {
    let Some(w) = wanted else { return Show::Text };
    let (a, keys) = ask::ask(0, out);
    typed.extend(split_keys(&String::from_utf8_lossy(&keys)));
    let why = if !a.graphics {
        "the terminal doesn't draw images"
    } else if ask::window_cell(0).or(a.cell).is_none() {
        "the terminal doesn't say its cell size in pixels"
    } else {
        match Fonts::load(w.font.as_deref()) {
            Ok(fonts) => {
                let cell = ask::window_cell(0).or(a.cell).unwrap_or((8, 17));
                let theme = Theme::default().with(&a.said);
                let ty = Type {
                    large: w.large,
                    ..Type::new(fonts)
                };
                return Show::Image(Box::new(Look::new(theme, cell, ty, w.patch)));
            }
            Err(e) => {
                st.msg = format!("Drawing the tour as text: {e}.");
                return Show::Text;
            }
        }
    };
    st.msg = format!("Drawing the tour as text: {why}.");
    Show::Text
}

/// Run the stepper on the terminal until act says it's done (true). act does what the
/// stepper can't: save a comment, send, accept, close. moved(at) is told of each step
/// the person goes to. With the drawn look wanted, the terminal is asked first.
fn stepper_tui(
    st: &mut Stepper,
    wanted: Option<Wanted>,
    mut act: impl FnMut(&mut Stepper, Act) -> Res<bool>,
    mut moved: impl FnMut(usize) -> Res<()>,
) -> Res<()> {
    let raw = Raw::new(0).map_err(|e| Fail(format!("couldn't set up the terminal: {e}")))?;
    let mut out = std::io::stdout();
    let mut decoder = Decoder::default();
    // its own screen; long lines are cut, not wrapped
    let _ = write!(out, "\x1b[?1049h\x1b[?7l{PASTE_ON}");
    let mut typed = Vec::new();
    let mut show = image_look(st, wanted, &mut out, &mut typed);
    let mut run = || -> Res<()> {
        let mut size = (0, 0);
        let mut dirty = true;
        loop {
            let now = term_size();
            if dirty || now != size || matches!(show, Show::Text) {
                draw(&mut out, st, &mut show);
                (size, dirty) = (now, false);
            }
            if typed.is_empty() {
                if let Show::Image(look) = &mut show {
                    let _ = look.ahead(st, &|| ready(0, 0), &mut out);
                }
                if !ready(0, 500) {
                    continue; // redraw: the pane may have changed size
                }
                typed = read_keys(0, &mut decoder);
            }
            let rows = term_size().1;
            let page = if matches!(show, Show::Image(_)) {
                rows - 2
            } else {
                rows - 4
            };
            for k in std::mem::take(&mut typed) {
                let at = st.at;
                if let Some(a) = st.key(&k, page) {
                    if a == Act::Accept {
                        st.msg = format!("Accepting {}…", st.tid);
                        draw(&mut out, st, &mut show);
                    }
                    if act(st, a)? {
                        return Ok(());
                    }
                }
                if st.at != at {
                    moved(st.at)?;
                }
            }
            dirty = true;
        }
    };
    let done = run();
    if let Show::Image(look) = &mut show {
        let _ = look.close(&mut out);
    }
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
        // the board marks it toured (not for the old code's tour): if it is still this
        // tour, under the tours' lock, and not one being written since the task was read
        *t = update_task(repo, &id, |t| match t.get_mut("tour") {
            Some(Value::Object(run))
                if run.get("state") == Some(&json!("written"))
                    && !truthy(run.get("stale"))
                    && run.get("commit") == Some(&commit) =>
            {
                run.insert("opened".into(), commit.clone());
                true
            }
            _ => false,
        })?;
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
    let (files, steps) = (Rc::new(files), Rc::new(steps));
    let wanted = (repo.cfg_str("board_look") == "image").then(|| {
        let (files, steps) = (files.clone(), steps.clone());
        let font = repo
            .cfg
            .get("board_font")
            .and_then(Value::as_str)
            .filter(|f| !f.is_empty())
            .map(PathBuf::from);
        Wanted {
            font,
            large: repo.cfg_str("board_text_size") == "large",
            patch: Box::new(move |i: usize| step_diff(&files, &steps[i])),
        }
    });
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
    stepper_tui(&mut st, wanted, act, |at| save_place(repo, &task, &commit, at))?;
    if back && !left.get() {
        let home = home_of(repo, t)?;
        let b = obj(&home, "board");
        if alive(s(b, "session"), s(b, "block"))? {
            rex_focus(s(b, "session"), s(b, "block"))?;
        }
    }
    Ok(())
}
