use super::*;

#[test]
fn rex_is_named_rex() {
    assert_eq!(REX.name(), "rex");
}

#[test]
fn an_agent_with_no_block_isnt_alive_and_rex_isnt_asked() {
    // with no block there is nothing to ask Rex: BULLPEN_REX is never run
    assert!(!Agent::on_rex("s1", "").alive().unwrap());
}

#[test]
fn rex_keeps_a_panes_label_for_its_title() {
    // a no-op: the label is how a pane is found again after a Rex restart
    assert!(Agent::on_rex("s1", "blk1").set_title("✔ BP-12 low · title").is_ok());
}
