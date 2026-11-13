use super::*;
use crate::tours::diff_of;
use crate::tours::tests::DIFF;

#[test]
fn the_schema_is_the_pythons() {
    let v: Value = serde_json::from_str(TOUR_SCHEMA).unwrap();
    assert_eq!(
        v["properties"]["steps"]["items"]["required"],
        json!(["title", "note", "hunks"])
    );
    assert_eq!(dumps_line(&v).replace(", ", ",").replace(": ", ":"), TOUR_SCHEMA);
}

#[test]
fn ref_lines_reads_each_way_of_naming_lines() {
    assert_eq!(ref_lines(None), Ok(None));
    assert_eq!(ref_lines(Some(&Value::Null)), Ok(None));
    assert_eq!(ref_lines(Some(&json!([3, 1]))), Ok(Some((1, 3))));
    assert_eq!(ref_lines(Some(&json!([5]))), Ok(Some((5, 5))));
    assert_eq!(ref_lines(Some(&json!(7))), Ok(Some((7, 7))));
    assert_eq!(ref_lines(Some(&json!("2-9"))), Ok(Some((2, 9))));
    assert_eq!(ref_lines(Some(&json!("4"))), Ok(Some((4, 4))));
}

#[test]
fn ref_lines_says_what_it_cant_read_as_python_does() {
    let err = |v: Value| ref_lines(Some(&v)).unwrap_err();
    assert_eq!(err(json!("x-2")), "invalid literal for int() with base 10: 'x'");
    assert_eq!(err(json!("-3")), "invalid literal for int() with base 10: ''");
    assert_eq!(err(json!([1, 2, 3])), "lines must be [start, end], not [1, 2, 3]");
    assert_eq!(err(json!(1.5)), "lines must be [start, end], not 1.5");
    assert_eq!(err(json!(true)), "lines must be [start, end], not true");
    assert_eq!(err(json!([1.0, 2])), "lines must be [start, end], not [1.0, 2]");
    assert_eq!(err(json!({"a": 1})), "lines must be [start, end], not {\"a\": 1}");
}

#[test]
fn json_objects_finds_the_outermost_ones() {
    let found = json_objects(r#"Here: {"a": 1} and {"b": {"c": 2}} then {bad and {"d": [1]}"#);
    assert_eq!(found, [json!({"a": 1}), json!({"b": {"c": 2}}), json!({"d": [1]})]);
    assert!(json_objects("none").is_empty());
}

#[test]
fn tour_steps_keeps_what_matches_and_adds_other_changes() {
    let files = diff_of(DIFF);
    let tour = json!({"steps": [
        {"title": "  First   one ", "note": " n ", "hunks": [{"file": "a.txt", "lines": [2, 2]},
                                                             {"file": "nope.txt"}, {"file": "old.txt", "lines": "1-2"}]},
        {"title": "Pic", "hunks": [{"file": "img.png"}]}]});
    let (steps, dropped) = tour_steps(&tour, &files).unwrap();
    assert_eq!(
        Value::Array(steps),
        json!([
            {"title": "First one", "note": "n", "hunks": [{"file": "a.txt", "lines": [2, 2]},
                                                          {"file": "old.txt", "lines": [1, 2]}]},
            {"title": "Pic", "note": "", "hunks": [{"file": "img.png"}]},
            {"title": "Other changes", "note": "The changes no step above covers.",
             "hunks": [{"file": "a.txt", "lines": [20, 21]}, {"file": "x y.txt", "lines": [6, 6]}]}])
    );
    assert_eq!(dropped, ["step 1: nope.txt"]);
}

#[test]
fn tour_steps_says_why_a_tour_isnt_the_format() {
    let files = diff_of(DIFF);
    let err = |v: Value| tour_steps(&v, &files).unwrap_err();
    assert_eq!(err(json!({})), "it has no list of steps");
    assert_eq!(err(json!({"steps": []})), "it has no list of steps");
    assert_eq!(err(json!({"steps": [{"title": " "}]})), "step 1 has no title");
    assert_eq!(
        err(json!({"steps": [{"title": "a", "note": null}]})),
        "step 1's note must be text and its hunks a list"
    );
    assert_eq!(
        err(json!({"steps": [{"title": "a", "hunks": [{"lines": [1, 2]}]}]})),
        "step 1 has a hunk with no file"
    );
    assert_eq!(
        err(json!({"steps": [{"title": "a", "hunks": [{"file": "a.txt", "lines": "q"}]}]})),
        "step 1: invalid literal for int() with base 10: 'q'"
    );
}

#[test]
fn parse_tour_takes_the_last_tour_in_text() {
    let files = diff_of(DIFF);
    let text = "Sure!\n{\"steps\": [{\"title\": \"x\"}]}\nOops, fixed:\n{\"steps\": [{\"title\": \"y\", \"hunks\": [{\"file\": \"a.txt\"}]}]}";
    let (steps, _, note) = parse_tour(&Answer::Text(text.into()), &files).unwrap();
    assert_eq!(steps[0]["title"], "y");
    assert_eq!(
        note.as_deref(),
        Some("its answer had text around the tour (2 JSON objects): the last one that is a tour was used")
    );
    let err = |t: &str| parse_tour(&Answer::Text(t.into()), &files).unwrap_err();
    assert_eq!(err("no tour"), "its tour doesn't parse (no JSON object in its output)");
    assert_eq!(
        err(r#"{"x": 1} then {"steps": 3}"#),
        "its tour isn't in the format: it has no list of steps"
    );
    assert_eq!(
        err(r#"{"steps": 3}"#),
        "its tour isn't in the format: it has no list of steps"
    );
}

#[test]
fn tour_answer_reads_claudes_result() {
    let structured = r#"{"type":"result","result":"hi","structured_output":{"steps":[]}}"#;
    assert_eq!(tour_answer(structured), Ok(Answer::Data(json!({"steps": []}))));
    assert_eq!(
        tour_answer(r#"{"type":"result","result":"text"}"#),
        Ok(Answer::Text("text".into()))
    );
    assert_eq!(tour_answer("plain"), Ok(Answer::Text("plain".into())));
    assert_eq!(
        tour_answer(r#"{"type":"result","is_error":true,"subtype":"error_max_turns"}"#),
        Err("the agent failed: error_max_turns".into())
    );
}

#[test]
fn review_number_reads_a_findings_files_round() {
    assert_eq!(review_number("BP-3.review-12.findings.md"), Some(12));
    assert_eq!(review_number("BP-3.review-x.findings.md"), None);
}
