//! test_helpers.py's line-edit and split-keys cases, with their names.

use super::*;

fn shown(e: &LineEdit) -> String {
    let t: Vec<char> = e.text().chars().collect();
    format!(
        "{}|{}",
        t[..e.at].iter().collect::<String>(),
        t[e.at..].iter().collect::<String>()
    )
}

fn before(e: &LineEdit) -> String {
    e.text().chars().take(e.at).collect()
}

#[test]
fn test_line_edit_moves_and_cuts_at_the_cursor() {
    let mut e = LineEdit::default();
    for k in ["u", "s", "e", " ", "p", "g", "\x1b[D", "\x1b[D", "X"] {
        assert!(e.key(k), "{k:?}");
    }
    assert_eq!(shown(&e), "use X|pg");
    let steps: &[(&[&str], &str)] = &[
        (&["\x01"], "|use Xpg"),
        (&["\x1b[F"], "use Xpg|"),
        (&["\x1bb"], "use |Xpg"),
        (&["\x1b[1;5C"], "use Xpg|"),
        (&["\x17"], "use |"),
        (&["\x1b[H", "\x1b[3~"], "|se "),
        (&["\x06", "\x0b"], "s|"),
        (&["\x15"], "|"),
        (&["\x1b[C", "\x7f"], "|"),
    ];
    for (keys, want) in steps {
        for k in keys.iter() {
            e.key(k);
        }
        assert_eq!(shown(&e), *want, "{keys:?}");
    }
    for k in ["\r", "\x1b", "\x03", "\x1b[Z"] {
        assert!(!e.key(k), "{k:?}"); // for the caller
    }
    // no cap
    e.key(&format!("\x1b[200~{}\x1b[201~", "x".repeat(3000)));
    assert_eq!(e.text().chars().count(), 3000);
}

#[test]
fn test_line_edit_takes_a_paste_on_one_line_or_with_its_lines() {
    let mut e = LineEdit::new("[]", false);
    e.key("\x1b[D");
    e.key("\x1b[200~one\r\ntwo\nthree\tfour\x07\x1b[201~");
    assert_eq!((e.text().as_str(), e.at), ("[one two three four]", 19));
    let mut e = LineEdit::new("", true);
    e.key("\x1b[200~one\r\ntwo\x1b[201~");
    assert_eq!(e.text(), "one\ntwo");
    e.key("\n"); // ctrl+j: a new line
    e.key("x");
    e.key("\x1b[A"); // up: the same column, or the line's end
    assert_eq!(before(&e), "one\nt");
    e.key("\x1b[A");
    e.key("\x15"); // ctrl+u: to the start of this line only
    assert_eq!(e.text(), "ne\ntwo\nx");
    e.key("\x1b[B");
    e.key("\x1b[B");
    e.key("\x05");
    e.key("\x1b\r"); // alt+⏎ too
    assert_eq!(e.text(), "ne\ntwo\nx\n");
}

#[test]
fn test_line_edit_views_keep_the_cursor_in_sight() {
    let mut e = LineEdit::new("abcdefghij", false);
    assert_eq!(e.view(5), ("ghij".into(), 4));
    for _ in 0..3 {
        e.key("\x1b[D");
    }
    assert_eq!(e.view(5), ("ghij".into(), 1)); // the view stays until the cursor reaches its edge
    e.key("\x1b[D");
    e.key("\x1b[D");
    assert_eq!(e.view(5), ("fghij".into(), 0));
    e.key("\x01");
    assert_eq!(e.view(5), ("abcde".into(), 0));
    let mut e = LineEdit::new("abcdefgh\nij", true);
    let v = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(e.rows(5, 9), (v(&["abcd", "efgh", "ij"]), (2, 2)));
    assert_eq!(e.rows(5, 2), (v(&["efgh", "ij"]), (1, 2))); // the rows around the cursor
    e.key("\x1b[A");
    e.key("\x01");
    assert_eq!(e.rows(5, 2), (v(&["abcd", "efgh"]), (0, 0)));
}

#[test]
fn test_split_keys_keeps_a_paste_and_alt_keys_whole() {
    assert_eq!(
        split_keys("a\x1b[200~x\ny\x1b[201~\x1bb\x1b\r\x1b\x1b"),
        ["a", "\x1b[200~x\ny\x1b[201~", "\x1bb", "\x1b\r", "\x1b", "\x1b"]
    );
}

#[test]
fn test_split_keys_keeps_escape_sequences_whole() {
    assert_eq!(
        split_keys("ab\x1b[Bc\x1bOA\x1b"),
        ["a", "b", "\x1b[B", "c", "\x1bOA", "\x1b"]
    );
}

#[test]
fn a_character_split_across_reads_waits_for_its_rest() {
    let mut d = Decoder::default();
    let bytes = "é宽".as_bytes();
    assert_eq!(d.decode(&bytes[..1]), "");
    assert_eq!(d.decode(&bytes[1..3]), "é");
    assert_eq!(d.decode(&bytes[3..]), "宽");
    assert_eq!(d.decode(b"a\xffb"), "a\u{fffd}b");
}
