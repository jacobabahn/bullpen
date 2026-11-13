use super::*;

fn at(text: &str) -> Stamp {
    DateTime::parse_from_rfc3339(text).unwrap()
}

#[test]
fn stamps_as_isoformat_writes_them() {
    let whole = at("2026-11-12T21:42:21-06:00");
    let part = at("2026-11-12T21:42:21.5+05:30");
    assert_eq!(iso_seconds(&whole), "2026-11-12T21:42:21-06:00");
    assert_eq!(iso_seconds(&part), "2026-11-12T21:42:21+05:30");
    assert_eq!(iso_micro(&whole), "2026-11-12T21:42:21.000000-06:00");
    assert_eq!(iso_micro(&part), "2026-11-12T21:42:21.500000+05:30");
    assert_eq!(isoformat(&whole), "2026-11-12T21:42:21-06:00"); // no .000000
    assert_eq!(isoformat(&part), "2026-11-12T21:42:21.500000+05:30");
    assert_eq!(iso_seconds(&at("2026-01-02T03:04:05Z")), "2026-01-02T03:04:05+00:00");
}

#[test]
fn now_has_the_shape_core_now_has() {
    let s = iso_now();
    assert_eq!(s.len(), "2026-11-12T21:42:21-06:00".len(), "{s}");
    let m = iso_now_micro();
    assert_eq!(m.len(), "2026-11-12T21:42:21.000000-06:00".len(), "{m}");
    assert!(parse(&s).is_some() && parse(&m).is_some());
}

#[test]
fn ages_count_from_the_stamp() {
    let later = at("2026-11-12T21:43:21.25-06:00");
    assert_eq!(seconds_since("2026-11-12T21:42:21-06:00", Some(later)), 60.25);
    assert_eq!(seconds_since("2026-11-13T03:42:21+00:00", Some(later)), 60.25);
    assert_eq!(seconds_since("not a stamp", Some(later)), 0.0);
}
