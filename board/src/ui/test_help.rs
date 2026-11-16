use crate::model::Layout;
use crate::ui::testing::*;
use crate::Mode;

#[test]
fn the_help_fits_a_narrow_board_and_scrolls_to_its_end() {
    let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
    a.mode = Mode::Help { top: 0 };
    let s = screen(&mut a, 42, 24);
    assert_eq!(s[..6], [
        " Board keys",
        "",
        " Moving",
        "   j k   select a task (↑ ↓ too)",
        "   PgDn  a page down (ctrl+d: half)",
        "   PgUp  a page up (ctrl+u: half)",
    ].map(String::from), "{s:#?}");
    // one short line per key: nothing is cut or wrapped at 42 columns
    assert!(!s.iter().any(|r| r.ends_with('…')), "{s:#?}");
    assert!(s.iter().all(|r| !r.starts_with("        ")), "{s:#?}");
    assert!(row(&s, 2).ends_with("↓"), "{s:#?}");
    assert_eq!(row(&s, 1), " j k space scroll · esc returns");
    assert_eq!(row(&s, 0), "", "the gap for Rex's capsules");

    // scrolled past the end, it stops at the last line: the marks' legend
    a.mode = Mode::Help { top: 999 };
    let s = screen(&mut a, 42, 24);
    let Mode::Help { top } = a.mode else { panic!() };
    assert!(top < 999 && top > 0);
    assert!(s[1].ends_with("↑"), "{s:#?}");
    assert_eq!(row(&s, 2), "   ◇     an idea on the list", "{s:#?}");
    for line in ["   ▌     needs you; red: stopped", "   ◌↻    came from the backlog", "   ●     needs you", "   ✔     ready to accept", "   ◜ ◐   in review (it turns)", "   ·     accepted"] {
        assert!(s.contains(&line.to_string()), "{line}: {s:#?}");
    }

    // narrower, a long line wraps under its own text
    a.mode = Mode::Help { top: 0 };
    let s = screen(&mut a, 30, 24);
    let i = s.iter().position(|r| r.starts_with("   ⏎")).unwrap();
    assert_eq!(s[i..i + 2], ["   ⏎     open its pane, or", "         its reviewer's"].map(String::from), "{s:#?}");

    // tall enough for all of it: no scrolling to offer
    let s = screen(&mut a, 42, 80);
    assert_eq!(row(&s, 1), " any key returns");
    assert!(!s.iter().any(|r| r.ends_with('↓') || r.ends_with('↑')), "{s:#?}");
}
