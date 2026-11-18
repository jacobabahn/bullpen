use super::*;
use serde_json::json;

#[test]
fn a_value_is_listed_as_a_shell_takes_a_command_and_json_otherwise() {
    assert_eq!(
        config_value(&json!(["claude", "--permission-mode", "auto"])),
        "claude --permission-mode auto"
    );
    assert_eq!(config_value(&json!(["my agent", "it's"])), r#"'my agent' 'it'"'"'s'"#);
    assert_eq!(config_value(&json!("rebase")), "rebase");
    assert_eq!(config_value(&json!(null)), "null");
    assert_eq!(config_value(&json!(true)), "true");
    assert_eq!(config_value(&json!([1, "a"])), "[1, \"a\"]");
}

#[test]
fn settings_are_grouped_in_the_tables_order_and_the_rest_under_other() {
    let keys: Vec<String> = ["stuck_after", "merge", "extra", "review", "prefix"]
        .map(String::from)
        .to_vec();
    let groups = config_groups(&keys);
    let titles: Vec<&str> = groups.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(titles, ["Tasks", "Review", "News", "Other"]);
    assert_eq!(groups[0].1, ["prefix", "merge"]);
    assert_eq!(groups[3].1, ["extra"]);
}

#[test]
fn every_setting_with_a_flag_is_in_the_spec_once_with_its_default() {
    let sp = spec();
    for s in SETTINGS.iter() {
        let n = sp.iter().filter(|a| a.name == s.flag()).count();
        assert_eq!(n, usize::from(s.kind != SKind::File), "{}", s.name);
    }
    let help = crate::args::help("config", "", &sp, 200);
    assert!(help.contains("(default 1 while workers or reviewers float, else 0)"));
    assert!(help.contains("--prefix PREFIX") && help.contains("--worker-model MODEL"));
    assert!(help.contains("--merge {rebase,squash,merge}"));
}
