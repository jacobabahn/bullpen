//! The hook that parks the waiter (`news --wake`, waiter.rs), in the repo's
//! `.claude/settings.local.json`: a Stop and a StopFailure entry, each
//! `{"type":"command","command":"<bullpen> news --wake","asyncRewake":true,"timeout":86400}`.
//!
//! `config --news wake` writes them, another value removes them, `up` keeps them
//! current and `doctor` checks them (doctor/news.rs). The file is the person's and
//! Claude Code's: bullpen finds its own entries by the command's `news --wake` ending
//! and touches nothing else. It keeps the keys' order, writes by rename, and removes
//! only the lists (and the file) it emptied itself. The file is added to the
//! checkout's `.git/info/exclude`: it holds one person's path to bullpen.

use bullpen_core::json::{s, strs, Obj};
use bullpen_core::proc::git;
use bullpen_core::text::shell_join;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The settings file, from the repo's root.
pub const FILE: &str = ".claude/settings.local.json";
/// The turn ends that park a waiter: one that went well, and one that ended in an API error.
pub const EVENTS: [&str; 2] = ["Stop", "StopFailure"];
/// How bullpen knows its own entries: their command ends with this.
const ENDING: &str = "news --wake";

pub fn path(repo: &Repo) -> PathBuf {
    repo.root.join(FILE)
}

/// The command the hook runs: the waiter, of the copy of bullpen the repo's coordinator
/// uses (as the watcher is; this copy with none recorded).
pub fn command(repo: &Repo) -> String {
    let copy = crate::copies::coordinator_copy(repo)
        .filter(|c| crate::copies::there(c))
        .unwrap_or_else(bullpen_core::paths::home);
    let argv = [
        copy.join("bin/bullpen").display().to_string(),
        "news".into(),
        "--wake".into(),
    ];
    shell_join(&crate::rex::with_state(&argv))
}

/// The entry for a command.
fn handler(command: &str) -> Value {
    json!({"type": "command", "command": command, "asyncRewake": true,
           "timeout": super::waiter::TIMEOUT as u64})
}

/// Is this handler one of bullpen's?
fn ours(h: &Value) -> bool {
    let command = h.get("command").and_then(Value::as_str).unwrap_or("");
    command.trim_end().ends_with(ENDING)
}

/// Is the coordinator's agent Claude Code? Only it has the hook.
pub fn claude(repo: &Repo) -> bool {
    program(repo).is_some_and(|p| Path::new(&p).file_name().is_some_and(|n| n == "claude"))
}

/// The program `coordinator_cmd` runs.
pub fn program(repo: &Repo) -> Option<String> {
    strs(&repo.cfg, "coordinator_cmd").into_iter().next()
}

/// The settings as the file has them; None with no file. Err: it doesn't parse, or
/// isn't an object.
pub fn read(path: &Path) -> Res<Option<Obj>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return fail(format!("{FILE} can't be read ({e})")),
    };
    if text.trim().is_empty() {
        return Ok(Some(Obj::new()));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(o)) => Ok(Some(o)),
        Ok(_) => fail(format!("{FILE} isn't a JSON object")),
        Err(e) => fail(format!("{FILE} doesn't parse ({e})")),
    }
}

/// An event's groups, each `{"matcher"?, "hooks": [handler, ...]}`: of bullpen's
/// handlers the first becomes `want` (with None it goes too) and the rest go, with any
/// group that leaves empty. (Changed, one was kept.)
fn settle(groups: &mut Vec<Value>, want: Option<&Value>) -> (bool, bool) {
    let (mut changed, mut kept) = (false, false);
    groups.retain_mut(|g| {
        let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true; // not a shape bullpen writes: not its own
        };
        let before = hs.len();
        hs.retain_mut(|h| {
            if !ours(h) {
                return true;
            }
            let Some(want) = want.filter(|_| !kept) else {
                return false;
            };
            kept = true;
            if h != want {
                *h = want.clone();
                changed = true;
            }
            true
        });
        changed |= hs.len() != before;
        !(hs.is_empty() && before > 0)
    });
    (changed, kept)
}

/// The `hooks` object of the settings, if it has one. Err: it is something else.
fn hooks_of(settings: &mut Obj) -> Res<Option<&mut Obj>> {
    match settings.get_mut("hooks") {
        None => Ok(None),
        Some(Value::Object(o)) => Ok(Some(o)),
        Some(_) => fail(format!("{FILE}'s `hooks` isn't an object")),
    }
}

fn groups_of<'a>(hooks: &'a mut Obj, event: &str) -> Res<Option<&'a mut Vec<Value>>> {
    match hooks.get_mut(event) {
        None => Ok(None),
        Some(Value::Array(a)) => Ok(Some(a)),
        Some(_) => fail(format!("{FILE}'s hooks.{event} isn't a list")),
    }
}

/// Put bullpen's entry for command under each event, once each. True if that changed
/// the settings.
pub fn put(settings: &mut Obj, command: &str) -> Res<bool> {
    let want = handler(command);
    if hooks_of(settings)?.is_none() {
        settings.insert("hooks".into(), json!({}));
    }
    let hooks = hooks_of(settings)?.expect("just made");
    let mut changed = false;
    for event in EVENTS {
        if groups_of(hooks, event)?.is_none() {
            hooks.insert(event.into(), json!([]));
        }
        let groups = groups_of(hooks, event)?.expect("just made");
        let (settled, kept) = settle(groups, Some(&want));
        if !kept {
            groups.push(json!({"hooks": [want.clone()]}));
        }
        changed |= settled || !kept;
    }
    Ok(changed)
}

/// Take bullpen's entries out, with the lists that leaves empty. True if there were any.
pub fn take(settings: &mut Obj) -> Res<bool> {
    let Some(hooks) = hooks_of(settings)? else {
        return Ok(false);
    };
    let mut changed = false;
    for event in EVENTS {
        let Some(groups) = groups_of(hooks, event)? else {
            continue;
        };
        let (settled, _) = settle(groups, None);
        if settled && groups.is_empty() {
            hooks.shift_remove(event);
        }
        changed |= settled;
    }
    if changed && hooks.is_empty() {
        settings.shift_remove("hooks");
    }
    Ok(changed)
}

/// (event, command) of each of bullpen's entries in the settings.
pub fn found(settings: &Obj) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for event in EVENTS {
        let groups = settings
            .get("hooks")
            .and_then(|h| h.get(event))
            .and_then(Value::as_array);
        for g in groups.into_iter().flatten() {
            let hs = g.get("hooks").and_then(Value::as_array);
            for h in hs.into_iter().flatten().filter(|h| ours(h)) {
                out.push((
                    event,
                    s(h.as_object().expect("it has a command"), "command").to_string(),
                ));
            }
        }
    }
    out
}

/// Would `put` change nothing: the entries are there, once each, for this command?
pub fn current(settings: &Obj, command: &str) -> bool {
    let mut copy = settings.clone();
    matches!(put(&mut copy, command), Ok(false))
}

/// Write the settings: 2 spaces, the keys in their order, by rename over the file a
/// link leads to, with the mode it had.
pub fn write(path: &Path, settings: &Obj) -> Res<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)? + "\n";
    let tmp = dir.join(format!(".settings.local.json.bullpen-{}", std::process::id()));
    let done = std::fs::write(&tmp, text)
        .and_then(|_| match std::fs::metadata(&path) {
            Ok(m) => std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(m.permissions().mode())),
            Err(_) => Ok(()),
        })
        .and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Git leaves the file out of the checkout's commits: a line in `.git/info/exclude`.
pub fn ignore(repo: &Repo) -> Res<()> {
    let line = format!("/{FILE}");
    let said = git(&repo.root, &["rev-parse", "--git-path", "info/exclude"], true)?;
    let exclude = repo.root.join(said.stdout.trim());
    let text = std::fs::read_to_string(&exclude).unwrap_or_default();
    if text.lines().any(|l| l.trim() == line || l.trim() == FILE) {
        return Ok(());
    }
    if let Some(dir) = exclude.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let gap = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&exclude, format!("{text}{gap}{line}\n"))?;
    Ok(())
}

/// Does git ignore the file? (A committed one isn't ignored, whatever the excludes say.)
pub fn ignored(repo: &Repo) -> Res<bool> {
    Ok(git(&repo.root, &["check-ignore", "-q", FILE], false)?.ok())
}

/// Write the hook, or bring it up to date. True if the file changed.
pub fn install(repo: &Repo) -> Res<bool> {
    let path = path(repo);
    let mut settings = read(&path)?.unwrap_or_default();
    ignore(repo)?;
    if !put(&mut settings, &command(repo))? {
        return Ok(false);
    }
    write(&path, &settings)?;
    Ok(true)
}

/// Remove the hook. True if it was there. A file left with nothing in it goes too.
pub fn remove(repo: &Repo) -> Res<bool> {
    let path = path(repo);
    let Some(mut settings) = read(&path)? else {
        return Ok(false);
    };
    if !take(&mut settings)? {
        return Ok(false);
    }
    if settings.is_empty() {
        std::fs::remove_file(&path)?;
    } else {
        write(&path, &settings)?;
    }
    Ok(true)
}

/// Why `wake` can't be this repo's news, if it can't: the coordinator isn't Claude Code.
pub fn refusal(repo: &Repo) -> Option<String> {
    if claude(repo) {
        return None;
    }
    let runs = program(repo).map_or("nothing".to_string(), |p| format!("`{p}`"));
    Some(format!(
        "`wake` needs Claude Code as the coordinator (its Stop hook parks the waiter), and this repo's \
         coordinator_cmd runs {runs}: use `bullpen config --news type`"
    ))
}

/// Make the file match the setting: the hook with `wake`, none otherwise. What was
/// done, in words; None when nothing had to be.
pub fn sync(repo: &Repo) -> Res<Option<String>> {
    if repo.cfg_str("news") == "wake" {
        if let Some(why) = refusal(repo) {
            return fail(why);
        }
        return Ok(install(repo)?.then(|| format!("wrote the news hook to {FILE}")));
    }
    Ok(remove(repo)?.then(|| format!("removed bullpen's news hook from {FILE}")))
}

#[cfg(test)]
#[path = "test_hook.rs"]
mod tests;
