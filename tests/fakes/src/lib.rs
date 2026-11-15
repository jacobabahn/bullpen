//! What the fake `rex` and `gh` share. They were Python scripts, and the tests read what
//! they write (rex.log, gh.log, reg.json) as Python wrote it, so JSON comes out as
//! Python's `json.dumps` writes it and files are read as `Path.read_text` reads them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// How a fake stops early: its exit code and what it says on stderr. Nothing after it
/// runs, so reg.json isn't written either.
#[derive(Debug)]
pub struct Fail {
    pub code: i32,
    pub msg: Option<String>,
}

pub type Res<T> = Result<T, Fail>;

/// `sys.exit("msg")`: the message on stderr, exit 1.
pub fn exit_msg(msg: &str) -> Fail {
    Fail {
        code: 1,
        msg: Some(msg.into()),
    }
}

/// `sys.exit(code)`: nothing said.
pub fn exit_code(code: i32) -> Fail {
    Fail { code, msg: None }
}

/// What raised an exception in the Python: exit 1, saying why.
pub fn broke(what: &str) -> Fail {
    Fail {
        code: 1,
        msg: Some(format!("fake: {what}")),
    }
}

/// Exit as the Python would have: Ok is 0.
pub fn finish(r: Res<()>) -> ! {
    let code = match r {
        Ok(()) => 0,
        Err(f) => {
            if let Some(m) = f.msg {
                eprintln!("{m}");
            }
            f.code
        }
    };
    std::process::exit(code)
}

/// The folder the tests keep the fakes' files in: $FAKE_DIR.
pub fn fake_dir() -> Res<PathBuf> {
    std::env::var_os("FAKE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| broke("FAKE_DIR isn't set"))
}

/// `Path.read_text()`: UTF-8, with "\r\n" and "\r" read as "\n".
pub fn read_text(p: &Path) -> Res<String> {
    let bytes = std::fs::read(p).map_err(|e| broke(&format!("{}: {e}", p.display())))?;
    let s = String::from_utf8(bytes).map_err(|_| broke(&format!("{}: not UTF-8", p.display())))?;
    Ok(if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        s
    })
}

/// `json.loads` of a file's text.
pub fn read_json(p: &Path) -> Res<Value> {
    loads(&read_text(p)?)
}

pub fn loads(s: &str) -> Res<Value> {
    serde_json::from_str(s).map_err(|e| broke(&format!("bad JSON: {e}")))
}

/// Add a line to a log file.
pub fn append(p: &Path, line: &str) -> Res<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
        .map_err(|e| broke(&format!("{}: {e}", p.display())))?;
    f.write_all(format!("{line}\n").as_bytes())
        .map_err(|e| broke(&e.to_string()))
}

/// `print(s)`: s and a newline on stdout.
pub fn print(s: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.write_all(b"\n");
}

/// Python's `json.dumps(v)`: ", " and ": " between items, and only ASCII (the rest as \uXXXX).
pub fn dumps(v: &Value) -> String {
    let mut s = String::new();
    dump(v, &mut s);
    s
}

fn dump(v: &Value, s: &mut String) {
    match v {
        Value::Null => s.push_str("null"),
        Value::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => s.push_str(&n.to_string()),
        Value::String(t) => quote(t, s),
        Value::Array(items) => {
            s.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                dump(item, s);
            }
            s.push(']');
        }
        Value::Object(m) => {
            s.push('{');
            for (i, (k, item)) in m.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                quote(k, s);
                s.push_str(": ");
                dump(item, s);
            }
            s.push('}');
        }
    }
}

fn quote(t: &str, s: &mut String) {
    s.push('"');
    for c in t.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            '\u{8}' => s.push_str("\\b"),
            '\u{c}' => s.push_str("\\f"),
            ' '..='~' => s.push(c),
            _ => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(s, "\\u{u:04x}");
                }
            }
        }
    }
    s.push('"');
}

/// Python's truth of a JSON value: null, false, 0, "", [] and {} are false.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

#[cfg(test)]
#[path = "test_lib.rs"]
mod tests;
