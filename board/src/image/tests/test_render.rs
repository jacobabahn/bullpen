use super::*;
use crate::image::theme;
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

fn draw(r: &Rows) -> Sheet {
    render(r, 0..r.rows.len(), GRID, &Theme::default(), &mut Fonts::builtin()).unwrap()
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
    let s = render(&r, 0..1, GRID, &theme, &mut Fonts::builtin()).unwrap();
    assert!(has(&row_px(&s, 0), theme.red));
}

#[test]
fn a_list_longer_than_the_view_is_drawn_whole_or_the_window_asked_for() {
    let many: Vec<Row> = (1..=50).map(|n| Row::Task(task(&format!("BP-{n}"), "todo"))).collect();
    let r = rows(many);
    let s = draw(&r);
    assert_eq!(s.pixmap.height(), 50 * 20);
    let part = render(&r, 10..20, GRID, &Theme::default(), &mut Fonts::builtin()).unwrap();
    assert_eq!(part.pixmap.height(), 10 * 20);
    assert_eq!(part.places[0].id.as_deref(), Some("BP-11"));
    assert_eq!(part.places[0].y, 0);
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
