use super::*;
use crate::stepper::look::ink::Type;
use bullpen_draw::font::Fonts;
use bullpen_draw::theme::Theme;

const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n-fn a() {}\n+fn b() {}\n x\n";

fn step<'a>(comments: Vec<&'a str>) -> Page<'a> {
    Page::Step(Step {
        title: "The score gets a `fourth` word",
        refs: vec![("cli/src/a.rs".into(), vec!["1–2".into()])],
        note: "`Score` gains `worth`.",
        comments,
        files: code::parse(PATCH),
    })
}

/// The page's height, measured, and the page drawn that high.
fn drawn(page: &Page, th: Theme) -> (f32, tiny_skia::Pixmap) {
    let mut ty = Type::new(Fonts::builtin());
    let h = draw(&mut Ink::new(1280, 1, &mut ty, th, 0.5).unwrap(), page);
    let mut ink = Ink::new(1280, h.ceil() as u32, &mut ty, th, 0.5).unwrap();
    assert_eq!(draw(&mut ink, page), h, "measured as drawn");
    (h, ink.pm)
}

fn px(pm: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let p = pm.pixel(x, y).unwrap().demultiply();
    [p.red(), p.green(), p.blue(), p.alpha()]
}

#[test]
fn a_comment_is_an_amber_card_and_makes_the_page_taller() {
    let th = Theme::default();
    let (bare, _) = drawn(&step(vec![]), th);
    let (with, pm) = drawn(&step(vec!["Say which word failed."]), th);
    assert!(with > bare + 20.0);
    // the card's bar, at the margin, in the terminal's yellow, somewhere down the page
    let x = (72.0 * 0.5) as u32 + 1;
    let y = (0..pm.height()).find(|&y| px(&pm, x, y)[..3] == [th.yellow.0, th.yellow.1, th.yellow.2]);
    assert!(y.is_some(), "no amber bar");
}

#[test]
fn the_page_is_clear_where_nothing_is_drawn() {
    let (_, pm) = drawn(&step(vec![]), Theme::light());
    assert_eq!(px(&pm, 2, 2)[3], 0, "the pane's own background shows");
}

#[test]
fn the_end_page_lists_the_comments_or_says_there_are_none() {
    let th = Theme::default();
    let none = Page::End(End {
        tid: "BP-3",
        steps: 4,
        comments: vec![],
    });
    let some = Page::End(End {
        tid: "BP-3",
        steps: 4,
        comments: vec![
            ("1".into(), "One".into(), "Fix this.".into()),
            ("3".into(), "`Three`".into(), "And that.".into()),
        ],
    });
    let (h0, _) = drawn(&none, th);
    let (h2, _) = drawn(&some, th);
    assert!(h2 > h0 + 50.0);
}

#[test]
fn a_step_with_no_code_says_so() {
    let page = Page::Step(Step {
        title: "Docs",
        refs: vec![],
        note: "",
        comments: vec![],
        files: vec![],
    });
    let (h, _) = drawn(&page, Theme::default());
    assert!(h > 50.0 && h < 200.0);
}
