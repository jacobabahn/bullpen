//! Where bullpen's things are (core.py's HOME, STATE, REX, SELF, PROMPTS, WATCH_LUA),
//! and paths in words for people: $HOME as ~.

use std::path::{Path, PathBuf};

/// Path.home(): $HOME ("/" when it isn't set).
pub fn user_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

/// bullpen's checkout (core.HOME): the nearest folder above this program that has
/// bin/bullpen-py. The installed program is HOME/target/bullpen and cargo's own is
/// HOME/target/release/bullpen; a test binary falls back to the folder above core/.
pub fn home() -> PathBuf {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .unwrap_or_default();
    exe.ancestors()
        .find(|d| d.join("bin/bullpen-py").is_file())
        .map(Path::to_path_buf)
        .unwrap_or_else(source_home)
}

/// The checkout this crate was built from: the folder above core/.
fn source_home() -> PathBuf {
    let core = Path::new(env!("CARGO_MANIFEST_DIR"));
    core.parent().unwrap_or(core).to_path_buf()
}

/// bullpen's state folder (core.STATE): $BULLPEN_STATE, else ~/.local/state/bullpen.
pub fn state() -> PathBuf {
    match std::env::var("BULLPEN_STATE") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(user_home()).join(".local/state/bullpen"),
    }
}

/// The rex program (core.REX): $BULLPEN_REX, else "rex".
pub fn rex() -> String {
    std::env::var("BULLPEN_REX").unwrap_or_else(|_| "rex".into())
}

/// The entry script (core.SELF): what Lua, the key bindings and bullpen's own
/// background jobs run.
pub fn self_bin() -> PathBuf {
    home().join("bin/bullpen")
}

/// The Python bullpen's entry script.
pub fn python_entry() -> PathBuf {
    home().join("bin/bullpen-py")
}

/// The agents' prompts.
pub fn prompts() -> PathBuf {
    home().join("prompts")
}

/// The watcher Rex runs.
pub fn watch_lua() -> PathBuf {
    home().join("lua/watch.lua")
}

/// A name made safe for a file name: each character but A-Z a-z 0-9 . _ - becomes _.
pub fn safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A path with $HOME as ~, for people to read (core.home).
pub fn home_path(path: impl AsRef<Path>) -> String {
    let path = path.as_ref().display().to_string();
    let h = user_home();
    if h != "/" && (path == h || path.starts_with(&format!("{h}/"))) {
        format!("~{}", &path[h.len()..])
    } else {
        path
    }
}

/// Python's `\w`: a letter, digit or underscore (in any script).
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Text with each path under $HOME written with ~, for people to read (core.homes):
/// $HOME where the next character isn't a word character, "." or "-".
pub fn homes(text: &str) -> String {
    let h = user_home();
    if h == "/" || h.is_empty() {
        return text.into();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(&h) {
        let after = &rest[i + h.len()..];
        let ends = after
            .chars()
            .next()
            .is_none_or(|c| !(is_word(c) || c == '.' || c == '-'));
        if ends {
            out.push_str(&rest[..i]);
            out.push('~');
            rest = after;
        } else {
            // no match here: go on from the next character, as re.sub does
            let step = i + rest[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&rest[..step]);
            rest = &rest[step..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "test_paths.rs"]
mod tests;
