use super::*;
use crate::image::font::Fonts;
use crate::image::render::Grid;
use crate::image::theme::Theme;
use crate::model::TextSize;
use tiny_skia::Pixmap;

fn tally(n: usize, word: &'static str, status: &'static str) -> Tally {
    Tally { n, word, status }
}

fn fixture() -> Vec<Tally> {
    vec![
        tally(3, "need you", "needs"),
        tally(1, "coordinator", "triage"),
        tally(3, "ready", "ready"),
        tally(1, "finished", "finished"),
        tally(1, "in review", "review"),
        tally(3, "working", "working"),
        tally(2, "queued", "todo"),
    ]
}

#[test]
fn the_bar_has_a_segment_per_open_task_in_the_boards_order_filling_its_width() {
    let segs = segments(&fixture(), 10.0, 570.0, 3.0);
    assert_eq!(segs.len(), 14);
    let statuses: Vec<&str> = segs.iter().map(|s| s.2).collect();
    assert_eq!(&statuses[..4], &["needs", "needs", "needs", "triage"]);
    assert_eq!(statuses.last(), Some(&"todo"));
    let (first, last) = (segs[0], segs[13]);
    assert_eq!(first.0, 10.0);
    assert!((last.0 + last.1 - 570.0).abs() < 0.01);
    // the same width each, 3 pixels apart
    assert!(segs.windows(2).all(|w| (w[1].0 - (w[0].0 + w[0].1) - 3.0).abs() < 0.01 && (w[1].1 - w[0].1).abs() < 0.01));
}

#[test]
fn with_too_many_tasks_for_a_segment_each_a_group_gets_one_as_wide_as_its_share() {
    let many = vec![tally(300, "working", "working"), tally(100, "queued", "todo")];
    let segs = segments(&many, 0.0, 403.0, 3.0);
    assert_eq!(segs.len(), 2);
    assert!((segs[0].1 - 300.0).abs() < 0.01 && (segs[1].1 - 100.0).abs() < 0.01);
    assert!(segments(&[], 0.0, 400.0, 3.0).is_empty());
}

/// The bar drawn on a board `cols` wide, 16 by 34 pixel cells, medium type.
fn drawn(tallies: &[Tally], cols: usize) -> (Pixmap, Canvas) {
    let c = Canvas::new(Grid { cols, cell_w: 16, cell_h: 34 }, TextSize::Medium, Theme::default());
    let mut fonts = Fonts::load(None).unwrap();
    let mut pm = Pixmap::new(c.grid.width(), 3 * 34).unwrap();
    draw(&mut Draw::new(&mut pm, &mut fonts, &c, 0.0), &c, tallies, 3.0 * 34.0);
    (pm, c)
}

/// The columns of row y that are inked.
fn inked(pm: &Pixmap, y: u32, from: u32) -> Vec<u32> {
    (from..pm.width()).filter(|&x| pm.pixels()[(y * pm.width() + x) as usize].alpha() > 0).collect()
}

#[test]
fn the_bar_sits_under_the_top_line_in_its_groups_colours() {
    let (pm, c) = drawn(&fixture(), 47);
    let y = (TOP * 34.0).round() as u32 + 4;
    let px = |x: u32| pm.pixels()[(y * pm.width() + x) as usize].demultiply();
    let t = c.theme;
    let first = px(16 + 20);
    assert_eq!((first.red(), first.green(), first.blue(), first.alpha()), (t.yellow.0, t.yellow.1, t.yellow.2, 255));
    // and nothing over it, under the top line
    assert!(inked(&pm, (1.4 * 34.0) as u32, 0).is_empty());
}

#[test]
fn the_counts_under_the_bar_are_the_groups_that_fit_then_how_many_tasks_the_rest_have() {
    // the counts' line is midway between the bar and the image's foot
    let line = (((TOP * 34.0).round() + (HEIGHT * 34.0).round() + 3.0 * 34.0) / 2.0) as u32;
    let (wide, _) = drawn(&fixture(), 120);
    let (narrow, c) = drawn(&fixture(), 35);
    let end = |pm: &Pixmap| (line - 8..line + 8).flat_map(|y| inked(pm, y, 0)).max().unwrap();
    assert!(end(&narrow) <= c.right() as u32);
    assert!(end(&wide) > end(&narrow) + 200);
    // in the bar's order, from what needs you: the amber dot keys its first segments
    let t = c.theme;
    let has = |pm: &Pixmap, c: crate::image::theme::Rgb| inked(pm, line, 0).into_iter().any(|x| {
        let p = pm.pixels()[(line * pm.width() + x) as usize].demultiply();
        (p.red(), p.green(), p.blue()) == (c.0, c.1, c.2)
    });
    assert!(has(&wide, t.yellow) && has(&wide, t.green) && has(&wide, t.blue));
    let first = inked(&narrow, line, 0).into_iter().next().unwrap();
    let p = narrow.pixels()[(line * narrow.width() + first + 2) as usize].demultiply();
    assert_eq!((p.red(), p.green(), p.blue()), (t.yellow.0, t.yellow.1, t.yellow.2), "the first count is what needs you");
    // the groups that fit, then "+N" for the rest's tasks
    let (pm, c) = drawn(&fixture(), 35);
    let mut fonts = Fonts::load(None).unwrap();
    let mut scratch = Pixmap::new(1, 1).unwrap();
    let d = Draw::new(&mut scratch, &mut fonts, &c, 0.0);
    let all = fixture();
    let groups: Vec<&Tally> = all.iter().collect();
    let (shown, more) = fitting(&d, &groups, c.right() - c.left(), c.m.label);
    assert!(shown >= 1 && shown < groups.len(), "{shown}");
    assert_eq!(more, format!("+{} more", groups[shown..].iter().map(|t| t.n).sum::<usize>()));
    assert_eq!(fitting(&d, &groups, 4000.0, c.m.label), (groups.len(), String::new()));
    assert!(end(&pm) > 200);
}
