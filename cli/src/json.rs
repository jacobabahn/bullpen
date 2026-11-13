//! JSON as Python's json module reads and writes it: keys in their order, and
//! `dumps(indent=2)`'s output (non-ASCII as \uXXXX, floats as Python's repr).

use serde_json::ser::{CharEscape, Formatter, PrettyFormatter};
use serde_json::{Map, Value};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub type Obj = Map<String, Value>;

/// serde_json's pretty printer (two spaces, ": " and ","), with strings escaped as
/// Python's ensure_ascii does and floats written as Python's repr.
struct Py(PrettyFormatter<'static>);

impl Formatter for Py {
    fn begin_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_array(w)
    }
    fn end_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array(w)
    }
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_array_value(w, first)
    }
    fn end_array_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array_value(w)
    }
    fn begin_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object(w)
    }
    fn end_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object(w)
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_object_key(w, first)
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object_value(w)
    }
    fn end_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object_value(w)
    }

    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, f: f64) -> io::Result<()> {
        w.write_all(float_repr(f).as_bytes())
    }

    fn write_string_fragment<W: ?Sized + Write>(&mut self, w: &mut W, s: &str) -> io::Result<()> {
        for c in s.chars() {
            if c.is_ascii() && c != '\x7f' {
                w.write_all(&[c as u8])?;
            } else {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    write!(w, "\\u{u:04x}")?;
                }
            }
        }
        Ok(())
    }

    fn write_char_escape<W: ?Sized + Write>(&mut self, w: &mut W, e: CharEscape) -> io::Result<()> {
        let s = match e {
            CharEscape::Quote => "\\\"",
            CharEscape::ReverseSolidus => "\\\\",
            CharEscape::Solidus => "/",
            CharEscape::Backspace => "\\b",
            CharEscape::FormFeed => "\\f",
            CharEscape::LineFeed => "\\n",
            CharEscape::CarriageReturn => "\\r",
            CharEscape::Tab => "\\t",
            CharEscape::AsciiControl(b) => return write!(w, "\\u{b:04x}"),
        };
        w.write_all(s.as_bytes())
    }
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
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
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
    let mant = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits };
    format!("{sign}{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

/// `json.dumps(v, indent=2)`.
pub fn dumps(v: &Value) -> String {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, Py(PrettyFormatter::with_indent(b"  ")));
    serde::Serialize::serialize(v, &mut ser).expect("a Value always serializes");
    String::from_utf8(out).expect("the output is ASCII")
}

/// A file's JSON; None when it is missing. Err when it doesn't parse.
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

/// Write JSON as core.write_json does: dumps, a newline, into a temporary file that
/// then replaces the old one.
pub fn write(path: &Path, v: &Value) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    // mkstemp's mode, as Python's file has it
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    f.write_all((dumps(v) + "\n").as_bytes())?;
    drop(f);
    std::fs::rename(&tmp, path)
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

// ---------- reading values as Python does ----------

/// Python's truth: null, false, 0, "", [] and {} are false.
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

/// `o.get(key)`, None for a missing key or null.
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
    o.get(key).and_then(Value::as_object).unwrap_or_else(|| EMPTY.get_or_init(Obj::new))
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
            let items: Vec<String> = o.iter().map(|(k, v)| format!("{}: {}", str_repr(k), py_repr(v))).collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// Python's repr() of a str: in single quotes unless it has one and no double quote.
fn str_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
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
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_as_python_writes_them() {
        for (f, want) in [(1.0, "1.0"), (0.5, "0.5"), (1e16, "1e+16"), (1.5e-5, "1.5e-05"), (0.0001, "0.0001"),
                          (123456.789, "123456.789"), (1731400000.25, "1731400000.25"), (-2.5e20, "-2.5e+20"),
                          (1e15, "1000000000000000.0")] {
            assert_eq!(float_repr(f), want);
        }
    }

    #[test]
    fn dumps_as_python() {
        let v: Value = serde_json::from_str(r#"{"b": [], "a": {"x": "é\u001b\u007f"}, "n": null, "l": [1, true]}"#).unwrap();
        assert_eq!(dumps(&v), "{\n  \"b\": [],\n  \"a\": {\n    \"x\": \"\\u00e9\\u001b\\u007f\"\n  },\n  \"n\": null,\n  \"l\": [\n    1,\n    true\n  ]\n}");
    }
}
