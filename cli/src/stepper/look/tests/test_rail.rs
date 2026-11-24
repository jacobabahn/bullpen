use super::*;
use crate::stepper::look::ink::{Sizes, Type};
use bullpen_draw::font::Fonts;
use bullpen_draw::theme::Theme;

fn rail(n: usize, at: usize) -> Rail {
    Rail {
        tid: "BP-9".into(),
        title: "Issue scoring".into(),
        steps: (0..n).map(|i| format!("Step number {i} with a title")).collect(),
        at,
        comments: (0..n).map(|i| usize::from(i == 1)).collect(),
    }
}

fn blue_rows(pm: &tiny_skia::Pixmap, th: Theme) -> Vec<u32> {
    (0..pm.height())
        .filter(|&y| {
            (0..pm.width()).any(|x| {
                let p = pm.pixel(x, y).unwrap().demultiply();
                [p.red(), p.green(), p.blue(), p.alpha()] == [th.blue.0, th.blue.1, th.blue.2, 255]
            })
        })
        .collect()
}

fn drawn(r: &Rail, h: u32) -> tiny_skia::Pixmap {
    let mut ty = Type::new(Fonts::builtin());
    let sz = Sizes::of((8, 17), &mut ty);
    let mut ink = Ink::new(300, h, &mut ty, Theme::default(), sz).unwrap();
    draw(&mut ink, r);
    ink.pm
}

#[test]
fn the_step_shown_is_blue_and_lower_down_the_further_on() {
    let th = Theme::default();
    let first = blue_rows(&drawn(&rail(5, 0), 400), th);
    let third = blue_rows(&drawn(&rail(5, 2), 400), th);
    assert!(!first.is_empty() && !third.is_empty());
    assert!(third[0] > first[0]);
}

#[test]
fn a_long_tour_scrolls_its_rail_to_keep_the_step_shown() {
    let th = Theme::default();
    let pm = drawn(&rail(60, 55), 400);
    let rows = blue_rows(&pm, th);
    assert!(!rows.is_empty(), "the step shown is on the rail");
    assert!(*rows.last().unwrap() < 400);
}

#[test]
fn the_foot_shows_fewer_keys_when_narrow() {
    let mut ty = Type::new(Fonts::builtin());
    let all = keys_width(&mut ty, STEP_KEYS[0], 10.0);
    let few = keys_width(&mut ty, STEP_KEYS[STEP_KEYS.len() - 1], 10.0);
    assert!(few < all / 2.0);
    for (w, end) in [(400, false), (2000, false), (400, true)] {
        let sz = Sizes::of((8, 17), &mut ty);
        let mut ink = Ink::new(w, 34, &mut ty, Theme::default(), sz).unwrap();
        foot(
            &mut ink,
            &Foot {
                msg: "Saved comment 1 on step 1.".into(),
                more: true,
                end,
            },
            17.0,
        );
    }
}

#[test]
fn a_short_rail_drawn_at_its_height_is_the_tall_one_cut_with_only_its_edge_under() {
    let mut ty = Type::new(Fonts::builtin());
    let sz = Sizes::of((8, 17), &mut ty);
    let r = rail(3, 1);
    let h = height(&mut ty, sz, 300.0, &r).ceil() as u32;
    assert!(h < 300, "{h}");
    let (short, tall) = (drawn(&r, h), drawn(&r, 600));
    assert_eq!(short.data(), &tall.data()[..h as usize * 300 * 4]);
    // under it, the edge drawn alone on the rail's last column (8 pixels) and nothing else
    let mut ink = Ink::new(8, 600 - h, &mut ty, Theme::default(), sz).unwrap();
    edge(&mut ink);
    for y in h..600 {
        for x in 0..300 {
            let want = if x >= 292 {
                ink.pm.pixel(x - 292, y - h).unwrap()
            } else {
                tiny_skia::PremultipliedColorU8::TRANSPARENT
            };
            assert_eq!(tall.pixel(x, y).unwrap(), want, "at {x},{y}");
        }
    }
}

#[test]
fn a_long_rail_needs_more_than_a_pane() {
    let mut ty = Type::new(Fonts::builtin());
    let sz = Sizes::of((8, 17), &mut ty);
    let (short, long) = (
        height(&mut ty, sz, 300.0, &rail(3, 0)),
        height(&mut ty, sz, 300.0, &rail(60, 0)),
    );
    assert!(short < 300.0 && long > 1000.0, "{short} {long}");
}
