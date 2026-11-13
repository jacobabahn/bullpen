use super::*;
use serde_json::json;

/// A JSON file with every kind of value, and what Python's json.dumps writes for it
/// (indent=2, then on one line), as JSON strings: python3 printed them.
const INPUT: &str = r#"{"b": [], "a": {"x": "é\u001b\u007f\\ \"q\" \t\b\f\r\n😀/"}, "e": {}, "n": null, "l": [1, true, 2.5, [], {"k": [false]}], "f": 1e16, "g": 1.0, "h": -0.0}"#;
const INDENTED: &str = r#""{\n  \"b\": [],\n  \"a\": {\n    \"x\": \"\\u00e9\\u001b\\u007f\\\\ \\\"q\\\" \\t\\b\\f\\r\\n\\ud83d\\ude00/\"\n  },\n  \"e\": {},\n  \"n\": null,\n  \"l\": [\n    1,\n    true,\n    2.5,\n    [],\n    {\n      \"k\": [\n        false\n      ]\n    }\n  ],\n  \"f\": 1e+16,\n  \"g\": 1.0,\n  \"h\": -0.0\n}""#;
const ONE_LINE: &str = r#""{\"b\": [], \"a\": {\"x\": \"\\u00e9\\u001b\\u007f\\\\ \\\"q\\\" \\t\\b\\f\\r\\n\\ud83d\\ude00/\"}, \"e\": {}, \"n\": null, \"l\": [1, true, 2.5, [], {\"k\": [false]}], \"f\": 1e+16, \"g\": 1.0, \"h\": -0.0}""#;

fn python(text: &str) -> String {
    serde_json::from_str(text).unwrap()
}

#[test]
fn dumps_writes_what_python_writes() {
    let v: Value = serde_json::from_str(INPUT).unwrap();
    assert_eq!(dumps(&v), python(INDENTED));
    assert_eq!(dumps_obj(v.as_object().unwrap()), python(INDENTED));
    assert_eq!(dumps_line(&v), python(ONE_LINE));
}

#[test]
fn keys_keep_their_order() {
    let v: Value = serde_json::from_str(r#"{"z": 1, "a": 2, "m": 3}"#).unwrap();
    assert_eq!(dumps_line(&v), r#"{"z": 1, "a": 2, "m": 3}"#);
    let mut o = v.as_object().unwrap().clone();
    o.insert("a".into(), json!(9)); // a key set again keeps its place
    o.shift_remove("z");
    o.insert("b".into(), json!(null));
    assert_eq!(dumps_line(&Value::Object(o)), r#"{"a": 9, "m": 3, "b": null}"#);
}

#[test]
fn empty_containers_and_scalars() {
    assert_eq!(dumps(&json!([])), "[]");
    assert_eq!(dumps(&json!({})), "{}");
    assert_eq!(dumps(&json!(null)), "null");
    assert_eq!(dumps(&json!("x")), "\"x\"");
    assert_eq!(dumps(&json!({"a": [[]]})), "{\n  \"a\": [\n    []\n  ]\n}");
}

#[test]
fn floats_as_python_writes_them() {
    for (f, want) in [
        (1.0, "1.0"),
        (0.5, "0.5"),
        (1e16, "1e+16"),
        (1.5e-5, "1.5e-05"),
        (0.0001, "0.0001"),
        (9.999e-5, "9.999e-05"),
        (123456.789, "123456.789"),
        (1731400000.25, "1731400000.25"),
        (-2.5e20, "-2.5e+20"),
        (1e15, "1000000000000000.0"),
        (0.1 + 0.2, "0.30000000000000004"),
        (5e-324, "5e-324"),
        (1.7976931348623157e308, "1.7976931348623157e+308"),
        (-0.0, "-0.0"),
    ] {
        assert_eq!(float_repr(f), want);
    }
}

#[test]
fn truth_as_python_sees_it() {
    for v in [json!(null), json!(false), json!(0), json!(0.0), json!(""), json!([]), json!({})] {
        assert!(!truthy(Some(&v)), "{v}");
    }
    for v in [json!(true), json!(1), json!(-0.5), json!("x"), json!([0]), json!({"a": null})] {
        assert!(truthy(Some(&v)), "{v}");
    }
    assert!(!truthy(None));
}

#[test]
fn getters() {
    let o = json!({"a": null, "b": "x", "c": {"d": 1}, "e": ["p", 2, "q"]});
    let o = o.as_object().unwrap();
    assert_eq!(get(o, "a"), None);
    assert_eq!(get(o, "b"), Some(&json!("x")));
    assert_eq!(s(o, "b"), "x");
    assert_eq!(s(o, "c"), "");
    assert_eq!(obj(o, "c").len(), 1);
    assert!(obj(o, "b").is_empty());
    assert_eq!(strs(o, "e"), vec!["p", "q"]);
}

#[test]
fn str_and_repr_as_python() {
    assert_eq!(py_str(&json!("it's")), "it's");
    assert_eq!(py_str(&json!(["claude", "--model", "it's"])), r#"['claude', '--model', "it's"]"#);
    assert_eq!(py_str(&json!({"a": [true, null, 1.0, 2]})), "{'a': [True, None, 1.0, 2]}");
    assert_eq!(py_repr(&json!("a\nb\\")), r"'a\nb\\'");
}

#[test]
fn files_are_written_whole_with_a_newline() {
    let dir = std::env::temp_dir().join(format!("bullpen-core-json-{}", std::process::id()));
    let path = dir.join("sub/t.json");
    write(&path, &json!({"id": "BP-1", "n": [1]})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n  \"id\": \"BP-1\",\n  \"n\": [\n    1\n  ]\n}\n");
    let left: Vec<_> = std::fs::read_dir(dir.join("sub")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left, vec!["t.json"]); // no .tmp-* left behind
    assert_eq!(read(&path).unwrap(), Some(json!({"id": "BP-1", "n": [1]})));
    assert_eq!(read(&dir.join("none.json")).unwrap(), None);
    std::fs::write(dir.join("bad.json"), "{").unwrap();
    assert!(read(&dir.join("bad.json")).is_err());
    let err = read_obj(&dir.join("bad.json")).unwrap_err().0;
    assert!(err.ends_with("doesn't parse (EOF while parsing an object at line 1 column 1): fix it, or move it away"), "{err}");
    std::fs::write(dir.join("list.json"), "[]").unwrap();
    assert!(read_obj(&dir.join("list.json")).unwrap_err().0.ends_with("isn't a JSON object ({...}): fix it, or move it away"));
    let _ = std::fs::remove_dir_all(&dir);
}
