use super::*;

/// A PNG's first bytes, enough for its size: `w` by `h`.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b
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
    // and so many pixels: one image as big as all of them leaves just itself
    s.preload(vec![Want { png: Some(png(10_000, 2_400)), ..want(Slot::List, 1) }], &mut Vec::new()).unwrap();
    assert_eq!(s.kept().len(), 1);
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
