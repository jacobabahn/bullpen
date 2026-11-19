use super::*;
use crate::image::render::Grid;
use crate::image::theme::{self, Theme};
use crate::model::TextSize;

fn canvas(size: TextSize) -> Canvas {
    Canvas::new(Grid { cols: 40, cell_w: 10, cell_h: 20 }, size, Theme::default())
}

fn head_of(counts: Vec<(&str, &'static str)>) -> Head {
    Head {
        name: "bullpen".into(),
        auto: String::new(),
        others: String::new(),
        others_amber: false,
        counts: counts.into_iter().map(|(t, st)| (t.to_string(), st)).collect(),
        sep: " · ",
        tallies: Vec::new(),
    }
}

/// The straight RGBA pixels of a block of the image: columns [x0, x1), rows [y0, y1).
fn block(pm: &Pixmap, (x0, x1): (u32, u32), (y0, y1): (u32, u32)) -> Vec<[u8; 4]> {
    let w = pm.width();
    let mut out = Vec::new();
    for y in y0..y1.min(pm.height()) {
        for x in x0..x1.min(w) {
            let c = pm.pixels()[(y * w + x) as usize].demultiply();
            out.push([c.red(), c.green(), c.blue(), c.alpha()]);
        }
    }
    out
}

fn has(px: &[[u8; 4]], c: theme::Rgb) -> bool {
    px.iter().any(|p| p[3] == 255 && (p[0], p[1], p[2]) == (c.0, c.1, c.2))
}

fn inked(px: &[[u8; 4]]) -> bool {
    px.iter().any(|p| p[3] > 0)
}

#[test]
fn the_header_is_its_cells_high_with_the_name_at_the_left_and_the_counts_in_their_colours_at_the_right() {
    let t = Theme::default();
    let pm = head(&head_of(vec![("1 needs you", "needs"), ("2 ready", "ready")]), 2, &canvas(TextSize::Medium), &mut Fonts::builtin()).unwrap();
    assert_eq!((pm.width(), pm.height()), (400, 40));
    assert!(inked(&block(&pm, (10, 60), (0, 40))));
    let right = block(&pm, (200, 400), (0, 40));
    assert!(has(&right, t.yellow) && has(&right, t.green));
    // nothing in the last column: it's the list's scroll marks'
    assert!(!inked(&block(&pm, (390, 400), (0, 40))));
}

#[test]
fn a_header_too_long_for_its_line_cuts_the_name_and_keeps_the_counts() {
    let mut h = head_of(vec![("3 ready", "ready")]);
    h.name = "a repository whose name is much too long for the line it's on".into();
    let pm = head(&h, 2, &canvas(TextSize::Large), &mut Fonts::builtin()).unwrap();
    assert!(has(&block(&pm, (250, 400), (0, 40)), Theme::default().green));
}

#[test]
fn the_bottom_rows_put_each_line_on_its_row_in_its_tone() {
    let t = Theme::default();
    let foot_rows = Foot {
        lines: vec![
            (1, FootLine::Msg("Couldn't accept BP-3: the rebase stopped.".into(), Tone::Red)),
            (2, FootLine::Keys(vec![("a".into(), "accept".into()), ("d".into(), "drop".into())])),
        ],
    };
    let pm = foot(&foot_rows, 3, &canvas(TextSize::Medium), &mut Fonts::builtin()).unwrap();
    assert_eq!((pm.width(), pm.height()), (400, 60));
    // the row above the message has only the rule, half a cell over it
    let above = block(&pm, (0, 400), (0, 20));
    assert!(above.iter().all(|p| p[3] <= t.rule()[3]) && inked(&above));
    assert!(has(&block(&pm, (0, 400), (20, 40)), t.red));
    let keys = block(&pm, (0, 400), (40, 60));
    assert!(inked(&keys) && !has(&keys, t.red));
    // `?` at the right edge
    assert!(inked(&block(&pm, (300, 390), (40, 60))));
}

#[test]
fn a_key_row_too_long_for_its_width_keeps_the_question_mark() {
    let many: Vec<(String, String)> = (0..12).map(|i| (format!("{i}"), "does something".into())).collect();
    let foot_rows = Foot { lines: vec![(0, FootLine::Keys(many))] };
    let c = Canvas::new(Grid { cols: 24, cell_w: 10, cell_h: 20 }, TextSize::Large, Theme::default());
    let pm = foot(&foot_rows, 1, &c, &mut Fonts::builtin()).unwrap();
    // `?` bare, just short of the scroll marks' column
    assert!(inked(&block(&pm, (200, 224), (0, 20))));
}

/// The rows of an image with a pixel of this colour (at its alpha, its colour within
/// what storing it premultiplied rounds off).
fn rows_with(pm: &Pixmap, c: [u8; 4]) -> Vec<u32> {
    (0..pm.height())
        .filter(|&y| block(pm, (0, pm.width()), (y, y + 1)).iter().any(|p| p[3] == c[3] && (0..3).all(|k| p[k].abs_diff(c[k]) <= 8)))
        .collect()
}

#[test]
fn the_headers_counts_are_chips_with_a_rule_under_the_line() {
    let t = Theme::default();
    let c = canvas(TextSize::Medium);
    let pm = head(&head_of(vec![("1 needs you", "needs"), ("2 ready", "ready")]), 2, &c, &mut Fonts::builtin()).unwrap();
    // each count on its colour, faintly, a pill a word and a half high
    let amber = rows_with(&pm, [t.yellow.0, t.yellow.1, t.yellow.2, 46]);
    let tall = amber.last().unwrap() - amber.first().unwrap() + 1;
    assert!((tall as f32 - 1.5 * (c.m.label + 1.0)).abs() <= 2.0, "{tall}");
    assert!(!rows_with(&pm, [t.green.0, t.green.1, t.green.2, 46]).is_empty());
    // the rule: under the chips, across the line
    let rule = rows_with(&pm, t.rule());
    assert!(!rule.is_empty() && rule[0] > *amber.last().unwrap());
    let y = rule[0];
    assert!(block(&pm, (14, 386), (y, y + 1)).iter().all(|p| p[3] > 0));
    // a header a cell high has no room for it
    let one = head(&head_of(vec![("2 ready", "ready")]), 1, &c, &mut Fonts::builtin()).unwrap();
    assert!(rows_with(&one, t.rule()).is_empty());
}

#[test]
fn a_three_cell_header_has_the_status_bar_in_the_rules_place_and_only_the_chip_of_what_needs_you() {
    use crate::ui::list::Tally;
    let t = Theme::default();
    let c = canvas(TextSize::Medium);
    let mut h = head_of(vec![("1 needs you", "needs"), ("2 ready", "ready")]);
    h.tallies = vec![
        Tally { n: 1, word: "needs you", status: "needs" },
        Tally { n: 2, word: "ready", status: "ready" },
        Tally { n: 1, word: "in review", status: "review" },
    ];
    let pm = head(&h, 3, &c, &mut Fonts::builtin()).unwrap();
    assert_eq!(pm.height(), 60);
    // the line: the amber chip, not the green one (the counts under the bar say it)
    assert!(!rows_with(&pm, [t.yellow.0, t.yellow.1, t.yellow.2, 46]).is_empty());
    assert!(rows_with(&pm, [t.green.0, t.green.1, t.green.2, 46]).is_empty());
    // the bar, solid in its groups' colours, and no rule (a row of it across the line)
    let ruled = |pm: &Pixmap| (0..pm.height()).any(|y| block(pm, (14, 386), (y, y + 1)).iter().all(|p| p[3] == t.rule()[3]));
    assert!(!ruled(&pm));
    let bar = block(&pm, (0, 400), (30, 35));
    assert!(has(&bar, t.yellow) && has(&bar, t.green) && has(&bar, t.blue));
    // with no task open, the rule as before
    h.tallies.clear();
    assert!(ruled(&head(&h, 3, &c, &mut Fonts::builtin()).unwrap()));
}

#[test]
fn the_counts_are_numbers_only_when_their_words_dont_fit() {
    let (h, f) = (head_of(vec![("3 need you", "needs"), ("4 ready", "ready")]), Fonts::builtin());
    let words = counts(h.counts.clone(), &f, 15.0, 400.0);
    assert_eq!(words, h.counts);
    let numbers = counts(h.counts.clone(), &f, 15.0, 120.0);
    assert_eq!(numbers, vec![("3".to_string(), "needs"), ("4".to_string(), "ready")]);
}

#[test]
fn each_key_is_in_a_cap() {
    let t = Theme::default();
    let keys = Foot { lines: vec![(0, FootLine::Keys(vec![("a".into(), "accept".into())]))] };
    let pm = foot(&keys, 1, &canvas(TextSize::Medium), &mut Fonts::builtin()).unwrap();
    // the cap's fill and its edge, a little stronger, round the key
    assert!(!rows_with(&pm, t.cap()).is_empty());
    assert!(!rows_with(&pm, t.cap_edge()).is_empty());
}

#[test]
fn at_36_columns_the_keys_and_question_mark_dont_overlap_and_the_question_mark_stays() {
    let fonts = Fonts::builtin();
    let keys: Vec<(String, String)> =
        [("a", "accept"), ("g", "tour"), ("d", "drop"), ("⏎", "open"), ("v", "diff"), ("t", "tell"), ("p", "peek")].map(|(k, w)| (k.into(), w.into())).to_vec();
    for size in [TextSize::Small, TextSize::Medium, TextSize::Large] {
        let c = Canvas::new(Grid { cols: 35, cell_w: 16, cell_h: 34 }, size, Theme::default());
        let room = c.right() - 16.0;
        for n in 0..=keys.len() {
            let (shown, help) = fit_keys(&fonts, &keys[..n], c.m.label, room);
            assert_eq!(help[0].0, "?", "{size:?}");
            assert_eq!(shown[..], keys[..shown.len()], "the last keys go first");
            let used = keys_width(&fonts, &shown, c.m.label) + keys_width(&fonts, &help, c.m.label);
            let gap = if shown.is_empty() { 0.0 } else { KEY_GAP * c.m.label };
            assert!(used + gap <= room, "{size:?}, {n} keys: {used} of {room}");
        }
    }
}

#[test]
fn the_rule_over_the_bottom_rows_stays_put_when_a_message_comes() {
    let (c, t) = (canvas(TextSize::Medium), Theme::default());
    let keys = || FootLine::Keys(vec![("a".into(), "accept".into())]);
    let quiet = foot(&Foot { lines: vec![(2, keys())] }, 3, &c, &mut Fonts::builtin()).unwrap();
    let said = foot(&Foot { lines: vec![(1, FootLine::Msg("Accepted BP-1.".into(), Tone::Green)), (2, keys())] }, 3, &c, &mut Fonts::builtin()).unwrap();
    let (a, b) = (rows_with(&quiet, t.rule()), rows_with(&said, t.rule()));
    // above the message's row, there or not
    assert!(!a.is_empty() && a == b, "{a:?} {b:?}");
    assert!(a[0] < c.grid.cell_h);
}
