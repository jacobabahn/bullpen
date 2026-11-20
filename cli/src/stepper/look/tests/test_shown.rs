//! The tour's images on the terminal: kept under the budget, the least recently wanted
//! deleted first, never one on screen or held, and one deleted is sent again.

use super::super::terminal::Terminal;
use super::*;

/// An image of 100×100 pixels: 40,000 bytes decoded.
fn image() -> Pixmap {
    Pixmap::new(100, 100).unwrap()
}

const CELL: (u32, u32) = (10, 20);

fn at(id: u32, row: u16) -> Place {
    Place {
        id,
        p: 1,
        col: 0,
        row,
        crop_x: 0,
        crop_y: 0,
        crop_w: 100,
        crop_h: 100,
        cols: 10,
        rows: 5,
        z: 0,
    }
}

fn send(s: &mut Shown, key: u64, out: &mut Vec<u8>) -> u32 {
    s.send(key, &image(), out).unwrap()
}

#[test]
fn past_the_budget_the_least_recently_wanted_is_deleted_and_sent_again_when_wanted() {
    let mut s = Shown::budgeted(1, CELL, 100_000);
    let mut out = Vec::new();
    let a = send(&mut s, 1, &mut out);
    send(&mut s, 2, &mut out);
    s.begin();
    s.id(1); // wanted again: 2 is the least recently wanted now
    s.begin();
    send(&mut s, 3, &mut out);
    assert_eq!(s.kept_bytes(), 80_000);
    assert!(s.id(2).is_none(), "2 was deleted for 3");
    assert_eq!(s.id(1), Some(a));
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.matches("a=d,d=I,i=2,").count(), 1, "{text:?}");
    // wanted again, it's sent again, as a new image
    let mut out = Vec::new();
    s.begin();
    assert!(send(&mut s, 2, &mut out) > 3);
    assert_eq!(cut::uploaded(&String::from_utf8(out).unwrap()).len(), 1);
}

#[test]
fn an_image_on_screen_or_held_is_never_deleted_for_room() {
    let mut s = Shown::budgeted(1, CELL, 100_000);
    let mut out = Vec::new();
    let a = send(&mut s, 1, &mut out);
    s.show(&[at(a, 0)], &mut out).unwrap();
    s.begin();
    send(&mut s, 2, &mut out); // held since begin
    send(&mut s, 3, &mut out); // over: nothing could go
    assert_eq!(s.kept_bytes(), 120_000);
    assert!(s.id(1).is_some() && s.id(2).is_some());
    // the next round, 2 isn't held: it goes for room, 1 on screen stays
    s.begin();
    send(&mut s, 4, &mut out);
    assert!(s.id(2).is_none() && s.id(1).is_some());
}

#[test]
fn the_terminal_has_every_image_placed() {
    // a terminal keeping less than the tour would (a quota of three images) loses some;
    // under a budget of two, the tour never places one it lost
    for (budget, blank) in [(1_000_000, true), (100_000, false)] {
        let mut s = Shown::budgeted(1, CELL, budget);
        let mut term = Terminal::new(120_000);
        for _ in 0..3 {
            for k in 0..4u64 {
                let mut out = Vec::new();
                s.begin();
                let id = send(&mut s, k, &mut out);
                s.show(&[at(id, 0)], &mut out).unwrap();
                term.feed(&String::from_utf8(out).unwrap());
            }
        }
        assert_eq!(!term.blanks.is_empty(), blank, "budget {budget}: {:?}", term.blanks);
        assert!(s.kept_bytes() <= budget);
    }
}
