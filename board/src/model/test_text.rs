use super::*;
use crate::model::ago;
use chrono::DateTime;

#[test]
fn fit_and_age() {
    assert_eq!(fit("Add a done command", 10), "Add a don…");
    assert_eq!(fit("short", 10), "short");
    let at = DateTime::parse_from_rfc3339("2026-01-01T12:00:00+00:00").unwrap();
    assert_eq!(ago(Some("2026-01-01T11:55:00+00:00"), at), "5m");
    assert_eq!(ago(Some("2026-01-01T09:55:00+00:00"), at), "2h");
    assert_eq!(ago(Some("2026-01-01T11:59:30.123456+00:00"), at), "now");
    assert_eq!(ago(None, at), "");
}

#[test]
fn wrap_cuts_to_the_lines_given() {
    assert_eq!(wrap("one two three", 7, 2), vec!["one two", "three"]);
    assert_eq!(wrap("one two three four", 7, 1), vec!["one tw…"]);
    assert_eq!(wrap("", 7, 2), vec![""]);
    assert_eq!(wrap("abcdefghij", 4, 3), vec!["abcd", "efgh", "ij"]);
}
