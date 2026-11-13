//! bullpen's key bindings in the Rex config (bullpen/keys.py): the line that loads
//! lua/keys.lua, `keys --install` adding it, and the hash accept checks to say that
//! keys.lua changed.

use crate::args::{flag, Arg, Args};
use bullpen_core::paths::{home, rex, self_bin, state, user_home};
use bullpen_core::proc::{file_hash, run, which};
use bullpen_core::text::strip;
use bullpen_core::{fail, Fail, Res};
use std::io::Write;
use std::path::{Path, PathBuf};

const KEYS: &[Arg] = &[flag(
    "--install",
    "add the line to the end of your Rex config if it's missing, then `rex config reload`",
)];

pub const KEYS_CHANGED: &str = "Keys changed: run `rex config reload`";

fn keys_lua() -> PathBuf {
    home().join("lua").join("keys.lua")
}

/// The line for the Rex config: keys.lua's setup, with this bullpen (and its state
/// folder, when BULLPEN_STATE sets one).
pub fn keys_line() -> String {
    let mut opts = format!("bin = \"{}\"", self_bin().display());
    if std::env::var("BULLPEN_STATE").is_ok_and(|v| !v.is_empty()) {
        opts += &format!(", state = \"{}\"", state().display());
    }
    format!("dofile(\"{}\").setup{{ {opts} }}", keys_lua().display())
}

/// keys.lua as it is, which Rex reads only when its config loads: an accept that
/// changes it says to reload.
pub fn keys_hash() -> Option<String> {
    file_hash(keys_lua()).ok()
}

/// The init.lua Rex reads: REX_CONFIG, else rex/init.lua in the config directory.
fn rex_config_path() -> PathBuf {
    if let Some(p) = std::env::var("REX_CONFIG").ok().filter(|v| !v.is_empty()) {
        return expanduser(&p);
    }
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => Path::new(&user_home()).join(".config"),
    };
    base.join("rex").join("init.lua")
}

/// Path.expanduser for ~ and ~/…
fn expanduser(p: &str) -> PathBuf {
    match p.strip_prefix('~') {
        Some("") => PathBuf::from(user_home()),
        Some(rest) if rest.starts_with('/') => PathBuf::from(format!("{}{rest}", user_home())),
        _ => PathBuf::from(p),
    }
}

/// Path.resolve (not strict, as os.path.realpath): each part's link followed where
/// it exists, ".." taking off the part before.
fn resolve(p: &Path) -> PathBuf {
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::from("/");
    for part in p.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Normal(name) => {
                out.push(name);
                if let Ok(real) = out.canonicalize() {
                    out = real;
                }
            }
            _ => {}
        }
    }
    out
}

/// Is c one of the characters Python's `\s` matches?
fn space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// The keys.lua files the config's text loads with `dofile("…keys.lua").setup` (the
/// regex `dofile\s*\(\s*["']([^"']*keys\.lua)["']\s*\)\s*\.\s*setup`, each match).
pub fn keys_in_config(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = text[at..].find("dofile") {
        let start = at + i;
        match keys_at(&text[start + "dofile".len()..]) {
            Some((file, used)) => {
                out.push(file);
                at = start + "dofile".len() + used;
            }
            None => at = start + 1,
        }
    }
    out
}

/// After "dofile": the file and how many bytes the rest of the match took.
fn keys_at(rest: &str) -> Option<(String, usize)> {
    let skip = |s: &str, i: usize| i + s[i..].len() - s[i..].trim_start_matches(space).len();
    let mut i = skip(rest, 0);
    i += rest[i..].strip_prefix('(').map(|_| 1)?;
    i = skip(rest, i);
    i += rest[i..].starts_with(['"', '\'']).then_some(1)?;
    let len = rest[i..].find(['"', '\''])?;
    let file = &rest[i..i + len];
    if !file.ends_with("keys.lua") {
        return None;
    }
    i += len + 1;
    for want in [")", ".", "setup"] {
        i = skip(rest, i);
        i += rest[i..].starts_with(want).then_some(want.len())?;
    }
    Some((file.to_string(), i))
}

/// `keys`: print the Rex config line for bullpen's key bindings (--install: add it).
pub fn cmd_keys(argv: &[String]) -> Res<i32> {
    let a = Args::parse("keys", argv, KEYS);
    if a.flag("--install") {
        install_keys()?;
        return Ok(0);
    }
    println!(
        "-- Add to your Rex config (rex config shows where), then run `rex config reload`\n\
         -- (`bullpen keys --install` does both):\n\
         {}\n\
         -- Defaults: cmd+;>n next task needing you, cmd+;>b board, cmd+;>c coordinator,\n\
         -- cmd+;>j jump to any open task (type part of it, in any repository),\n\
         -- and ctrl+escape puts the worker pane you're in back into its capsule.\n\
         -- Change them with prefix = \"...\", or keys = {{ next = ..., board = ..., coordinator = ..., jump = ..., \
         hide = ... }}.",
        keys_line()
    );
    Ok(0)
}

/// Add bullpen's line to the end of the Rex config if it isn't there, then reload. It
/// only ever appends: what's above it (a hook that must run first, say) stays put.
fn install_keys() -> Res<()> {
    let path = rex_config_path();
    let text = match std::fs::read(&path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let found = keys_in_config(&text);
    let ours = resolve(&keys_lua());
    if found.iter().any(|f| resolve(&expanduser(f)) == ours) {
        println!("already in {}", path.display());
    } else if let Some(other) = found.first() {
        return fail(format!(
            "{} already loads another keys.lua ({other}): change that line to\n{}",
            path.display(),
            keys_line()
        ));
    } else {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // a blank line after what's there
        let gap = if text.is_empty() {
            ""
        } else if text.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let mut f = std::fs::OpenOptions::new().append(true).create(true).open(&path)?;
        let line = format!(
            "{gap}-- bullpen's keys (`bullpen keys` says what they are)\n{}\n",
            keys_line()
        );
        f.write_all(line.as_bytes())?;
        println!("added to the end of {}", path.display());
    }
    if which(&rex()).is_none() {
        return fail("rex isn't on PATH: run `rex config reload` once it is");
    }
    let p = run(&[rex().as_str(), "config", "reload"], false)?;
    if !p.ok() {
        let said = strip(if p.stderr.is_empty() { &p.stdout } else { &p.stderr });
        let said = if said.is_empty() { "it said nothing" } else { said };
        return Err(Fail(format!("`rex config reload` failed: {said}")));
    }
    println!("Rex config reloaded: the cmd+; keys work now");
    Ok(())
}

#[cfg(test)]
#[path = "test_keys.rs"]
mod tests;
