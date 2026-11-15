use super::*;
use serde_json::json;

#[test]
fn dumps_writes_json_as_python_does() {
    let v = json!({"argv": ["pr", "close"], "body": "", "n": 7, "x": null, "ok": true, "e": {}, "l": []});
    assert_eq!(
        dumps(&v),
        r#"{"argv": ["pr", "close"], "body": "", "n": 7, "x": null, "ok": true, "e": {}, "l": []}"#
    );
}

#[test]
fn dumps_keeps_only_ascii() {
    let v = json!("a\"b\\c\nd\re\tf\u{8}\u{c}\u{1}\u{7f}\u{e9}\u{276f}\u{1f600}");
    assert_eq!(dumps(&v), r#""a\"b\\c\nd\re\tf\b\f\u0001\u007f\u00e9\u276f\ud83d\ude00""#);
}

#[test]
fn read_text_reads_line_ends_as_newlines() {
    let d = std::env::temp_dir().join(format!("bullpen-fakes-{}", std::process::id()));
    std::fs::write(&d, "a\r\nb\rc\n").unwrap();
    assert_eq!(read_text(&d).unwrap(), "a\nb\nc\n");
    std::fs::remove_file(&d).unwrap();
}

#[test]
fn truthy_is_python_truth() {
    for v in [json!(null), json!(false), json!(0), json!(""), json!([]), json!({})] {
        assert!(!truthy(&v), "{v}");
    }
    for v in [json!(true), json!(1), json!("w1"), json!([0]), json!({"a": 1})] {
        assert!(truthy(&v), "{v}");
    }
}
