use super::*;
use crate::image::render::{self, render, Cut, Grid};
use crate::image::sprite::{self, Sprite};
use crate::image::Theme;
use crate::model::{Config, Look, TextSize};
use crate::ui::testing::{asking, asks, json, ready, screen, task};
use crate::App;
use tiny_skia::{IntRect, Pixmap, PixmapPaint, Transform};

const W: u16 = 40;
const H: u16 = 24;

fn canvas(size: TextSize) -> Canvas {
    Canvas::new(Grid { cols: W as usize - 1, cell_w: 10, cell_h: 20 }, size, Theme::default())
}

/// A board with tasks of every kind: some with long titles and lines, one working, one
/// in review, one with a question. Grouped, it lists BP-1 4 2 6 5 3 7 8 9.
fn board() -> App {
    let long = "A title long enough that the selected task's wraps to a second line and more";
    let tasks = vec![
        asking("BP-1"),
        ready("BP-2").with(json!({"title": long, "diffstat": "3 files, +13 −16"})),
        task("BP-3", "working").with(json!({"title": "Working on it"})),
        asks("BP-4"),
        task("BP-5", "review").with(json!({"title": long})),
        ready("BP-6"),
        task("BP-7", "todo").with(json!({"title": long})),
        task("BP-8", "todo").with(json!({"title": "Task 8"})),
        task("BP-9", "todo").with(json!({"title": "Task 9"})),
    ];
    let mut a = App::for_test(tasks);
    a.cfg = Config::default_for_test().with(json!({"board_bottom_gap": 0}));
    a.look = Look::Image;
    a
}

/// The board's view with `sel` selected, as the image look lays it out at `size`.
fn view_of(app: &mut App, sel: &str, size: TextSize) -> View {
    app.sel = Some(sel.into());
    app.image = true;
    app.detail_cols = Some(render::detail_cols(&canvas(size), &Fonts::builtin()));
    screen(app, W, H);
    app.view.clone().unwrap()
}

/// Rows [from, from + n) of cells of an image.
fn cells(pm: &Pixmap, from: u32, n: u32, ch: u32) -> Pixmap {
    pm.clone_rect(IntRect::from_xywh(0, (from * ch) as i32, pm.width(), n * ch).unwrap()).unwrap()
}

/// The list as the terminal shows it: the base image's crops, the selected block's
/// image, the tail's, and (`bare`) each mark's frame placed over them.
fn put_together(view: &View, p: &Plan, c: &Canvas, bare: bool) -> Pixmap {
    let mut fonts = Fonts::builtin();
    let ch = c.grid.cell_h;
    let room = view.rows_n as u32 * ch;
    let is_bare = |row: &Row| super::bare(row, bare, &|_| false);
    let mut pm = Pixmap::new(c.grid.width(), room).unwrap();
    let mut put = |img: &Pixmap, y: u32| pm.draw_pixmap(0, y as i32, img.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    let rows = if p.whole { &view.rows } else { &view.base };
    let b = |i: usize| is_bare(&rows.rows[i]);
    let base = render(rows, &p.base, Cut { range: p.top..p.base_end, y: 0, h: room, marks: Marks::default(), bare: &b }, c, &mut fonts).unwrap().pixmap;
    for &(from, at, n) in &p.crops {
        put(&cells(&base, from as u32, n as u32, ch), at as u32 * ch);
    }
    let d = |i: usize| is_bare(&view.rows.rows[i]);
    for (first, n) in [p.sel, p.tail].into_iter().flatten() {
        let cut = Cut { range: p.top..p.end, y: first as u32 * ch, h: n as u32 * ch, marks: Marks::default(), bare: &d };
        put(&render(&view.rows, &p.laid, cut, c, &mut fonts).unwrap().pixmap, first as u32 * ch);
    }
    for i in p.top..p.end {
        let row = &view.rows.rows[i];
        if !is_bare(row) {
            continue;
        }
        let mark = sprite_mark(row).unwrap();
        let strip = sprite::draw(&Sprite::Mark(mark), c, &mut fonts).unwrap();
        let at = sprite::at(c, mark_y(p, i, c), 0, view.rows_n).unwrap();
        let w = sprite::COLS as u32 * c.grid.cell_w;
        let frame = strip.clone_rect(IntRect::from_xywh(0, at.crop_y as i32, w, at.crop_h).unwrap()).unwrap();
        put(&frame, at.row as u32 * ch);
    }
    pm
}

/// The list drawn whole, as one image: what the pieces must come to.
fn whole(view: &View, p: &Plan, c: &Canvas) -> Pixmap {
    let room = view.rows_n as u32 * c.grid.cell_h;
    render(&view.rows, &p.laid, Cut::rows(p.top..p.end, room), c, &mut Fonts::builtin()).unwrap().pixmap
}

/// The most two images' channels differ by.
fn most_apart(a: &Pixmap, b: &Pixmap) -> u8 {
    a.data().iter().zip(b.data()).map(|(x, y)| x.abs_diff(*y)).max().unwrap()
}

#[test]
fn the_pieces_put_together_are_the_list_drawn_whole() {
    for size in [TextSize::Small, TextSize::Medium, TextSize::Large] {
        let c = canvas(size);
        let mut app = board();
        for sel in ["BP-1", "BP-2", "BP-3", "BP-5", "BP-7", "BP-9"] {
            let view = view_of(&mut app, sel, size);
            let p = plan(&view, &c, &Fonts::builtin());
            app.top = p.top;
            assert!(p.sel.is_some(), "{sel} at {size:?}: the selected block is an image of its own");
            assert_eq!(most_apart(&put_together(&view, &p, &c, false), &whole(&view, &p, &c)), 0, "{sel} at {size:?}");
        }
    }
}

#[test]
fn the_marks_placed_over_the_list_are_where_it_draws_them() {
    for size in [TextSize::Small, TextSize::Medium, TextSize::Large] {
        let c = canvas(size);
        let mut app = board();
        let view = view_of(&mut app, "BP-3", size);
        let p = plan(&view, &c, &Fonts::builtin());
        // BP-3's working mark and BP-5's review mark (above it) are sprites; their first
        // frame is the still mark
        assert!(view.rows.rows[p.top..p.end].iter().filter(|r| bare(r, true, &|_| false)).count() >= 2);
        // a mark laid over the image is smoothed a shade differently from one drawn in it;
        // one a pixel out would be hundreds of levels apart
        assert!(most_apart(&put_together(&view, &p, &c, true), &whole(&view, &p, &c)) <= 8, "{size:?}");
    }
}

#[test]
fn moving_the_selection_keeps_the_base_image_and_moves_its_crops() {
    let c = canvas(TextSize::Medium);
    let mut app = board();
    let a = plan(&view_of(&mut app, "BP-2", TextSize::Medium), &c, &Fonts::builtin());
    app.top = a.top;
    // the next task down (the board groups them: BP-6 is under BP-2)
    let view = view_of(&mut app, "BP-6", TextSize::Medium);
    let b = plan(&view, &c, &Fonts::builtin());
    assert_eq!((a.top, a.base_end, &a.base), (b.top, b.base_end, &b.base));
    assert!(!a.whole && !b.whole);
    assert_ne!(a.crops, b.crops);
    assert_ne!(a.sel, b.sel);
    // the block's image is a few cells; the crops and it cover the list's cells once each
    let (first, n) = b.sel.unwrap();
    assert!(n < view.rows_n / 2 + 4, "{n} cells");
    let mut covered = vec![0; view.rows_n as usize];
    for (at, n) in b.crops.iter().map(|&(_, at, n)| (at, n)).chain([(first, n)]).chain(b.tail) {
        (at..at + n).for_each(|k| covered[k as usize] += 1);
    }
    assert!(covered.iter().all(|k| *k <= 1));
}

#[test]
fn the_rows_under_the_block_move_by_whole_cells() {
    let c = canvas(TextSize::Medium);
    let mut app = board();
    let view = view_of(&mut app, "BP-2", TextSize::Medium);
    let p = plan(&view, &c, &Fonts::builtin());
    let s0 = view.rows.rows.iter().position(Row::is_selected).unwrap();
    let after = s0 + 1 + view.rows.rows[s0 + 1..].iter().take_while(|r| r.is_detail()).count();
    let b1 = s0 + 1 + view.base.rows[s0 + 1..].iter().take_while(|r| r.is_detail()).count();
    // the row after the block is lower than in the base image by whole cells, and the space is under the block
    let moved = p.y(after) - p.base.tall(p.top, b1);
    assert!(moved > 0 && moved % c.grid.cell_h == 0, "{moved}");
    assert!(p.laid.after[after - 1] > 0);
}

#[test]
fn with_nothing_selected_in_view_the_base_image_is_the_list() {
    let c = canvas(TextSize::Medium);
    let mut app = App::for_test(vec![]);
    app.look = Look::Image;
    let view = view_of(&mut app, "BP-1", TextSize::Medium);
    let p = plan(&view, &c, &Fonts::builtin());
    assert!(p.whole && p.sel.is_none() && p.tail.is_none());
    assert_eq!(p.crops, vec![(0, 0, view.rows_n)]);
}


