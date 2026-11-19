use super::super::{term, Probe};
use super::*;
use crate::app::testing::press;
use crate::model::testing::{json, task};
use crate::model::{Config, Look};
use crate::ui::testing::screen;
use crate::Note;
use std::sync::Arc;

const AREA: Rect = Rect { x: 0, y: 0, width: 40, height: 40 };

/// A board of tasks with long titles and lines, in the image look, and its picture, the
/// terminal having said it draws images with a cell 10 by 20.
fn board() -> (App, Picture) {
    let long = "A title long enough that the selected task's wraps to a second line and more";
    let tasks = (1..=6).map(|i| task(&format!("BP-{i}"), if i % 2 == 0 { "ready" } else { "todo" }).with(json!({"title": long}))).collect();
    let mut app = App::for_test(tasks);
    app.cfg = Config::default_for_test().with(json!({"board_bottom_gap": 0}));
    app.look = Look::Image;
    app.sel = Some("BP-2".into());
    let p = Picture { probe: Probe::Yes, on: true, cell: Some((10, 20)), window: || None, clock: Some(0), ..Picture::new(Arc::new(term::Gate::default())) };
    (app, p)
}

/// A frame, as the main loop draws it: what was written.
fn frame(app: &mut App, p: &mut Picture) -> String {
    let mut out = Vec::new();
    loop {
        app.image = p.drawing(app);
        screen(app, AREA.width, AREA.height);
        if !p.show(app, &mut out) {
            return String::from_utf8(out).unwrap();
        }
    }
}

/// Draw ahead until it's done: what was written.
fn ahead(app: &mut App, p: &mut Picture) -> String {
    let mut out = Vec::new();
    while p.ahead(app, AREA, &mut out) {}
    String::from_utf8(out).unwrap()
}

#[test]
fn drawn_ahead_a_j_and_a_k_only_place_their_images() {
    let (mut app, mut p) = board();
    frame(&mut app, &mut p);
    let view = app.view.clone();
    let out = ahead(&mut app, &mut p);
    // sent, not placed; the board is as it was
    assert!(out.contains("a=t,") && !out.contains("a=p,"), "{out:?}");
    assert_eq!((app.sel.as_deref(), &app.view), (Some("BP-2"), &view));
    for key in ['j', 'k', 'j', 'j', 'j'] {
        press(&mut app, key);
        let out = frame(&mut app, &mut p);
        assert!(!out.contains("a=t,") && out.contains("a=p,"), "{key}: {out:?}");
        ahead(&mut app, &mut p);
    }
}

#[test]
fn nothing_is_drawn_ahead_twice_or_while_a_question_is_asked() {
    let (mut app, mut p) = board();
    frame(&mut app, &mut p);
    ahead(&mut app, &mut p);
    // the same view again: nothing more to draw
    frame(&mut app, &mut p);
    assert_eq!(ahead(&mut app, &mut p), "");
    // a question on the bottom line: a j would answer it, not move
    app.pending = Some((crate::model::Action::Accept, "BP-2".into()));
    app.msg = Note::info("Accept BP-2? y/n");
    frame(&mut app, &mut p);
    assert_eq!(ahead(&mut app, &mut p), "");
    assert!(app.pending.is_some());
}


#[test]
fn the_lists_next_window_is_drawn_ahead_on_its_own_thread() {
    let mut app = App::for_test((1..=30).map(|i| task(&format!("BP-{i}"), "todo").with(json!({"title": format!("Task {i}")}))).collect());
    app.cfg = Config::default_for_test().with(json!({"board_bottom_gap": 0}));
    app.look = Look::Image;
    let (_, mut p) = board();
    let (tx, rx) = std::sync::mpsc::channel();
    p.use_thread(tx);
    let list = |p: &Picture| p.screen.id(&super::super::Slot::List);
    frame(&mut app, &mut p);
    // down a task at a time, drawing ahead between, the worker's images sent as they
    // come back: the list's image changes as it scrolls, but no key sends it
    let mut ids = vec![list(&p)];
    for k in 2..=30 {
        while p.ahead(&mut app, AREA, &mut Vec::new()) {}
        while p.busy() {
            if let Ok(crate::Msg::Ready(r)) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
                p.ready(r, &mut Vec::new());
            }
        }
        press(&mut app, 'j');
        let out = frame(&mut app, &mut p);
        assert_eq!(app.sel.as_deref(), Some(format!("BP-{k}").as_str()));
        assert!(!out.contains("a=t,"), "BP-{k}: {out:?}");
        ids.push(list(&p));
    }
    ids.dedup();
    assert!(ids.len() > 2, "{ids:?}");
}
