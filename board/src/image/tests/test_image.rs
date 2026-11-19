use super::*;
use crate::model::testing::{json, task};
use crate::model::{Config, Task};
use crate::model::TextSize;
use crate::ui::testing::screen;

/// A terminal's answer: it draws images (or not), with a cell 10 by 20 pixels.
fn answer(graphics: bool) -> term::Answers {
    term::Answers { graphics, done: true, cell: Some((10, 20)), ..Default::default() }
}

fn tasks(n: usize) -> Vec<Task> {
    (1..=n).map(|i| task(&format!("BP-{i}"), "todo").with(json!({"title": format!("Task {i}")}))).collect()
}

fn board(n: usize) -> App {
    let mut a = App::for_test(tasks(n));
    a.cfg = Config::default_for_test().with(json!({"board_bottom_gap": 0}));
    a.look = Look::Image;
    a
}

fn picture() -> Picture {
    // the time stands still unless a test moves it
    Picture { window: || None, clock: Some(0), ..Picture::new(Arc::new(term::Gate::default())) }
}

/// The picture once the terminal has said it draws images.
fn asked(app: &mut App, p: &mut Picture) -> String {
    let mut out = Vec::new();
    assert!(!p.show(app, &mut out));
    assert!(out.is_empty() && p.gate.wanted()); // the key reader is to stop first
    p.parked(&mut out); // it has
    p.gate.done(); // and has read the answers
    p.answered(&answer(true));
    assert!(p.drawing(app));
    String::from_utf8(out).unwrap()
}

/// Draw a frame (the board as images) and show it, as the main loop does: again at once
/// when the picture says to. What was written to the terminal.
fn frame(app: &mut App, p: &mut Picture) -> String {
    frame_in(app, p, 40, 12)
}

/// The same, on a board `w` by `h`.
fn frame_in(app: &mut App, p: &mut Picture, w: u16, h: u16) -> String {
    let mut out = Vec::new();
    loop {
        app.image = p.drawing(app);
        screen(app, w, h);
        if !p.show(app, &mut out) {
            return String::from_utf8(out).unwrap();
        }
    }
}

/// The images' IDs: the header's, the list's and the bottom rows', as first sent; then
/// the selected block's.
const HEAD: u32 = FIRST;
const LIST: u32 = FIRST + 1;
const FOOT: u32 = FIRST + 2;
const SEL: u32 = FIRST + 3;

/// The commands written: each upload as its first chunk's control, and each other command.
fn commands(out: &str) -> Vec<String> {
    out.split("\x1b_G")
        .skip(1)
        .filter_map(|c| c.split([';', '\x1b']).next())
        .filter(|c| !c.starts_with("q=2,m="))
        .map(String::from)
        .collect()
}

/// The placements written: image, placement, first row (1-based), rows.
fn placed(out: &str) -> Vec<(u32, u32, u16, u16)> {
    let field = |c: &str, k: &str| c.split(',').find_map(|f| f.strip_prefix(k)).and_then(|v| v.parse::<u32>().ok());
    out.split("\x1b7\x1b[")
        .skip(1)
        .filter_map(|c| {
            let row = c.split(';').next()?.parse().ok()?;
            let control = c.split("\x1b_G").nth(1)?;
            Some((field(control, "i=")?, field(control, "p=")?, row, field(control, "r=")? as u16))
        })
        .collect()
}

/// A board with tasks of every kind, as the pieces' tests have, in a taller pane: one
/// needs you, two ready, one in review, one working, three queued.
fn mixed() -> App {
    use crate::ui::testing::{asking, ready};
    let long = "A title long enough that the selected task's wraps to a second line and more";
    let mut a = App::for_test(vec![
        asking("BP-1"),
        ready("BP-2").with(json!({"title": long, "diffstat": "3 files, +13 −16"})),
        task("BP-3", "working").with(json!({"title": "Working on it"})),
        ready("BP-4"),
        task("BP-5", "review").with(json!({"title": long})),
        task("BP-6", "todo").with(json!({"title": long})),
        task("BP-7", "todo").with(json!({"title": "Task 7"})),
        task("BP-8", "todo").with(json!({"title": "Task 8"})),
    ]);
    a.cfg = Config::default_for_test().with(json!({"board_bottom_gap": 0}));
    a.look = Look::Image;
    a
}

#[test]
fn the_board_asks_the_terminal_once_before_drawing_an_image_and_draws_text_meanwhile() {
    let (mut app, mut p) = (board(3), picture());
    assert!(!p.drawing(&app));
    let q = asked(&mut app, &mut p);
    assert!(q.starts_with(&kitty::query(PROBE)) && q.ends_with("\x1b[c"));
}

#[test]
fn the_board_is_sent_as_images_placed_in_their_cells_and_its_rows_left_blank() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    let out = frame(&mut app, &mut p);
    let s = screen(&mut app, 40, 12);
    assert!(s.iter().all(|l| l.is_empty())); // the top line, the rows and the keys are the images'
    let view = app.view.clone().unwrap();
    assert_eq!((view.row0, view.row, view.col, view.cols, view.rows_n, view.foot_n), (0, 3, 0, 39, 6, 3));
    // the header's three rows (the top line and the status bar) and the three under the
    // list, 39 columns of 10 pixels
    for (id, row, rows) in [(HEAD, 1, 3), (FOOT, 10, 3)] {
        assert!(out.contains(&format!("a=t,f=100,t=d,i={id},q=2,")));
        let h = rows * 20;
        assert!(out.contains(&format!("\x1b7\x1b[{row};1H\x1b_Ga=p,i={id},p=1,x=0,y=0,w=390,h={h},c=39,r={rows},C=1,q=2")));
    }
    // the list's six rows: each covered once, by the list's crops, the selected block or the tail
    let mut covered = [0; 6];
    for (id, _, row, rows) in placed(&out) {
        if (LIST..LIST + 4).contains(&id) && id != FOOT {
            (row..row + rows).for_each(|r| covered[r as usize - 4] += 1);
        }
    }
    assert!(covered.iter().all(|n| *n <= 1) && covered[..4].iter().all(|n| *n == 1), "{covered:?}");
    assert!(out.contains(&format!("a=p,i={SEL},p=1,")));
    // nothing changed: nothing is sent
    assert_eq!(frame(&mut app, &mut p), "");
}

#[test]
fn a_change_sends_only_the_images_it_changes_and_places_each_before_the_old_one_goes() {
    let (mut app, mut p) = (mixed(), picture());
    asked(&mut app, &mut p);
    frame_in(&mut app, &mut p, 40, 40);
    app.sel = Some("BP-4".into()); // the selection moves down a task: the list stays
    let out = frame_in(&mut app, &mut p, 40, 40);
    assert!(!out.contains(&format!("i={HEAD}")) && !out.contains(&format!("a=t,f=100,t=d,i={LIST},")));
    let new = commands(&out).iter().find_map(|c| c.strip_prefix("a=t,f=100,t=d,i=").map(|r| r.split(',').next().unwrap().parse::<u32>().unwrap())).unwrap();
    let (sent, put, off) =
        (out.find(&format!("a=t,f=100,t=d,i={new}")).unwrap(), out.find(&format!("a=p,i={new}")).unwrap(), out.find(&kitty::hide(SEL)).unwrap());
    assert!(sent < put && put < off);
    assert!(p.screen.kept().contains(&SEL)); // taken off, kept to place again
    // the list's crops move: placed again, not sent
    assert!(out.contains(&format!("a=p,i={LIST},")));
    // a message changes the bottom rows' image alone
    app.msg = Note::failed("Couldn't accept BP-2.");
    let out = frame_in(&mut app, &mut p, 40, 40);
    let uploads: Vec<_> = commands(&out).into_iter().filter(|c| c.starts_with("a=t")).collect();
    assert_eq!(uploads.len(), 1, "{uploads:?}");
    let view = app.view.clone().unwrap();
    assert!(placed(&out).iter().all(|(_, _, row, _)| *row == view.row + view.rows_n + 1));
    assert!(!out.contains(&kitty::delete(HEAD)) && !out.contains(&kitty::delete(LIST)));
}

#[test]
fn the_header_with_its_status_bar_is_sent_again_only_when_a_count_changes() {
    let (mut app, mut p) = (mixed(), picture());
    asked(&mut app, &mut p);
    frame_in(&mut app, &mut p, 40, 40);
    // a title changes, and the selection: the counts don't
    app.tasks[2].title = "Working on something else".into();
    app.sel = Some("BP-4".into());
    let out = frame_in(&mut app, &mut p, 40, 40);
    assert!(!out.contains(&format!("i={HEAD},")), "{out}");
    // a queued task starts: one queued fewer, one more working
    app.tasks[6] = task("BP-7", "working").with(json!({"title": "Task 7"}));
    let out = frame_in(&mut app, &mut p, 40, 40);
    let head = placed(&out).into_iter().find(|(_, _, row, rows)| (*row, *rows) == (1, 3)).map(|(id, ..)| id);
    assert!(head.is_some_and(|id| id != HEAD && out.contains(&format!("a=t,f=100,t=d,i={id},"))), "{out}");
}

#[test]
fn moving_the_selection_sends_a_small_image_not_the_list() {
    let (mut app, mut p) = (mixed(), picture());
    asked(&mut app, &mut p);
    p.cell = Some((16, 34)); // a Mac's cell
    let first = frame_in(&mut app, &mut p, 60, 45);
    let list = |out: &str| out.matches(&format!("a=t,f=100,t=d,i={LIST},")).count();
    assert_eq!(list(&first), 1);
    let mut most = 0;
    for sel in ["BP-2", "BP-4", "BP-5", "BP-3", "BP-6"] {
        app.sel = Some(sel.into());
        let out = frame_in(&mut app, &mut p, 60, 45);
        assert!(!out.contains(&format!("a=t,f=100,t=d,i={LIST},")), "{sel}");
        let sizes: Vec<_> = out.split("\x1b_Ga=t,f=100,t=d,i=").skip(1).map(|c| c.split("a=p").next().unwrap().len()).collect();
        eprintln!("{sel}: {} bytes, images {sizes:?}", out.len());
        most = most.max(out.len());
    }
    eprintln!("first frame {} bytes; a j or k at most {most}", first.len());
    assert!(most * 3 < first.len(), "{most} of {}", first.len());
}

#[test]
fn the_list_scrolls_by_its_rows_heights_to_keep_the_selection_in_view() {
    let (mut app, mut p) = (board(30), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    let text_page = {
        let mut text = board(30);
        screen(&mut text, 40, 12);
        text.page
    };
    // a medium row is taller than a cell: fewer tasks to a page than the text list's
    assert!(app.page < text_page, "{} < {text_page}", app.page);
    app.sel = Some("BP-20".into());
    frame(&mut app, &mut p);
    let view = app.view.clone().unwrap();
    let at = view.rows.rows.iter().position(|r| r.is_selected()).unwrap();
    assert!(app.top > 0 && app.top <= at);
    // the selected task and its detail are within the list's 6 rows of 20 pixels
    let (c, fonts) = (Canvas::new(Grid { cols: 39, cell_w: 10, cell_h: 20 }, TextSize::Medium, p.theme), Fonts::builtin());
    let laid = render::lay(&view.rows, &c, &fonts);
    assert!(laid.heights[app.top..at + 2].iter().sum::<u32>() <= 120);
    // the text list's scrolling is its own: it goes by rows
    assert_eq!(crate::ui::list::scroll_by(&view.rows.rows, 0, &vec![1; view.rows.rows.len()], 6), {
        let mut text = board(30);
        text.sel = Some("BP-20".into());
        screen(&mut text, 40, 12);
        text.top
    });
}

#[test]
fn while_a_line_is_typed_or_a_question_asked_the_bottom_rows_are_text() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    app.mode = crate::Mode::Tell { id: "BP-1".into(), text: crate::edit::LineEdit::default(), reply: false };
    let out = frame(&mut app, &mut p);
    assert_eq!(out, kitty::hide(FOOT)); // the list and the top line stay
    let s = screen(&mut app, 40, 12);
    assert!(s[11].contains("Tell BP-1:"));
    assert!(s[..11].iter().all(|l| l.is_empty()));
    // back: the image is placed again, not sent again
    app.mode = crate::Mode::Normal;
    let out = frame(&mut app, &mut p);
    assert!(out.contains(&format!("a=p,i={FOOT}")) && !out.contains("a=t"));
    // a y/n question: its keys are text
    app.pending = Some((crate::model::Action::Drop, "BP-1".into()));
    frame(&mut app, &mut p);
    assert!(screen(&mut app, 40, 12)[11].contains("y drop"));
}

#[test]
fn the_text_size_setting_sets_the_rows_heights() {
    let heights = |size: &str| {
        let (mut app, mut p) = (board(3), picture());
        app.cfg = app.cfg.with(json!({"board_text_size": size}));
        asked(&mut app, &mut p);
        frame(&mut app, &mut p);
        let c = Canvas::new(Grid { cols: 39, cell_w: 10, cell_h: 20 }, app.cfg.text_size, p.theme);
        render::lay(&app.view.unwrap().rows, &c, &Fonts::builtin()).heights
    };
    assert!(heights("small").iter().all(|h| *h == 20));
    assert!(heights("large").iter().sum::<u32>() > heights("medium").iter().sum::<u32>());
}

#[test]
fn the_images_are_hidden_under_an_overlay_and_deleted_when_the_board_is_text_again() {
    // (a board with room for the list's image under the selected block)
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame_in(&mut app, &mut p, 40, 14);
    app.mode = crate::Mode::Help { top: 0 };
    let out = frame_in(&mut app, &mut p, 40, 14);
    let hidden = commands(&out);
    assert!(hidden.iter().all(|c| c.starts_with("a=d,d=i,") && !c.contains("p=")), "{hidden:?}");
    assert!([HEAD, LIST, FOOT, SEL].iter().all(|id| out.contains(&kitty::hide(*id))));
    app.mode = crate::Mode::Normal;
    let out = frame_in(&mut app, &mut p, 40, 14);
    // placed again, not sent again
    assert!([HEAD, LIST, FOOT, SEL].iter().all(|id| out.contains(&format!("a=p,i={id},"))) && !out.contains("a=t"));
    app.look = Look::Text;
    let out = frame_in(&mut app, &mut p, 40, 14);
    assert!(commands(&out).iter().all(|c| c.starts_with("a=d,d=I,")));
    assert!([HEAD, LIST, FOOT, SEL].iter().all(|id| out.contains(&kitty::delete(*id))));
    assert_eq!(frame_in(&mut app, &mut p, 40, 14), "");
}

#[test]
fn a_terminal_without_kitty_graphics_gets_the_text_list_and_a_line_saying_why() {
    let (mut app, mut p) = (board(3), picture());
    let mut out = Vec::new();
    p.show(&mut app, &mut out);
    p.parked(&mut out);
    p.answered(&answer(false));
    assert!(p.show(&mut app, &mut out)); // draw again, as text
    assert_eq!(app.look, Look::Text);
    assert_eq!(app.msg, "Drawing the list as text: the terminal doesn't draw kitty graphics.");
    assert!(screen(&mut app, 40, 12).iter().any(|l| l.contains("BP-1")));
    // one that doesn't answer at all
    let (mut app, mut p) = (board(3), picture());
    p.show(&mut app, &mut out);
    p.answered(&term::Answers::default());
    assert!(p.show(&mut app, &mut out));
    assert_eq!(app.msg, "Drawing the list as text: the terminal didn't answer the kitty graphics query.");
}

#[test]
fn a_font_that_cant_be_loaded_gets_the_text_list_and_a_line_saying_why() {
    let (mut app, mut p) = (board(3), picture());
    app.cfg = app.cfg.with(json!({"board_font": "/nonexistent/font.ttf"}));
    asked(&mut app, &mut p);
    app.image = true;
    screen(&mut app, 40, 12);
    let mut out = Vec::new();
    assert!(p.show(&mut app, &mut out));
    assert_eq!(app.look, Look::Text);
    assert!(app.msg.text.starts_with("Drawing the list as text: can't read the font /nonexistent/font.ttf"));
    assert!(out.is_empty());
}

#[test]
fn a_terminal_that_doesnt_say_its_cell_size_gets_the_text_list() {
    let (mut app, mut p) = (board(3), picture());
    let mut out = Vec::new();
    p.show(&mut app, &mut out);
    p.answered(&term::Answers { graphics: true, done: true, ..Default::default() });
    app.image = p.drawing(&app);
    screen(&mut app, 40, 12);
    assert!(p.show(&mut app, &mut out));
    assert_eq!(app.msg, "Drawing the list as text: the terminal doesn't say its cell size in pixels.");
}

#[test]
fn the_compact_layout_stays_text() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    app.cfg = app.cfg.with(json!({"board_layout": "compact"}));
    assert!(!p.drawing(&app));
}

#[test]
fn colours_asked_again_change_the_image() {
    // (a board with room for the list's image under the selected block)
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame_in(&mut app, &mut p, 40, 14);
    p.asking = Some(false);
    let mut light = answer(true);
    light.said.fg = Some(theme::Rgb(0, 0, 0));
    p.answered(&light);
    let out = frame_in(&mut app, &mut p, 40, 14);
    // each sent again, the old ones taken off and kept a while
    assert!([HEAD, LIST, FOOT, SEL].iter().all(|id| out.contains(&kitty::hide(*id)) && p.screen.kept().contains(id)));
}

#[test]
fn i_switches_the_look_for_this_run() {
    let mut app = App::for_test(tasks(1));
    assert_eq!(app.look, Look::Text);
    crate::app::testing::press(&mut app, 'I');
    assert_eq!((app.look, app.msg.text.as_str()), (Look::Image, "Drawing the list as an image."));
    crate::app::testing::press(&mut app, 'I');
    assert_eq!((app.look, app.msg.text.as_str()), (Look::Text, "Drawing the list as text."));
}

#[test]
fn after_a_resize_the_image_is_sent_again() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    let uploads = |out: &str| commands(out).iter().filter(|c| c.starts_with("a=t")).count();
    let shown = uploads(&frame(&mut app, &mut p));
    p.resized();
    let out = frame(&mut app, &mut p);
    // each image sent again, the old one deleted after
    assert_eq!(uploads(&out), shown);
    assert!([HEAD, LIST, FOOT, SEL].iter().all(|id| out.contains(&kitty::delete(*id))));
}

/// The key reader reads the answers asked: back to keys, with these answers.
fn answer_with(p: &mut Picture, a: &term::Answers) {
    let mut out = Vec::new();
    p.parked(&mut out);
    assert_eq!(String::from_utf8(out).unwrap(), term::questions(None)); // the colours, no graphics query
    p.gate.done();
    p.answered(a);
}

#[test]
fn the_colours_are_never_asked_again_by_time_alone() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    for _ in 0..3 {
        frame(&mut app, &mut p);
        app.sel = Some("BP-2".into()); // the board changes, as it does each second
        frame(&mut app, &mut p);
        app.sel = Some("BP-1".into());
    }
    assert!(!p.gate.wanted());
}

#[test]
fn a_resize_asks_the_colours_and_cell_size_again() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    p.resized();
    frame(&mut app, &mut p);
    assert!(p.gate.wanted());
    let mut light = answer(true);
    light.said.fg = Some(theme::Rgb(0, 0, 0));
    light.cell = Some((12, 24));
    answer_with(&mut p, &light);
    assert_eq!((p.theme.fg, p.cell), (theme::Rgb(0, 0, 0), Some((12, 24))));
    assert!(frame(&mut app, &mut p).contains("a=t,f=100")); // drawn in them
    frame(&mut app, &mut p);
    assert!(!p.gate.wanted()); // once
}

#[test]
fn i_twice_asks_the_colours_again() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    crate::app::testing::press(&mut app, 'I');
    frame(&mut app, &mut p); // text: the image goes, nothing is asked
    assert!(!p.gate.wanted());
    crate::app::testing::press(&mut app, 'I');
    frame(&mut app, &mut p);
    assert!(p.gate.wanted());
    let mut light = answer(true);
    light.said.bg = Some(theme::Rgb(255, 255, 255));
    answer_with(&mut p, &light);
    assert_eq!(p.theme.bg, theme::Rgb(255, 255, 255));
    assert!(frame(&mut app, &mut p).contains("a=t,f=100"));
}

#[test]
fn a_resize_while_the_answers_are_read_asks_once_theyre_read() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    p.resized();
    frame(&mut app, &mut p);
    assert!(p.gate.wanted());
    let mut out = Vec::new();
    p.parked(&mut out);
    p.resized(); // the reader is reading the answers: asked again after
    frame(&mut app, &mut p);
    p.answered(&answer(true));
    frame(&mut app, &mut p);
    assert!(p.due); // the reader hasn't opened the gate yet
    p.gate.done();
    frame(&mut app, &mut p);
    assert!(p.gate.wanted() && !p.due);
}

#[test]
fn answers_that_came_late_keep_the_colours_they_didnt_say() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    let mut first = answer(true);
    first.said.fg = Some(theme::Rgb(1, 2, 3));
    first.said.bg = Some(theme::Rgb(4, 5, 6));
    p.asking = Some(false);
    p.answered(&first);
    // late, with only the foreground
    let mut late = term::Answers::default();
    late.said.fg = Some(theme::Rgb(7, 8, 9));
    p.asking = Some(false);
    p.answered(&late);
    assert_eq!((p.theme.fg, p.theme.bg), (theme::Rgb(7, 8, 9), theme::Rgb(4, 5, 6)));
    assert_eq!(p.cell, Some((10, 20)));
}

#[test]
fn a_reload_waits_until_the_answers_are_read_and_a_failed_one_leaves_the_image_and_keys_working() {
    let (mut app, mut p) = (board(3), picture());
    assert!(p.idle());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    p.resized();
    frame(&mut app, &mut p);
    assert!(!p.idle()); // asking: no reload
    let mut out = Vec::new();
    p.parked(&mut out);
    assert!(!p.idle());
    p.answered(&answer(true));
    assert!(!p.idle()); // the reader is still reading what's left
    p.gate.done();
    assert!(p.idle());
    frame(&mut app, &mut p);
    // the reload deletes the image; the exec fails: the next frame sends it again,
    // and the board can still ask the terminal (the reader is reading keys)
    let mut out = Vec::new();
    p.clear(&mut out);
    assert!(String::from_utf8(out).unwrap().starts_with("\x1b_Ga=d"));
    assert!(frame(&mut app, &mut p).contains("a=t,f=100"));
    p.resized();
    frame(&mut app, &mut p);
    assert!(p.gate.wanted());
}

#[test]
fn an_age_changing_off_screen_sends_nothing() {
    let (mut app, mut p) = (board(30), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    app.tasks[29] = task("BP-30", "todo").with(json!({"title": "Task 30", "updated": "2020-01-01T00:00:00+00:00"}));
    assert_eq!(frame(&mut app, &mut p), "");
}

#[test]
fn a_board_too_short_for_rows_under_its_list_keeps_them_as_text() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    app.image = true;
    let s = screen(&mut app, 40, 3);
    assert!(app.view.as_ref().is_some_and(|v| v.foot.is_none()));
    assert!(s.iter().any(|l| l.contains("keys")));
}


/// The image a mark's frames are in: the image placed at z 1 that isn't the scroll marks'.
fn marks_placed(out: &str) -> Vec<String> {
    commands(out).into_iter().filter(|c| c.starts_with("a=p") && c.contains(",z=1,") && !c.contains(",c=1,")).collect()
}

#[test]
fn a_working_tasks_mark_turns_by_placing_another_frame_not_sending_the_list() {
    let (mut app, mut p) = (board(0), picture());
    app.tasks = vec![task("BP-1", "working").with(json!({"title": "Working"})), task("BP-2", "todo")];
    asked(&mut app, &mut p);
    let first = frame(&mut app, &mut p);
    let marks = marks_placed(&first);
    assert_eq!(marks.len(), 1, "{marks:?}"); // BP-1's, over the list
    // a frame later: the same placement, cropped to the next frame; nothing else
    p.clock = Some(200);
    let out = frame(&mut app, &mut p);
    let next = commands(&out);
    assert_eq!(next.len(), 1, "{next:?}");
    assert!(next[0].starts_with("a=p") && next[0] != marks[0]);
    assert!(out.len() < 100, "{} bytes", out.len());
    // within the frame: nothing
    p.clock = Some(399);
    assert_eq!(frame(&mut app, &mut p), "");
    // round again: the first frame's placement
    p.clock = Some(2000);
    assert_eq!(commands(&frame(&mut app, &mut p)), marks);
}

#[test]
fn with_motion_off_the_marks_are_still_in_the_list_and_time_sends_nothing() {
    let (mut app, mut p) = (board(0), picture());
    app.tasks = vec![task("BP-1", "working").with(json!({"title": "Working"})), task("BP-2", "review")];
    app.cfg = app.cfg.with(json!({"board_motion": false}));
    asked(&mut app, &mut p);
    assert!(marks_placed(&frame(&mut app, &mut p)).is_empty());
    p.clock = Some(1000);
    assert_eq!(frame(&mut app, &mut p), "");
    // a status changes: drawn in the list as it is, nothing settles in
    app.tasks[1] = task("BP-2", "ready");
    assert!(marks_placed(&frame(&mut app, &mut p)).is_empty());
    assert_eq!(p.tick(), None);
}

#[test]
fn a_mark_settles_in_when_its_task_changes_status_then_goes_back_in_the_list() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    assert_eq!(p.tick(), None);
    // BP-2 is ready: its mark is placed over the list, small, and the board redraws soon
    p.clock = Some(1000);
    app.tasks[1] = task("BP-2", "ready").with(json!({"title": "Task 2"}));
    let out = frame(&mut app, &mut p);
    let first = marks_placed(&out);
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(p.tick(), Some(Duration::from_millis(40)));
    // it grows: only its placement changes, each 40 ms, and then it stands
    let mut crops = vec![first[0].clone()];
    for ms in [1040, 1080, 1120] {
        p.clock = Some(ms);
        let next = commands(&frame(&mut app, &mut p));
        assert_eq!(next.len(), 1, "{ms}: {next:?}");
        crops.push(next[0].clone());
    }
    assert!(crops.windows(2).all(|w| w[0] != w[1]));
    p.clock = Some(1160);
    assert_eq!(frame(&mut app, &mut p), "");
    assert_eq!(p.tick(), None);
    // the list is drawn again for something else: the mark is back in it, its placement gone
    app.tasks[1] = task("BP-2", "ready").with(json!({"title": "Task 2, renamed"}));
    let out = frame(&mut app, &mut p);
    assert!(marks_placed(&out).is_empty());
    let mark = placed(&out).len(); // the list's crops and the block, placed again
    assert!(mark > 0 && commands(&out).iter().any(|c| c.starts_with("a=d,d=i,") && !c.contains("p=")), "{:?}", commands(&out));
}

#[test]
fn a_j_or_k_sends_its_new_images_before_anything_moves_and_takes_the_old_off_after() {
    let (mut app, mut p) = (mixed(), picture());
    asked(&mut app, &mut p);
    frame_in(&mut app, &mut p, 40, 40);
    for sel in ["BP-4", "BP-2", "BP-5", "BP-1"] {
        app.sel = Some(sel.into());
        let c = commands(&frame_in(&mut app, &mut p, 40, 40));
        let last = |pre: &str| c.iter().rposition(|x| x.starts_with(pre));
        let first = |pre: &str| c.iter().position(|x| x.starts_with(pre));
        // no cell is left without its image while the new block is on its way (BP-1's
        // was kept: it's only placed)
        assert!((sel == "BP-1" || last("a=t").is_some()) && last("a=t") < first("a=p"), "{sel}: {c:?}");
        let off = c.iter().position(|x| (x.starts_with("a=d,d=i,") && !x.contains("p=")) || x.starts_with("a=d,d=I"));
        assert!(last("a=p") < off, "{sel}: {c:?}");
    }
}

#[test]
fn under_an_overlay_a_mark_settling_doesnt_keep_the_board_waking() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    p.clock = Some(1000);
    app.tasks[1] = task("BP-2", "ready").with(json!({"title": "Task 2"}));
    frame(&mut app, &mut p);
    assert!(p.tick().is_some());
    // `?` opens: the images are hidden; time goes on, and the board needn't redraw for them
    app.mode = crate::Mode::Help { top: 0 };
    p.clock = Some(1500);
    frame(&mut app, &mut p);
    assert_eq!(p.tick(), None);
}

#[test]
fn the_list_is_sent_again_for_a_change_on_screen_or_scrolling_out_of_its_window_only() {
    let (mut app, mut p) = (board(40), picture());
    asked(&mut app, &mut p);
    frame_in(&mut app, &mut p, 40, 40);
    let list = |p: &Picture| p.screen.id(&Slot::List);
    // down until the list scrolls out of its first window (the page): sent again, the
    // page and the next one
    let mut k = 1;
    while app.top == 0 {
        k += 1;
        app.sel = Some(format!("BP-{k}"));
        frame_in(&mut app, &mut p, 40, 40);
    }
    let first = list(&p);
    // on down a little: the crops move, the list isn't sent
    let top = app.top;
    while app.top == top {
        k += 1;
        app.sel = Some(format!("BP-{k}"));
        frame_in(&mut app, &mut p, 40, 40);
        assert_eq!(list(&p), first, "BP-{k}");
    }
    assert_eq!(list(&p), first, "scrolled to {} at BP-{k}", app.top);
    // a task off screen but in the window changes: nothing is sent; one shown changes:
    // the list is
    let view = app.view.clone().unwrap();
    let c = Canvas::new(Grid { cols: view.cols as usize, cell_w: 10, cell_h: 20 }, app.cfg.text_size, Theme::default());
    let plan = pieces::plan(&view, &c, &Fonts::builtin(), p.list.as_ref().map(|l| l.win));
    let off = (plan.end..plan.win.end).find_map(|i| match &view.base.rows[i] {
        crate::ui::rows::Row::Task(t) => Some(t.id.clone()),
        _ => None,
    });
    let off = off.expect("a task under the list, in its window");
    app.tasks.iter_mut().find(|t| t.id == off).unwrap().title = "Renamed".into();
    assert_eq!(frame_in(&mut app, &mut p, 40, 40), "", "{off}");
    app.tasks[k].title = "Renamed".into();
    let out = frame_in(&mut app, &mut p, 40, 40);
    assert!(list(&p) != first && out.contains(&format!("a=t,f=100,t=d,i={},", list(&p).unwrap())));
    // on to the end: sent again a page on, not at each of the moves
    let mut ids = vec![list(&p)];
    for k in k + 1..=40 {
        app.sel = Some(format!("BP-{k}"));
        frame_in(&mut app, &mut p, 40, 40);
        ids.push(list(&p));
    }
    ids.dedup();
    assert!(ids.len() > 1 && ids.len() * 2 < 40 - k, "{} for {} moves", ids.len(), 40 - k);
}

#[test]
fn going_back_to_a_task_places_its_block_again_and_sends_nothing() {
    let (mut app, mut p) = (mixed(), picture());
    asked(&mut app, &mut p);
    frame_in(&mut app, &mut p, 40, 40);
    let first = p.screen.id(&Slot::Sel);
    app.sel = Some("BP-4".into());
    frame_in(&mut app, &mut p, 40, 40);
    app.sel = Some("BP-1".into());
    let out = frame_in(&mut app, &mut p, 40, 40);
    assert!(!out.contains("a=t,"), "{:?}", commands(&out));
    assert_eq!(p.screen.id(&Slot::Sel), first);
    assert!(out.contains(&format!("a=p,i={},p=1,", first.unwrap())));
}

#[test]
fn the_images_kept_are_bounded_and_deleted_with_the_rest() {
    let (mut app, mut p) = (board(30), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    let mut all = vec![];
    for k in 2..=30 {
        app.sel = Some(format!("BP-{k}"));
        let out = frame(&mut app, &mut p);
        all.extend(commands(&out).into_iter().filter_map(|c| c.strip_prefix("a=t,f=100,t=d,i=").map(|r| r.split(',').next().unwrap().to_string())));
        assert!(p.screen.kept().len() <= 16);
    }
    // the oldest were deleted as more were kept; the rest go when the board is text again
    app.look = Look::Text;
    let out = frame(&mut app, &mut p);
    let deleted: Vec<&String> = all.iter().filter(|id| out.contains(&format!("a=d,d=I,i={id},"))).collect();
    assert!(p.screen.kept().is_empty() && deleted.len() >= 16, "{}", deleted.len());
}


