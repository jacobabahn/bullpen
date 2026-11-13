use super::*;
use bullpen_core::timeutil::parse;
use chrono::{Timelike, Utc};
use serde_json::json;

/// dt.datetime(2026, 11, d, h, m).astimezone(): local time.
fn day(d: u32, h: u32, m: u32) -> Stamp {
    Local
        .with_ymd_and_hms(2026, 11, d, h, m, 0)
        .single()
        .expect("a local time")
        .fixed_offset()
}

#[test]
fn limit_phrase_says_when_it_resets() {
    let at = parse("2026-05-04T10:00:00+00:00");
    assert_eq!(limit_phrase(None, at), "reset time unknown");
    assert_eq!(limit_phrase(Some(&json!(null)), at), "reset time unknown");
    assert!(limit_phrase(Some(&json!("2026-05-04T15:00:00+00:00")), at).starts_with("resets "));
}

#[test]
fn test_only_the_agent_a_task_waits_on_is_stuck_or_limited() {
    // (is_stuck and stuck_phrase: core/src/test_words.rs, under this name)
    let at = day(11, 13, 0);
    let lim = json!({"who": "worker", "resets": iso_seconds(&day(11, 15, 0)), "text": "x"});
    let lim = lim.as_object().unwrap().clone();
    let task = |status: &str| json!({"status": status, "limit": lim}).as_object().unwrap().clone();
    assert!(is_limited(&task("working")));
    assert!(!is_limited(&task("ready")));
    assert!(limited(&task("working"), "worker"));
    assert!(!limited(&task("working"), "reviewer"));
    assert_eq!(limit_why(&lim, Some(at)), "Usage limit, resets 3:00 PM");
}

#[test]
fn test_reset_times_are_read() {
    let at = day(11, 13, 0); // a Wednesday, 1 PM
    let when = |text: &str| limit_reset(text, Some(at));
    assert_eq!(when("You've hit your 5-hour limit · resets 3pm"), Some(day(11, 15, 0)));
    assert_eq!(
        when("You've hit your session limit · resets 3:45pm"),
        Some(day(11, 15, 45))
    );
    assert_eq!(when("Resets at 3:10 PM"), Some(day(11, 15, 10)));
    assert_eq!(when("resets 15:00"), Some(day(11, 15, 0)));
    assert_eq!(when("You've hit your Opus limit · resets 9am"), Some(day(12, 9, 0))); // tomorrow
    assert_eq!(when("resets 12:58pm"), Some(day(11, 12, 58))); // just gone: now, not tomorrow
    assert_eq!(
        when("You've hit your weekly limit · resets Mon 12:00am"),
        Some(day(16, 0, 0))
    );
    assert_eq!(
        when("weekly limit · resets Nov 14, 3pm · progress saved"),
        Some(day(14, 15, 0))
    );
    let unix = Local.timestamp_opt(1794430800, 0).unwrap().fixed_offset();
    assert_eq!(when("Claude AI usage limit reached|1794430800"), Some(unix));
    let ny = when("Claude usage limit reached. Your limit will reset at 3pm (America/New_York).").unwrap();
    // 3 PM in New York, told in local time
    assert_eq!((ny.offset(), ny.with_timezone(&Utc).hour()), (at.offset(), 20));
    for text in [
        "Usage limit reached",
        "spend limit reached · resets Aug 8",
        "resets in 8 minutes",
        "resets 3",
        "resets 13pm",
        "resets 3pm (Nowhere/Land)",
    ] {
        assert_eq!(when(text), None, "{text}");
    }
    assert_eq!(clock(day(11, 15, 0), Some(at)), "3:00 PM");
    assert_eq!(clock(day(16, 0, 0), Some(at)), "Mon 12:00 AM");
    assert_eq!(clock(day(30, 9, 5), Some(at)), "Nov 30 9:05 AM");
}

#[test]
fn reset_times_are_read_as_re_reads_them() {
    let at = day(11, 13, 0); // a Wednesday, 1 PM
    let when = |text: &str| limit_reset(text, Some(at));
    // a.m. and p.m., in any case, with or without the dots and the space
    assert_eq!(when("RESETS 3 P.M."), Some(day(11, 15, 0)));
    assert_eq!(when("resets 3p.m"), Some(day(11, 15, 0)));
    assert_eq!(when("resets 9 a.m. tomorrow"), Some(day(12, 9, 0)));
    assert_eq!(when("resets 12am"), Some(day(12, 0, 0)));
    assert_eq!(when("resets 12pm"), Some(day(12, 12, 0))); // an hour gone: tomorrow
                                                           // "pm" must end a word: "3pmx" is "3" with no am/pm, which is no time
    assert_eq!(when("resets 3pmx"), None);
    assert_eq!(when("resets 3:30pmx"), Some(day(12, 3, 30))); // 3:30 in the morning
    assert_eq!(when("resets 3 pm_"), None);
    // "resets at", "reset", weekdays in full and with a comma, months with a dot
    assert_eq!(when("Your limit will reset at 4:15 pm"), Some(day(11, 16, 15)));
    assert_eq!(when("resets Monday 9am"), Some(day(16, 9, 0)));
    assert_eq!(when("resets Wednesday, 9am"), Some(day(18, 9, 0))); // today, gone: next week
    assert_eq!(when("resets wed 1:30pm"), Some(day(11, 13, 30)));
    // at's clock is its fixed offset: 3 PM there, as Python's replace and timedelta do it
    assert_eq!(when("resets Sept. 3 at 3pm"), Some(on_at(2027, 9, 3, 15)));
    assert_eq!(when("resets November 30 3pm"), Some(day(30, 15, 0)));
    assert_eq!(when("resets nov 10, 3pm"), Some(day(10, 15, 0))); // a day back: still this year
    assert_eq!(when("resets Nov 9, 3pm"), Some(on_at(2027, 11, 9, 15)));
    assert_eq!(when("resets Feb 30, 3pm"), None);
    assert_eq!(when("resets Nov 3pm"), None); // a month needs its date
                                              // the first "reset" that reads wins; one that doesn't is passed over
    assert_eq!(when("reset soon, resets 4pm"), Some(day(11, 16, 0)));
    assert_eq!(when("resets 123pm"), None); // 12, then "3pm" isn't read
    assert_eq!(when("resets 3:4pm"), None);
    assert_eq!(when("resetsx 3pm"), None);
    // hours and minutes out of range
    for text in ["resets 0am", "resets 24:00", "resets 9:60", "resets 3:00 (Nowhere)"] {
        assert_eq!(when(text), None, "{text}");
    }
    // a zone in brackets that doesn't read is no zone: local time
    assert_eq!(when("resets 3pm (America/New York)"), Some(day(11, 15, 0)));
    assert_eq!(when("resets 3pm (leapseconds)"), None); // a file, but not a zone
    assert_eq!(when("resets 3pm (America)"), None); // a folder
                                                    // Unicode digits are \d to re, and int() reads them
    assert_eq!(when("resets ٣pm"), Some(day(11, 15, 0)));
    // the Unix time: ten digits exactly, then a word's end, and case counts
    assert_eq!(when("limit reached|17944308001"), None);
    assert_eq!(when("Limit Reached|1794430800"), None);
}

/// h o'clock on another day at the clock of day(11, 13, 0)'s offset, told in local time.
fn on_at(y: i32, mo: u32, d: u32, h: u32) -> Stamp {
    let at = *day(11, 13, 0).offset();
    at.with_ymd_and_hms(y, mo, d, h, 0, 0)
        .unwrap()
        .with_timezone(&Local)
        .fixed_offset()
}

#[test]
fn a_named_zone_keeps_its_own_clock() {
    // New York's midnight, EST; by 3 PM it is EDT
    let at = "2026-03-08T05:00:00+00:00";
    assert_eq!(utc_at("resets 3pm (America/New_York)", at), "2026-03-08T19:00:00+00:00");
    // 2:30 AM isn't on New York's clock that day: zoneinfo reads it by EST
    assert_eq!(
        utc_at("resets 2:30am (America/New_York)", at),
        "2026-03-08T07:30:00+00:00"
    );
    // midnight, EDT: 1:30 AM comes twice, and it is the first
    let at = "2026-11-01T04:00:00+00:00";
    assert_eq!(
        utc_at("resets 1:30am (America/New_York)", at),
        "2026-11-01T05:30:00+00:00"
    );
    assert_eq!(utc_at("resets 3pm (UTC)", at), "2026-11-01T15:00:00+00:00");
    // 1:10 AM the second time (EST): replace keeps that, so 1:45 is the second too
    let at = "2026-11-01T06:10:00+00:00";
    assert_eq!(
        utc_at("resets 1:45am (America/New_York)", at),
        "2026-11-01T06:45:00+00:00"
    );
    // and on a day with a gap, the second is read by the offset after it
    assert_eq!(
        utc_at("resets Mar 14, 2:30am (America/New_York)", at),
        "2027-03-14T06:30:00+00:00"
    );
}

/// limit_reset at a stamp, told in UTC ("" for None).
fn utc_at(text: &str, at: &str) -> String {
    let when = limit_reset(text, parse(at));
    when.map_or(String::new(), |w| w.with_timezone(&Utc).to_rfc3339())
}

#[test]
fn limit_messages_are_found_as_re_finds_them() {
    for text in [
        "You've hit your 5-hour limit · resets 3pm",
        "you’ve hit your weekly limit",
        "YOU'VE HIT YOUR LIMIT",
        "You've hit your Opus 4 limit",
        "You've hit your Claude's-weekly limit",
        "Claude usage limit reached",
        "Your limit will reset at 3pm",
        "You're out of usage credits",
        "You've hit your session limits", // "limit" needn't end a word
        "You've hit your limit.",
        "you\u{2019}ve hit your l\u{130}mit", // (?i): İ and ı are i to re, ſ is s
        "you've hit your L\u{131}MIT",
        "u\u{17f}age limit reached",
    ] {
        assert!(says_limit(text), "{text}");
    }
    for text in [
        "You have hit your limit",
        "You`ve hit your limit",
        "You've hit your 5-hour · limit",
        "Usage  limit reached",
        "spend limit reached",
        "",
    ] {
        assert!(!says_limit(text), "{text}");
    }
}

#[test]
fn digits_are_unicode_decimals() {
    assert_eq!(digit('7'), Some(7));
    assert_eq!(digit('٣'), Some(3));
    assert_eq!(digit('\u{ff19}'), Some(9)); // fullwidth 9
    assert_eq!(digit('²'), None);
    assert_eq!(digit('a'), None);
    assert_eq!(digit('/'), None);
}

#[test]
fn a_turn_that_finished_says_its_limit_in_its_msg_only() {
    let msg = "You've hit your 5-hour limit · resets 3pm";
    assert_eq!(limit_text("", "b1", "done", msg, None).unwrap().as_deref(), Some(msg));
    assert_eq!(limit_text("", "b1", "done", "all done", None).unwrap(), None);
    assert_eq!(limit_text("", "b1", "blocked", msg, None).unwrap(), None);
    assert_eq!(limit_text("", "b1", "working", msg, None).unwrap(), None);
}
