use super::*;

#[test]
fn py_format_fills_named_fields_as_str_format_does() {
    let f = [("id", "BP-3".to_string()), ("bin", "/b".to_string())];
    assert_eq!(
        py_format("{id} at {bin} {{id}} }} {other}", &f),
        "BP-3 at /b {id} } {other}"
    );
    assert_eq!(py_format("no fields", &f), "no fields");
    assert_eq!(py_format("ends {", &f), "ends {");
}

#[test]
fn tell_says_coordinator_only_from_a_coordinators_pane() {
    assert_eq!(told("Rebase onto main.", true), "Coordinator: Rebase onto main.");
    assert_eq!(told("Rebase onto main.", false), "Rebase onto main.");
    assert_eq!(told("Coordinator: once.", true), "Coordinator: once.");
}
