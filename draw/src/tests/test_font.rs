use super::*;

#[test]
fn tabular_figures_are_one_width_and_inters_own_not_a_narrow_one_spaced_out() {
    let f = Fonts::builtin();
    for weight in [Weight::Regular, Weight::Bold] {
        let w = |s: &str| f.width(&tabular(s), weight, 20.0);
        assert_eq!(w("BP-111"), w("BP-208"));
        assert!(f.width("BP-111", weight, 20.0) < w("BP-111"));
        // the face's "one.tf", with its foot: its ink is wider than a plain 1's
        let ink = |s: &str| {
            let (_, face, g) = f.glyph_of(weight, s.chars().next().unwrap());
            face.glyph_bounding_box(g).map(|b| b.x_max - b.x_min).unwrap()
        };
        assert!(ink(&tabular("1")) > ink("1") + 200);
    }
    // the rest of the text as it was
    assert_eq!(tabular("BP-12 ·"), "BP-\u{E001}\u{E002} ·");
}

#[test]
fn a_font_without_tabular_figures_draws_its_own() {
    let f = Fonts::mono();
    assert_eq!(f.width(&tabular("1"), Weight::Regular, 20.0), f.width("1", Weight::Regular, 20.0));
}
