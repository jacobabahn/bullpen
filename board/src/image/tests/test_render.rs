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
        yours: false,
        key: None,
    }
}

fn detail(text: &str, selected: bool) -> Row {
    Row::Detail(DetailRow { text: text.into(), lines: 1, loud: false, lead: 0, selected, stale: false, flag: None })
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
    assert_eq!(laid.lines[0].len(), 1);
    assert!(laid.lines[0][0].ends_with('…'));
    assert!(laid.lines[1].len() > 1 && laid.lines[1].len() <= TITLE_LINES);
    assert_eq!(laid.heights[1], (m.pad + laid.lines[1].len() as f32 * m.line + m.title_gap) as u32);
    let joined = laid.lines[1].join(" ");
    assert!(long.starts_with(joined.trim_end_matches('…')));
}

#[test]
fn the_rows_that_fit_are_whole_rows_and_one_at_least() {
    let laid = Laid { heights: vec![30, 30, 30, 30], after: vec![0; 4], lines: vec![vec![]; 4], idw: 0.0 };
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
fn a_detail_is_wrapped_by_pixels_no_line_breaking_with_room_for_its_next_word() {
    let fonts = Fonts::builtin();
    let text = "Asks: should the inbox show a coordinator's question above the tasks that need you, or in its repo's group? It can't tell.";
    for size in [TextSize::Small, TextSize::Medium, TextSize::Large] {
        let c = canvas(size);
        let r = rows(vec![Row::Task(task("BP-1", "needs")), Row::Detail(DetailRow { lines: 3, ..detail_row(text) })]);
        let laid = lay(&r, &c, &fonts);
        let lines = &laid.lines[1];
        // as many lines as it needs, three at most, the row as tall as they are
        assert!(lines.len() > 1 && lines.len() <= 3, "{size:?}: {lines:?}");
        assert_eq!(laid.heights[1], (lines.len() as f32 * c.m.detail_line).round() as u32);
        let room = c.right() - Geo { c, idw: laid.idw }.text_x();
        for (k, line) in lines.iter().enumerate() {
            assert!(fonts.width(line, Weight::Regular, c.m.detail) <= room, "{size:?}: {line}");
            // the next line's first word didn't fit on this one
            if let Some(next) = lines.get(k + 1).and_then(|l| l.split(' ').next()) {
                let with = format!("{line} {next}");
                assert!(fonts.width(&with, Weight::Regular, c.m.detail) > room, "{size:?}: {with}");
            }
        }
    }
}

#[test]
fn a_word_longer_than_a_line_is_broken_where_the_line_ends() {
    let f = Fonts::builtin();
    let path = "board/src/image/a_very_long_file_name_that_wont_fit_on_one_line.rs";
    let lines = f.wrap(&format!("Reading {path} now"), Weight::Regular, 12.0, (120.0, 120.0), 4);
    assert!(lines.iter().all(|l| f.width(l, Weight::Regular, 12.0) <= 120.0), "{lines:?}");
    assert!(lines.concat().replace(' ', "").starts_with(&format!("Reading{}", &path[..20])), "{lines:?}");
}

fn detail_row(text: &str) -> DetailRow {
    DetailRow { text: text.into(), lines: 1, loud: false, lead: 0, selected: false, stale: false, flag: None }
}

#[test]
fn a_title_thats_yours_takes_two_lines_at_any_width() {
    let fonts = Fonts::builtin();
    let long = "Mission control: show each repo's coordinator question in the inbox";
    let r = rows(vec![
        Row::Task(TaskRow { title: long.into(), yours: true, ..task("BP-1", "needs") }),
        Row::Task(TaskRow { title: long.into(), ..task("BP-2", "working") }),
    ]);
    for cols in [35, 42, 43, 47] {
        let c = Canvas::new(Grid { cols, cell_w: 16, cell_h: 34 }, TextSize::Medium, Theme::default());
        let laid = lay(&r, &c, &fonts);
        assert_eq!(laid.lines[0].len(), 2, "{cols}");
        assert_eq!(laid.lines[1].len(), 1, "{cols}: another's is cut to one");
        assert!(laid.heights[0] > laid.heights[1]);
    }
    // with room for it on one line, one
    let c = Canvas::new(Grid { cols: 79, cell_w: 16, cell_h: 34 }, TextSize::Medium, Theme::default());
    assert_eq!(lay(&r, &c, &fonts).lines[0], vec![long.to_string()]);
    // small type keeps a cell a row, as the text list
    let c = Canvas::new(Grid { cols: 35, cell_w: 16, cell_h: 34 }, TextSize::Small, Theme::default());
    assert_eq!(lay(&r, &c, &fonts).lines[0].len(), 1);
}

#[test]
fn a_cut_leaves_no_stub_of_a_word_and_no_comma_before_its_ellipsis() {
    let f = Fonts::builtin();
    let w = |s: &str| f.width(s, Weight::Regular, 12.0);
    // room for "one two, t" and the ellipsis: the cut goes back to the word before, and its comma goes
    assert_eq!(f.fit("one two, three four", Weight::Regular, 12.0, w("one two, t…") + 0.5), "one two…");
    // most of a long word fits: it's cut in the word
    assert_eq!(f.fit("one background jobs", Weight::Regular, 12.0, w("one backgrou…") + 0.5), "one backgrou…");
    // a single word is cut where it must be
    assert_eq!(f.fit("background", Weight::Regular, 12.0, w("bac…") + 0.5), "bac…");
}

#[test]
fn a_wrapped_texts_last_word_isnt_left_alone() {
    let f = Fonts::builtin();
    let w = |s: &str| f.width(s, Weight::Regular, 12.0);
    let room = w("probably with a question. p shows its") + 0.5;
    let lines = f.wrap("probably with a question. p shows its screen.", Weight::Regular, 12.0, (room, room), 3);
    assert_eq!(lines, vec!["probably with a question. p shows", "its screen."]);
    // a line of two words keeps both
    let room = w("shows its") + 0.5;
    assert_eq!(f.wrap("shows its screen.", Weight::Regular, 12.0, (room, room), 3), vec!["shows its", "screen."]);
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
    // its mark is inside it: the card starts three quarters of a cell in, left of the mark
    let mid = first + m.line as u32 / 2;
    assert!(px_at(&s, 9, mid)[3] > 0, "the card at its left edge");
    assert_eq!(px_at(&s, 6, mid)[3], 0, "nothing left of the card");
    // as far from the image's right edge as from its left
    let (x0, x1, _, _) = drawn_box(&s);
    assert_eq!((x0, s.pixmap.width() - 1 - x1), (8, 8));
    // it reaches past its first and last lines by `inset`, and no further
    let (top, bottom) = (first - m.inset as u32, last + m.inset as u32);
    assert!(m.inset >= (0.25 * m.title).floor());
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
    assert_eq!((s.pad, s.title_gap, s.line, s.inset, s.cells), (0.0, 0.0, 34.0, -1.0, true));
}

#[test]
fn the_title_leads_its_row_the_id_and_age_are_a_details_size() {
    let r = rows(vec![Row::Task(task("BP-123", "todo"))]);
    let (c, fonts) = (canvas(TextSize::Medium), Fonts::builtin());
    let laid = lay(&r, &c, &fonts);
    assert_eq!(laid.idw, fonts.width("BP-123", Weight::Bold, c.m.detail));
    let g = Geo { c, idw: laid.idw };
    let t = task("BP-123", "todo");
    assert_eq!(g.right_side(&t, &fonts), fonts.width("00m", Weight::Bold, c.m.detail));
    // an unselected ID is dim, never the foreground
    let s = draw(&r);
    let id: Vec<_> = row_px(&s, 0).iter().enumerate().filter(|(i, _)| (30..52).contains(&(i % 400))).map(|(_, p)| *p).collect();
    assert!(id.iter().any(|p| p[3] > 0) && id.iter().all(|p| p[3] < 255 || (p[0], p[1], p[2]) != (0xe5, 0xe5, 0xe5)));
}

#[test]
fn a_title_thats_yours_is_the_foreground_and_anothers_a_step_back() {
    let theme = Theme::default();
    let full = |t: TaskRow| has(&row_px(&draw(&rows(vec![Row::Task(TaskRow { age: String::new(), ..t })])), 0), theme.fg);
    assert!(full(TaskRow { yours: true, ..task("BP-1", "ready") }));
    assert!(!full(task("BP-1", "working")));
    assert!(!full(TaskRow { quiet: true, ..task("BP-1", "triage") }));
    // selected, it's the foreground whoever's it is
    assert!(full(TaskRow { selected: true, ..task("BP-1", "working") }));
    assert!(theme.quiet()[3] < theme.soft()[3] && theme.dim()[3] < theme.quiet()[3]);
}

#[test]
fn a_groups_name_is_a_little_dim_so_its_titles_lead() {
    let s = draw(&rows(vec![Row::Head { name: "Ready".into(), count: 1 }]));
    let px = row_px(&s, 0);
    let theme = Theme::default();
    assert!(!has(&px, theme.fg));
    assert!(px.iter().any(|p| p[3] == theme.heading()[3]));
}

/// The box around what's drawn in an image: its first and last columns and rows.
fn drawn_box(s: &Sheet) -> (u32, u32, u32, u32) {
    let (w, h) = (s.pixmap.width(), s.pixmap.height());
    let on: Vec<(u32, u32)> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| px_at(s, x, y)[3] > 0).collect();
    let xs = on.iter().map(|p| p.0);
    let ys = on.iter().map(|p| p.1);
    (xs.clone().min().unwrap(), xs.max().unwrap(), ys.clone().min().unwrap(), ys.max().unwrap())
}

/// A faint pixel is the colour asked for: its alpha exactly, its colour within what
/// storing it premultiplied rounds off.
fn faintly(px: [u8; 4], c: [u8; 4]) -> bool {
    px[3] == c[3] && (0..3).all(|k| px[k].abs_diff(c[k]) <= 8)
}

fn flagged(flag: Flag, selected: bool) -> Rows {
    let d = DetailRow { text: "what it asks".into(), lines: 1, loud: true, lead: 0, selected, stale: false, flag: Some(flag) };
    rows(vec![Row::Task(TaskRow { flag: Some(flag), selected, ..task("BP-1", "needs") }), Row::Detail(d)])
}

#[test]
fn a_task_that_needs_you_is_washed_in_its_colour_over_the_cards_rectangle_with_a_round_bar() {
    let theme = Theme::default();
    for size in [TextSize::Small, TextSize::Medium, TextSize::Large] {
        let (_, washed) = draw_in(&flagged(Flag::Needs, false), size);
        let (_, card) = draw_in(&flagged(Flag::Needs, true), size);
        // the wash covers what the card would
        assert_eq!(drawn_box(&washed), drawn_box(&card), "{size:?}");
        // in the flag's colour, faintly, where there's no type
        let (_, x1, _, y1) = drawn_box(&washed);
        let wash = px_at(&washed, x1 - 4, y1 / 2);
        assert!(faintly(wash, theme.wash(Flag::Needs)), "{size:?}: {wash:?}");
        let (_, _, top, _) = drawn_box(&washed);
        // the bar: 3 pixels of amber across, half a cell in, its ends round
        let mid = y1 / 2;
        assert!((8..11).all(|x| px_at(&washed, x, mid) == solid(theme.yellow)) && px_at(&washed, 7, mid)[3] == 0, "{size:?}");
        assert!(px_at(&washed, 8, top)[3] < 255, "{size:?}: the bar's end is round");
    }
    let (_, stopped) = draw_in(&flagged(Flag::Stopped, false), TextSize::Medium);
    let (_, x1, _, y1) = drawn_box(&stopped);
    assert!(faintly(px_at(&stopped, x1 - 4, y1 / 2), theme.wash(Flag::Stopped)));
}

#[test]
fn the_selected_card_keeps_its_wash_and_its_bar_with_an_edge_of_its_flags_colour() {
    let theme = Theme::default();
    let (_, s) = draw_in(&flagged(Flag::Needs, true), TextSize::Medium);
    let (x0, x1, y0, y1) = drawn_box(&s);
    assert!(faintly(px_at(&s, x1 - 4, y1 / 2), theme.wash(Flag::Needs)));
    assert_eq!(px_at(&s, 9, y1 / 2), solid(theme.yellow));
    // its edge is amber, stronger than the hairline round a wash that isn't selected
    let (_, washed) = draw_in(&flagged(Flag::Needs, false), TextSize::Medium);
    let mid = (x0 + x1) / 2;
    let (edge, line) = (px_at(&s, mid, y0), px_at(&washed, mid, y0));
    assert!(edge[3] > 2 * line[3] && line[3] > theme.wash(Flag::Needs)[3], "{edge:?} {line:?}");
    assert!(edge[0] > edge[2] && line[0] > line[2], "amber: {edge:?} {line:?}");
    // a selected task that doesn't need you has the foreground's
    let plain = rows(vec![Row::Task(TaskRow { selected: true, ..task("BP-1", "todo") }), detail("what it does", true)]);
    let (_, p) = draw_in(&plain, TextSize::Medium);
    let (px0, px1, py0, py1) = drawn_box(&p);
    assert!(faintly(px_at(&p, px1 - 4, py1 / 2), theme.tint()));
    let e = px_at(&p, (px0 + px1) / 2, py0);
    assert!(e[3] > 2 * theme.tint()[3] && e[0] == e[2], "{e:?}");
}

#[test]
fn a_risk_is_a_chip_its_word_in_its_colour_inside_a_line_of_it_before_the_age_column() {
    let theme = Theme::default();
    let c = canvas(TextSize::Medium);
    let r = rows(vec![Row::Task(TaskRow { risk: Some("high".into()), ..task("BP-1", "ready") })]);
    let (_, s) = draw_in(&r, TextSize::Medium);
    // its line: red at 45%, a word and a half from its top to its bottom; no fill
    let line = [theme.red.0, theme.red.1, theme.red.2, 115];
    let (w, h) = (s.pixmap.width(), s.pixmap.height());
    let lined: Vec<(u32, u32)> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| faintly(px_at(&s, x, y), line)).collect();
    let ys = lined.iter().map(|p| p.1);
    let (top, bottom) = (ys.clone().min().unwrap(), ys.max().unwrap());
    let tall = bottom - top + 1;
    assert!(tall as f32 >= 1.5 * c.m.label - 2.0 && tall as f32 <= 1.5 * c.m.label, "{tall}");
    let x0 = lined.iter().map(|p| p.0).min().unwrap();
    assert_eq!(px_at(&s, x0 + 3, (top + bottom) / 2)[3], 0, "no fill inside the line");
    assert!(has(&row_px(&draw(&r), 0), theme.red));
    // the title makes room for it: the chip's width and the age's
    let g = Geo { c, idw: 0.0 };
    let fonts = Fonts::builtin();
    let t = TaskRow { risk: Some("high".into()), ..task("BP-1", "ready") };
    // the age's column is two figures wide, so chips line up whatever the ages
    let want = fonts.width("00m", Weight::Bold, c.m.detail) + 0.5 * c.m.detail + chip::width(&fonts, "high", c.m.label);
    let wide = TaskRow { age: "100m".into(), ..t.clone() };
    assert!(g.right_side(&wide, &fonts) > want);
    assert_eq!(g.right_side(&t, &fonts), want);
}

/// Do two images differ in columns [x0, x1) of rows [y0, y1)?
fn differ(a: &Sheet, b: &Sheet, (x0, x1): (u32, u32), (y0, y1): (u32, u32)) -> bool {
    (y0..y1).any(|y| (x0..x1).any(|x| px_at(a, x, y) != px_at(b, x, y)))
}

#[test]
fn the_selected_tasks_key_is_in_a_cap_at_its_cards_lower_right_and_its_last_line_makes_room() {
    let long = "a detail long enough to run to the right edge of the card and on to a second line of it";
    let block = |key: Option<(String, String)>| {
        let t = TaskRow { selected: true, key, ..task("BP-1", "ready") };
        rows(vec![Row::Task(t), Row::Detail(DetailRow { lines: 3, selected: true, ..detail_row(long) }), Row::Task(task("BP-2", "todo"))])
    };
    for size in [TextSize::Small, TextSize::Medium, TextSize::Large] {
        let (c, fonts) = (canvas(size), Fonts::builtin());
        let (laid, s) = draw_in(&block(Some(("a".into(), "accept".into()))), size);
        let (bare_laid, bare) = draw_in(&block(None), size);
        let g = Geo { c, idw: laid.idw };
        let key_w = g.key_width("a", "accept", &fonts);
        let last = laid.lines[1].last().unwrap();
        assert!(fonts.width(last, Weight::Regular, c.m.detail) <= g.detail_room() - key_w, "{size:?}: {last}");
        // without a key the line had the whole width
        assert!(fonts.width(&bare_laid.lines[1][0], Weight::Regular, c.m.detail) > g.detail_room() - key_w, "{size:?}");
        // on the block's last line, at the right; nothing else on the title's lines
        let (top, bottom) = (laid.heights[0], laid.heights[0] + laid.heights[1]);
        let at = ((c.right() - key_w + 0.8 * c.m.label) as u32, c.right() as u32);
        let last_line = (bottom - c.m.detail_line as u32, bottom);
        assert!(differ(&s, &bare, at, last_line), "{size:?}");
        assert!(!differ(&s, &bare, (0, s.pixmap.width()), (0, top)), "{size:?}");
        // the key's cap: its faint fill
        let cy = bottom - c.m.detail_line as u32 / 2;
        assert!(px_at(&s, at.0 + 3, cy)[3] > px_at(&bare, at.0 + 3, cy)[3], "{size:?}");
    }
}

#[test]
fn a_last_line_the_key_would_cut_keeps_its_text_and_the_key_goes_under_it() {
    let fonts = Fonts::builtin();
    let c = canvas(TextSize::Medium);
    let g = Geo { c, idw: 0.0 };
    // one line, as wide as its room: the key beside it would cut it
    let mut text = String::from("14 files");
    while fonts.width(&format!("{text} more"), Weight::Regular, c.m.detail) <= g.detail_room() {
        text.push_str(" more");
    }
    let with = |key: Option<(String, String)>, size| {
        let t = TaskRow { selected: true, key, ..task("BP-1", "ready") };
        lay(&rows(vec![Row::Task(t), Row::Detail(DetailRow { selected: true, ..detail_row(&text) })]), &canvas(size), &fonts)
    };
    let key = || Some(("a".to_string(), "accept".to_string()));
    let (laid, bare) = (with(key(), TextSize::Medium), with(None, TextSize::Medium));
    assert_eq!(laid.lines[1], vec![text.clone(), String::new()]);
    assert_eq!(laid.heights[1], bare.heights[1] + c.m.detail_line as u32);
    // small type keeps its rows a cell each: the line is cut
    let small = with(key(), TextSize::Small);
    assert_eq!(small.lines[1].len(), 1);
}

#[test]
fn a_short_last_line_keeps_its_room_beside_the_key_and_a_task_with_no_key_has_none() {
    let with = |key: Option<(String, String)>| {
        let t = TaskRow { selected: true, key, ..task("BP-1", "needs") };
        rows(vec![Row::Task(t), detail("Asks: which one?", true), detail("3 files, +4 −1", true)])
    };
    let (laid, s) = draw_in(&with(Some(("i".into(), "answer".into()))), TextSize::Medium);
    let (_, bare) = draw_in(&with(None), TextSize::Medium);
    assert_eq!(laid.lines[2], vec!["3 files, +4 −1".to_string()]);
    let bottom = laid.heights.iter().sum::<u32>();
    assert!(differ(&s, &bare, (250, 400), (bottom - 25, bottom)));
    // only the last line has it
    assert!(!differ(&s, &bare, (0, 400), (0, bottom - 25)));
}
