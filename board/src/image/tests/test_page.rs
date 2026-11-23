use super::*;
use crate::image::render::Grid;
use crate::image::theme::Theme;
use crate::model::TextSize;

fn canvas(theme: Theme) -> Canvas {
    Canvas::new(Grid { cols: 60, cell_w: 10, cell_h: 20 }, TextSize::Medium, theme)
}

fn page(said: usize) -> Page {
    Page {
        id: "BP-219".into(),
        place: "bullpen · ~/dev/bullpen".into(),
        title: "Drawn tour: type at the terminal's own size, code on the cell grid".into(),
        mark: Mark::Still("ready".into()),
        status: "ready".into(),
        words: "ready to accept".into(),
        age: "1h".into(),
        keys: vec![("p".into(), "peek".into()), ("⏎".into(), "open".into()), ("a".into(), "accept".into())],
        lines: vec![("Local branch, approved by full review.".into(), Ink::Soft), ("14 files, +455 −228".into(), Ink::Dim)],
        said: (0..said).map(|i| format!("Paragraph {i} of what its worker said, long enough to wrap onto a second line here.")).collect(),
    }
}

#[test]
fn the_page_is_as_tall_as_what_it_says_and_no_taller_than_it_may_be() {
    let mut fonts = Fonts::builtin();
    let c = canvas(Theme::default());
    let short = rows(&page(0), &c, &mut fonts, 100);
    let long = rows(&page(6), &c, &mut fonts, 100);
    assert!(short >= 6 && long > short + 6, "{short} {long}");
    assert_eq!(rows(&page(6), &c, &mut fonts, 9), 9);
    let pm = draw(&page(6), 9, &c, &mut fonts).unwrap();
    assert_eq!((pm.width(), pm.height()), (600, 180));
}

#[test]
fn the_status_is_in_its_colour_in_both_themes() {
    for theme in [Theme::default(), Theme::light()] {
        let c = canvas(theme);
        let mut fonts = Fonts::builtin();
        let n = rows(&page(1), &c, &mut fonts, 100);
        let pm = draw(&page(1), n, &c, &mut fonts).unwrap();
        let green = solid(lit(theme.green));
        let found = pm.pixels().iter().any(|p| {
            let p = p.demultiply();
            p.alpha() == 255 && [p.red(), p.green(), p.blue()] == green[..3]
        });
        assert!(found, "no green status");
    }
}
