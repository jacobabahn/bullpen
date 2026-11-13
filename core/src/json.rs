//! JSON as Python's json module reads and writes it: keys in their order, and
//! `dumps(indent=2)`'s output (non-ASCII as \uXXXX, floats as Python's repr). Also
//! reading values the way Python's truth and str() see them.

use crate::fail::{Fail, Res};
use serde_json::{Map, Value};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// A JSON object, its keys in their order: a task, a config.
pub type Obj = Map<String, Value>;

// ---------- writing ----------

/// `json.dumps(v, indent=2)`.
pub fn dumps(v: &Value) -> String {
    let mut out = String::new();
    put(&mut out, v, Some(2), 0);
    out
}

/// `json.dumps(o, indent=2)` of an object.
pub fn dumps_obj(o: &Obj) -> String {
    let mut out = String::new();
    put_obj(&mut out, o, Some(2), 0);
    out
}

/// `json.dumps(v)`: on one line, ", " between items and ": " after keys.
pub fn dumps_line(v: &Value) -> String {
    let mut out = String::new();
    put(&mut out, v, None, 0);
    out
}

/// Write v at depth (in indents) into out; indent None writes it on one line.
fn put(out: &mut String, v: &Value, indent: Option<usize>, depth: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => put_str(out, s),
        Value::Array(a) => {
            if a.is_empty() {
                return out.push_str("[]");
            }
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                gap(out, indent, depth + 1, i == 0);
                put(out, item, indent, depth + 1);
            }
            gap_end(out, indent, depth);
            out.push(']');
        }
        Value::Object(o) => put_obj(out, o, indent, depth),
    }
}

fn put_obj(out: &mut String, o: &Obj, indent: Option<usize>, depth: usize) {
    if o.is_empty() {
        return out.push_str("{}");
    }
    out.push('{');
    for (i, (k, item)) in o.iter().enumerate() {
        gap(out, indent, depth + 1, i == 0);
        put_str(out, k);
        out.push_str(": ");
        put(out, item, indent, depth + 1);
    }
    gap_end(out, indent, depth);
    out.push('}');
}

/// What goes before an item: "," unless it is the first, then a new line and its
/// indent (or a space, on one line).
fn gap(out: &mut String, indent: Option<usize>, depth: usize, first: bool) {
    if !first {
        out.push(',');
    }
    match indent {
        Some(n) => {
            out.push('\n');
            out.push_str(&" ".repeat(n * depth));
        }
        None if !first => out.push(' '),
        None => {}
    }
}

/// What goes before a closing bracket: a new line and the indent, when indented.
fn gap_end(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(n) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * depth));
    }
}

/// A number as Python writes it: an integer as it is, a float as its repr.
fn number(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() => float_repr(f),
        _ => n.to_string(),
    }
}

/// A string in quotes, escaped as ensure_ascii does: \" \\ \n \r \t \b \f, other
/// controls, DEL and everything past ASCII as \uXXXX (two, for a surrogate pair).
fn put_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{u:04x}");
                }
            }
        }
    }
    out.push('"');
}

/// A float as Python's repr writes it: the shortest digits that read back the same,
/// positional from 1e-4 up to 1e16, else with an exponent ("1e+16", "1.5e-05").
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let sci = format!("{f:e}"); // "-1.5e-5": the shortest digits
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mant) = mant.strip_prefix('-').map_or(("", mant), |m| ("-", m));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    if (-4..16).contains(&exp) {
        let point = exp + 1; // digits before the point
        let body = if point <= 0 {
            format!("0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!("{digits}{}.0", "0".repeat(point as usize - digits.len()))
        } else {
            format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
        };
        return format!("{sign}{body}");
    }
    let mant = if digits.len() > 1 {
        format!("{}.{}", &digits[..1], &digits[1..])
    } else {
        digits
    };
    format!("{sign}{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

// ---------- files ----------

/// A file's JSON (read_json); None when it is missing. Err with the reason when it
/// can't be read or doesn't parse.
pub fn read(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            serde_json::from_str(&text).map(Some).map_err(|e| e.to_string())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// A file's JSON object; None when it is missing. A Fail when it doesn't parse or
/// isn't an object, naming the file.
pub fn read_obj(path: &Path) -> Res<Option<Obj>> {
    let shown = path.display();
    if path.is_dir() {
        return Err(Fail(format!("{shown} is a folder, not a file: move it away")));
    }
    match read(path) {
        Ok(None) => Ok(None),
        Ok(Some(Value::Object(o))) => Ok(Some(o)),
        Ok(Some(_)) => Err(Fail(format!(
            "{shown} isn't a JSON object ({{...}}): fix it, or move it away"
        ))),
        Err(e) => Err(Fail(format!("{shown} doesn't parse ({e}): fix it, or move it away"))),
    }
}

/// Write JSON as core.write_json does: dumps and a newline, into a temporary file in
/// the same folder (.tmp-*) that then replaces the old one.
pub fn write(path: &Path, v: &Value) -> io::Result<()> {
    write_text(path, &dumps(v))
}

/// write, for an object.
pub fn write_obj(path: &Path, o: &Obj) -> io::Result<()> {
    write_text(path, &dumps_obj(o))
}

fn write_text(path: &Path, text: &str) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{}", tmp_suffix()));
    // mkstemp's mode, which the file keeps after the rename, as Python's does
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let written = f.write_all(text.as_bytes()).and_then(|_| f.write_all(b"\n"));
    drop(f);
    if let Err(e) = written.and_then(|_| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// A name no other writer uses at the same time: the process, the clock and a count.
fn tmp_suffix() -> String {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{}-{nanos}-{}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    )
}

// ---------- reading values as Python does ----------

/// Python's truth: missing, null, false, 0, "", [] and {} are false.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `o.get(key)`: None for a missing key or null.
pub fn get<'a>(o: &'a Obj, key: &str) -> Option<&'a Value> {
    o.get(key).filter(|v| !v.is_null())
}

/// The string at key, or "" (missing, null, or not a string).
pub fn s<'a>(o: &'a Obj, key: &str) -> &'a str {
    o.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `o.get(key) or {}`: the object at key, or an empty one.
pub fn obj<'a>(o: &'a Obj, key: &str) -> &'a Obj {
    static EMPTY: std::sync::OnceLock<Obj> = std::sync::OnceLock::new();
    o.get(key)
        .and_then(Value::as_object)
        .unwrap_or_else(|| EMPTY.get_or_init(Obj::new))
}

/// The strings of the list at key (a command, say); what isn't a string is left out.
pub fn strs(o: &Obj, key: &str) -> Vec<String> {
    let items = o.get(key).and_then(Value::as_array);
    items
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

/// Python's str() of a value: a string as it is, others as Python prints them.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python's repr() of a value read from JSON.
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => float_repr(f).replace("Infinity", "inf").replace("NaN", "nan"),
            _ => n.to_string(),
        },
        Value::String(s) => str_repr(s),
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, v)| format!("{}: {}", str_repr(k), py_repr(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// Python's repr() of a str: in single quotes unless it has one and no double quote.
fn str_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

#[cfg(test)]
#[path = "test_json.rs"]
mod tests;
