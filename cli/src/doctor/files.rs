//! doctor's checks of bullpen's files: the state folder, the global config.json and
//! this repository's.

use super::{Check, DoctorCheck, Fix, Fixer};
use bullpen_core::config::{global_config_path, read_global_file};
use bullpen_core::paths::state;
use bullpen_core::settings::{is_internal, is_repo_only};
use bullpen_core::Fail;
use serde_json::Value;
use std::path::Path;

pub(super) const CHECKS: &[DoctorCheck] = &[STATE, TEMP, GLOBAL, CONFIG];

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

const STATE: DoctorCheck = DoctorCheck {
    name: "state",
    shared: true,
    run: |_| check_state(),
    ..DoctorCheck::NEW
};

/// The temp folder (claude_tmp.rs): how full it is, and the Claude scratch folders closed
/// tasks left in it, which its fix removes.
const TEMP: DoctorCheck = DoctorCheck {
    name: "temp",
    shared: true,
    slow: true,
    run: |_| Ok(crate::claude_tmp::check()),
    fix: Some(Fix {
        help: "the Claude scratch folders closed tasks of any repository left in the temp folder",
        run: Fixer::Anywhere(crate::claude_tmp::fix),
    }),
    ..DoctorCheck::NEW
};

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

const CONFIG: DoctorCheck = DoctorCheck {
    name: "config",
    needs_dir: true,
    run: |c| {
        c.dir
            .map_or_else(|| Ok(("skip", "not in a git repository".into())), check_config)
    },
    ..DoctorCheck::NEW
};

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

const GLOBAL: DoctorCheck = DoctorCheck {
    name: "global",
    shared: true,
    run: |_| check_global(),
    ..DoctorCheck::NEW
};

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
