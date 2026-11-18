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
