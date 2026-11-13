//! doctor's checks (doctor.py's check_*). Each returns (state, what it found): "ok",
//! "fail" (with the fix in the text) or "skip" (it doesn't apply, or can't be told).

use crate::board::old_board;
use crate::build::{build_log, building, Program, BOARD, CLI};
use crate::coordinators::{alive, coordinators};
use crate::modes::repo_base_ref;
use crate::watcher::{watcher_record, watcher_stamp};
use bullpen_core::config::{global_config_path, read_global_file};
use bullpen_core::json::{get, obj, py_str, strs, truthy, Obj};
use bullpen_core::paths::{rex, self_bin, state};
use bullpen_core::proc::{git, run, run_in, run_input, which};
use bullpen_core::settings::{is_internal, is_repo_only};
use bullpen_core::text::{split, splitlines, strip};
use bullpen_core::{Fail, Repo, Res};
use serde_json::Value;
use std::path::Path;

/// A check's result.
pub type Check = Res<(&'static str, String)>;

/// preview.13: minimized floating panes. A release sorts after its previews (u64::MAX).
pub const REX_MIN: (u64, u64, u64, u64) = (0, 1, 0, 13);
const KEY_ACTIONS: [&str; 4] = ["bullpen_next", "bullpen_board", "bullpen_coordinator", "bullpen_jump"];

/// (major, minor, patch, preview) from "0.1.0-preview.13"; a release sorts after its
/// previews. None if it isn't a version (`v?(\d+)\.(\d+)\.(\d+)(?:-preview\.(\d+))?$`).
pub fn rex_version_key(text: &str) -> Option<(u64, u64, u64, u64)> {
    let t = strip(text);
    let t = t.strip_prefix('v').unwrap_or(t);
    let (nums, pre) = match t.split_once("-preview.") {
        Some((n, p)) => (n, Some(p)),
        None => (t, None),
    };
    let num = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let parts: Vec<&str> = nums.split('.').collect();
    let [a, b, c] = parts[..] else {
        return None;
    };
    let pre = match pre {
        Some(p) => num(p)?,
        None => u64::MAX,
    };
    Some((num(a)?, num(b)?, num(c)?, pre))
}

/// The program a #! line runs: its first word, or for env, the first word after
/// env's options (-S splits its string) and NAME=value settings.
pub fn shebang_program(argv: &[&str]) -> Option<String> {
    let first = *argv.first()?;
    if Path::new(first).file_name().is_none_or(|n| n != "env") {
        return Some(first.into());
    }
    let mut skip = false;
    for w in &argv[1..] {
        if skip {
            skip = false;
        } else if ["-u", "-C", "--unset", "--chdir"].contains(w) {
            skip = true; // their argument follows
        } else if w.starts_with("-S") && w.chars().count() > 2 {
            return Some(w[2..].into()); // -Sbash
        } else if !w.starts_with('-') && !w.contains('=') {
            return Some((*w).into());
        }
    }
    None
}

/// shlex.split (POSIX, no comments): the words, or None where it raises ValueError
/// (an unclosed quote, or a backslash at the end).
pub fn shlex_split(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut it = text.chars();
    while let Some(c) = it.next() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\\' => {
                cur.push(it.next()?);
                in_word = true;
            }
            '\'' => {
                in_word = true;
                loop {
                    match it.next()? {
                        '\'' => break,
                        q => cur.push(q),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match it.next()? {
                        '"' => break,
                        '\\' => {
                            let n = it.next()?;
                            if n != '"' && n != '\\' {
                                cur.push('\\');
                            }
                            cur.push(n);
                        }
                        q => cur.push(q),
                    }
                }
            }
            _ => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Some(words)
}

/// os.access(path, X_OK): a folder counts too.
fn executable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: c is a valid NUL-terminated path for the length of the call
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

/// The OS's words for an error (Python's strerror), without Rust's " (os error N)".
fn strerror(e: &std::io::Error) -> String {
    let s = e.to_string();
    s.split(" (os error").next().unwrap_or(&s).to_string()
}

/// A Python OSError's str() about path: "[Errno 21] Is a directory: '/x'".
fn os_error(e: &std::io::Error, path: &Path) -> Fail {
    match e.raw_os_error() {
        Some(n) => Fail(format!("[Errno {n}] {}: '{}'", strerror(e), path.display())),
        None => Fail(e.to_string()),
    }
}

pub fn check_rex() -> Check {
    let r = rex();
    let Some(exe) = which(&r) else {
        return Ok((
            "fail",
            format!("{r} isn't on PATH: install Rex (https://www.superlogical.com/rex)"),
        ));
    };
    let p = run(&[r.as_str(), "server", "status", "--json"], false)?;
    if !p.ok() {
        return Ok((
            "fail",
            format!("{exe}, but its server doesn't answer: start Rex, or run `rex server start`"),
        ));
    }
    let version = match serde_json::from_str::<Value>(&p.stdout) {
        Ok(Value::Object(o)) => o.get("rex_version").filter(|v| !v.is_null()).map(py_str),
        _ => None,
    };
    let key = version.as_deref().and_then(rex_version_key);
    let Some(key) = key else {
        let shown = version.filter(|v| !v.is_empty()).unwrap_or_else(|| "none given".into());
        return Ok(("skip", format!("{exe}; can't tell the server's version ({shown})")));
    };
    let version = version.unwrap_or_default();
    if key < REX_MIN {
        return Ok((
            "fail",
            format!(
                "the server is {version}; bullpen needs 0.1.0-preview.13 or later \
                 (minimized floating panes): update Rex and restart its server"
            ),
        ));
    }
    Ok(("ok", format!("server {version}, {exe}")))
}

pub fn check_gh(repo: Option<&Repo>) -> Check {
    let repo = match repo {
        Some(r) if r.has_origin()? => r,
        _ => return Ok(("skip", "local mode (no origin remote): gh isn't needed".into())),
    };
    let gh = repo.gh();
    let Some(exe) = which(&gh) else {
        return Ok((
            "fail",
            format!("{gh} isn't on PATH, and this repo has a remote: install gh (https://cli.github.com)"),
        ));
    };
    if !run(&[exe.as_str(), "auth", "status"], false)?.ok() {
        return Ok(("fail", format!("{exe} isn't signed in: run `gh auth login`")));
    }
    Ok(("ok", format!("signed in, {exe}")))
}

/// The programs the coordinator, workers and reviewers run (claude, unless changed).
pub fn check_agents(cfg: &Obj) -> Check {
    let mut progs: Vec<(String, Vec<&str>)> = Vec::new();
    for key in ["coordinator_cmd", "worker", "reviewer"] {
        if !truthy(cfg.get(key)) {
            continue;
        }
        let Some(p) = strs(cfg, key).into_iter().next() else {
            continue;
        };
        match progs.iter_mut().find(|(q, _)| *q == p) {
            Some((_, keys)) => keys.push(key),
            None => progs.push((p, vec![key])),
        }
    }
    let missing: Vec<String> = progs
        .iter()
        .filter(|(p, _)| which(p).is_none())
        .map(|(p, keys)| {
            let what = if Path::new(p).file_name().is_some_and(|n| n == "claude") {
                format!("{p} isn't on PATH: install Claude Code (https://claude.com/claude-code)")
            } else {
                format!("{p} isn't on PATH")
            };
            format!("{what} (config.json's {})", keys.join(", "))
        })
        .collect();
    if !missing.is_empty() {
        return Ok(("fail", missing.join("; ")));
    }
    let found: Vec<String> = progs.iter().filter_map(|(p, _)| which(p)).collect();
    Ok(("ok", found.join(", ")))
}

/// Built from the board's source as it is now (see build::Program).
pub fn check_board() -> Check {
    if let Some(b) = std::env::var_os("BULLPEN_BOARD").filter(|b| !b.is_empty()) {
        let exe: std::path::PathBuf = Path::new(&b).components().collect();
        if executable(&exe) {
            return Ok(("ok", format!("{} (BULLPEN_BOARD)", exe.display())));
        }
        return Ok((
            "fail",
            format!("BULLPEN_BOARD is {}, which isn't an executable file", exe.display()),
        ));
    }
    check_build(&BOARD, "running boards reload when it's done")
}

/// bin/bullpen's Rust build, built from its source as it is now.
pub fn check_cli() -> Check {
    check_build(&CLI, "commands run it from their next call")
}

/// A program bullpen builds from its own checkout: built from its source as it is now, or being built.
fn check_build(prog: &Program, then: &str) -> Check {
    let build = format!("run `{} build`", self_bin().display());
    let cargo = if which("cargo").is_some() {
        ""
    } else {
        " (it needs cargo: https://rustup.rs)"
    };
    let src = prog
        .crates
        .iter()
        .map(|c| format!("{c}/"))
        .collect::<Vec<_>>()
        .join(", ");
    let log = build_log();
    let log = log.display();
    if building()? {
        return Ok(("ok", format!("being rebuilt now (log: {log}); {then}")));
    }
    if !executable(&prog.installed()) {
        return Ok(("fail", format!("not built: {build}{cargo}")));
    }
    let st = prog.stamp()?;
    let hash = Value::String(prog.source_hash()?);
    if st.get("source") == Some(&hash) {
        return Ok((
            "ok",
            format!("{}, built from {src} as they are", prog.installed().display()),
        ));
    }
    if st.get("failed") == Some(&hash) {
        return Ok((
            "fail",
            format!("older than {src}: its build failed (log: {log}). Fix that, then {build}{cargo}"),
        ));
    }
    if prog.dirty()? {
        return Ok((
            "fail",
            format!(
                "older than {src}, which have uncommitted changes, so bullpen won't build it by \
                 itself: commit them, or {build} to build them as they are"
            ),
        ));
    }
    Ok((
        "fail",
        format!("older than {src}: `bullpen up` rebuilds it in the background, or {build}{cargo}"),
    ))
}

/// Rex lists the actions its config defines: bullpen's keys define four.
pub fn check_keys() -> Check {
    let r = rex();
    if which(&r).is_none() {
        return Ok(("skip", "can't tell without rex".into()));
    }
    let p = run(&[r.as_str(), "actions", "--json"], false)?;
    let actions = match serde_json::from_str::<Value>(&p.stdout) {
        Ok(Value::Object(o)) if p.ok() => o.get("actions").and_then(Value::as_array).cloned(),
        _ => None,
    };
    let Some(actions) = actions else {
        return Ok((
            "skip",
            "can't tell: `rex actions --json` didn't list the actions".into(),
        ));
    };
    let have: Vec<&Obj> = actions.iter().filter_map(Value::as_object).collect();
    // the last action of a name wins, as a dict built from the list keeps it
    let named = |n: &str| {
        have.iter()
            .rev()
            .find(|a| get(a, "name").and_then(Value::as_str) == Some(n))
            .copied()
    };
    let missing: Vec<&str> = KEY_ACTIONS.iter().copied().filter(|a| named(a).is_none()).collect();
    if !missing.is_empty() && missing.len() < KEY_ACTIONS.len() {
        // the line is in the config, but Rex loaded keys.lua before it had these
        return Ok((
            "fail",
            format!(
                "{} missing: Rex loaded an older keys.lua. Run `rex config reload`",
                missing.join(", ")
            ),
        ));
    }
    if !missing.is_empty() {
        return Ok(("fail", "not in your Rex config: run `bullpen keys --install`".into()));
    }
    let first = named(KEY_ACTIONS[0]).unwrap_or(have[0]);
    let source = match get(first, "source") {
        Some(v) if truthy(Some(v)) => py_str(v),
        _ => String::new(),
    };
    let src = source.rsplit_once(':').map_or(source.as_str(), |(a, _)| a);
    Ok((
        "ok",
        if src.is_empty() {
            "set up".into()
        } else {
            format!("set up, from {src}")
        },
    ))
}

pub fn check_watcher() -> Check {
    let w = watcher_record()?;
    if !truthy(w.get("block")) {
        return Ok(("fail", "never started: run `bullpen up`".into()));
    }
    let block = py_str(&w["block"]);
    let session = w
        .get("session")
        .filter(|v| !v.is_null())
        .map(py_str)
        .unwrap_or_default();
    if !crate::rex::alive(&session, &block)? {
        return Ok((
            "fail",
            format!("not running (its pane {block} is gone): run `bullpen up`"),
        ));
    }
    if w.get("stamp") != Some(&watcher_stamp()?) {
        return Ok((
            "fail",
            format!("running ({block}), but from an older watch.lua or bullpen: run `bullpen up` to restart it"),
        ));
    }
    Ok(("ok", format!("running and current, {block}")))
}

/// Make STATE, and a file in it (gone at once).
fn write_in_state(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let path = dir.join(format!(".doctor-{}-{nanos:08x}", std::process::id()));
    std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    std::fs::remove_file(&path)
}

pub fn check_state() -> Check {
    let st = state();
    if let Err(e) = write_in_state(&st) {
        return Ok((
            "fail",
            format!(
                "can't write to {}: {}. Fix its permissions, or set BULLPEN_STATE",
                st.display(),
                strerror(&e)
            ),
        ));
    }
    Ok(("ok", format!("{} is writable", st.display())))
}

/// Python's JSONDecodeError message for text that doesn't parse, as near as serde's
/// error lets us say it: its words, and where (line, column, character offset).
pub fn json_error(text: &str, e: &serde_json::Error) -> String {
    if text.starts_with('\u{feff}') {
        return "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)".into();
    }
    let raw = e.to_string();
    let msg = raw.split(" at line ").next().unwrap_or(&raw);
    let chars: Vec<char> = text.chars().collect();
    // where serde stopped, as a character offset: its column is the character it
    // stopped at, or at the end of the text the last one
    let before: usize = text
        .split('\n')
        .take(e.line().saturating_sub(1))
        .map(|l| l.chars().count() + 1)
        .sum();
    let mut pos = (before + e.column()).saturating_sub(1);
    if e.is_eof() {
        pos = chars.len();
    }
    pos = pos.min(chars.len());
    let last = chars[..pos].iter().rev().copied().find(|c| !c.is_whitespace());
    let what = match msg {
        "EOF while parsing a value" | "expected value" | "expected ident" | "invalid number" => {
            // Python points at the start of the value
            while pos > 0 && (chars[pos - 1].is_alphanumeric() || "+-.".contains(chars[pos - 1])) {
                pos -= 1;
            }
            "Expecting value"
        }
        "trailing characters" => "Extra data",
        "key must be a string" => "Expecting property name enclosed in double quotes",
        "EOF while parsing an object" => match last {
            Some('{' | ',') => "Expecting property name enclosed in double quotes",
            Some(':') => "Expecting value",
            _ => "Expecting ',' delimiter",
        },
        "expected `:`" => "Expecting ':' delimiter",
        "expected `,` or `}`" | "expected `,` or `]`" | "EOF while parsing a list" => "Expecting ',' delimiter",
        "EOF while parsing a string" => {
            // Python points at the string's opening quote
            let mut i = pos;
            while i > 0 && !(chars[i - 1] == '"' && (i < 2 || chars[i - 2] != '\\')) {
                i -= 1;
            }
            pos = i.saturating_sub(1);
            "Unterminated string starting at"
        }
        "trailing comma" => {
            let kind = if chars.get(pos) == Some(&']') {
                "array"
            } else {
                "object"
            };
            pos = chars[..pos].iter().rposition(|&c| c == ',').unwrap_or(pos);
            if kind == "array" {
                "Illegal trailing comma before end of array"
            } else {
                "Illegal trailing comma before end of object"
            }
        }
        _ => return raw,
    };
    let line = chars[..pos].iter().filter(|&&c| c == '\n').count() + 1;
    let col = pos - chars[..pos].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1) + 1;
    format!("{what}: line {line} column {col} (char {pos})")
}

pub fn check_config(d: &Path) -> Check {
    let path = d.join("config.json");
    if !path.exists() {
        return Ok(("skip", "none yet: `bullpen up` here makes one".into()));
    }
    let bytes = std::fs::read(&path).map_err(|e| os_error(&e, &path))?;
    let why = match String::from_utf8(bytes) {
        Err(e) => Some(e.to_string()),
        Ok(text) => serde_json::from_str::<Value>(&text)
            .err()
            .map(|e| json_error(&text, &e)),
    };
    if let Some(why) = why {
        return Ok((
            "fail",
            format!(
                "{} doesn't parse ({why}): fix it, or move it away and run `bullpen up`",
                path.display()
            ),
        ));
    }
    Ok(("ok", format!("{} parses", path.display())))
}

/// The global config.json, if there is one, parses and holds no repository's own keys.
pub fn check_global() -> Check {
    let path = global_config_path();
    if !path.exists() {
        return Ok((
            "skip",
            format!("none ({}; `bullpen config --global` makes one)", path.display()),
        ));
    }
    let g = match read_global_file() {
        Ok(g) => g,
        Err(e) => return Ok(("fail", e.0)),
    };
    let bad: Vec<&str> = g
        .keys()
        .map(String::as_str)
        .filter(|k| is_repo_only(k) || is_internal(k))
        .collect();
    if !bad.is_empty() {
        return Ok((
            "fail",
            format!(
                "{} sets {}, which only a repository can have: remove it from the file \
                 (each repo's `bullpen config` sets it)",
                path.display(),
                bad.join(", ")
            ),
        ));
    }
    Ok(("ok", format!("{} parses", path.display())))
}

/// Is word something sh can run here: a program on PATH, a path, or a builtin?
fn shell_word_found(word: &str, cwd: &Path) -> Res<bool> {
    Ok(run_in(&["sh", "-c", "command -v \"$1\" >/dev/null", "sh", word], cwd, false)?.ok())
}

/// The setup (or teardown) a new task would get: config.json's command, else
/// .bullpen/<name>.sh as committed on the base. It must parse and find what it runs.
pub fn check_hook(repo: Option<&Repo>, name: &str, why_not: Option<&str>) -> Check {
    let Some(repo) = repo else {
        return Ok((
            "skip",
            format!("can't tell: {}", why_not.unwrap_or("not in a git repository")),
        ));
    };
    let path = format!(".bullpen/{name}.sh");
    if truthy(repo.cfg.get(name)) {
        let cmd = py_str(&repo.cfg[name]);
        let source = format!("config.json's {name}");
        if !run(&["sh", "-n", "-c", cmd.as_str()], false)?.ok() {
            return Ok((
                "fail",
                format!("{source} isn't valid sh: fix it with `bullpen config --{name} \"<cmd>\"`"),
            ));
        }
        let word = shlex_split(&cmd).and_then(|w| w.into_iter().next()).unwrap_or_default();
        if !word.is_empty() && !word.contains('=') && !shell_word_found(&word, &repo.root)? {
            return Ok((
                "fail",
                format!("{source} runs {word}, which isn't on PATH: install it, or fix the command"),
            ));
        }
        return Ok(("ok", format!("{source}: {cmd}")));
    }
    let r = repo_base_ref(repo)?;
    let p = git(&repo.root, &["show", &format!("{r}:{path}")], false)?;
    if !p.ok() || strip(&p.stdout).is_empty() {
        if repo.root.join(&path).exists() {
            return Ok((
                "fail",
                format!("{path} isn't committed on {r}, so it won't run: commit it (and push it)"),
            ));
        }
        return Ok(("skip", format!("none ({path} or `bullpen config --{name}`)")));
    }
    let text = p.stdout;
    let first = splitlines(&text).first().copied().unwrap_or("");
    let bang = first.starts_with("#!");
    let shell = if bang {
        let argv = split(&first[2..]);
        if argv.is_empty() {
            return Ok((
                "fail",
                format!("{path} on {r} has an empty #! line: name its interpreter, or remove the line"),
            ));
        }
        let Some(prog) = shebang_program(&argv) else {
            return Ok((
                "fail",
                format!("{path} on {r} has a #! line that names no program: fix it"),
            ));
        };
        let here = if prog.contains('/') {
            executable(Path::new(&prog))
        } else {
            which(&prog).is_some()
        };
        if !here {
            return Ok((
                "fail",
                format!("{path} on {r} runs under {prog}, which isn't here: install it, or fix its #! line"),
            ));
        }
        Path::new(&prog)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        "sh".to_string()
    };
    if ["sh", "bash", "dash", "zsh"].contains(&shell.as_str())
        && !run_input(&[shell.as_str(), "-n"], &text, false)?.ok()
    {
        return Ok((
            "fail",
            format!("{path} on {r} isn't valid {shell}: `{shell} -n {path}` says where"),
        ));
    }
    Ok((
        "ok",
        format!("{path} on {r}{}", if bang { "" } else { ", run under sh" }),
    ))
}

/// f"{d.get(key)}": the value as Python prints it, None when it isn't there.
fn shown(o: &Obj, key: &str) -> String {
    o.get(key).map_or_else(|| "None".into(), py_str)
}

/// The coordinators config.json lists still have their panes (and boards).
pub fn check_coordinators(repo: Option<&Repo>, why_not: Option<&str>) -> Check {
    if let (None, Some(why)) = (repo, why_not) {
        return Ok(("skip", format!("can't tell: {why}")));
    }
    let Some(repo) = repo.filter(|r| truthy(r.cfg.get("coordinators"))) else {
        return Ok(("skip", "none yet: run `bullpen up`".into()));
    };
    let (mut gone, mut old, mut ok) = (Vec::new(), Vec::new(), Vec::new());
    for c in coordinators(repo) {
        let id = shown(c, "id");
        if !alive(Some(c))? {
            gone.push(format!("{id}'s pane ({}) is gone", shown(c, "block")));
            continue;
        }
        let b = obj(c, "board");
        let board_block = b.get("block").filter(|v| truthy(Some(v))).map(py_str);
        if let Some(bb) = &board_block {
            let session = b
                .get("session")
                .filter(|v| !v.is_null())
                .map(py_str)
                .unwrap_or_default();
            if !crate::rex::alive(&session, bb)? {
                gone.push(format!("{id}'s board ({bb}) is gone"));
                continue;
            }
            if old_board(b)?.is_some_and(|o| !o.is_empty()) {
                old.push(format!("{id}'s board is from an older build"));
                continue;
            }
        }
        ok.push(format!("{id} ({})", shown(c, "block")));
    }
    if gone.is_empty() && old.is_empty() {
        return Ok(("ok", ok.join(", ")));
    }
    let mut parts = Vec::new();
    if !gone.is_empty() {
        parts.push(format!(
            "{}: run `bullpen up`, which drops gone coordinators and reopens boards",
            gone.join("; ")
        ));
    }
    if !old.is_empty() {
        let them = if old.len() == 1 { "it" } else { "them" };
        parts.push(format!("{}: `bullpen up` restarts {them}", old.join(", ")));
    }
    Ok(("fail", parts.join("; ")))
}
