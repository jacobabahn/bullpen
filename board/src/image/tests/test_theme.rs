use super::*;

#[test]
fn a_colour_is_read_as_terminals_say_it() {
    assert_eq!(parse_color("rgb:ffff/8080/0000"), Some(Rgb(255, 128, 0)));
    assert_eq!(parse_color("rgb:ff/80/00"), Some(Rgb(255, 128, 0)));
    assert_eq!(parse_color("rgb:f/8/0"), Some(Rgb(255, 136, 0)));
    assert_eq!(parse_color("rgba:ffff/0000/0000/ffff"), Some(Rgb(255, 0, 0)));
    assert_eq!(parse_color("#1e1e2e"), Some(Rgb(0x1e, 0x1e, 0x2e)));
    assert_eq!(parse_color("rgb:zz/00/00"), None);
    assert_eq!(parse_color("blue"), None);
}

#[test]
fn what_the_terminal_said_is_used_and_the_rest_is_the_default() {
    let said = Said { fg: Some(Rgb(1, 2, 3)), bg: None, palette: vec![(1, Rgb(9, 9, 9)), (4, Rgb(7, 7, 7))] };
    let t = Theme::default().with(&said);
    let d = Theme::default();
    assert_eq!((t.fg, t.bg, t.red, t.green, t.blue), (Rgb(1, 2, 3), d.bg, Rgb(9, 9, 9), d.green, Rgb(7, 7, 7)));
}

#[test]
fn status_and_risk_colours_mean_what_the_text_boards_do() {
    let t = Theme::default();
    assert_eq!(t.status("needs"), Some(solid(t.yellow)));
    assert_eq!(t.status("stopped"), Some(solid(t.red)));
    assert_eq!(t.status("ready"), Some(solid(t.green)));
    assert_eq!(t.status("review"), Some(solid(t.blue)));
    assert_eq!(t.status("todo"), None);
    assert_eq!((t.risk("high"), t.risk("medium"), t.risk("low")), (solid(t.red), solid(t.yellow), solid(t.green)));
}

/// The contrast of colour `c` drawn over `bg`, as WCAG works it out.
fn contrast(c: [u8; 4], bg: Rgb) -> f32 {
    let over = |f: u8, b: u8| (f as f32 * c[3] as f32 + b as f32 * (255 - c[3]) as f32) / 255.0;
    let lin = |v: f32| {
        let v = v / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let lum = |r: f32, g: f32, b: f32| 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    let a = lum(over(c[0], bg.0), over(c[1], bg.1), over(c[2], bg.2));
    let b = lum(bg.0 as f32, bg.1 as f32, bg.2 as f32);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn dim_text_reads_on_a_light_theme_and_a_dark_one() {
    for t in [Theme::light(), Theme::default()] {
        assert!(contrast(t.dim(), t.bg) >= 4.5, "{t:?}");
    }
}

#[test]
fn a_theme_is_named_light_or_dark() {
    assert_eq!(Theme::named("light"), Some(Theme::light()));
    assert_eq!(Theme::named("dark"), Some(Theme::default()));
    assert_eq!(Theme::named("blue"), None);
}

#[test]
fn dim_text_reads_on_the_wash_behind_a_task_that_needs_you() {
    use crate::ui::rows::Flag;
    for t in [Theme::light(), Theme::default()] {
        for flag in [Flag::Needs, Flag::Stopped] {
            let w = t.wash(flag);
            let over = |f: u8, b: u8| ((f as f32 * w[3] as f32 + b as f32 * (255 - w[3]) as f32) / 255.0).round() as u8;
            let bg = Rgb(over(w[0], t.bg.0), over(w[1], t.bg.1), over(w[2], t.bg.2));
            assert!(contrast(t.dim(), bg) >= 4.5, "{flag:?} on {t:?}: {}", contrast(t.dim(), bg));
        }
    }
}
