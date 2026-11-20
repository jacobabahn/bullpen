use super::*;

/// An image `w` by `h` pixels, in one piece (its PNG's bytes don't matter here).
fn png(w: u32, h: u32) -> Pngs {
    Pngs { cell: (10, 20), pieces: vec![(Piece { x: 0, y: 0, w, h }, b"png".to_vec())] }
}

fn want(slot: Slot, key: u64) -> Want {
    Want { slot, key, png: Some(png(10, 10)), places: vec![] }
}

#[test]
fn the_images_kept_are_the_latest_few() {
    let mut s = Screen::new(1);
    let mut out = Vec::new();
    for k in 1..=KEEP as u64 + 3 {
        s.preload(vec![want(Slot::Sel, k)], &mut out).unwrap();
    }
    assert_eq!(s.kept().len(), KEEP);
    assert!(!s.has(&Slot::Sel, 3) && s.has(&Slot::Sel, 4) && s.has(&Slot::Sel, KEEP as u64 + 3));
    // the oldest three were deleted
    assert_eq!(String::from_utf8(out).unwrap().matches("a=d,d=I,").count(), 3);
    // and so many bytes: one image as big as the budget leaves just itself
    s.preload(vec![Want { png: Some(png(10_000, 2_400)), ..want(Slot::Foot, 1) }], &mut Vec::new()).unwrap();
    assert_eq!(s.kept().len(), 1);
}

#[test]
fn one_of_the_lists_images_is_kept_its_latest() {
    let mut s = Screen::new(1);
    for k in 1..=3 {
        s.preload(vec![Want { png: Some(png(3_000, 3_000)), ..want(Slot::List, k) }, want(Slot::Sel, k)], &mut Vec::new()).unwrap();
    }
    assert!(s.has(&Slot::List, 3) && !s.has(&Slot::List, 2) && !s.has(&Slot::List, 1));
    assert!((1..=3).all(|k| s.has(&Slot::Sel, k)));
}

#[test]
fn the_images_on_the_terminal_stay_under_the_budget_and_room_is_made_before_a_send() {
    // a big board: the list's window in four strips on screen (38 MB), windows more drawn
    // ahead, each as big
    let mut s = Screen::new(1);
    let list = || Some(Pngs { cell: (10, 20), pieces: (0..4).map(|k| (Piece { x: 0, y: k * 900, w: 2_500, h: 950 }, b"png".to_vec())).collect() });
    s.show(vec![Want { png: list(), places: vec![at(1, 0, 0, 10)], ..want(Slot::List, 1) }], &mut Vec::new()).unwrap();
    for k in 2..=5 {
        let mut out = Vec::new();
        s.preload(vec![want(Slot::Sel, k), Want { png: list(), ..want(Slot::List, k) }], &mut out).unwrap();
        assert!(s.bytes() <= BUDGET as u64, "{}", s.bytes());
        // past the second, what's kept is deleted before the list is sent, not after
        let out = String::from_utf8(out).unwrap();
        if k > 2 {
            let (deleted, list_sent) = (out.find("a=d,d=I,").unwrap(), out.rfind("a=t,").unwrap());
            assert!(deleted < list_sent, "{out:?}");
        }
    }
    assert!(s.has(&Slot::List, 1) && s.has(&Slot::List, 5), "the one shown, the latest kept");
}

#[test]
fn what_a_key_needs_isnt_dropped_to_keep_the_rest_of_it() {
    let mut s = Screen::new(1);
    for k in 1..=KEEP as u64 {
        s.preload(vec![want(Slot::Sel, k)], &mut Vec::new()).unwrap();
    }
    // the oldest kept is this key's block; its key row is new
    s.preload(vec![Want { png: None, ..want(Slot::Sel, 1) }, want(Slot::Foot, 99)], &mut Vec::new()).unwrap();
    assert!(s.has(&Slot::Sel, 1) && s.has(&Slot::Foot, 99) && !s.has(&Slot::Sel, 2));
}

/// An image cut in three strips, each a cell over the next, for a cell 10 by 20.
fn strips() -> Pngs {
    let piece = |y, h| (Piece { x: 0, y, w: 100, h }, b"png".to_vec());
    Pngs { cell: (10, 20), pieces: vec![piece(0, 120), piece(100, 120), piece(200, 100)] }
}

fn at(p: u32, row: u16, crop_y: u32, rows: u16) -> kitty::Place {
    kitty::Place { id: 0, p, col: 0, row, crop_x: 0, crop_y, crop_w: 100, crop_h: rows as u32 * 20, cols: 10, rows, z: 0 }
}

#[test]
fn an_image_in_pieces_is_sent_and_placed_as_its_pieces() {
    let mut s = Screen::new(1);
    let mut out = Vec::new();
    s.show(vec![Want { slot: Slot::List, key: 1, png: Some(strips()), places: vec![at(1, 0, 0, 15)] }], &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(out.matches("a=t,f=100").count(), 3, "a piece each");
    // its 15 rows: 5 from the first strip, 5 from the second, 5 from the third
    for (id, y, r) in [(1, 0, 5), (2, 0, 5), (3, 0, 5)] {
        assert!(out.contains(&format!("a=p,i={id},p=1,x=0,y={y},w=100,h=100,c=10,r={r},")), "{id}: {out:?}");
    }
    // scrolled by half a cell: the cells straddling a strip's edge are in the one above
    let mut out = Vec::new();
    s.show(vec![Want { slot: Slot::List, key: 1, png: None, places: vec![at(1, 0, 10, 14)] }], &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("a=p,i=1,p=1,x=0,y=10,w=100,h=100,c=10,r=5,"), "{out:?}");
    assert!(out.contains("a=p,i=2,p=1,x=0,y=10,w=100,h=100,c=10,r=5,"), "{out:?}");
    assert!(out.contains("a=p,i=3,p=1,x=0,y=10,w=100,h=80,c=10,r=4,"), "{out:?}");
    assert!(!out.contains("a=d,"), "each piece's placement replaced in place: {out:?}");
    // fewer rows: the third strip's placement is taken off; hiding takes off every piece
    let mut out = Vec::new();
    s.show(vec![Want { slot: Slot::List, key: 1, png: None, places: vec![at(1, 0, 0, 8)] }], &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("a=d,d=i,i=3,p=1,"), "{out:?}");
    let mut out = Vec::new();
    s.clear(&mut out);
    assert_eq!(String::from_utf8(out).unwrap().matches("a=d,d=I,").count(), 3);
}
