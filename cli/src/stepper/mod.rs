//! `tour` (stepper.py and tours.py's command half): a tour of a task's change. On a
//! terminal a written tour opens in the stepper; off one (an agent, --path) its path is
//! printed; with none, the tour agent (`_tour`, tours/write.rs) starts writing one, in
//! the background or waited for with `--wait`. `--show`/`--step` print it for the board,
//! `--send` sends the person's draft comments back to the worker (`send_comments`).
//!
//! The board's g runs `tour <ID> --pane` (`open_stepper`): with a current tour (or a stale one, once the
//! person chose it), it opens the stepper in a large floating pane in the middle of the
//! window, the way the jump picker opens; with none, it starts the agent and the stepper
//! opens once the tour is written. The stepper (`tour <ID> --view`, view.rs) shows one
//! step at a time: its title and note, the person's comments on it, then its hunks from
//! git (in delta when installed). After the last step comes a summary of the comments,
//! from which they're sent back or the task accepted. Where the person was is kept with
//! the drafts, so opening the tour again resumes there.

pub mod screen;
mod view;

use crate::args::{flag, pos, usage_error, value, Arg, Args, Nargs, SUPPRESS};
use crate::coordinators::home_of;
use crate::jobs::{background, Job};
use crate::modes::task_base_ref;
use crate::news::kind::Kind;
use crate::news::read::py_int;
use crate::news::tell_coordinator;
use crate::review::close_reviewer;
use crate::rex::{find_pane, find_panes, rex_api, rex_close, rex_view, set_layer, with_state};
use crate::start::launch::hand_back;
use crate::stops::stop;
use crate::tours::comments::read_comments;
use crate::tours::{comments_path, step_diff, step_files, tour_diff, tour_failed, tour_log, tour_path};
use crate::tours::{tour_state, write_task};
use bullpen_core::json::{self, obj, py_str, s, truthy, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::{head, merge_base};
use bullpen_core::repo::repo_root;
use bullpen_core::settings::TERMINAL;
use bullpen_core::task::{clip, is_status};
use bullpen_core::text::rjust;
use bullpen_core::timeutil::iso_now;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// most of the window: x, y, w, h
const TOUR_BOUNDS: [f64; 4] = [0.05, 0.04, 0.9, 0.9];

const TOUR: &[Arg] = &[
    pos("id", Nargs::One, "the task, like BP-3"),
    flag("--wait", "write it now and print its path, not in the background"),
    flag("--new", "write a new tour even though there is one"),
    flag("--show", "the tour's state and steps, starting nothing"),
    value("--step", "N", "step N's hunks from git, as a patch"),
    flag("--json", "with --show or --step: as JSON (for the board)"),
    flag(
        "--send",
        "send your draft comments to the worker: the task goes back to working",
    ),
    flag(
        "--pane",
        "step through it in a floating pane (the board's g); with no tour, write one and open it when it's written",
    ),
    flag("--view", "step through it on this terminal"),
    flag(
        "--path",
        "print a written tour's path, even on a terminal (where it opens by default)",
    ),
    flag("--back", SUPPRESS), // then focus the board
    value(
        "--from",
        "FROM_BLOCK",
        "with --pane: the pane whose window it opens over (default: this one)",
    ),
    value("--session", "SESSION", "with --pane: the session you're in"),
];

fn isatty(fd: i32) -> bool {
    // SAFETY: isatty only looks at the fd
    unsafe { libc::isatty(fd) == 1 }
}

fn some(v: Option<&str>) -> Option<&str> {
    v.filter(|v| !v.is_empty())
}

/// `tour`: a tour of a task's change: step through the current one (its path, off a
/// terminal), or start writing one.
pub fn cmd_tour(argv: &[String]) -> Res<i32> {
    let a = Args::parse("tour", argv, TOUR);
    let step = a.opt("--step").map(|v| match py_int(v) {
        Some(n) => n,
        None => usage_error("tour", TOUR, &format!("argument --step: invalid int value: '{v}'")),
    });
    let from_env = std::env::var("REX_BLOCK").ok();
    let from_block = a.opt("--from").map(str::to_string).or(from_env);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    if a.flag("--send") {
        println!("{}", send_comments(&repo, &mut t)?);
        return Ok(0);
    }
    if a.flag("--show") || step.is_some() {
        show_tour(&repo, &mut t, step, a.flag("--json"))?;
        return Ok(0);
    }
    if a.flag("--view") {
        view::view_tour(&repo, &mut t, a.flag("--back"))?;
        return Ok(0);
    }
    tour_status(&t)?;
    let (state, _, why) = tour_state(&repo, &mut t)?;
    let path = tour_path(&repo, &t);
    if a.flag("--pane") {
        let session = some(a.opt("--session"))
            .map(str::to_string)
            .or_else(|| std::env::var("REX_SESSION").ok().filter(|v| !v.is_empty()))
            .map(Value::from)
            .unwrap_or_else(|| t.get("session").cloned().unwrap_or(Value::Null));
        tour_pane(&repo, &mut t, state, a.flag("--new"), session, from_block)?;
        return Ok(0);
    }
    // On a terminal, a written tour opens in the stepper; else (an agent, --path) its path.
    let here = !a.flag("--path") && isatty(0) && isatty(1);
    let shown = if here { "opens it" } else { "prints its path" };
    let written = |path: &Path| -> Res<i32> {
        if here {
            view::view_tour(&repo, &mut repo.task(&id)?, false)?;
        } else {
            println!("{}", path.display());
        }
        Ok(0)
    };
    let wait = a.flag("--wait");
    if state == "writing" && !wait {
        println!("{id}'s tour is being written: `bullpen tour {id}` {shown} once it's done");
        return Ok(0);
    }
    if state == "writing" {
        while tour_state(&repo, &mut t)?.0 == "writing" {
            std::thread::sleep(std::time::Duration::from_secs(1));
            t = repo.task(&id)?;
        }
        let (state, _, why) = tour_state(&repo, &mut t)?;
        if state == "failed" {
            return fail(format!(
                "{id}'s tour failed: {} (log: {})",
                why.unwrap_or_else(|| "None".into()),
                tour_log(&repo, &t).display()
            ));
        }
        return written(&path);
    }
    let new = a.flag("--new");
    if state == "current" && !new {
        return written(&path);
    }
    if state == "stale" && !new {
        let why = why.unwrap_or_default();
        let _ = std::io::stdout().flush();
        eprintln!("{id}'s tour is stale: {why}. `bullpen tour {id} --new` writes a new one; this is the old one:");
        return written(&path);
    }
    if let Some(out) = start_tour(&repo, &mut t, wait, None)? {
        return written(&out);
    }
    let model = repo.cfg.get("tour_model").filter(|v| truthy(Some(v))).map(py_str);
    println!(
        "writing a tour of {id} in the background ({}); `bullpen tour {id}` {shown} once it's done",
        model.as_deref().unwrap_or("default model")
    );
    Ok(0)
}

/// A tour is only for a task whose code isn't changing.
fn tour_status(t: &Obj) -> Res<()> {
    if !matches!(s(t, "status"), "ready" | "review") {
        return fail(format!(
            "{}: a tour is only for a task that is ready or in review, whose code isn't changing",
            is_status(t)
        ));
    }
    Ok(())
}

/// The board's g: open the stepper on a current tour (or a stale one: the board asked
/// first), or start writing a tour that opens when it's written.
fn tour_pane(repo: &Repo, t: &mut Obj, state: &str, new: bool, session: Value, from: Option<String>) -> Res<()> {
    let id = s(t, "id").to_string();
    let opener = json!({"session": session, "from": from});
    if state == "writing" {
        if let Some(Value::Object(run)) = t.get_mut("tour") {
            run.insert("open".into(), opener);
        }
        write_task(repo, t)?;
        println!("{id}'s tour is being written: it opens when it's done");
        return Ok(());
    }
    if matches!(state, "none" | "failed") || new {
        start_tour(repo, t, false, Some(opener))?;
        println!("writing a tour of {id}: it opens when it's done");
        return Ok(());
    }
    let session = if truthy(Some(&session)) {
        py_str(&session)
    } else {
        String::new()
    };
    open_stepper(repo, t, &session, from.as_deref().unwrap_or(""))?;
    let stale = if state == "stale" {
        " (stale: the code changed since it was written)"
    } else {
        ""
    };
    println!("opened {id}'s tour{stale}");
    Ok(())
}

pub fn tour_label(t: &Obj) -> String {
    format!("bullpen: {} tour", s(t, "id"))
}

/// The stepper in a floating pane over the window of from_block (else of the task's
/// board or coordinator), focused. One per task: an old one closes.
pub fn open_stepper(repo: &Repo, t: &Obj, session: &str, from_block: &str) -> Res<()> {
    let view = rex_view(session)?;
    for w in find_panes(view.as_ref(), &tour_label(t)) {
        rex_close(session, &w.block)?;
    }
    let me = self_bin().to_string_lossy().into_owned();
    let argv = vec![me, "tour".into(), s(t, "id").into(), "--view".into(), "--back".into()];
    let mut spec = json!({"bounds": TOUR_BOUNDS, "layout": {"block": {
        "flavor": TERMINAL, "label": tour_label(t),
        "options": {"command": with_state(&argv), "cwd": repo.root_str()}}}});
    let home = home_of(repo, t)?;
    let anchor = [from_block, s(obj(&home, "board"), "block"), s(&home, "block")]
        .into_iter()
        .find(|b| !b.is_empty())
        .unwrap_or("");
    let at = if anchor.is_empty() {
        None
    } else {
        find_pane(view.as_ref(), anchor, "")
    };
    if let Some(window) = at.and_then(|w| w.window) {
        spec["window_id"] = Value::from(window);
    }
    let r = rex_api(session, "session.new_layer", &spec, true)?;
    let layer = r.as_ref().and_then(|r| r.get("layer_id")).map_or(String::new(), py_str);
    set_layer(session, &layer, "normal", true)?;
    Ok(())
}

/// Write a tour of the task (`_tour`): now (wait), or by a process of its own in the
/// background. t["tour"] says it's being written, with that process's pid, so the board
/// can show it. With opener ({"session", "from"}), the stepper opens there once it's
/// written. Returns the tour's path when it waited.
fn start_tour(repo: &Repo, t: &mut Obj, wait: bool, opener: Option<Value>) -> Res<Option<PathBuf>> {
    let id = s(t, "id").to_string();
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).is_dir() {
        return fail(format!("{id} has no worktree, so there's nothing to tour"));
    }
    let mut run = json!({"state": "writing", "at": iso_now(), "pid": std::process::id()});
    if let Some(opener) = opener {
        run["open"] = opener;
    }
    t.insert("tour".into(), run);
    if s(t, "note").starts_with("tour failed") {
        t.insert("note".into(), Value::Null);
    }
    write_task(repo, t)?;
    let me = self_bin().to_string_lossy().into_owned();
    if wait {
        let _ = std::io::stdout().flush();
        let _ = std::process::Command::new(&me)
            .args(["_tour", &id])
            .current_dir(&repo.root)
            .stdin(Stdio::null())
            .status();
        let fresh = repo.task(&id)?;
        let run = obj(&fresh, "tour");
        if s(run, "state") == "written" {
            return Ok(Some(tour_path(repo, t)));
        }
        let mut why = run.get("why").map_or("None".into(), py_str);
        if s(run, "state") != "failed" {
            why = "its writer stopped before the tour was written".into();
            tour_failed(repo, &id, &why)?;
        }
        return fail(format!(
            "{id}'s tour failed: {why} (log: {})",
            tour_log(repo, t).display()
        ));
    }
    let log = repo.dir.join("jobs.log");
    let job = Job {
        argv: vec![me, "_tour".into(), id.clone()],
        cwd: &repo.root,
        log: &log,
        what: format!("Writing {id}'s tour"),
        task: Some(&id),
        pass_fds: vec![],
    };
    let pid = background(&job, false);
    if let Some(Value::Object(run)) = t.get_mut("tour") {
        run.insert("pid".into(), pid.map_or(Value::Null, Value::from));
    }
    write_task(repo, t)?;
    Ok(None)
}

/// The tour as it is, for the board: its state and steps (--json), or one step's hunks
/// from git (--step N), as a patch for delta or a pager.
fn show_tour(repo: &Repo, t: &mut Obj, step: Option<i64>, as_json: bool) -> Res<()> {
    let (state, tour, why) = tour_state(repo, t)?;
    let id = s(t, "id").to_string();
    let steps: Vec<Obj> = tour
        .as_ref()
        .and_then(|o| o.get("steps"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| v.as_object().cloned().unwrap_or_default())
        .collect();
    let field = |o: &Obj, k: &str| o.get(k).cloned().unwrap_or(Value::Null);
    let Some(n) = step else {
        let drafts = read_comments(repo, t)?
            .get("comments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let shown: Vec<Value> = steps
            .iter()
            .enumerate()
            .map(|(i, st)| {
                let on = drafts.iter().filter(|c| c.get("step") == Some(&json!(i + 1))).count();
                json!({"n": i + 1, "title": field(st, "title"), "note": field(st, "note"),
                       "hunks": field(st, "hunks"), "files": step_files(st), "comments": on})
            })
            .collect();
        if as_json {
            let tour_field = |k: &str| tour.as_ref().map_or(Value::Null, |o| field(o, k));
            let out = json!({"id": id, "state": state, "why": why,
                             "path": tour.as_ref().map(|_| tour_path(repo, t).display().to_string()),
                             "commit": tour_field("commit"), "written": tour_field("written"),
                             "comments": drafts.len(), "steps": shown});
            println!("{}", json::dumps(&out));
            return Ok(());
        }
        let why = why.map(|w| format!(" ({w})")).unwrap_or_default();
        println!("{id}'s tour: {state}{why}");
        for st in &shown {
            let on = st["comments"].as_u64().unwrap_or(0);
            let comments = match on {
                0 => String::new(),
                1 => "  (1 comment)".into(),
                n => format!("  ({n} comments)"),
            };
            println!(
                "{}. {}  [{}]{comments}",
                rjust(&py_str(&st["n"]), 3),
                py_str(&st["title"]),
                py_str(&st["files"])
            );
        }
        return Ok(());
    };
    let Some(tour) = tour else {
        return fail(format!("{id} has no tour: `bullpen tour {id}` writes one"));
    };
    if n < 1 || n > steps.len() as i64 {
        return fail(format!("{id}'s tour has steps 1 to {}", steps.len()));
    }
    let st = &steps[n as usize - 1];
    let wt = s(t, "worktree");
    let diff = if !wt.is_empty() && Path::new(wt).is_dir() {
        step_diff(&tour_diff(repo, t, &tour)?, st)
    } else {
        String::new()
    };
    if as_json {
        let out = json!({"n": n, "of": steps.len(), "title": field(st, "title"), "note": field(st, "note"),
                         "hunks": field(st, "hunks"), "files": step_files(st), "diff": diff});
        println!("{}", json::dumps(&out));
        return Ok(());
    }
    print!("{diff}");
    Ok(())
}

/// The findings-style file of the person's comments: each one with its step, files and lines.
fn comments_text(t: &Obj, comments: &[Obj]) -> String {
    let mut parts = vec![format!(
        "# The person's comments on {}, after a guided tour of the change\n\n\
         Each comment is on one step of the tour: the step's title, the files and lines it covers \
         (in the code the tour was written on), then the comment. A line the person points at \
         is in the comment's text.",
        s(t, "id")
    )];
    for (i, c) in comments.iter().enumerate() {
        let commit: String = s(c, "commit").chars().take(12).collect();
        let f = |k: &str| c.get(k).map_or("None".into(), py_str);
        parts.push(format!(
            "## Comment {}: step {}, {}\n\nFiles: {} (commit {commit})\n\n{}",
            i + 1,
            f("step"),
            f("title"),
            step_files(c),
            f("text")
        ));
    }
    parts.join("\n\n") + "\n"
}

/// Send the person's draft comments to the worker, as a findings-style file: the task
/// goes back to working, and its next ready gets a review of them. Returns what to say.
pub fn send_comments(repo: &Repo, t: &mut Obj) -> Res<String> {
    tour_status(t)?;
    let id = s(t, "id").to_string();
    let drafts: Vec<Obj> = read_comments(repo, t)?
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| v.as_object().cloned().unwrap_or_default())
        .collect();
    if drafts.is_empty() {
        return fail(format!(
            "{id} has no draft comments to send: `bullpen tour-comment {id} STEP TEXT` adds one"
        ));
    }
    let k = t.get("tours_sent").and_then(Value::as_i64).unwrap_or(0) + 1;
    let path = repo.dir.join("briefs").join(format!("{id}.tour-{k}.comments.md"));
    std::fs::write(&path, comments_text(t, &drafts))?;
    let wt = s(t, "worktree").to_string();
    let base_ref = task_base_ref(repo, t)?;
    let n = drafts.len();
    let said = screen::plural(n, "comment");
    let sent = json!({"n": k, "file": path.display().to_string(), "count": n, "at": iso_now(),
                      "commit": head(&wt)?, "base": merge_base(&wt, &base_ref)?});
    t.insert("tours_sent".into(), json!(k));
    t.insert("tour_sent".into(), sent);
    if s(t, "status") == "review" {
        close_reviewer(t)?; // it reviews code the person has just sent back
    }
    let msg = format!(
        "The person went through your change with a guided tour and sent back {said}. Read {}, \
         address each comment, commit, and run your ready command again. If you disagree with a \
         comment, say why in your ready summary.",
        path.display()
    );
    let failed = match hand_back(repo, t, &msg, "person") {
        Ok(failed) => failed,
        Err(e) => Some(clip(&e.0, 160)),
    };
    let _ = std::fs::remove_file(comments_path(repo, t)); // sent: in the file now, no longer drafts
    if let Some(failed) = failed {
        let line = format!(
            "{id} was sent back by the person after the tour ({said}), but the message didn't reach its \
             worker ({failed}). The comments are in {}: `bullpen tell {id}` it to read them",
            path.display()
        );
        let why = "The person's tour comments didn't reach its worker";
        stop(repo, t, "worker", "not_sent", why, &[line], Kind::NotSent)?;
        return Ok(format!(
            "{said} written to {}, but they didn't reach {id}'s worker: its coordinator is told",
            path.display()
        ));
    }
    let text = format!("{id} sent back by the person after the tour: {said}");
    tell_coordinator(repo, t, Kind::TourSent, &text)?;
    Ok(format!("sent {said} to {id}'s worker ({})", path.display()))
}

/// Where the person is in the tour, kept with the drafts: opening it again resumes there.
fn save_place(repo: &Repo, t: &Obj, commit: &Value, at: usize) -> Res<()> {
    let mut c = read_comments(repo, t)?;
    c.insert("at".into(), json!({"commit": commit, "step": at + 1}));
    Ok(json::write_obj(&comments_path(repo, t), &c)?)
}
