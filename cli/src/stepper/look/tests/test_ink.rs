use super::*;
use bullpen_draw::font::Fonts;

fn texts(ws: &[Word]) -> Vec<(&str, bool, bool)> {
    ws.iter().map(|w| (w.text.as_str(), w.code, w.glued)).collect()
}

#[test]
fn code_spans_are_words_and_punctuation_after_them_is_glued() {
    let ws = words("`Score` gains `worth` (`yes`, `no`), then more");
    assert_eq!(
        texts(&ws),
        vec![
            ("Score", true, false),
            ("gains", false, false),
            ("worth", true, false),
            ("(", false, false),
            ("yes", true, true),
            (",", false, true),
            ("no", true, false),
            ("),", false, true),
            ("then", false, false),
            ("more", false, false),
        ]
    );
}

#[test]
fn a_long_code_span_breaks_at_its_spaces_and_a_stray_backtick_is_text() {
    let ws = words("run `bullpen tour BP-3 --view --step 2 --json` now");
    assert!(ws.iter().filter(|w| w.code).count() > 1);
    assert_eq!(texts(&words("a ` b")), vec![("a", false, false), ("b", false, false)]);
}

#[test]
fn prose_wraps_to_its_room_and_glued_words_stay_on_their_line() {
    let mut ty = Type::new(Fonts::builtin());
    let mut ink = Ink::new(400, 10, &mut ty, Theme::default(), 1.0).unwrap();
    let lines = ink.lines(
        "one two three four five six seven eight nine ten `x`, eleven",
        false,
        24.0,
        200.0,
    );
    assert!(lines.len() > 2);
    for line in &lines {
        assert!(!line[0].glued, "a line never starts with a glued word");
    }
    let h = ink.prose_height("one\ntwo", false, 24.0, 200.0, 38.0);
    assert_eq!(h, 76.0);
}

#[test]
fn a_row_of_keys_is_as_wide_as_its_caps_and_words() {
    let mut ty = Type::new(Fonts::builtin());
    let one = keys_width(&mut ty, &[("n", "next")], 20.0);
    let two = keys_width(&mut ty, &[("n", "next"), ("p", "back")], 20.0);
    assert!(one > 40.0 && two > 2.0 * one);
}
