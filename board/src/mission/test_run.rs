use super::*;

#[test]
fn the_command_line_and_what_a_reload_starts_with() {
    let a = parse_args(["--state", "/s", "--bullpen", "/b"].map(String::from)).unwrap();
    assert_eq!((a.state.to_str(), a.bullpen.to_str(), a.select.clone()), (Some("/s"), Some("/b"), None));
    assert!(parse_args(["--state", "/s"].map(String::from)).is_err());
    assert!(parse_args(["--what", "x"].map(String::from)).is_err());
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut m = Mission::new("/s".into(), "/b".into(), None, tx);
    m.sel = Some(("/s/repos/shop".into(), "BP-3".into()));
    let again = parse_args(restart_args(&m).into_iter().skip(1)).unwrap();
    assert_eq!(again.select, Some(("/s/repos/shop".into(), "BP-3".to_string())));
    assert!(again.reloaded);
}

#[test]
fn which_coordinators_are_alive_from_bullpen_repos_json() {
    let json = r#"{"repos": [
        {"dir": "/s/repos/shop", "coordinators": [{"id": "c1", "alive": true}, {"id": "c2", "alive": false}]},
        {"dir": "/s/repos/web", "coordinators": [{"id": "c1", "alive": null}]}
    ]}"#;
    assert_eq!(
        alive_from(json),
        Some(vec![("/s/repos/shop".into(), "c1".into(), true), ("/s/repos/shop".into(), "c2".into(), false)])
    );
    assert_eq!(alive_from("bullpen: no"), None);
}
