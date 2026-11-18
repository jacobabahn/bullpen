//! The tour agent (tours.py's write_tour): `_tour` runs it on a task's code, away from
//! whoever asked, checks its answer against the diff and writes the tour. If the person
//! asked on the board to open the tour once written, it opens in the stepper there.
//! The stepper starts `_tour` (`tour`, `tour --wait` and the board's g: stepper/).

use super::{append, diff_files, lines_of, tour_failed, tour_log, tour_path, write_task, DiffFile, OTHER};
use crate::args::{pos, Arg, Args, Nargs};
use crate::modes::task_base_ref;
use crate::review::patch_id;
use crate::start::launch::py_format;
use bullpen_core::json::{dumps_line, obj, py_repr, py_str, s, truthy, Obj};
use bullpen_core::paths::prompts;
use bullpen_core::proc::{git, head, merge_base, resolve, text};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{clip, is_claude, model_in, with_model};
use bullpen_core::text::{rstrip, shell_join, split, splitlines, strip};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Fail, Repo, Res};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TOUR_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "Bash(git diff:*)",
    "Bash(git log:*)",
    "Bash(git show:*)",
    "Bash(git status:*)",
    "Bash(git blame:*)",
    "Bash(git ls-files:*)",
];
/// seconds before a tour agent counts as failed
const TOUR_TIMEOUT: u64 = 20 * 60;
/// longer, and the agent reads the diff with git itself
const MAX_TOUR_DIFF_LINES: usize = 3000;
/// an answer that isn't a tour is asked for once more
const TOUR_ATTEMPTS: u32 = 2;
/// the answer's shape, for claude's structured output (--json-schema); tour_steps checks the rest
const TOUR_SCHEMA: &str = concat!(
    r#"{"type":"object","required":["steps"],"properties":{"steps":{"type":"array","minItems":1,"items":"#,
    r#"{"type":"object","required":["title","note","hunks"],"properties":{"title":{"type":"string"},"#,
    r#""note":{"type":"string"},"hunks":{"type":"array","items":{"type":"object","required":["file"],"#,
    r#""properties":{"file":{"type":"string"},"lines":{"type":"array","items":{"type":"integer"},"#,
    r#""minItems":2,"maxItems":2}}}}}}}}}"#
);

/// How writing a tour went wrong, as the Python's exceptions: a ValueError (the tour
/// failed: recorded on the task), a Fail (said, nothing recorded), or anything else
/// (recorded by `_tour`).
#[derive(Debug)]
pub enum Wrong {
    Value(String),
    /// what it said: `_tour` exits 1 without saying it, as tours.py's did
    Fail(#[allow(dead_code)] Fail),
    Other(String),
}

impl From<Fail> for Wrong {
    fn from(f: Fail) -> Wrong {
        Wrong::Fail(f)
    }
}

impl From<std::io::Error> for Wrong {
    fn from(e: std::io::Error) -> Wrong {
        Wrong::Other(os_error(&e, None))
    }
}

/// An OS error as Python names it: "FileNotFoundError: [Errno 2] No such file or directory: 'x'".
fn os_error(e: &std::io::Error, path: Option<&str>) -> String {
    let said = e.to_string();
    let words = said.split(" (os error").next().unwrap_or(&said);
    let kind = match e.raw_os_error() {
        Some(libc::ENOENT) => "FileNotFoundError",
        Some(libc::EACCES) | Some(libc::EPERM) => "PermissionError",
        Some(libc::EISDIR) => "IsADirectoryError",
        _ => "OSError",
    };
    let at = path.map(|p| format!(": '{p}'")).unwrap_or_default();
    match e.raw_os_error() {
        Some(n) => format!("{kind}: [Errno {n}] {words}{at}"),
        None => format!("{kind}: {words}{at}"),
    }
}

type Wr<T> = Result<T, Wrong>;

fn value<T>(msg: impl Into<String>) -> Wr<T> {
    Err(Wrong::Value(msg.into()))
}

// ---------- reading its answer ----------

/// A hunk reference's lines, as [start, end]: from [a, b], [a], a or "a-b". None for
/// the whole file. Err (a ValueError's words) for anything else.
pub fn ref_lines(v: Option<&Value>) -> Result<Option<(i64, i64)>, String> {
    let v = match v {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(text)) => {
            let mut parts = Vec::new();
            for x in text.splitn(2, '-') {
                match crate::news::read::py_int(x) {
                    Some(n) => parts.push(json!(n)),
                    None => {
                        return Err(format!(
                            "invalid literal for int() with base 10: {}",
                            py_repr(&json!(x))
                        ))
                    }
                }
            }
            Value::Array(parts)
        }
        Some(n @ Value::Number(_)) if n.is_i64() || n.is_u64() => Value::Array(vec![n.clone()]),
        Some(other) => other.clone(),
    };
    let ints: Option<Vec<i64>> = v
        .as_array()
        .filter(|a| (1..=2).contains(&a.len()))
        .and_then(|a| a.iter().map(Value::as_i64).collect());
    let Some(ints) = ints else {
        return Err(format!("lines must be [start, end], not {}", dumps_line(&v)));
    };
    let (a, b) = (ints[0], ints[ints.len() - 1]);
    Ok(Some((a.min(b), a.max(b))))
}

/// Every JSON object in text, in order, outermost ones only: an agent may wrap its
/// answer in a code fence or a sentence, or follow it with a corrected one.
pub fn json_objects(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut i = text.find('{');
    while let Some(at) = i {
        let mut it = serde_json::Deserializer::from_str(&text[at..]).into_iter::<Value>();
        match it.next() {
            Some(Ok(v)) => {
                let end = at + it.byte_offset();
                out.push(v);
                i = text[end..].find('{').map(|j| end + j);
            }
            _ => i = text[at + 1..].find('{').map(|j| at + 1 + j),
        }
    }
    out
}

/// A tour's steps, the references dropped, and a note on any text around it.
pub type Parsed = (Vec<Value>, Vec<String>, Option<String>);

/// What the agent answered: claude's structured output, or its text.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Data(Value),
    Text(String),
}

/// The checked steps of an agent's answer: the text as JSON, else the last JSON object
/// in it that is a tour. Err for an answer with no tour. Returns (steps, references
/// dropped, a note on any text around the tour, else None).
pub fn parse_tour(answer: &Answer, files: &[DiffFile]) -> Result<Parsed, String> {
    let text = match answer {
        Answer::Data(v) => {
            let (steps, dropped) = tour_format(v, files)?;
            return Ok((steps, dropped, None));
        }
        Answer::Text(text) => strip(text),
    };
    if let Ok(data) = serde_json::from_str::<Value>(text) {
        let (steps, dropped) = tour_format(&data, files)?;
        return Ok((steps, dropped, None));
    }
    let objs = json_objects(text);
    if objs.is_empty() {
        return Err("its tour doesn't parse (no JSON object in its output)".into());
    }
    let mut first = None;
    for o in objs.iter().rev() {
        match tour_format(o, files) {
            Ok((steps, dropped)) => {
                let note = format!(
                    "its answer had text around the tour ({} JSON objects): the last one that is a tour was used",
                    objs.len()
                );
                return Ok((steps, dropped, Some(note)));
            }
            Err(e) => {
                first.get_or_insert(e);
            }
        }
    }
    Err(first.unwrap_or_default())
}

/// tour_steps, with its error saying the tour isn't in the format.
fn tour_format(data: &Value, files: &[DiffFile]) -> Result<(Vec<Value>, Vec<String>), String> {
    tour_steps(data, files).map_err(|e| format!("its tour isn't in the format: {e}"))
}

/// The steps of an agent's tour, checked, with only the hunk references that match the
/// diff, then "Other changes" for each hunk no step covers. Err for a tour that isn't
/// the format. Returns (steps, references dropped).
pub fn tour_steps(data: &Value, files: &[DiffFile]) -> Result<(Vec<Value>, Vec<String>), String> {
    let steps = data.get("steps").and_then(Value::as_array).filter(|a| !a.is_empty());
    let Some(steps) = steps else {
        return Err("it has no list of steps".into());
    };
    let (mut out, mut dropped, mut covered) = (Vec::new(), Vec::new(), Vec::new());
    for (i, st) in steps.iter().enumerate().map(|(i, st)| (i + 1, st)) {
        let title = st.get("title").and_then(Value::as_str).filter(|t| !strip(t).is_empty());
        let (Some(st), Some(title)) = (st.as_object(), title) else {
            return Err(format!("step {i} has no title"));
        };
        let note = st.get("note").map_or(Some(""), Value::as_str);
        let hunks = st
            .get("hunks")
            .map_or(Some(&[][..]), |h| h.as_array().map(Vec::as_slice));
        let (Some(note), Some(hunks)) = (note, hunks) else {
            return Err(format!("step {i}'s note must be text and its hunks a list"));
        };
        let mut refs = Vec::new();
        for r in hunks {
            let Some(file) = r.get("file").and_then(Value::as_str).filter(|_| r.is_object()) else {
                return Err(format!("step {i} has a hunk with no file"));
            };
            let lines = ref_lines(r.get("lines")).map_err(|e| format!("step {i}: {e}"))?;
            let reference = match lines {
                Some((a, b)) => json!({"file": file, "lines": [a, b]}),
                None => json!({"file": file}),
            };
            let reference = reference.as_object().cloned().unwrap_or_default();
            let mut hit = Vec::new();
            for (fi, f) in files.iter().enumerate() {
                if f.hunks.is_empty() {
                    if file == f.file {
                        hit.push((fi, 0));
                    }
                    continue;
                }
                for (hi, h) in f.hunks.iter().enumerate() {
                    if super::covers(&reference, f, h) {
                        hit.push((fi, hi));
                    }
                }
            }
            if hit.is_empty() {
                let shown = lines.map_or(String::new(), |(a, b)| format!("[{a}, {b}]"));
                dropped.push(rstrip(&format!("step {i}: {file} {shown}")).to_string());
                continue;
            }
            covered.extend(hit);
            refs.push(Value::Object(reference));
        }
        out.push(json!({"title": split(title).join(" "), "note": strip(note), "hunks": refs}));
    }
    let mut rest = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        if f.hunks.is_empty() {
            if !covered.contains(&(fi, 0)) {
                rest.push(json!({"file": f.file}));
            }
            continue;
        }
        for (hi, h) in f.hunks.iter().enumerate() {
            if !covered.contains(&(fi, hi)) {
                let (a, b) = lines_of(f, h);
                rest.push(json!({"file": f.file, "lines": [a, b]}));
            }
        }
    }
    if !rest.is_empty() {
        out.push(json!({"title": OTHER, "note": "The changes no step above covers.", "hunks": rest}));
    }
    Ok((out, dropped))
}

/// A tour agent's answer: claude's structured output when it has one, else its text.
/// Err for an agent error.
pub fn tour_answer(out: &str) -> Result<Answer, String> {
    if let Ok(Value::Object(r)) = serde_json::from_str::<Value>(out) {
        if s(&r, "type") == "result" {
            if let Some(data @ Value::Object(_)) = r.get("structured_output") {
                agent_result(out)?; // an error is still an error
                return Ok(Answer::Data(data.clone()));
            }
        }
    }
    agent_result(out)
}

/// The answer of a headless claude: its --output-format json result, or the output as
/// it is from another agent. Err for an agent error.
pub fn agent_result(out: &str) -> Result<Answer, String> {
    let Ok(Value::Object(r)) = serde_json::from_str::<Value>(out) else {
        return Ok(Answer::Text(out.into()));
    };
    if s(&r, "type") != "result" {
        return Ok(Answer::Text(out.into()));
    }
    if truthy(r.get("is_error")) {
        let said = if truthy(r.get("result")) {
            r.get("result")
        } else {
            r.get("subtype")
        };
        let said = said.map_or("None".into(), py_str);
        return Err(format!("the agent failed: {}", clip(&said, 160)));
    }
    Ok(match r.get("result").filter(|v| truthy(Some(v))) {
        Some(Value::String(text)) => Answer::Text(text.clone()),
        Some(other) if other.is_object() => Answer::Data(other.clone()),
        Some(other) => Answer::Text(py_str(other)),
        None => Answer::Text(String::new()),
    })
}

// ---------- running it ----------

/// What the tour agent is given: the brief, the worker's summary, the review findings,
/// the diff (or how to get it) and its hunks.
fn tour_inputs(repo: &Repo, t: &Obj, base_ref: &str, files: &[DiffFile]) -> Res<Vec<(&'static str, String)>> {
    let id = s(t, "id");
    let briefs = repo.dir.join("briefs");
    let brief = briefs.join(format!("{id}.md"));
    let mut findings: Vec<(Option<i64>, String)> = Vec::new();
    let (before, after) = (format!("{id}.review-"), ".findings.md");
    for e in std::fs::read_dir(&briefs).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.len() >= before.len() + after.len() && name.starts_with(&before) && name.ends_with(after) {
            findings.push((review_number(&name), name));
        }
    }
    findings.sort();
    let mut found = Vec::new();
    for (_, name) in &findings {
        let text = std::fs::read_to_string(briefs.join(name))?;
        found.push(format!("### {name}\n\n{}", strip(&text)));
    }
    let found = if found.is_empty() {
        "(none)".into()
    } else {
        found.join("\n\n")
    };
    let wt = s(t, "worktree");
    let mut diff = git(wt, &["diff", "--no-color", &format!("{base_ref}...HEAD")], true)?.stdout;
    diff.truncate(diff.trim_end_matches('\n').len());
    if diff.split('\n').count() > MAX_TOUR_DIFF_LINES {
        diff = format!(
            "({} lines, too long to include: run the command)",
            splitlines(&diff).len()
        );
    }
    let hunks: Vec<String> = files
        .iter()
        .map(|f| {
            if f.hunks.is_empty() {
                return format!("- {} (binary)", f.file);
            }
            let spans: Vec<String> = f
                .hunks
                .iter()
                .map(|h| lines_of(f, h))
                .map(|(a, b)| format!("{a}-{b}"))
                .collect();
            format!("- {}: {}", f.file, spans.join(", "))
        })
        .collect();
    let brief = match std::fs::read_to_string(&brief) {
        Ok(text) => strip(&text).to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "(none)".into(),
        Err(e) => return Err(e.into()),
    };
    let summary = t
        .get("summary")
        .filter(|v| truthy(Some(v)))
        .map_or("(none)".into(), py_str);
    Ok(vec![
        ("id", id.into()),
        ("title", t.get("title").map_or("None".into(), py_str)),
        ("worktree", wt.into()),
        ("base_ref", base_ref.into()),
        ("brief", brief),
        ("summary", summary),
        ("findings", found),
        ("diff", diff),
        ("hunks", hunks.join("\n")),
    ])
}

/// The N of "review-N" in a findings file's name.
fn review_number(name: &str) -> Option<i64> {
    let mut rest = name;
    while let Some(i) = rest.find("review-") {
        rest = &rest[i + "review-".len()..];
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if end > 0 {
            return rest[..end].parse().ok();
        }
    }
    None
}

/// The tour agent's argv: headless, on tour_model, with read-only tools.
pub fn tour_command(repo: &Repo) -> Vec<String> {
    let model = repo.cfg.get("tour_model").filter(|v| truthy(Some(v))).map(py_str);
    let cmd = with_model(&bullpen_core::json::strs(&repo.cfg, "tour"), model.as_deref());
    let mut argv = resolve(&cmd);
    if !is_claude(&cmd) {
        return argv;
    }
    for a in [
        "-p",
        "--output-format",
        "json",
        "--no-session-persistence",
        "--permission-mode",
        "dontAsk",
    ] {
        argv.push(a.into());
    }
    argv.extend(["--tools".into(), "Read,Grep,Glob,Bash".into(), "--allowedTools".into()]);
    argv.extend(TOUR_TOOLS.iter().map(|t| t.to_string()));
    argv.extend(["--json-schema".into(), TOUR_SCHEMA.into()]);
    argv
}

/// Run the tour agent on the task's code now and write its tour. Returns the tour file;
/// a tour that failed is recorded on the task, and is a Fail saying why. If the person
/// asked on the board to open it once written, it opens there.
pub fn write_tour(repo: &Repo, tid: &str) -> Wr<PathBuf> {
    let t = repo.task(tid)?;
    let log = tour_log(repo, &t);
    let (commit, base, base_ref, steps) = match agent_tour(repo, &t, &log) {
        Ok(done) => done,
        Err(Wrong::Value(e)) => {
            tour_failed(repo, tid, &e)?;
            return Err(Wrong::Fail(Fail(format!(
                "{tid}'s tour failed: {e} (log: {})",
                log.display()
            ))));
        }
        Err(other) => return Err(other),
    };
    let wt = s(&t, "worktree");
    let path = tour_path(repo, &t);
    let tour = json!({"task": s(&t, "id"), "commit": commit, "base": base, "base_ref": base_ref,
                      "patch": patch_id(wt, &base_ref)?, "written": iso_now(),
                      "model": model_in(&tour_command(repo)), "steps": steps});
    bullpen_core::json::write(&path, &tour)?;
    let mut t = repo.task(tid)?; // read again: it may have changed while the agent ran
    let opener = obj(&t, "tour").get("open").filter(|v| truthy(Some(v))).cloned();
    t.insert(
        "tour".into(),
        json!({"state": "written", "at": iso_now(), "commit": commit}),
    );
    if s(&t, "note").starts_with("tour failed") {
        t.insert("note".into(), Value::Null);
    }
    write_task(repo, &t)?;
    if let Some(opener) = opener {
        open_when_written(repo, &t, &opener);
    }
    Ok(path)
}

/// The agent's tour of the task's change, checked: (commit, base, base_ref, steps).
fn agent_tour(repo: &Repo, t: &Obj, log: &Path) -> Wr<(String, Option<String>, String, Vec<Value>)> {
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).is_dir() {
        return value("the task has no worktree");
    }
    let base_ref = task_base_ref(repo, t)?;
    let (commit, base) = (head(wt)?, merge_base(wt, &base_ref)?);
    let files = diff_files(wt, base.as_deref().unwrap_or(""), &commit)?;
    if files.is_empty() {
        return value(format!("it has no changes against {base_ref}"));
    }
    let template = prompts().join("tour.md");
    let template = std::fs::read_to_string(&template)
        .map_err(|e| Wrong::Other(os_error(&e, Some(&template.display().to_string()))))?;
    let prompt = py_format(&template, &tour_inputs(repo, t, &base_ref, &files)?);
    let argv = tour_command(repo);
    let mut attempt = 1;
    let (steps, dropped, extra) = loop {
        let out = run_tour_agent(&argv, &prompt, wt, log, attempt)?;
        let answer = tour_answer(&out).map_err(Wrong::Value)?;
        match parse_tour(&answer, &files) {
            Ok(done) => break done,
            Err(e) if attempt == TOUR_ATTEMPTS => return value(e),
            Err(e) => append(log, &format!("{} attempt {attempt}: {e}; asking again\n\n", iso_now()))?,
        }
        attempt += 1;
    };
    if let Some(extra) = extra {
        append(log, &format!("{extra}\n"))?;
    }
    if !dropped.is_empty() {
        let line = format!(
            "hunks that match nothing in the diff, left out: {}\n",
            dropped.join("; ")
        );
        append(log, &line)?;
    }
    Ok((commit, base, base_ref, steps))
}

/// Run the tour agent once, its output in the log (a new log on the first attempt).
/// Returns its stdout; a ValueError if it didn't finish.
fn run_tour_agent(argv: &[String], prompt: &str, wt: &str, log: &Path, attempt: u32) -> Wr<String> {
    let said = format!(
        "{} attempt {attempt}: {}\n(the prompt on stdin: {} characters)\n\n",
        iso_now(),
        shell_join(argv),
        prompt.chars().count()
    );
    if attempt == 1 {
        std::fs::write(log, &said)?;
    } else {
        append(log, &said)?;
    }
    let (code, stdout, stderr) = match run_timed(argv, prompt, wt, Duration::from_secs(TOUR_TIMEOUT)) {
        Ok(Some(done)) => done,
        Ok(None) => return value(format!("the agent took over {} minutes", TOUR_TIMEOUT / 60)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return value(format!("{} isn't installed", argv[0]));
        }
        Err(e) => return Err(Wrong::Other(os_error(&e, Some(&argv[0])))),
    };
    append(
        log,
        &format!("exit {code}\n--- stdout\n{stdout}\n--- stderr\n{stderr}\n"),
    )?;
    if code != 0 {
        let said = strip(if stderr.is_empty() { &stdout } else { &stderr }).to_string();
        let lines = splitlines(&said);
        let last = lines.last().copied().unwrap_or("no output");
        return value(format!("the agent exited with {code}: {}", clip(last, 120)));
    }
    Ok(stdout)
}

/// Run argv in cwd with input on its stdin, for at most limit: (exit code, stdout,
/// stderr), or None when it took longer (it is killed).
fn run_timed(
    argv: &[String],
    input: &str,
    cwd: &str,
    limit: Duration,
) -> std::io::Result<Option<(i32, String, String)>> {
    use std::os::unix::process::ExitStatusExt;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take();
    let bytes = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        if let Some(w) = stdin.as_mut() {
            let _ = w.write_all(&bytes); // an agent that exits without reading it is fine
        }
    });
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut out);
            }
            out
        })
    };
    let out = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = writer.join();
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    Ok(status.map(|st| {
        let code = st.code().unwrap_or_else(|| -st.signal().unwrap_or(1));
        (code, text(&out), text(&err))
    }))
}

// ---------- `_tour` ----------

const TOUR_BG: &[Arg] = &[pos("id", Nargs::One, "")];

/// `_tour` (internal): write a task's tour, away from whoever asked for it.
pub fn cmd_tour_bg(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_tour", argv, TOUR_BG);
    let tid = a.arg("id").unwrap_or("");
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(tid)?;
    let pid = json!(std::process::id()); // this process writes it, whoever started it
    match t.get_mut("tour") {
        Some(Value::Object(run)) => {
            run.insert("pid".into(), pid);
        }
        _ => {
            t.insert("tour".into(), json!({"pid": pid}));
        }
    }
    write_task(&repo, &t)?;
    match write_tour(&repo, tid) {
        Ok(_) => Ok(0),
        Err(Wrong::Fail(_)) => Ok(1), // recorded on the task
        Err(Wrong::Value(e) | Wrong::Other(e)) => {
            // anything else is a failure too, never a tour left "writing"
            tour_failed(&repo, tid, &clip(&e, 160))?;
            Ok(1)
        }
    }
}

// ---------- opening it ----------

/// The tour the person asked for on the board is written: open it where they asked.
fn open_when_written(repo: &Repo, t: &Obj, opener: &Value) {
    let get = |k: &str| opener.get(k).filter(|v| truthy(Some(v))).map_or(String::new(), py_str);
    if let Err(e) = crate::stepper::open_stepper(repo, t, &get("session"), &get("from")) {
        // the tour is written all the same: g opens it
        let _ = append(
            &tour_log(repo, t),
            &format!("{} couldn't open the tour: {e}\n", iso_now()),
        );
    }
}

#[cfg(test)]
#[path = "tests/test_write.rs"]
mod tests;
