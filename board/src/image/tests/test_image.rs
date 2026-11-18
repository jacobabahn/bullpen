use super::*;
use crate::model::testing::{json, task};
use crate::model::{Config, Task};
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
    Picture { window: || None, ..Picture::new(Arc::new(term::Gate::default())) }
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

/// Draw a frame (the list as an image) and show it: what was written to the terminal.
fn frame(app: &mut App, p: &mut Picture) -> String {
    app.image = p.drawing(app);
    screen(app, 40, 12);
    let mut out = Vec::new();
    p.show(app, &mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn the_board_asks_the_terminal_once_before_drawing_an_image_and_draws_text_meanwhile() {
    let (mut app, mut p) = (board(3), picture());
    assert!(!p.drawing(&app));
    let q = asked(&mut app, &mut p);
    assert!(q.starts_with(&kitty::query(PROBE)) && q.ends_with("\x1b[c"));
}

#[test]
fn the_list_is_sent_once_placed_in_its_cells_and_its_rows_left_blank() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    app.image = true;
    let s = screen(&mut app, 40, 12);
    assert!(s[0].contains("repo")); // the top line is text
    assert!(!s.iter().any(|l| l.contains("BP-1"))); // the rows are the image's
    let view = app.view.clone().unwrap();
    assert_eq!((view.row, view.col, view.cols, view.top), (2, 0, 38, 0));
    let out = frame(&mut app, &mut p);
    assert!(out.starts_with(&format!("\x1b_Ga=t,f=100,t=d,i={FIRST},q=2,")));
    // the rows the list has room for (6 of its 7: the heading, three tasks and their
    // details), a cell each
    assert_eq!((view.rows.rows.len(), view.rows_n), (7, 6));
    assert!(out.contains(&format!("a=p,i={FIRST},p=1,x=0,y=0,w=380,h=120,c=38,r=6,C=1,q=2")));
    assert!(out.contains("\x1b7\x1b[3;1H"));
    // nothing changed: nothing is sent
    assert_eq!(frame(&mut app, &mut p), "");
}

#[test]
fn scrolling_moves_the_crop_and_sends_no_image() {
    let (mut app, mut p) = (board(30), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    let mut view = app.view.clone().unwrap();
    view.top = 5;
    app.view = Some(view);
    let mut out = Vec::new();
    p.show(&mut app, &mut out);
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("a=t"));
    assert!(out.contains(&format!("a=p,i={FIRST},p=1,x=0,y=100,w=380,h=")));
}

#[test]
fn a_change_sends_a_new_image_and_places_it_before_the_old_one_goes() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    app.sel = Some("BP-2".into()); // the selection moves: the rows change
    let out = frame(&mut app, &mut p);
    let (sent, placed, deleted) = (
        out.find(&format!("a=t,f=100,t=d,i={}", FIRST + 1)).unwrap(),
        out.find(&format!("a=p,i={}", FIRST + 1)).unwrap(),
        out.find(&kitty::delete(FIRST)).unwrap(),
    );
    assert!(sent < placed && placed < deleted);
}

#[test]
fn the_image_is_hidden_under_an_overlay_and_deleted_when_the_list_is_text_again() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    app.mode = crate::Mode::Help { top: 0 };
    assert_eq!(frame(&mut app, &mut p), kitty::hide(FIRST));
    app.mode = crate::Mode::Normal;
    assert!(frame(&mut app, &mut p).contains(&format!("a=p,i={FIRST}"))); // placed again, not sent again
    app.look = Look::Text;
    assert_eq!(frame(&mut app, &mut p), kitty::delete(FIRST));
    assert_eq!(frame(&mut app, &mut p), "");
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
fn a_list_too_tall_for_one_image_is_sent_a_window_at_a_time() {
    assert_eq!(window(10, 0, 5, 100, None), (0, 10));
    assert_eq!(window(300, 0, 10, 100, None), (0, 100));
    // the window sent before, while it holds the rows shown
    assert_eq!(window(300, 50, 10, 100, Some((0, 100))), (0, 100));
    // a new one round them once it doesn't
    assert_eq!(window(300, 95, 10, 100, Some((0, 100))), (50, 150));
    assert_eq!(window(300, 295, 5, 100, None), (200, 300));
}

#[test]
fn colours_asked_again_change_the_image() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    p.asking = Some(false);
    let mut light = answer(true);
    light.said.fg = Some(theme::Rgb(0, 0, 0));
    p.answered(&light);
    assert!(frame(&mut app, &mut p).contains(&format!("a=t,f=100,t=d,i={}", FIRST + 1)));
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
    frame(&mut app, &mut p);
    p.resized();
    let out = frame(&mut app, &mut p);
    assert!(out.contains(&format!("a=t,f=100,t=d,i={}", FIRST + 1)) && out.ends_with(&kitty::delete(FIRST)));
}

#[test]
fn colours_are_asked_again_every_few_seconds_unless_the_answers_came_late() {
    let (mut app, mut p) = (board(3), picture());
    asked(&mut app, &mut p);
    frame(&mut app, &mut p);
    p.asked_at = Instant::now() - RECHECK * 2;
    frame(&mut app, &mut p);
    assert!(p.gate.wanted()); // asked again
    p.gate.done();
    p.answered(&term::Answers { cell: Some((10, 20)), ..Default::default() }); // late: no attributes
    p.asked_at = Instant::now() - RECHECK * 2;
    frame(&mut app, &mut p);
    assert!(!p.gate.wanted());
}
