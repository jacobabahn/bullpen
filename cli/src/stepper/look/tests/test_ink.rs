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
    let sz = Sizes::of((16, 34), &mut ty);
    let mut ink = Ink::new(400, 10, &mut ty, Theme::default(), sz).unwrap();
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

#[test]
fn codes_advance_is_a_cell_and_prose_is_the_boards_medium_at_least() {
    let mut ty = Type::new(Fonts::builtin());
    for cell in [(19, 37), (8, 17), (16, 34)] {
        let sz = Sizes::of(cell, &mut ty);
        let advance = ty.width("0", Face::Mono, sz.code);
        assert!((advance - cell.0 as f32).abs() < 0.01, "{cell:?}: {advance}");
        assert_eq!(sz.body, 0.8 * cell.1 as f32);
    }
    // a cell too short for its width: the code fits the row, its pitch still the cell
    let sz = Sizes::of((20, 17), &mut ty);
    assert!(sz.code <= 0.9 * 17.0);
    ty.large = true;
    assert!(Sizes::of((19, 37), &mut ty).body > 0.9 * 37.0);
}

/// The columns of a pixmap's row band with any ink.
fn inked(pm: &Pixmap) -> Vec<u32> {
    (0..pm.width())
        .filter(|&x| (0..pm.height()).any(|y| pm.pixel(x, y).unwrap().alpha() > 0))
        .collect()
}

#[test]
fn code_is_a_character_a_cell_each_on_whole_pixels() {
    let mut ty = Type::new(Fonts::builtin());
    let sz = Sizes::of((19, 37), &mut ty);
    let mut one = Ink::new(19 * 4, 37, &mut ty, Theme::default(), sz).unwrap();
    assert_eq!(one.cells("M", Face::Mono, 19.0, 0.0, 9, [255; 4]), 1);
    let first = inked(&one.pm);
    let mut ty = Type::new(Fonts::builtin());
    let mut two = Ink::new(19 * 4, 37, &mut ty, Theme::default(), sz).unwrap();
    assert_eq!(two.cells("MM", Face::Mono, 19.0, 0.0, 9, [255; 4]), 2);
    // the second M is the first, a cell (19 pixels) on: the same pixels
    let second: Vec<u32> = inked(&two.pm).into_iter().filter(|x| *x >= 38).collect();
    assert_eq!(second, first.iter().map(|x| x + 19).collect::<Vec<_>>());
    for y in 0..37 {
        for x in first.iter() {
            assert_eq!(one.pm.pixel(*x, y), two.pm.pixel(x + 19, y));
        }
    }
    // a wide character takes two cells; past `most` nothing is drawn
    let mut ty = Type::new(Fonts::builtin());
    let mut ink = Ink::new(19 * 4, 37, &mut ty, Theme::default(), sz).unwrap();
    assert_eq!(ink.cells("日a", Face::Mono, 0.0, 0.0, 3, [255; 4]), 3);
    assert_eq!(ink.cells("日a", Face::Mono, 0.0, 0.0, 1, [255; 4]), 0);
}
