use super::*;

fn white(pm: &Pixmap, x: u32, y: u32) -> bool {
    pm.pixel(x, y).expect("in the image").red() == 255
}

#[test]
fn the_patterns_fill_the_rows_under_the_text_to_the_pixel() {
    let pm = sample((19, 37), 2 * 19 * 4, 3 * 37, "ab").expect("a sample");
    let mid = 19 * 4;
    // upright lines every third pixel at the left, lines across at the right
    assert_eq!(
        (0..6).map(|x| white(&pm, x, 40)).collect::<Vec<_>>(),
        [true, false, false, true, false, false]
    );
    assert_eq!(
        (37..43).map(|y| white(&pm, mid + 1, y)).collect::<Vec<_>>(),
        [true, false, false, true, false, false]
    );
    // checkerboards of one-pixel squares, then two-pixel ones
    assert_eq!(
        (0..4).map(|x| white(&pm, x, 74)).collect::<Vec<_>>(),
        [false, true, false, true]
    );
    assert_eq!(
        (0..4).map(|x| white(&pm, x, 75)).collect::<Vec<_>>(),
        [true, false, true, false]
    );
    assert_eq!(
        (0..4).map(|x| white(&pm, mid + x, 74)).collect::<Vec<_>>(),
        [true, true, false, false]
    );
    assert!(
        pm.pixels().iter().all(|p| p.alpha() == 255),
        "opaque, so the pane shows nothing through"
    );
}

#[test]
fn the_text_is_the_tours_code_on_the_first_row() {
    let pm = sample((19, 37), 19 * 4, 3 * 37 + 1, "m").expect("a sample");
    let row = |x0: u32, x1: u32| {
        (x0..x1)
            .flat_map(|x| (0..37).map(move |y| (x, y)))
            .filter(|&(x, y)| white(&pm, x, y))
            .count()
    };
    assert!(row(0, 19) > 20, "the m is drawn in the first cell");
    assert_eq!(row(19, 19 * 4), 0, "and nothing after it");
    assert_eq!(
        pm.height(),
        3 * 37 + 1,
        "the image a pixel over its cells keeps its size"
    );
}
