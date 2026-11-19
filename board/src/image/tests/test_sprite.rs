use super::*;
use crate::image::Theme;
use crate::model::TextSize;

fn canvas(size: TextSize) -> Canvas {
    Canvas::new(Grid { cols: 39, cell_w: 10, cell_h: 20 }, size, Theme::default())
}

/// How much of a tile is drawn: the sum of its alpha.
fn ink(pm: &Pixmap, tile: u32, h: u32) -> u64 {
    let w = pm.width() as usize;
    pm.pixels()[(tile * h) as usize * w..((tile + 1) * h) as usize * w].iter().map(|p| p.alpha() as u64).sum()
}

#[test]
fn the_marks_turn_at_the_text_boards_pace_and_settle_once() {
    assert_eq!(frames(&Mark::Working), (10, 200));
    assert_eq!(frames(&Mark::Review), (4, 200));
    assert_eq!(frames(&Mark::Job), (8, 100));
    assert_eq!(frames(&Mark::Tour("review".into())), (8, 100));
    // a turning mark goes round; one settling in stops at its last size
    assert_eq!([0, 199, 200, 1999, 2000].map(|ms| frame(&Mark::Working, ms)), [0, 0, 1, 9, 0]);
    let still = Mark::Still("ready".into());
    assert_eq!([0, 40, 80, 120, 160, 5000].map(|ms| frame(&still, ms)), [0, 1, 2, 3, 3, 3]);
    assert!(settling_ms() < 200);
}

#[test]
fn a_strip_has_a_tile_a_frame_each_turned_or_sized() {
    let c = canvas(TextSize::Medium);
    let mut fonts = Fonts::builtin();
    let t = tile(&c);
    let working = draw(&Sprite::Mark(Mark::Working), &c, &mut fonts).unwrap();
    // as many columns as the mark reaches, at its largest
    assert_eq!((working.width(), working.height()), (cols(&c) as u32 * 10, 10 * t.h));
    assert!(cols(&c) >= 3 && mark_at(&c).0 + mark_reach(&c, POP_MAX) <= working.width() as f32);
    // the turning arc is somewhere else in each frame
    let tile_px = |pm: &Pixmap, f: u32| pm.clone_rect(tiny_skia::IntRect::from_xywh(0, (f * t.h) as i32, pm.width(), t.h).unwrap()).unwrap();
    assert_ne!(tile_px(&working, 0).data(), tile_px(&working, 1).data());
    // a mark settling in grows past its size, then is its size
    let ready = draw(&Sprite::Mark(Mark::Still("ready".into())), &c, &mut fonts).unwrap();
    let sizes: Vec<u64> = (0..4).map(|f| ink(&ready, f, t.h)).collect();
    assert!(sizes[0] < sizes[1] && sizes[1] < sizes[2] && sizes[3] < sizes[2], "{sizes:?}");
    // the scroll marks: ↑ in the top cell, ↓ in the one under
    let chevrons = draw(&Sprite::Chevrons, &c, &mut fonts).unwrap();
    assert_eq!((chevrons.width(), chevrons.height()), (10, 40));
    assert!(ink(&chevrons, 0, 20) > 0 && ink(&chevrons, 1, 20) > 0);
}

#[test]
fn a_frame_is_placed_on_the_cells_its_mark_covers_wherever_it_falls_in_one() {
    let c = canvas(TextSize::Medium);
    let t = tile(&c);
    for cy in [t.reach, 33, 40, 47, 59, 100] {
        let a = at(&c, cy, 2, 18).unwrap();
        // the cells reach past the mark both ways
        let (top, bottom) = (a.row as u32 * 20, (a.row + a.rows) as u32 * 20);
        assert!(top + t.reach <= cy && cy + t.reach < bottom, "{cy}: {a:?}");
        // the crop is in frame 2's tile, with the mark's centre where the list has it
        assert!(a.crop_y >= 2 * t.h && a.crop_y + a.crop_h <= 3 * t.h, "{cy}: {a:?}");
        assert_eq!(a.crop_y - 2 * t.h + (cy - top), t.centre);
        assert_eq!(a.crop_h, a.rows as u32 * 20);
    }
    // cut at the list's last cell; none past it
    let a = at(&c, 18 * 20 - 3, 0, 18).unwrap();
    assert_eq!(a.row + a.rows, 18);
    assert_eq!(at(&c, 18 * 20 + 3 * t.reach, 0, 18), None);
}

