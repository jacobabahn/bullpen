use super::*;
use serde_json::json;
use std::path::Path;

fn o(v: serde_json::Value) -> Obj {
    v.as_object().unwrap().clone()
}

#[test]
fn an_added_section_is_dated_and_ends_the_brief() {
    let c = added("Make it work.\n", "  Also cover logout.\n", "2026-11-16 09:30");
    assert_eq!(c.what, "## Added 2026-11-16 09:30\n\nAlso cover logout.");
    assert_eq!(
        c.brief,
        "Make it work.\n\n## Added 2026-11-16 09:30\n\nAlso cover logout.\n"
    );
    assert_eq!(c.how, "added");
}

#[test]
fn a_replaced_brief_keeps_its_diff_and_the_same_text_is_no_change() {
    assert_eq!(replaced("a\nb\n", "a\nb\n\n\n"), None);
    let c = replaced("a\nb\nc\n", "a\nB\nc").unwrap();
    assert_eq!(c.brief, "a\nB\nc\n");
    assert_eq!(
        c.what,
        "--- brief (before)\n+++ brief (now)\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c"
    );
}

#[test]
fn a_short_change_is_typed_and_a_long_one_is_pointed_at() {
    let (full, kept) = (Path::new("/s/BP-1.full.md"), Path::new("/s/BP-1.brief-1.md"));
    let short = added("x\n", "Also y.", "2026-11-16 09:30");
    let msg = message(&short, full, kept);
    assert!(msg.starts_with("Your brief changed: follow it as it is now. This was added to it"));
    assert!(msg.ends_with("\n\n## Added 2026-11-16 09:30\n\nAlso y."));
    assert!(msg.contains("/s/BP-1.full.md") && !msg.contains("brief-1"));
    let long = added("x\n", &"line\n".repeat(60), "2026-11-16 09:30");
    let msg = message(&long, full, kept);
    assert!(!msg.contains('\n'));
    assert!(msg.contains("What changed is in /s/BP-1.brief-1.md"));
}

#[test]
fn a_follow_up_round_hears_of_changes_the_last_round_didnt_see() {
    let t = o(json!({"brief_changes": [{"n": 1, "file": "/s/a.md"}, {"n": 2, "file": "/s/b.md"}]}));
    assert_eq!(changed_since(&t, None), "");
    assert_eq!(changed_since(&t, Some(&o(json!({"round": 1, "brief": 2})))), "");
    let said = changed_since(&t, Some(&o(json!({"round": 1, "brief": 1}))));
    assert!(said.starts_with("\n\nThe brief changed since round 1 looked (what changed: `/s/b.md`)."));
    let said = changed_since(&t, Some(&o(json!({"round": 2})))); // a round from before bullpen counted
    assert!(said.contains("`/s/a.md`, `/s/b.md`"));
    assert_eq!(changed_since(&o(json!({})), Some(&o(json!({"round": 1})))), "");
}
