use super::*;
use tiny_skia::IntSize;

const CELL: (u32, u32) = (19, 37);

/// An image whose every pixel is its own (from where it is).
fn pattern(w: u32, h: u32) -> Pixmap {
    let mut data = vec![0u8; w as usize * h as usize * 4];
    for (i, px) in data.chunks_exact_mut(4).enumerate() {
        let n = (i as u32).wrapping_mul(2_654_435_761);
        px.copy_from_slice(&[(n >> 24) as u8 / 2, (n >> 16) as u8 / 2, (n >> 8) as u8 / 2, 255]);
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h).unwrap()).unwrap()
}

/// What placements of the pieces show, put together as one picture the crop's size.
fn shown(whole: &Place, cut: &[Piece], parts: &[Pixmap], (cw, ch): (u32, u32)) -> Pixmap {
    let mut out = Pixmap::new(whole.cols as u32 * cw, whole.rows as u32 * ch).unwrap();
    let mut covered = vec![0u8; whole.cols as usize * whole.rows as usize];
    for (i, at) in split(whole, cut, (cw, ch)) {
        assert_eq!((at.crop_w, at.crop_h), (at.cols as u32 * cw, at.rows as u32 * ch), "pixel for pixel");
        let piece = cut[i];
        assert!(at.crop_x + at.crop_w <= piece.w && at.crop_y + at.crop_h <= piece.h, "{at:?} in {piece:?}");
        let rect = IntRect::from_xywh(at.crop_x as i32, at.crop_y as i32, at.crop_w, at.crop_h).unwrap();
        let part = parts[i].clone_rect(rect).unwrap();
        let (x, y) = ((at.col - whole.col) as u32 * cw, (at.row - whole.row) as u32 * ch);
        out.draw_pixmap(x as i32, y as i32, part.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        for r in at.row - whole.row..at.row - whole.row + at.rows {
            for c in at.col - whole.col..at.col - whole.col + at.cols {
                covered[r as usize * whole.cols as usize + c as usize] += 1;
            }
        }
    }
    assert!(covered.iter().all(|n| *n == 1), "every cell placed once");
    out
}

fn cut_up(pm: &Pixmap, cell: (u32, u32)) -> (Vec<Piece>, Vec<Pixmap>) {
    let cut = pieces(pm.width(), pm.height(), cell).unwrap();
    let parts = cut.iter().map(|p| pm.clone_rect(IntRect::from_xywh(p.x as i32, p.y as i32, p.w, p.h).unwrap()).unwrap()).collect();
    (cut, parts)
}

#[test]
fn an_image_under_the_limit_is_one_piece_placed_as_it_is() {
    assert_eq!(pieces(1463, 1500, CELL), Some(vec![Piece { x: 0, y: 0, w: 1463, h: 1500 }]));
    let p = Place { id: 1, p: 2, col: 3, row: 4, crop_x: 0, crop_y: 5, crop_w: 7, crop_h: 9, cols: 1, rows: 1, z: 1 };
    assert_eq!(split(&p, &[Piece { x: 0, y: 0, w: 10, h: 10 }], CELL), vec![(0, p)]);
}

#[test]
fn every_piece_is_under_the_limit_and_they_cover_the_image() {
    // a 300×100 pane's worth, a 77-column board's two pages, and one too wide for a strip
    for (w, h) in [(5700, 3700), (1463, 5328), (130_000, 200), (9_500_000 / 4 / 74 + 1, 400)] {
        let cut = pieces(w, h, CELL).unwrap();
        assert!(cut.len() > 1, "{w}×{h}");
        for p in &cut {
            assert!(fits(p.w, p.h) && p.x + p.w <= w && p.y + p.h <= h, "{p:?} of {w}×{h}");
        }
        assert!(cut.iter().any(|p| p.x + p.w == w) && cut.iter().any(|p| p.y + p.h == h));
        assert!(cut.iter().all(|p| p.y == 0 || cut.iter().any(|q| q.x == p.x && q.y < p.y && q.y + q.h >= p.y + CELL.1)), "a cell's overlap");
    }
    assert_eq!(pieces(9_500_000 / 4 / 74 + 1, 400, CELL).unwrap().iter().filter(|p| p.y == 0).count(), 2, "side by side");
    // a cell so big not even two of them are under it
    assert_eq!(pieces(4000, 4000, (1600, 1600)), None);
}

#[test]
fn strips_placed_are_the_image_whole_whatever_the_crop() {
    let pm = pattern(1463, 5328);
    let (cut, parts) = cut_up(&pm, CELL);
    assert!(cut.len() >= 3);
    // crops at the board's list's phases (rows that aren't whole cells), and from the top
    for crop_y in [0, 1, 36, 37, 1000, 1999, 2400, 2405, 3000] {
        let rows = ((5328 - crop_y) / 37).min(72) as u16;
        let whole = Place { id: 9, p: 1, col: 2, row: 3, crop_x: 0, crop_y, crop_w: 1463, crop_h: rows as u32 * 37, cols: 77, rows, z: 0 };
        let got = shown(&whole, &cut, &parts, CELL);
        let want = pm.clone_rect(IntRect::from_xywh(0, crop_y as i32, 1463, rows as u32 * 37).unwrap()).unwrap();
        assert!(got.data() == want.data(), "crop from {crop_y}");
    }
}

#[test]
fn pieces_side_by_side_placed_are_the_image_whole() {
    let cell = (40, 37);
    let w = 9_500_000 / 4 / 74 + 300; // a strip two cells high is over
    let pm = pattern(w / 40 * 40, 300);
    let (cut, parts) = cut_up(&pm, cell);
    assert!(cut.iter().filter(|p| p.y == 0).count() >= 2);
    let cols = (pm.width() / 40) as u16;
    for crop_y in [0, 20, 37] {
        let rows = ((300 - crop_y) / 37) as u16;
        let whole = Place { id: 9, p: 1, col: 0, row: 0, crop_x: 0, crop_y, crop_w: pm.width(), crop_h: rows as u32 * 37, cols, rows, z: 0 };
        let got = shown(&whole, &cut, &parts, cell);
        let want = pm.clone_rect(IntRect::from_xywh(0, crop_y as i32, pm.width(), rows as u32 * 37).unwrap()).unwrap();
        assert!(got.data() == want.data(), "crop from {crop_y}");
    }
}

#[test]
fn an_upload_says_its_images_size() {
    let pm = pattern(30, 7);
    let out = crate::kitty::upload(3, &png::encode(&pm)) + &crate::kitty::upload(4, &png::encode(&pattern(5, 9)));
    assert_eq!(uploaded(&out), vec![(30, 7), (5, 9)]);
}
