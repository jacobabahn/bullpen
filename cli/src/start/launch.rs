//! Starting a task's worker: its worktree, setup and brief, the
//! agent's pane, and handing a worker a message: `start`, `resume` and `tell`, with
//! `start_task`, the worker's brief (`write_full`), its pane (`launch`) and `hand_back`.

use super::by_id;
use crate::args::{flag, pos, value, Arg, Args, Nargs};
use crate::coordinators::{calling_coordinator, session_for};
use crate::hooks::{hook_log, set_up};
use crate::limit::{limit_active, limit_phrase};
use crate::modes::task_base_ref;
use crate::host::Agent;
use crate::rules::{committed_rules_on, rule_sources, rules_text};
use crate::saves::save;
use bullpen_core::json::{py_str, s, truthy, Obj};
use bullpen_core::paths::{prompts, self_bin};
use bullpen_core::proc::{git, resolve};
use bullpen_core::repo::{index_block, repo_root, unindex_block};
use bullpen_core::settings::{PLACES, WAITING};
use bullpen_core::task::{
    back_to_work, is_claude, is_status, model_in, pr_short, set_auto_accept, set_status, slug, status_words, with_model,
};
use bullpen_core::text::strip;
use bullpen_core::timeline;
use bullpen_core::words::open_deps;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

const ID: Arg = pos("id", Nargs::One, "the task, like BP-3");
const SESSION: &str = "the Rex session for its pane (default: the coordinator's)";
const PANE: &str = "where its worker's pane goes, over config --workers: float, tab, tabs or background";

const START: &[Arg] = &[
    ID,
    value(
        "--model",
        "MODEL",
        "claude's --model for this task's worker, over the config's",
    ),
    value("--session", "SESSION", SESSION),
    flag(
        "--force",
        "start even while the usage limit is reached or what it waits on isn't done",
    ),
    flag(
        "--auto-accept",
        "accept it by itself once review approves it, whatever config --auto-accept says",
    ),
    flag(
        "--no-auto-accept",
        "keep it for you to accept even with config --auto-accept on",
    ),
    value("--pane", "PANE", PANE).choices(&PLACES),
];

const RESUME: &[Arg] = &[
    ID,
    pos(
        "message",
        Nargs::Maybe,
        "what to type to it first (default: carry on with your brief)",
    ),
    value("--session", "SESSION", SESSION),
    value("--pane", "PANE", PANE).choices(&PLACES),
];

const TELL: &[Arg] = &[ID, pos("message", Nargs::Some, "the words to type")];

/// `start`: make a task's worktree and start its worker.
pub fn cmd_start(argv: &[String]) -> Res<i32> {
    let t = start(argv)?;
    println!(
        "{} started in {} (branch {}, pane {})",
        s(&t, "id"),
        s(&t, "worktree"),
        s(&t, "branch"),
        s(&t, "block")
    );
    Ok(0)
}

/// Start the task as `start` would with these arguments; the task, started.
pub fn start(argv: &[String]) -> Res<bullpen_core::Obj> {
    let a = Args::parse("start", argv, START);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let (id, status) = (s(&t, "id").to_string(), s(&t, "status").to_string());
    let force = a.flag("--force");
    let waiting = WAITING.contains(&status.as_str());
    if !matches!(status.as_str(), "todo" | "stopped") && !(truthy(t.get("needs_setup")) && waiting) {
        return fail(format!("{}; use `resume` or `tell`", is_status(&t)));
    }
    if !force && status == "todo" {
        let tasks = by_id(&repo)?;
        let deps = open_deps(&t, &tasks);
        if !deps.is_empty() {
            let said: Vec<String> = deps
                .iter()
                .map(|(d, st)| match st {
                    Some("done") => format!("{d} (closed with its branch kept, not merged)"),
                    st => format!("{d} ({})", st.map_or("no such task".into(), status_words)),
                })
                .collect();
            return fail(format!(
                "{id} waits on {}: it starts by itself once they're accepted; `start {id} --force` starts it now",
                said.join(", ")
            ));
        }
    }
    if !force {
        if let Some(lim) = limit_active(None)? {
            return fail(format!(
                "the usage limit is reached ({}): a worker started now would stop at once. `start` works again \
                 after the reset, or once a limited worker is working again; `start {id} --force` starts it anyway",
                limit_phrase(lim.get("resets"), None)
            ));
        }
    }
    let session = session_for(&repo, a.opt("--session"))?;
    if session.is_empty() {
        return fail("no Rex session: run `bullpen up` first or pass --session");
    }
    set_auto_accept(
        &mut t,
        a.last(&["--auto-accept", "--no-auto-accept"])
            .map(|f| f == "--auto-accept"),
    );
    set_placement(&mut t, a.opt("--pane"));
    let model = a.opt("--model").filter(|m| !m.is_empty());
    if !start_task(&repo, &mut t, &session, model)? {
        return fail(format!(
            "{id}'s setup failed, so its worker didn't start: see {}",
            hook_log(&repo, &t, "setup").display()
        ));
    }
    Ok(t)
}

/// --pane: where the task's worker pane goes from its next start on, over the `workers`
/// setting (panes::for_new reads it). Its reviewer's too, with `--reviewers same`.
fn set_placement(t: &mut Obj, pane: Option<&str>) {
    if let Some(p) = pane.filter(|p| !p.is_empty()) {
        t.insert("placement".into(), json!(p));
    }
}

/// Make the task's worktree, set it up, write its brief and start its worker. Returns
/// false, with the task stopped (saved, news sent), if its setup failed.
pub fn start_task(repo: &Repo, t: &mut Obj, session: &str, model: Option<&str>) -> Res<bool> {
    if !truthy(t.get("worktree")) {
        t.insert("needs_setup".into(), json!(true)); // until its setup succeeds: a retry runs it again
        let base = py_str(t.get("base").unwrap_or(&Value::Null));
        let mut start_point = base.clone();
        if git(&repo.root, &["remote", "get-url", "origin"], false)?.ok() {
            git(&repo.root, &["fetch", "-q", "origin", &base], false)?;
            let on_origin = format!("origin/{base}");
            if git(&repo.root, &["rev-parse", "-q", "--verify", &on_origin], false)?.ok() {
                start_point = on_origin;
            }
        }
        let branch = format!("bullpen/{}-{}", s(t, "id").to_lowercase(), slug(s(t, "title"), 40));
        t.insert("branch".into(), json!(branch));
        let wt = repo.worktree_dir(t);
        if let Some(dir) = wt.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let wt = wt.display().to_string();
        git(
            &repo.root,
            &["worktree", "add", "-q", "--no-track", "-b", &branch, &wt, &start_point],
            true,
        )?;
        t.insert("worktree".into(), json!(wt));
        save(repo, t)?; // setup can take minutes: if bullpen dies in it, a retry knows the worktree
    }
    if truthy(t.get("needs_setup")) && !set_up(repo, t)? {
        return Ok(false);
    }
    let id = s(t, "id").to_string();
    // committed on its base, not the worktree's copy: a restarted task may have edited it
    let committed = committed_rules_on(repo, &task_base_ref(repo, t)?)?;
    let rules = rules_text(&rule_sources(repo, committed.as_deref(), "")?);
    let saved = if rules.is_empty() {
        String::new()
    } else {
        format!("{rules}\n")
    };
    std::fs::write(repo.dir.join("briefs").join(format!("{id}.rules.md")), saved)?;
    let full = write_full(repo, t, &rules)?;
    let first = format!(
        "You are the worker for bullpen task {id}. Read your brief at {} and follow it.",
        full.display()
    );
    if let Some(m) = model {
        t.insert("worker_model".into(), json!(m)); // this task's own, over the config's
    }
    launch(repo, t, session, &first, false)?;
    crate::issues::write::started(repo, t)?; // an issue's task: `bullpen: started` on it
    Ok(true)
}

/// Write the brief the worker reads (briefs/<ID>.full.md): the task's brief
/// (briefs/<ID>.md), what it builds on, the repository's rules and the worker's footer.
/// Its path. `start` writes it, and `brief` again when the task's brief changes.
pub fn write_full(repo: &Repo, t: &Obj, rules: &str) -> Res<std::path::PathBuf> {
    let id = s(t, "id");
    let briefs = repo.dir.join("briefs");
    let brief = read_text(&briefs.join(format!("{id}.md")))?;
    let full = briefs.join(format!("{id}.full.md"));
    let section = if rules.is_empty() {
        String::new()
    } else {
        format!("## Rules for this repository\n\n{rules}\n\n")
    };
    let text = format!(
        "# {id}: {}\n\n{brief}\n{}{section}{}",
        s(t, "title"),
        deps_section(repo, t)?,
        footer(repo, t)?
    );
    std::fs::write(&full, text)?;
    Ok(full)
}

/// A file's text, as Path.read_text() reads it (universal newlines).
fn read_text(path: &std::path::Path) -> Res<String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        let n = e.raw_os_error().unwrap_or(0);
        let said = e.to_string();
        let said = said.trim_end_matches(&format!(" (os error {n})")).to_string();
        bullpen_core::Fail(format!("[Errno {n}] {said}: '{}'", path.display()))
    })?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// For the brief of a task with dependencies: what each one landed.
fn deps_section(repo: &Repo, t: &Obj) -> Res<String> {
    let after = bullpen_core::json::strs(t, "after");
    if after.is_empty() {
        return Ok(String::new());
    }
    let tasks = by_id(repo)?;
    let mut parts = Vec::new();
    for d in after {
        let Some(o) = tasks.get(&d) else {
            continue;
        };
        let status = s(o, "status");
        let state = match status {
            "done" if truthy(o.get("kept")) => "closed with its branch kept, not merged".to_string(),
            "done" if truthy(o.get("closed")) => "finished with nothing to merge, closed".to_string(),
            "done" => "accepted".to_string(),
            _ => format!("{status}, not accepted yet"),
        };
        let at = if truthy(o.get("pr")) {
            pr_short(s(o, "pr"))
        } else if truthy(o.get("merged")) {
            format!("merged into {}", py_str(o.get("base").unwrap_or(&Value::Null)))
        } else {
            String::new()
        };
        let at = if at.is_empty() { at } else { format!(", {at}") };
        let summary = o
            .get("summary")
            .filter(|v| truthy(Some(v)))
            .map_or("No summary.".into(), py_str);
        let mut lines = vec![
            format!("### {d}: {} ({state}{at})", s(o, "title")),
            String::new(),
            strip(&summary).to_string(),
        ];
        let findings = findings_of(repo, &d);
        if !findings.is_empty() {
            lines.push(String::new());
            lines.push(format!("Review findings: {}", findings.join(", ")));
        }
        parts.push(lines.join("\n"));
    }
    if parts.is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "## What this builds on\n\nThis task waited on these tasks. Their work is on your base branch now \
         (unless marked otherwise): read it before you start.\n\n{}\n\n",
        parts.join("\n\n")
    ))
}

/// The paths of task d's review findings (<d>.review-*.findings.md), in order.
fn findings_of(repo: &Repo, d: &str) -> Vec<String> {
    let dir = repo.dir.join("briefs");
    let (head, tail) = (format!("{d}.review-"), ".findings.md");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.len() >= head.len() + tail.len() && n.starts_with(&head) && n.ends_with(tail))
        .map(|n| dir.join(n).display().to_string())
        .collect();
    out.sort();
    out
}

/// The worker's footer (prompts/worker-footer.md), filled in for t.
fn footer(repo: &Repo, t: &Obj) -> Res<String> {
    let text = read_text(&prompts().join("worker-footer.md"))?;
    let bin = self_bin().display().to_string();
    let fields = [
        ("id", s(t, "id").to_string()),
        ("title", s(t, "title").to_string()),
        ("worktree", py_str(t.get("worktree").unwrap_or(&Value::Null))),
        ("branch", py_str(t.get("branch").unwrap_or(&Value::Null))),
        ("base", py_str(t.get("base").unwrap_or(&Value::Null))),
        ("bin", bin),
        ("root", repo.root_str().to_string()),
        ("briefs", repo.dir.join("briefs").display().to_string()),
    ];
    Ok(py_format(&text, &fields))
}

/// str.format with named fields: {name} filled in, {{ and }} as { and }.
pub fn py_format(text: &str, fields: &[(&str, String)]) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let (true, Some(end)) = (tail.starts_with('{'), tail.find('}')) {
            if let Some((_, v)) = fields.iter().find(|(k, _)| *k == &tail[1..end]) {
                out.push_str(v);
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// Start the task's worker (in a new pane, on Rex), with first as its first message (resume:
/// continuing its conversation), and save it as working.
pub fn launch(repo: &Repo, t: &mut Obj, session: &str, first: &str, resume: bool) -> Res<()> {
    let worker: Vec<String> = bullpen_core::json::strs(&repo.cfg, "worker");
    let model = [t.get("worker_model"), repo.cfg.get("worker_model")]
        .into_iter()
        .flatten()
        .find(|v| truthy(Some(v)))
        .map(py_str);
    let cmd = with_model(&worker, model.as_deref());
    let running = if is_claude(&cmd) { model_in(&cmd) } else { None };
    t.insert("model".into(), json!(running));
    crate::stop_hook::install(repo, t, &cmd); // Claude Code's: what it waits on when its turn ends
    let mut argv = resolve(&cmd);
    if resume {
        argv.push("--continue".into());
    }
    argv.extend(["-n".into(), format!("{}: {}", s(t, "id"), s(t, "title")), first.into()]);
    unindex_block(s(t, "block"))?;
    let label = format!("bullpen: {}", s(t, "id"));
    let host = crate::host::for_new(repo, t);
    let (worker, at) = host.start(repo, t, session, &label, &argv, "worker")?;
    index_block(&worker.id, repo, s(t, "id"))?;
    set_status(t, "working", None, None);
    worker.record(t);
    t.insert("last".into(), Value::Null);
    t.insert("pane".into(), json!(at));
    save(repo, t)
}

/// Give the task's worker msg to act on: typed into its pane, or, with its pane gone,
/// resumed in a new one with msg. Once the worker has it the task is working (saved),
/// and its next event is never taken for a repeat. Returns None, or why the message
/// didn't reach the worker: the task is left as it was, unsaved. A Fail when a new pane
/// can't be started. back: why it goes back to work, for its timeline ("review",
/// "rebase", "person", "pr", "limit").
pub fn hand_back(repo: &Repo, t: &mut Obj, msg: &str, back: &str) -> Res<Option<String>> {
    if !Agent::of(t).alive()? {
        timeline::send_back(t, back);
        let mut session = session_for(repo, None)?;
        if session.is_empty() {
            session = s(t, "session").to_string();
        }
        launch(repo, t, &session, msg, true)?;
        return Ok(None);
    }
    if let Some(failed) = Agent::of(t).tell(msg)? {
        return Ok(Some(failed));
    }
    if s(t, "status") != "working" {
        back_to_work(t);
        timeline::send_back(t, back);
    }
    t.insert("last".into(), Value::Null);
    save(repo, t)?;
    Ok(None)
}

/// `resume`: restart a stopped worker, continuing its conversation.
pub fn cmd_resume(argv: &[String]) -> Res<i32> {
    let a = Args::parse("resume", argv, RESUME);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    if Agent::of(&t).alive()? {
        return fail(format!("{id}'s agent is still running; use `tell`"));
    }
    if !truthy(t.get("worktree")) {
        return fail(format!("{id} never started; use `start`"));
    }
    set_placement(&mut t, a.opt("--pane"));
    let mut session = session_for(&repo, a.opt("--session"))?;
    if session.is_empty() {
        session = s(&t, "session").to_string();
    }
    if truthy(t.get("needs_setup")) {
        // its worker never ran: set it up and start it afresh
        if !start_task(&repo, &mut t, &session, None)? {
            return fail(format!(
                "{id}'s setup failed again: see {}",
                hook_log(&repo, &t, "setup").display()
            ));
        }
        println!("{id} set up and started in pane {}", s(&t, "block"));
        return Ok(0);
    }
    let msg = a
        .arg("message")
        .filter(|m| !m.is_empty())
        .unwrap_or("You were stopped before finishing. Check where you were and carry on with your brief.");
    launch(&repo, &mut t, &session, msg, true)?;
    println!("{id} resumed in pane {}", s(&t, "block"));
    Ok(0)
}

/// What `tell` types: the words bare from the person, and after `Coordinator: ` from a
/// coordinator's pane, so the worker knows who is typing (its footer says so).
fn told(words: &str, coordinator: bool) -> String {
    if coordinator && !words.starts_with(FROM_COORDINATOR) {
        return format!("{FROM_COORDINATOR}{words}");
    }
    words.to_string()
}

const FROM_COORDINATOR: &str = "Coordinator: ";

/// `tell`: type a message to a task's worker.
pub fn cmd_tell(argv: &[String]) -> Res<i32> {
    let a = Args::parse("tell", argv, TELL);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    if !Agent::of(&t).alive()? {
        return fail(format!("{id}'s agent isn't running; use `resume {id} \"...\"`"));
    }
    let words = told(&a.args("message").join(" "), calling_coordinator(&repo).is_some());
    Agent::of(&t).send(&words)?;
    if matches!(s(&t, "status"), "needs" | "triage" | "ready" | "finished") {
        if matches!(s(&t, "status"), "ready" | "finished") {
            timeline::send_back(&mut t, "person"); // a ready task told more: sent back
        }
        back_to_work(&mut t);
        t.insert("last".into(), Value::Null);
        save(&repo, &mut t)?;
    }
    println!("sent to {id}");
    Ok(0)
}

#[cfg(test)]
#[path = "tests/test_launch.rs"]
mod tests;
