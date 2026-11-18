use super::*;
use crate::image::theme;
use crate::model::TextSize;
use crate::ui::rows::{DetailRow, TaskRow};

const GRID: Grid = Grid { cols: 40, cell_w: 10, cell_h: 20 };

fn task(id: &str, status: &str) -> TaskRow {
    TaskRow {
        id: id.into(),
        title: format!("Task {id}"),
        mark: Mark::Still(status.into()),
        from_backlog: false,
        flag: None,
        age: "3m".into(),
        risk: None,
        stale: false,
        selected: false,
        asked: None,
        quiet: false,
    }
}

fn detail(text: &str, selected: bool) -> Row {
    Row::Detail(DetailRow { text: text.into(), loud: false, lead: 0, selected, stale: false, flag: None })
}

fn rows(rows: Vec<Row>) -> Rows {
    Rows { rows, idw: 5 }
}

fn canvas(size: TextSize) -> Canvas {
    Canvas::new(GRID, size, Theme::default())
}

/// All the rows, in a type size, in an image as tall as they are.
fn draw_in(r: &Rows, size: TextSize) -> (Laid, Sheet) {
    let (c, mut fonts) = (canvas(size), Fonts::builtin());
    let laid = lay(r, &c, &fonts);
    let h: u32 = laid.heights.iter().sum();
    let sheet = render(r, &laid, Cut::rows(0..r.rows.len(), h), &c, &mut fonts).unwrap();
    (laid, sheet)
}

/// Small type: a row a cell, as the text list's.
fn draw(r: &Rows) -> Sheet {
    draw_in(r, TextSize::Small).1
}

/// The pixels of row i (straight RGBA).
fn row_px(s: &Sheet, i: usize) -> Vec<[u8; 4]> {
    let (w, h) = (s.pixmap.width() as usize, GRID.cell_h as usize);
    s.pixmap.pixels()[i * h * w..(i + 1) * h * w]
        .iter()
        .map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

fn has(px: &[[u8; 4]], c: theme::Rgb) -> bool {
    px.iter().any(|p| p[3] == 255 && (p[0], p[1], p[2]) == (c.0, c.1, c.2))
}

#[test]
fn the_image_is_the_lists_columns_wide_and_a_cell_high_a_row() {
    let r = rows(vec![Row::Head { name: "Ready".into(), count: 1 }, Row::Task(task("BP-1", "ready")), Row::Gap]);
    let s = draw(&r);
    assert_eq!((s.pixmap.width(), s.pixmap.height()), (400, 60));
}

#[test]
fn each_row_sits_a_cell_under_the_last_and_a_task_row_says_its_id() {
    let r = rows(vec![Row::Head { name: "Ready".into(), count: 2 }, Row::Task(task("BP-1", "ready")), detail("x", false), Row::Task(task("BP-2", "ready"))]);
    let s = draw(&r);
    let at: Vec<(usize, u32, u32, Option<&str>)> = s.places.iter().map(|p| (p.row, p.y, p.h, p.id.as_deref())).collect();
    assert_eq!(at, vec![(0, 0, 20, None), (1, 20, 20, Some("BP-1")), (2, 40, 20, None), (3, 60, 20, Some("BP-2"))]);
}

#[test]
fn a_selected_row_differs_from_the_same_row_unselected() {
    let plain = draw(&rows(vec![Row::Task(task("BP-1", "todo"))]));
    let selected = draw(&rows(vec![Row::Task(TaskRow { selected: true, ..task("BP-1", "todo") })]));
    assert_ne!(row_px(&plain, 0), row_px(&selected, 0));
    // and its block: the detail under it sits on the tint too
    let a = draw(&rows(vec![Row::Task(task("BP-1", "todo")), detail("what it does", false)]));
    let b = draw(&rows(vec![Row::Task(TaskRow { selected: true, ..task("BP-1", "todo") }), detail("what it does", true)]));
    assert_ne!(row_px(&a, 1), row_px(&b, 1));
}

#[test]
fn a_risk_is_drawn_in_its_colour_and_a_status_in_its_own() {
    let theme = Theme::default();
    let high = draw(&rows(vec![Row::Task(TaskRow { risk: Some("high".into()), ..task("BP-1", "todo") })]));
    assert!(has(&row_px(&high, 0), theme.red));
    let low = draw(&rows(vec![Row::Task(TaskRow { risk: Some("low".into()), ..task("BP-1", "todo") })]));
    assert!(has(&row_px(&low, 0), theme.green) && !has(&row_px(&low, 0), theme.red));
    // a task that needs you: an amber mark and an amber bar
    let needs = draw(&rows(vec![Row::Task(TaskRow { flag: Some(Flag::Needs), ..task("BP-1", "needs") })]));
    assert!(has(&row_px(&needs, 0), theme.yellow));
    let review = draw(&rows(vec![Row::Task(TaskRow { mark: Mark::Review, ..task("BP-1", "review") })]));
    assert!(has(&row_px(&review, 0), theme.blue));
}

#[test]
fn the_colours_are_the_terminals() {
    let mut theme = Theme::default();
    theme.red = theme::Rgb(200, 10, 20);
    let r = rows(vec![Row::Task(TaskRow { risk: Some("high".into()), ..task("BP-1", "todo") })]);
    let c = Canvas::new(GRID, TextSize::Small, theme);
    let laid = lay(&r, &c, &Fonts::builtin());
    let s = render(&r, &laid, Cut::rows(0..1, 20), &c, &mut Fonts::builtin()).unwrap();
    assert!(has(&row_px(&s, 0), theme.red));
}

#[test]
fn the_rows_asked_for_are_drawn_from_the_images_top_in_an_image_the_height_asked_for() {
    let many: Vec<Row> = (1..=50).map(|n| Row::Task(task(&format!("BP-{n}"), "todo"))).collect();
    let r = rows(many);
    let c = canvas(TextSize::Small);
    let laid = lay(&r, &c, &Fonts::builtin());
    let part = render(&r, &laid, Cut::rows(10..20, 240), &c, &mut Fonts::builtin()).unwrap();
    assert_eq!(part.pixmap.height(), 240);
    assert_eq!(part.places.len(), 10);
    assert_eq!((part.places[0].id.as_deref(), part.places[0].y), (Some("BP-11"), 0));
}

#[test]
fn larger_type_makes_taller_rows_with_space_between_tasks_and_a_smaller_detail() {
    let r = rows(vec![Row::Head { name: "Ready".into(), count: 1 }, Row::Task(task("BP-1", "ready")), detail("what it does", false)]);
    let small = Metrics::of(TextSize::Small, 20);
    let (medium, large) = (Metrics::of(TextSize::Medium, 20), Metrics::of(TextSize::Large, 20));
    assert!(small.title < medium.title && medium.title < large.title);
    for m in [medium, large] {
        assert!(m.detail < m.title && m.line > 20.0 && m.pad > 0.0);
    }
    let (laid, sheet) = draw_in(&r, TextSize::Medium);
    let task_h = laid.heights[1];
    assert_eq!(task_h, (medium.pad + medium.line + medium.title_gap) as u32);
    assert!(task_h > 20 && task_h % 20 != 0); // not a whole number of cells
    assert!(laid.heights[2] < task_h);
    assert_eq!(sheet.places[2].y, laid.heights[0] + task_h);
    // small is the text list's: a cell a row
    assert_eq!(draw_in(&r, TextSize::Small).0.heights, vec![20, 20, 20]);
}

#[test]
fn the_selected_tasks_title_wraps_and_another_is_cut_to_its_row() {
    let long = "a title much too long for one row of a board this narrow, which goes on";
    let other = TaskRow { title: long.into(), ..task("BP-1", "todo") };
    let selected = TaskRow { title: long.into(), selected: true, ..task("BP-2", "todo") };
    let r = rows(vec![Row::Task(other), Row::Task(selected)]);
    let (laid, _) = draw_in(&r, TextSize::Medium);
    let m = Metrics::of(TextSize::Medium, 20);
    assert_eq!(laid.titles[0].len(), 1);
    assert!(laid.titles[0][0].ends_with('…'));
    assert!(laid.titles[1].len() > 1 && laid.titles[1].len() <= TITLE_LINES);
    assert_eq!(laid.heights[1], (m.pad + laid.titles[1].len() as f32 * m.line + m.title_gap) as u32);
    let joined = laid.titles[1].join(" ");
    assert!(long.starts_with(joined.trim_end_matches('…')));
}

#[test]
fn the_rows_that_fit_are_whole_rows_and_one_at_least() {
    let laid = Laid { heights: vec![30, 30, 30, 30], after: vec![0; 4], titles: vec![vec![]; 4], idw: 0.0 };
    assert_eq!(fitting(&laid, 0, 100), 3);
    assert_eq!(fitting(&laid, 2, 100), 4);
    assert_eq!(fitting(&laid, 1, 10), 2); // too tall to fit: cut, but drawn
}

#[test]
fn the_scroll_marks_are_drawn_in_the_last_column() {
    let r = rows(vec![Row::Task(task("BP-1", "todo"))]);
    let (c, mut fonts) = (canvas(TextSize::Medium), Fonts::builtin());
    let laid = lay(&r, &c, &fonts);
    let last_col = |s: &Sheet, (y0, y1): (u32, u32)| {
        let w = s.pixmap.width();
        (y0..y1).any(|y| (w - 10..w).any(|x| s.pixmap.pixels()[(y * w + x) as usize].alpha() > 0))
    };
    let none = render(&r, &laid, Cut::rows(0..1, 100), &c, &mut fonts).unwrap();
    assert!(!last_col(&none, (0, 100)));
    let both = render(&r, &laid, Cut { marks: Marks { up: true, down: true }, ..Cut::rows(0..1, 100) }, &c, &mut fonts).unwrap();
    assert!(last_col(&both, (0, 20)) && last_col(&both, (80, 100)));
}

#[test]
fn a_detail_line_holds_more_columns_than_the_texts_where_the_type_is_narrower() {
    let fonts = Fonts::builtin();
    // the text list's detail lines hold the board's columns less 5: here 36
    let cols = |size| detail_cols(&Canvas::new(Grid { cols: 40, cell_w: 10, cell_h: 22 }, size, Theme::default()), &fonts);
    assert!(cols(TextSize::Small) > cols(TextSize::Medium) && cols(TextSize::Medium) > cols(TextSize::Large));
    assert!(cols(TextSize::Large) > 20);
}

#[test]
fn the_background_is_clear_so_the_panes_own_shows() {
    let s = draw(&rows(vec![Row::Gap, Row::Task(task("BP-1", "todo"))]));
    assert!(row_px(&s, 0).iter().all(|p| p[3] == 0));
    assert!(row_px(&s, 1).iter().any(|p| p[3] > 0));
}

#[test]
fn a_title_too_long_for_the_row_is_cut_with_an_ellipsis() {
    let f = Fonts::builtin();
    let long = "a title much too long to fit in the room a narrow board has for it";
    let cut = f.fit(long, Weight::Regular, 12.0, 100.0);
    assert!(cut.ends_with('…') && cut.len() < long.len());
    assert!(f.width(&cut, Weight::Regular, 12.0) <= 100.0);
    assert_eq!(f.fit("short", Weight::Regular, 12.0, 100.0), "short");
}

#[test]
fn a_title_wraps_at_its_spaces_and_its_last_line_is_cut() {
    let f = Fonts::builtin();
    let text = "one two three four five six seven eight nine ten eleven twelve";
    let lines = f.wrap(text, Weight::Regular, 12.0, (60.0, 90.0), 3);
    assert_eq!(lines.len(), 3);
    assert!(f.width(&lines[0], Weight::Regular, 12.0) <= 60.0);
    assert!(lines[1..].iter().all(|l| f.width(l, Weight::Regular, 12.0) <= 90.0));
    assert!(lines[2].ends_with('…') && !lines[0].ends_with('…'));
    assert_eq!(f.wrap("short", Weight::Regular, 12.0, (60.0, 90.0), 3), vec!["short"]);
}

/// The straight RGBA of pixel (x, y).
fn px_at(s: &Sheet, x: u32, y: u32) -> [u8; 4] {
    let c = s.pixmap.pixel(x, y).unwrap().demultiply();
    [c.red(), c.green(), c.blue(), c.alpha()]
}

#[test]
fn the_selected_card_holds_its_mark_and_clears_its_type() {
    let r = rows(vec![Row::Task(TaskRow { selected: true, ..task("BP-1", "todo") }), detail("what it does", true), Row::Task(task("BP-2", "todo"))]);
    let (laid, s) = draw_in(&r, TextSize::Medium);
    let m = canvas(TextSize::Medium).m;
    let (first, last) = (m.pad as u32, laid.heights[0] + laid.heights[1]);
    // its mark is inside it: the card starts a quarter of a cell in, left of the mark
    let mid = first + m.line as u32 / 2;
    assert!(px_at(&s, 4, mid)[3] > 0, "the card at its left edge");
    assert_eq!(px_at(&s, 1, mid)[3], 0, "nothing left of the card");
    // it reaches past its first and last lines by `inset`, and no further
    let (top, bottom) = (first - m.inset as u32, last + m.inset as u32);
    assert!(m.inset >= (0.28 * m.title).floor());
    let x = 100;
    assert!(px_at(&s, x, top + 1)[3] > 0 && px_at(&s, x, bottom - 2)[3] > 0);
    assert_eq!((px_at(&s, x, top - 2)[3], px_at(&s, x, bottom + 1)[3]), (0, 0));
}

#[test]
fn the_selected_id_has_no_pill_but_is_in_the_foreground() {
    let theme = Theme::default();
    let s = draw(&rows(vec![Row::Task(TaskRow { selected: true, ..task("BP-1", "todo") })]));
    let id = |s: &Sheet| row_px(s, 0).iter().enumerate().filter(|(i, _)| (30..52).contains(&(i % 400))).map(|(_, p)| *p).collect::<Vec<_>>();
    // the pill was the foreground behind the ID's letters in the background's colour
    assert!(!has(&id(&s), theme.bg));
    assert!(has(&id(&s), theme.fg));
    let plain = draw(&rows(vec![Row::Task(task("BP-1", "todo"))]));
    assert!(!has(&id(&plain), theme.fg));
}

#[test]
fn medium_type_has_room_above_each_task_and_between_its_title_and_lines() {
    let m = Metrics::of(TextSize::Medium, 34);
    assert!(m.pad >= 0.7 * m.title && m.title_gap > 0.0);
    assert!(m.line >= 1.35 * m.title && m.detail_line >= 1.4 * m.detail);
    // small stays a cell a row, its card a pixel inside its rows
    let s = Metrics::of(TextSize::Small, 34);
    assert_eq!((s.pad, s.title_gap, s.line, s.inset), (0.0, 0.0, 34.0, -1.0));
}

#[test]
fn the_title_leads_its_row_the_id_and_age_are_a_details_size() {
    let r = rows(vec![Row::Task(task("BP-123", "todo"))]);
    let (c, fonts) = (canvas(TextSize::Medium), Fonts::builtin());
    let laid = lay(&r, &c, &fonts);
    assert_eq!(laid.idw, fonts.width("BP-123", Weight::Bold, c.m.detail));
    let g = Geo { c, idw: laid.idw };
    let t = task("BP-123", "todo");
    assert_eq!(g.right_side(&t, &fonts), fonts.width(&t.age, Weight::Bold, c.m.detail));
    // an unselected ID is dim, never the foreground
    let s = draw(&r);
    let id: Vec<_> = row_px(&s, 0).iter().enumerate().filter(|(i, _)| (30..52).contains(&(i % 400))).map(|(_, p)| *p).collect();
    assert!(id.iter().any(|p| p[3] > 0) && id.iter().all(|p| p[3] < 255 || (p[0], p[1], p[2]) != (0xe5, 0xe5, 0xe5)));
}

#[test]
fn a_groups_name_is_a_little_dim_so_its_titles_lead() {
    let s = draw(&rows(vec![Row::Head { name: "Ready".into(), count: 1 }]));
    let px = row_px(&s, 0);
    let theme = Theme::default();
    assert!(!has(&px, theme.fg));
    assert!(px.iter().any(|p| p[3] == theme.heading()[3]));
}
