//! The drawn tour's frames as bytes: what's sent once, what a key places, what typing
//! shows as text, and what closing deletes. `render_tour` (ignored) writes pictures.

use super::terminal::{Terminal, REX_QUOTA};
use super::*;
use crate::stepper::screen::DiffFn;
use bullpen_draw::font::Fonts;
use bullpen_draw::png;
use serde_json::json;
use tiny_skia::Pixmap;

fn as_obj(v: Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}

/// A long step (120 lines of patch), a short one and one with no code.
fn patch(i: usize) -> String {
    match i {
        0 => {
            let mut p = "diff --git a/cli/src/a.rs b/cli/src/a.rs\nindex 1..2 100644\n--- a/cli/src/a.rs\n+++ b/cli/src/a.rs\n@@ -1,120 +1,120 @@ fn main() {\n".to_string();
            for n in 0..120 {
                p += &if n % 7 == 3 { format!("+    let x{n} = \"added\"; // {n}\n") } else { format!("     let y{n} = {n};\n") };
            }
            p
        }
        1 => "diff --git a/b.py b/b.py\n--- a/b.py\n+++ b/b.py\n@@ -3,2 +3,2 @@\n-def old():\n+def new():\n     return 1\n".into(),
        _ => String::new(),
    }
}

fn stepper() -> Stepper {
    let tour = as_obj(json!({"commit": "c1", "steps": [
        {"title": "The score gets a `fourth` word", "note": "`Score` gains `worth` (`yes`, `no`), and the parse takes four.", "hunks": [{"file": "cli/src/a.rs", "lines": [1, 120]}]},
        {"title": "Only yes passes", "note": "Short.", "hunks": [{"file": "b.py", "lines": [3, 4]}]},
        {"title": "Docs", "note": "", "hunks": []}]}));
    let comments = vec![as_obj(
        json!({"id": 1, "step": 1, "title": "The score", "commit": "c1", "text": "Say which word failed."}),
    )];
    let diff: DiffFn = Box::new(|_, _| vec![]);
    Stepper::new("BP-9", "Issue scoring", &tour, comments, 0, diff)
}

fn look(cell: (u32, u32)) -> Look {
    Look::new(Theme::default(), cell, Type::new(Fonts::builtin()), Box::new(patch))
}

const SIZE: (i64, i64) = (120, 30);
const CELL: (u32, u32) = (8, 17);

/// A frame's bytes.
fn frame(l: &mut Look, st: &mut Stepper, size: (i64, i64)) -> String {
    let mut out = Vec::new();
    l.frame(st, size, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn uploads(out: &str) -> usize {
    out.matches("a=t,f=100").count()
}

fn places(out: &str) -> usize {
    out.matches("\x1b_Ga=p,").count()
}

#[test]
fn the_first_frame_sends_the_rail_a_tile_and_the_foot_and_places_them() {
    let (mut l, mut st) = (look(CELL), stepper());
    let out = frame(&mut l, &mut st, SIZE);
    assert_eq!(uploads(&out), 3, "rail, the page's first tile, the foot");
    assert_eq!(places(&out), 3);
    assert!(out.ends_with("\x1b[?25l"), "no cursor while reading");
    // the same frame again sends nothing but the cursor's state
    assert_eq!(frame(&mut l, &mut st, SIZE), "\x1b[?25l");
}

#[test]
fn a_scroll_places_crops_of_the_tiles_sent() {
    let (mut l, mut st) = (look(CELL), stepper());
    frame(&mut l, &mut st, SIZE);
    // scrolled, the window is the end of the first tile and the start of the second
    st.key("j", 28);
    let out = frame(&mut l, &mut st, SIZE);
    assert_eq!((uploads(&out), places(&out)), (1, 2), "{out:?}");
    st.key("j", 28);
    let out = frame(&mut l, &mut st, SIZE);
    assert_eq!((uploads(&out), places(&out)), (0, 2));
    assert!(out.len() < 200, "{} bytes: {out:?}", out.len());
    st.key("k", 28);
    assert_eq!(uploads(&frame(&mut l, &mut st, SIZE)), 0);
}

#[test]
fn drawn_ahead_the_first_scroll_only_places_crops() {
    let (mut l, mut st) = (look(CELL), stepper());
    frame(&mut l, &mut st, SIZE);
    l.ahead(&st, &|| false, &mut Vec::new()).unwrap();
    st.key("j", 28);
    let out = frame(&mut l, &mut st, SIZE);
    assert_eq!((uploads(&out), places(&out)), (0, 2));
}

#[test]
fn the_scroll_stops_at_the_pages_end() {
    let (mut l, mut st) = (look(CELL), stepper());
    st.key("G", 28);
    frame(&mut l, &mut st, SIZE);
    let lay = Lay::new(SIZE.0, SIZE.1, 0);
    let rows = l.page_rows(&st, 0, lay);
    assert_eq!(st.top, rows - lay.view as i64);
    assert!(st.top > 0);
}

#[test]
fn drawn_ahead_the_next_step_only_places_images() {
    let (mut l, mut st) = (look(CELL), stepper());
    frame(&mut l, &mut st, SIZE);
    let mut sent = Vec::new();
    l.ahead(&st, &|| false, &mut sent).unwrap();
    assert!(
        uploads(&String::from_utf8_lossy(&sent)) >= 4,
        "the next tile, the next step's rail, tile and foot"
    );
    st.key("n", 28);
    let out = frame(&mut l, &mut st, SIZE);
    assert_eq!(uploads(&out), 0, "{out:?}");
    assert!(out.len() < 600, "{} bytes", out.len());
    // and back, with nothing drawn ahead for it: the rail and page are kept, hidden
    st.key("p", 28);
    assert_eq!(uploads(&frame(&mut l, &mut st, SIZE)), 0);
}

#[test]
fn a_key_waiting_stops_drawing_ahead() {
    let (mut l, mut st) = (look(CELL), stepper());
    frame(&mut l, &mut st, SIZE);
    let mut sent = Vec::new();
    assert!(l.ahead(&st, &|| true, &mut sent).unwrap());
    assert!(sent.is_empty());
    st.key("n", 28);
    assert!(uploads(&frame(&mut l, &mut st, SIZE)) > 0);
}

#[test]
fn typing_a_comment_is_text_where_the_foot_was() {
    let (mut l, mut st) = (look(CELL), stepper());
    frame(&mut l, &mut st, SIZE);
    st.key("c", 28);
    st.key("h", 28);
    let out = frame(&mut l, &mut st, SIZE);
    assert!(out.contains("Comment on step 1:") && out.contains("⏎ saves"), "{out:?}");
    assert!(out.contains("\x1b_Ga=d,d=i"), "the foot taken off");
    assert!(out.contains("\x1b[?25h"), "the cursor shows");
    // the text starts at the page's column, right of the rail
    let rail = Lay::new(SIZE.0, SIZE.1, 0).rail;
    assert!(out.contains(&format!(";{}H\x1b[K", rail + 1)));
    st.key("\x1b", 28);
    let out = frame(&mut l, &mut st, SIZE);
    assert!(out.contains("\x1b[K") && out.contains("Comment not saved") == false);
    assert_eq!(uploads(&out), 1, "the foot with its message");
}

#[test]
fn closing_deletes_every_image_sent() {
    let (mut l, mut st) = (look(CELL), stepper());
    frame(&mut l, &mut st, SIZE);
    l.ahead(&st, &|| false, &mut Vec::new()).unwrap();
    let kept = l.kept();
    let mut out = Vec::new();
    l.close(&mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(out.matches("a=d,d=I").count(), kept);
}

#[test]
fn a_resize_takes_the_images_off_and_draws_them_at_the_new_width() {
    let (mut l, mut st) = (look(CELL), stepper());
    frame(&mut l, &mut st, SIZE);
    let out = frame(&mut l, &mut st, (100, 30));
    assert!(out.contains("a=d,d=i") && out.contains("\x1b[2J"));
    assert_eq!(uploads(&out), 3);
}

/// Each placement's crop and its cells: (w, h, c, r).
fn crops(out: &str) -> Vec<(u32, u32, u32, u32)> {
    let field = |c: &str, k: &str| {
        c.split(',')
            .find_map(|f| f.strip_prefix(k))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    out.split("\x1b_Ga=p,")
        .skip(1)
        .map(|c| c.split('\x1b').next().unwrap_or(""))
        .map(|c| (field(c, "w="), field(c, "h="), field(c, "c="), field(c, "r=")))
        .collect()
}

#[test]
fn a_new_cell_deletes_what_was_drawn_at_the_old_one_and_draws_at_the_new() {
    // a floating pane just opened says 8x16 cells; Rex sizes it to 19x37 a moment later
    let (mut l, mut st) = (look((8, 16)), stepper());
    let first = frame(&mut l, &mut st, SIZE);
    let mut out = Vec::new();
    assert!(!l.set_cell((8, 16), &mut out).unwrap(), "the same cell changes nothing");
    assert!(out.is_empty());
    assert!(l.set_cell((19, 37), &mut out).unwrap());
    let out = String::from_utf8(out).unwrap();
    assert_eq!(
        out.matches("a=d,d=I").count(),
        uploads(&first),
        "every image drawn at 8x16 is deleted"
    );
    assert!(out.ends_with("\x1b[2J"));
    assert_eq!(l.kept(), 0);
    let next = frame(&mut l, &mut st, SIZE);
    assert_eq!(uploads(&next), 3, "rail, tile and foot drawn again");
    let placed = crops(&next);
    assert_eq!(placed.len(), 3);
    for (w, h, c, r) in placed {
        assert_eq!(
            (w, h),
            (19 * c, 37 * r),
            "each image is its cells' pixels at the new cell, not stretched"
        );
    }
}

#[test]
fn a_narrow_pane_has_no_rail() {
    assert_eq!(Lay::new(70, 30, 0).rail, 0);
    assert_eq!(Lay::new(80, 30, 0).rail, 24);
    assert_eq!(Lay::new(160, 46, 0).rail, 41);
    assert_eq!(Lay::new(217, 65, 0).rail, 48);
    let (mut l, mut st) = (look(CELL), stepper());
    let out = frame(&mut l, &mut st, (70, 30));
    assert_eq!(uploads(&out), 2);
}

#[test]
fn the_end_page_follows_the_last_step() {
    let (mut l, mut st) = (look(CELL), stepper());
    st.go(3);
    assert!(st.at_end());
    let out = frame(&mut l, &mut st, SIZE);
    assert_eq!(uploads(&out), 3);
}

/// A step whose patch is `lines` added lines long.
fn long(lines: usize) -> PatchFn {
    Box::new(move |_| {
        let mut p = format!("diff --git a/big.rs b/big.rs\n--- a/big.rs\n+++ b/big.rs\n@@ -0,0 +1,{lines} @@\n");
        for n in 0..lines {
            p += &format!("+let line_{n} = \"{n}\";\n");
        }
        p
    })
}

#[test]
fn a_long_steps_page_is_never_an_image_of_its_own() {
    let size = (160, 46);
    let (cell, mut st) = ((16, 34), stepper());
    let mut l = Look::new(Theme::default(), cell, Type::new(Fonts::builtin()), long(4000));
    let started = std::time::Instant::now();
    frame(&mut l, &mut st, size);
    l.ahead(&st, &|| false, &mut Vec::new()).unwrap();
    st.key("G", 44);
    frame(&mut l, &mut st, size);
    let lay = Lay::new(size.0, size.1, 0);
    assert!(
        l.page_rows(&st, 0, lay) > 3600,
        "the page is as long as its code (4,000 lines of 31 pixels)"
    );
    // the biggest image drawn is the pane's, not the page's
    let pane = size.0 as usize * 16 * size.1 as usize * 34 * 4;
    assert!(l.largest <= pane, "{} bytes of pixels", l.largest);
    eprintln!(
        "4,000 lines: first frame, ahead, G and its frame in {:?}; largest image {} bytes",
        started.elapsed(),
        l.largest
    );
    assert!(started.elapsed().as_secs() < 20);
}

#[test]
fn a_pages_tiles_are_the_page_drawn_whole_cut_up() {
    let (mut l, st) = (look(CELL), stepper());
    let lay = Lay::new(SIZE.0, SIZE.1, 0);
    let (w, ch) = (lay.page_cols as u32 * CELL.0, CELL.1);
    let rows = l.page_rows(&st, 0, lay);
    let page = l.page_of(&st, 0);
    let mut whole = l.ink(w, rows as u32 * ch, 0.0).unwrap();
    page::draw(&mut whole, &page);
    let whole = whole.pm;
    let tile = lay.tile as i64;
    for t in 0..(rows + tile - 1) / tile {
        let n = (rows - t * tile).min(tile);
        let mut ink = l.ink(w, n as u32 * ch, (t * tile * ch as i64) as f32).unwrap();
        page::draw(&mut ink, &page);
        let cut = whole
            .clone_rect(tiny_skia::IntRect::from_xywh(0, (t * tile * ch as i64) as i32, w, n as u32 * ch).unwrap())
            .unwrap();
        assert!(
            ink.pm.data() == cut.data(),
            "tile {t} differs from the page drawn whole"
        );
    }
}

#[test]
fn an_image_that_cant_be_made_is_an_error_the_tour_falls_back_on() {
    let mut l = Look::new(
        Theme::default(),
        (5_000_000, 17),
        Type::new(Fonts::builtin()),
        Box::new(patch),
    );
    let e = l.frame(&mut stepper(), SIZE, &mut Vec::new()).unwrap_err();
    assert!(e.to_string().starts_with("can't make an image"), "{e}");
}

/// The bytes the keys that matter write, on a tour of three steps of 100 lines each, at
/// a pane `size` with a cell `cell`, drawing ahead between keys as the tour does: the
/// first frame, what's drawn ahead after it (and its largest image), j, a page down
/// (each with what was drawn ahead before it), and n drawn ahead and not.
fn bytes(size: (i64, i64), cell: (u32, u32)) -> [usize; 7] {
    let page = size.1 - 2;
    let look = || Look::new(Theme::default(), cell, Type::new(Fonts::builtin()), long(100));
    let (mut l, mut st) = (look(), stepper());
    let first = frame(&mut l, &mut st, size).len();
    let mut ahead = Vec::new();
    l.ahead(&st, &|| false, &mut ahead).unwrap();
    let ahead = String::from_utf8(ahead).unwrap();
    let largest = ahead.split("\x1b_Ga=t,").map(str::len).max().unwrap_or(0);
    let mut key = |l: &mut Look, st: &mut Stepper, k: &str| {
        st.key(k, page);
        let n = frame(l, st, size).len();
        l.ahead(st, &|| false, &mut Vec::new()).unwrap();
        n
    };
    let j = key(&mut l, &mut st, "j");
    let down = key(&mut l, &mut st, " ");
    st.key("g", page);
    frame(&mut l, &mut st, size);
    l.ahead(&st, &|| false, &mut Vec::new()).unwrap();
    let n = key(&mut l, &mut st, "n");
    let (mut cold, mut st2) = (look(), stepper());
    frame(&mut cold, &mut st2, size);
    st2.key("n", page);
    let n_cold = frame(&mut cold, &mut st2, size).len();
    [first, ahead.len(), largest, j, down, n, n_cold]
}

/// The bytes of the keys that matter, at a large floating pane (160×46) with a 2× cell.
#[test]
fn bytes_at_a_large_pane() {
    let size = (160, 46);
    let (mut l, mut st) = (look((16, 34)), stepper());
    frame(&mut l, &mut st, size);
    let mut ahead = Vec::new();
    l.ahead(&st, &|| false, &mut ahead).unwrap();
    st.key("j", 44);
    let j = frame(&mut l, &mut st, size).len();
    st.key("n", 44);
    let n = frame(&mut l, &mut st, size).len();
    let (mut cold, mut st2) = (look((16, 34)), stepper());
    frame(&mut cold, &mut st2, size);
    st2.key("n", 44);
    let n_cold = frame(&mut cold, &mut st2, size).len();
    assert!(j < 400 && n < 600 && n_cold > 10_000, "{j} {n} {n_cold}");
}

/// The bytes at a large floating pane (160×46 with a 2× cell) and the person's (217×65,
/// 19×37-pixel cells), on long steps, as docs/tours.md says them (half a minute in a
/// debug build: `bin/bullpen-cargo test -p bullpen-cli bytes_at_large -- --ignored
/// --nocapture`).
#[test]
#[ignore]
fn bytes_at_large_panes() {
    for (size, cell) in [((160, 46), (17, 34)), ((217, 65), (19, 37))] {
        let [first, ahead, largest, j, down, n, n_cold] = bytes(size, cell);
        eprintln!(
            "{}×{} at {}×{}: first frame {first} B, then drawn ahead {ahead} B (its largest image {largest} B); j {j} B, a page down {down} B, n {n} B (not drawn ahead {n_cold} B)",
            size.0, size.1, cell.0, cell.1
        );
        // at 217×65 the budget holds a page down and the next step's first tile, not its
        // whole first window: from the top of a long step, n sends the rest of it
        let n_most = if size.0 > 200 { n_cold * 2 / 3 } else { 2_000 };
        assert!(
            j < 400 && largest < 1_000_000 && n < n_most && n_cold > 100_000,
            "{j} {largest} {n} {n_cold}"
        );
    }
}

/// The person's pane: 211×65 cells of 19×37 pixels, a page 16 rows a tile.
const PANE: ((i64, i64), (u32, u32)) = ((211, 65), (19, 37));

/// A tour of three long steps at the person's pane, every byte fed to a terminal with
/// Rex's quota, keys pressed with drawing ahead between them as the tour does: the
/// bytes each key's frame wrote, and the terminal.
fn tour_on_rex(keys: &[&str], budget: usize) -> (Vec<usize>, Terminal, Look) {
    let (size, cell) = PANE;
    let mut l = Look::budgeted(Theme::default(), cell, Type::new(Fonts::builtin()), long(300), budget);
    let mut st = stepper();
    let mut term = Terminal::new(REX_QUOTA);
    let mut feed = |l: &mut Look, out: Vec<u8>| {
        term.feed(&String::from_utf8(out).unwrap());
        if budget <= REX_QUOTA {
            assert_eq!(l.kept_bytes(), term.bytes(), "the tour and the terminal keep the same");
        }
    };
    let mut sent = Vec::new();
    for k in std::iter::once("").chain(keys.iter().copied()) {
        if !k.is_empty() {
            st.key(k, size.1 - 2);
        }
        let mut out = Vec::new();
        l.frame(&mut st, size, &mut out).unwrap();
        sent.push(out.len());
        feed(&mut l, out);
        let mut out = Vec::new();
        l.ahead(&st, &|| false, &mut out).unwrap();
        feed(&mut l, out);
    }
    (sent, term, l)
}

/// n and p, and a long step paged down to its end and back up.
const BACK: [&str; 2] = ["n", "p"];
const DOWN_UP: [&str; 14] = [" ", " ", " ", " ", " ", " ", " ", "b", "b", "b", "b", "b", "b", "b"];

#[test]
fn going_back_a_step_places_only_images_the_terminal_has() {
    let (sent, term, l) = tour_on_rex(&BACK, shown::BUDGET);
    assert!(term.blanks.is_empty(), "placed but gone: {:?}", term.blanks);
    assert!(
        term.most <= shown::BUDGET && l.kept_bytes() <= shown::BUDGET,
        "{}",
        term.most
    );
    assert!(sent[1] < 2_000, "n only places: {sent:?}");
}

#[test]
fn a_long_step_paged_down_and_back_up_places_only_images_the_terminal_has() {
    let (sent, term, _) = tour_on_rex(&DOWN_UP, shown::BUDGET);
    assert!(term.blanks.is_empty(), "placed but gone: {:?}", term.blanks);
    assert!(term.most <= shown::BUDGET, "{}", term.most);
    // a page down only places; a page up past the tile over the window sends again
    // what was deleted for room
    assert!(
        sent[1..8].iter().all(|n| *n < 2_000),
        "each page down only places: {sent:?}"
    );
}

#[test]
fn a_budget_over_the_terminals_quota_leaves_blanks() {
    // what the tour did before it kept a budget: it placed images Rex had deleted
    let (_, term, _) = tour_on_rex(&BACK, usize::MAX);
    assert!(!term.blanks.is_empty(), "the fake terminal deletes past its quota");
}

/// The bytes of n then p, and of paging a long step down and back up, at the person's
/// pane, and the most kept (`bin/bullpen-cargo test -p bullpen-cli bytes_going_back --
/// --ignored --nocapture`).
#[test]
#[ignore]
fn bytes_going_back() {
    for (what, keys) in [("n, p", &BACK[..]), ("down 7 pages, up 7", &DOWN_UP[..])] {
        let (sent, term, _) = tour_on_rex(keys, shown::BUDGET);
        eprintln!(
            "{what}: frames {sent:?} B; most kept {} B; blanks {}",
            term.most,
            term.blanks.len()
        );
    }
}

/// Every kind of image at a pane far bigger than the person's (300×100 cells of 19×37
/// pixels): the rail, the tiles of a long page, the foot and the end page's tiles (what's
/// drawn ahead is these too).
#[test]
fn no_image_is_over_the_terminals_limit() {
    let (size, cell) = ((300, 100), (19, 37));
    let mut l = Look::new(Theme::default(), cell, Type::new(Fonts::builtin()), long(120));
    let mut st = stepper();
    let mut out = frame(&mut l, &mut st, size);
    st.key("G", size.1 - 2);
    out += &frame(&mut l, &mut st, size);
    st.go(3);
    out += &frame(&mut l, &mut st, size);
    let sizes = bullpen_draw::cut::uploaded(&out);
    assert!(sizes.len() > 10, "{sizes:?}");
    for (w, h) in &sizes {
        assert!(bullpen_draw::cut::fits(*w, *h), "an image {w}×{h}: {} bytes", w * h * 4);
    }
    let rail = Lay::new(size.0, size.1, 0).rail as u32 * cell.0;
    assert!(
        sizes.iter().filter(|(w, _)| *w == rail).count() >= 4,
        "two rails in pieces: {sizes:?}"
    );
}

/// At the person's pane a tile is a strip under the limit, a few to a window, and the
/// strips are the page drawn whole, cut up.
#[test]
fn a_pages_strips_are_the_page_drawn_whole_cut_up() {
    let (size, cell) = ((160, 46), (17, 34));
    let mut l = Look::new(Theme::default(), cell, Type::new(Fonts::builtin()), long(150));
    let st = stepper();
    let lay = Lay::new(size.0, size.1, 0).fitted(cell);
    assert!(lay.tile < lay.view && lay.tile > 20, "{lay:?}");
    let (w, ch) = (lay.page_cols as u32 * cell.0, cell.1);
    assert!(bullpen_draw::cut::fits(w, lay.tile as u32 * ch));
    let rows = l.page_rows(&st, 0, lay);
    let page = l.page_of(&st, 0);
    let mut whole = l.ink(w, rows as u32 * ch, 0.0).unwrap();
    page::draw(&mut whole, &page);
    let whole = whole.pm;
    let tile = lay.tile as i64;
    for t in 0..(rows + tile - 1) / tile {
        let n = (rows - t * tile).min(tile);
        let mut ink = l.ink(w, n as u32 * ch, (t * tile * ch as i64) as f32).unwrap();
        page::draw(&mut ink, &page);
        let cut = whole
            .clone_rect(tiny_skia::IntRect::from_xywh(0, (t * tile * ch as i64) as i32, w, n as u32 * ch).unwrap())
            .unwrap();
        assert!(
            ink.pm.data() == cut.data(),
            "strip {t} differs from the page drawn whole"
        );
    }
}

/// A picture of the tour as the terminal would show it: the rail, the window on the
/// page and the foot, over the pane's background.
fn picture(l: &mut Look, st: &mut Stepper, (cols, rows): (i64, i64)) -> Pixmap {
    let mut sink = Vec::new();
    l.frame(st, (cols, rows), &mut sink).unwrap();
    let lay = Lay::new(cols, rows, 0);
    let (cw, ch) = l.cell;
    let mut pm = Pixmap::new(cols as u32 * cw, rows as u32 * ch).unwrap();
    let bg = l.th.bg;
    pm.fill(tiny_skia::Color::from_rgba8(bg.0, bg.1, bg.2, 255));
    let paint = tiny_skia::PixmapPaint::default();
    let id = tiny_skia::Transform::identity();
    if lay.rail > 0 {
        let comments = (0..st.steps.len()).map(|k| Look::on_step(st, k).len()).collect();
        let r = rail::Rail {
            tid: st.tid.clone(),
            title: st.title.clone(),
            steps: st.steps.iter().map(|o| s(o, "title").to_string()).collect(),
            at: st.at,
            comments,
        };
        let mut ink = Ink::new(lay.rail as u32 * cw, rows as u32 * ch, &mut l.ty, l.th, l.sz).unwrap();
        rail::draw(&mut ink, &r);
        pm.draw_pixmap(0, 0, ink.pm.as_ref(), &paint, id, None);
    }
    let page_rows = l.page_rows(st, st.at, lay);
    let page = l.page_of(st, st.at);
    let mut window = l
        .ink(
            lay.page_cols as u32 * cw,
            lay.view as u32 * ch,
            (st.top * ch as i64) as f32,
        )
        .unwrap();
    page::draw(&mut window, &page);
    let window = window.pm;
    pm.draw_pixmap((lay.rail as u32 * cw) as i32, 0, window.as_ref(), &paint, id, None);
    let f = rail::Foot {
        msg: st.msg.clone(),
        more: page_rows - st.top - lay.view as i64 > 0,
        end: st.at_end(),
    };
    let mut ink = Ink::new(lay.page_cols as u32 * cw, FOOT as u32 * ch, &mut l.ty, l.th, l.sz).unwrap();
    rail::foot(&mut ink, &f, ch as f32);
    pm.draw_pixmap(
        (lay.rail as u32 * cw) as i32,
        ((rows as u32 - FOOT as u32) * ch) as i32,
        ink.pm.as_ref(),
        &paint,
        id,
        None,
    );
    pm
}

/// "217x65" as (217, 65), or the default when it's not set.
fn pair(var: &str, default: (u32, u32)) -> (u32, u32) {
    std::env::var(var)
        .ok()
        .and_then(|v| v.split_once('x').map(|(a, b)| (a.parse().ok(), b.parse().ok())))
        .and_then(|(a, b)| Some((a?, b?)))
        .unwrap_or(default)
}

/// Pictures of a tour from a fixture (TOUR_FIXTURE: a tour's steps, each with its
/// "patch", and comments) to TOUR_RENDER, dark and light, at a pane TOUR_PANE cells
/// (160x46) of TOUR_CELL pixels (16x34).
#[test]
#[ignore]
fn render_tour() {
    let (Ok(fixture), Ok(dir)) = (std::env::var("TOUR_FIXTURE"), std::env::var("TOUR_RENDER")) else {
        return;
    };
    let (cols, rows) = pair("TOUR_PANE", (160, 46));
    let cell = pair("TOUR_CELL", (16, 34));
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    let steps: Vec<Value> = fx["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| json!({"title": s["title"], "note": s["note"], "hunks": s["refs"]}))
        .collect();
    let patches: Vec<String> = fx["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["patch"].as_str().unwrap().to_string())
        .collect();
    let tour = as_obj(json!({"commit": "c1", "steps": steps}));
    let comments: Vec<Obj> = fx["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let mut c = as_obj(c.clone());
            c.insert("commit".into(), json!("c1"));
            c
        })
        .collect();
    for (name, th) in [("dark", Theme::default()), ("light", Theme::light())] {
        for (shot, at, top) in [("step1", 0, 0), ("step1-down", 0, 30), ("step2", 1, 0), ("end", 99, 0)] {
            let patches = patches.clone();
            let mut l = Look::new(
                th,
                cell,
                Type::new(Fonts::builtin()),
                Box::new(move |i| patches[i].clone()),
            );
            let mut st = Stepper::new(
                fx["tid"].as_str().unwrap(),
                fx["title"].as_str().unwrap(),
                &tour,
                comments.clone(),
                at,
                Box::new(|_, _| vec![]),
            );
            st.top = top;
            let pm = picture(&mut l, &mut st, (cols as i64, rows as i64));
            std::fs::write(format!("{dir}/{shot}-{name}.png"), png::encode(&pm)).unwrap();
        }
    }
}
