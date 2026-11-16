use super::*;

fn said(rows: &[(&str, &str)]) -> Vec<(String, String, String)> {
    rows.iter()
        .map(|(w, n)| (w.to_string(), n.to_string(), "blk".to_string()))
        .collect()
}

#[test]
fn a_first_up_names_what_started_and_says_what_to_do() {
    let s = said(&[("started", "coordinator"), ("started", "board"), ("started", "watcher")]);
    assert_eq!(
        up_line(&s, true),
        "Coordinator, board and watcher started. Tell the coordinator what you want."
    );
    assert_eq!(up_line(&s, false), "Coordinator, board and watcher started.");
}

#[test]
fn an_up_again_says_what_was_running_and_what_started() {
    let s = said(&[
        ("already running", "coordinators c1 and c2"),
        ("started", "board"),
        ("restarted (watch.lua or bullpen changed)", "watcher"),
    ]);
    assert_eq!(
        up_line(&s, true),
        "Coordinators c1 and c2 already running. Board started. Watcher restarted (watch.lua or bullpen changed)."
    );
}

#[test]
fn the_first_message_has_the_rules_read_before_ls() {
    assert_eq!(
        first_message("/r", "/b/bullpen", false),
        "You are the bullpen coordinator for /r. The bullpen CLI is /b/bullpen. Run `/b/bullpen rules` and \
         `/b/bullpen ls`, then say in one short line that you are ready."
    );
    let m = first_message("/r", "bp", true);
    assert!(m.contains("bp. News comes by monitor: start it first, as your guide says. Run `bp rules` and `bp ls`,"));
}
