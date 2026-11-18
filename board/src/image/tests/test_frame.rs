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
    assert!(!inked(&block(&pm, (0, 400), (0, 20)))); // the row above the message is empty
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
