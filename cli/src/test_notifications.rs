use super::*;

fn t(status: &str, more: Value) -> Obj {
    let mut o = json!({"id": "BP-3", "title": "Fix it", "status": status})
        .as_object()
        .unwrap()
        .clone();
    o.extend(more.as_object().unwrap().clone());
    o
}

fn n(new: &Obj, old: &Obj) -> Option<(Kind, String)> {
    notice(new, old)
}

#[test]
fn what_a_status_change_tells_the_person() {
    use Kind::*;
    let working = t("working", json!({}));
    let needs = |why: &str| t("needs", json!({"why": why}));
    let perm = needs("Needs you (permission): Allow rm?");
    assert_eq!(
        n(&perm, &working),
        Some((Needs, "BP-3 needs you (permission): Allow rm?".into()))
    );
    let rev = needs("Reviewer needs you (input)");
    assert_eq!(
        n(&rev, &t("review", json!({}))),
        Some((Needs, "BP-3's reviewer needs you (input)".into()))
    );
    let late = needs("Failed: boom (coordinator didn't resolve it)");
    let want = "BP-3 needs you: Failed: boom (coordinator didn't resolve it)";
    assert_eq!(n(&late, &t("triage", json!({}))), Some((Needs, want.into())));
    assert_eq!(n(&perm, &perm), None); // no change
    let asked = t("needs", json!({"why": "Use SQLite?", "ask": true}));
    assert_eq!(n(&asked, &perm), Some((Needs, "BP-3 needs you: Use SQLite?".into()))); // asked at a prompt
    assert_eq!(n(&asked, &asked), None);
    let again = t("needs", json!({"why": "Or Postgres?", "ask": true}));
    assert_eq!(n(&again, &asked), Some((Needs, "BP-3 needs you: Or Postgres?".into())));
    let ended = t("triage", json!({"why": "Ended its turn without saying ready"}));
    assert_eq!(n(&ended, &working), None);
    let ready = t("ready", json!({}));
    assert_eq!(
        n(&ready, &t("review", json!({}))),
        Some((News, "BP-3 is ready: Fix it".into()))
    );
    assert_eq!(n(&ready, &ready), None);
    assert_eq!(
        n(&t("finished", json!({})), &working),
        Some((News, "BP-3 finished: Fix it".into()))
    );
    let auto = t("done", json!({"auto_accepted": true}));
    assert_eq!(n(&auto, &ready), Some((News, "BP-3 was auto-accepted: Fix it".into())));
    assert_eq!(n(&t("done", json!({"auto_accepted": false})), &ready), None); // the person accepted it
    assert_eq!(n(&working, &Obj::new()), None);
}

#[test]
fn needs_you_as_the_pattern_reads_it() {
    assert_eq!(needs_you("Needs you: Allow?"), Some((false, 11)));
    assert_eq!(needs_you("needs you"), Some((false, 9)));
    assert_eq!(needs_you("Reviewer needs you (input)"), Some((true, 19)));
    assert_eq!(needs_you("Needs yourself"), None); // \b
    assert_eq!(needs_you("Reviewer exited"), None);
    assert_eq!(
        n(&t("needs", json!({})), &Obj::new()),
        Some((Kind::Needs, "BP-3 needs you: it is stopped".into()))
    );
    let bare = t("needs", json!({"why": "Needs you"}));
    assert_eq!(n(&bare, &Obj::new()), Some((Kind::Needs, "BP-3 needs you".into())));
}

#[test]
fn the_osc_has_no_control_characters_and_a_title_without_semicolons() {
    assert_eq!(
        osc("bullpen · a;b", "x\ny;z\x07"),
        "\x1b]777;notify;bullpen · a b;x y;z\x07"
    );
    let long = "w ".repeat(200);
    let sent = osc("t", &long);
    assert!(sent.ends_with("…\x07"));
    assert_eq!(sent.chars().count(), "\x1b]777;notify;t;".chars().count() + 240 + 1);
}
