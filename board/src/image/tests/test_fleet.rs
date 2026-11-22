use super::*;
use crate::image::render::{lay, render, Cut, Grid};
use crate::image::theme::Theme;
use crate::model::TextSize;
use crate::ui::rows::{Row, Rows};

fn canvas(cols: usize, size: TextSize, theme: Theme) -> Canvas {
    Canvas::new(Grid { cols, cell_w: 10, cell_h: 20 }, size, theme)
}

fn repo(name: &str, word: &'static str, tasks: &[&'static str], said: &str) -> FleetRepo {
    FleetRepo { name: name.into(), word, tasks: tasks.to_vec(), said: said.into() }
}

fn four() -> Vec<FleetRepo> {
    vec![
        repo("ai-overview-searxng", "blocked", &["ready", "ready", "todo"], "2 ready"),
        repo("bullpen", "working", &["stopped", "needs", "triage", "ready", "review", "working", "todo"], "2 need you · 1 ready"),
        repo("chat-egw", "idle", &["needs", "working"], "1 needs you"),
        repo("jj-evolved", "gone", &["finished"], "1 ready"),
    ]
}

/// The pixels of a block of the image, straight RGBA: columns [x0, x1), rows [y0, y1).
fn block(pm: &Pixmap, (x0, x1): (u32, u32), (y0, y1): (u32, u32)) -> Vec<[u8; 4]> {
    let w = pm.width();
    let mut out = Vec::new();
    for y in y0..y1.min(pm.height()) {
        for x in x0..x1.min(w) {
            let c = pm.pixels()[(y * w + x) as usize].demultiply();
            out.push([c.red(), c.green(), c.blue(), c.alpha()]);
        }
    }
    out
}

fn has(px: &[[u8; 4]], c: [u8; 4]) -> bool {
    px.iter().any(|p| p[3] == 255 && p[..3] == c[..3])
}

#[test]
fn the_repositories_go_on_one_line_while_their_names_fit_and_wrap_rather_than_be_cut() {
    let wide = lines(four(), 211);
    assert_eq!(wide.iter().map(Vec::len).collect::<Vec<_>>(), vec![4]);
    // narrow: no name is cut, so the last starts a line of its own
    let narrow = lines(four(), 64);
    assert_eq!(narrow.iter().map(Vec::len).collect::<Vec<_>>(), vec![3, 1]);
    for line in &narrow {
        let need: usize = line.iter().map(|r| crate::model::width(&r.name) + 3).sum::<usize>() + 2 * (line.len() - 1);
        assert!(need <= 61, "{need}");
    }
    // a name longer than the pane has a line to itself
    let long = lines(vec![repo(&"x".repeat(80), "idle", &[], ""), repo("b", "idle", &[], "")], 40);
    assert_eq!(long.len(), 2);
}

#[test]
fn the_header_is_three_rows_for_a_line_of_the_bar_and_more_for_each_line_after() {
    assert_eq!(rows(1), 3);
    assert_eq!(rows(2), 5);
    assert_eq!(rows(0), 2);
}

#[test]
fn the_header_draws_each_repository_s_run_its_name_and_its_coordinator_in_amber_when_blocked() {
    for theme in [Theme::default(), Theme::light()] {
        let c = canvas(64, TextSize::Medium, theme);
        let f = Fleet {
            name: "Mission".into(),
            after: "3/13 · 4 repos".into(),
            chips: vec![("1 coordinator blocked".into(), "c1".into(), "needs"), ("3 need you".into(), "3".into(), "needs")],
            clock: String::new(),
            lines: lines(four(), 64),
        };
        let n = rows(f.lines.len());
        let pm = head(&f, n, &c, &mut Fonts::builtin()).unwrap();
        assert_eq!((pm.width(), pm.height()), (640, n as u32 * 20));
        let amber = solid(theme.yellow);
        // the blocked repository's name (the first, at the left of the first line's names) is amber
        let names = block(&pm, (0, 200), (40, 60));
        assert!(has(&names, amber), "no amber in the blocked repository's name");
        // the second line of the bar has its run and its name
        assert!(block(&pm, (0, 640), (72, 100)).iter().any(|p| p[3] > 0));
    }
}

#[test]
fn a_blocked_repository_s_heading_is_washed_amber_with_its_question_under_it() {
    let c = canvas(60, TextSize::Medium, Theme::default());
    let heading = |question: Option<&str>| Row::Repo(RepoRow {
        name: "bullpen".into(),
        place: "~/dev/bullpen · 5 open".into(),
        coordinators: vec![("c4 blocked".into(), if question.is_some() { "blocked" } else { "idle" })],
        question: question.map(String::from),
    });
    let mut fonts = Fonts::builtin();
    let draw = |row: Row, fonts: &mut Fonts| {
        let rows = Rows { rows: vec![row], idw: 0 };
        let laid = lay(&rows, &c, fonts);
        let h = laid.tall(0, 1);
        (h, render(&rows, &laid, Cut::rows(0..1, h), &c, fonts).unwrap().pixmap)
    };
    let (quiet_h, quiet) = draw(heading(None), &mut fonts);
    let (blocked_h, blocked) = draw(heading(Some("c4 asks: Allow `git push`?")), &mut fonts);
    // its question takes a line more
    assert!(blocked_h > quiet_h + c.m.body_line as u32 - 2);
    // the bar at its left edge is amber; a quiet one has none
    let edge = |pm: &Pixmap| block(pm, (6, 9), (blocked_h / 2, blocked_h / 2 + 1));
    let amber = solid(lit(Theme::default().yellow));
    assert!(has(&edge(&blocked), amber));
    assert!(!has(&edge(&quiet), amber));
}

#[test]
fn a_narrow_heading_keeps_its_name_and_the_coordinators_marks_and_drops_the_place_first() {
    let mut fonts = Fonts::builtin();
    let c = canvas(30, TextSize::Medium, Theme::default());
    let r = RepoRow {
        name: "ai-overview-searxng".into(),
        place: "~/dev/ai-overview-searxng · 3 open".into(),
        coordinators: vec![("c2 working".into(), "working"), ("c3 idle".into(), "idle")],
        question: None,
    };
    let mut pm = Pixmap::new(300, 60).unwrap();
    let mut d = Draw::new(&mut pm, &mut fonts, &c, 0.0);
    let (full, short) = (d.coordinators_width(&r, c.m.detail, true), d.coordinators_width(&r, c.m.detail, false));
    assert!(short < full);
    d.repo(&r, 0.0, 40.0);
    assert_eq!(super::short("c2 working"), "c2");
    assert_eq!(super::short("no coordinator"), "none");
}
