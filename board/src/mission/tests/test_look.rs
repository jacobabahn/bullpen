use super::*;
use crate::image::Picture;
use crate::mission::testing::{key, press, screen, t, State};
use crate::model::Config;
use serde_json::{json, Value};

/// Mission control on four repos: one whose coordinator is blocked, one working, one
/// idle, one with none; tasks in several statuses. With the image look, at the
/// default type.
fn fleet(s: &State) -> Mission {
    let co = |id: &str| json!([{"id": id, "block": format!("block:{id}"), "session": "s"}]);
    s.repo("alpha", co("c1"), &[t("BP-1", "needs", "Answer me", 9), t("BP-2", "ready", "Take me", 30), t("BP-3", "todo", "Later", 60)]);
    s.repo("bravo", co("c2"), &[t("BP-1", "working", "On it", 5), t("BP-2", "review", "Look at it", 8), t("BP-3", "stopped", "Gone quiet", 3)]);
    s.repo("charlie", co("c3"), &[t("BP-1", "triage", "Its coordinator's", 2)]);
    s.repo("delta", json!([]), &[]);
    std::fs::create_dir_all(s.0.join("doing")).unwrap();
    std::fs::write(s.0.join("doing/block_c1.json"), json!({"state": "blocked", "text": "Allow `git push`?"}).to_string()).unwrap();
    std::fs::write(s.0.join("doing/block_c2.json"), json!({"state": "working", "text": ""}).to_string()).unwrap();
    let mut m = s.mission();
    m.cfg = Config::default_for_test();
    m.look = Look::Image;
    m
}

/// Draw a frame and show it, as the loop does (again at once when the picture says so):
/// what was written to the terminal.
fn frame(m: &mut Mission, p: &mut Picture, w: u16, h: u16) -> String {
    let mut out = Vec::new();
    loop {
        m.image = p.drawing(m);
        for app in m.boards.values_mut() {
            app.image = m.image;
        }
        screen(m, w, h);
        if !p.show(m, &mut out) {
            return String::from_utf8(out).unwrap();
        }
    }
}

/// The uploads written: each image's ID.
fn sent(out: &str) -> Vec<u32> {
    out.split("\x1b_Ga=t,f=100,t=d,i=").skip(1).filter_map(|c| c.split(',').next()?.parse().ok()).collect()
}

/// The placements written: image and its rows (first, how many).
fn placed(out: &str) -> Vec<(u32, u16, u16)> {
    let field = |c: &str, k: &str| c.split(',').find_map(|f| f.strip_prefix(k)).and_then(|v| v.parse::<u32>().ok());
    out.split("\x1b7\x1b[")
        .skip(1)
        .filter_map(|c| {
            let row = c.split(';').next()?.parse().ok()?;
            let control = c.split("\x1b_G").nth(1)?;
            Some((field(control, "i=")?, row, field(control, "r=")? as u16))
        })
        .collect()
}

#[test]
fn the_image_look_leaves_every_cell_blank_and_places_the_header_list_page_and_keys() {
    let s = State::new("look-whole");
    let mut m = fleet(&s);
    let mut p = Picture::answered_for_test((10, 20));
    let out = frame(&mut m, &mut p, 160, 50);
    assert!(screen(&mut m, 160, 50).iter().all(String::is_empty));
    let v = m.view.clone().unwrap();
    // the header has a line of the bar; the keys and a message row under the list
    assert_eq!((v.row0, v.row, v.foot_n, v.full, v.cols), (0, 3, 2, 159, 71));
    let side = v.side.as_ref().unwrap();
    assert_eq!((side.col, side.cols), (72, 87));
    assert!(sent(&out).len() >= 4, "{out:?}");
    // the header on the top rows, the page from the list's first row
    let places = placed(&out);
    assert!(places.iter().any(|(_, row, rows)| (*row, *rows) == (1, 3)));
    assert!(places.iter().any(|(_, row, _)| *row == 4));
    assert!(places.iter().any(|(_, row, rows)| (*row, *rows) == (49, 2)));
    // nothing changed: nothing is sent
    assert_eq!(frame(&mut m, &mut p, 160, 50), "");
}

#[test]
fn the_list_is_the_boards_rows_with_a_heading_for_each_repository_and_the_inbox_naming_theirs() {
    let s = State::new("look-rows");
    let mut m = fleet(&s);
    let rows = list(&m, true, crate::model::now()).rows;
    let heads: Vec<String> = rows.iter().filter_map(|r| if let Row::Head { name, count } = r { Some(format!("{name} {count}")) } else { None }).collect();
    assert_eq!(heads, vec!["Needs you 2", "Ready to accept 1"]);
    let repos: Vec<&RepoRow> = rows.iter().filter_map(|r| if let Row::Repo(r) = r { Some(r) } else { None }).collect();
    assert_eq!(repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["alpha", "bravo", "charlie", "delta"]);
    assert_eq!(repos[0].question.as_deref(), Some("c1 asks: Allow `git push`?"));
    assert_eq!(repos[1].coordinators, vec![("c2 working".to_string(), "working")]);
    assert_eq!(repos[3].coordinators, vec![("no coordinator".to_string(), "gone")]);
    // the inbox's tasks name their repository; the selected one has ⏎ in its corner
    let inbox: Vec<(String, String, bool)> = rows
        .iter()
        .filter_map(|r| if let Row::Task(t) = r { Some((t.repo.clone(), t.id.clone(), t.selected)) } else { None })
        .take(3)
        .collect();
    assert_eq!(inbox, vec![("alpha".into(), "BP-1".into(), true), ("bravo".into(), "BP-3".into(), false), ("alpha".into(), "BP-2".into(), false)]);
    let sel = rows.iter().find_map(|r| if let Row::Task(t) = r { t.key.clone() } else { None });
    assert_eq!(sel, Some(("⏎".into(), "open".into())));
    // the base image's rows have nothing selected
    assert!(list(&m, false, crate::model::now()).rows.iter().all(|r| !r.is_selected()));
    press(&mut m, 'j');
    assert!(list(&m, true, crate::model::now()).rows.iter().any(|r| matches!(r, Row::Task(t) if t.selected && t.id == "BP-3")));
}

#[test]
fn j_and_k_drawn_ahead_only_place_images() {
    let s = State::new("look-jk");
    let mut m = fleet(&s);
    let mut p = Picture::answered_for_test((10, 20));
    frame(&mut m, &mut p, 160, 50);
    let area = ratatui::layout::Rect::new(0, 0, 160, 50);
    let mut sink = Vec::new();
    while p.ahead(&mut m, area, &mut sink) {}
    // what j wants was sent ahead: the key only places
    press(&mut m, 'j');
    let out = frame(&mut m, &mut p, 160, 50);
    assert!(sent(&out).is_empty(), "j sent {:?}", sent(&out));
    assert!(!placed(&out).is_empty() && out.len() < 2000, "{} bytes", out.len());
    // drawing ahead put back what the key changed: the selection moved once
    assert_eq!(crate::mission::testing::selected(&m), "bravo BP-3");
}

#[test]
fn a_narrow_pane_has_no_page_and_wraps_the_fleets_bar_rather_than_cut_a_name() {
    let s = State::new("look-narrow");
    for name in ["a-long-repository-name", "another-long-one", "and-a-third-one"] {
        s.repo(name, json!([]), &[t("BP-1", "todo", "A task", 1)]);
    }
    let mut m = s.mission();
    m.cfg = Config::default_for_test();
    m.look = Look::Image;
    let mut p = Picture::answered_for_test((10, 20));
    frame(&mut m, &mut p, 50, 40);
    let v = m.view.clone().unwrap();
    assert!(v.side.is_none());
    let f = v.fleet.unwrap();
    assert!(f.lines.len() >= 2 && f.name == "Mission", "{:?}", f.lines);
    assert_eq!(v.row, crate::image::fleet::rows(f.lines.len()));
}

#[test]
fn every_image_is_under_the_terminals_limit_and_cut_up_covers_its_cells_once() {
    // a large floating pane, large type, a long page and many tasks
    let s = State::new("look-limit");
    let long = "A title long enough that it wraps to a second line and more, and more again so";
    let summary: String = (0..40).map(|i| format!("Paragraph {i}: what its worker said, at length, so the page fills the pane.\n")).collect();
    let mut tasks: Vec<Value> = (1..=40).map(|i| t(&format!("BP-{i}"), ["todo", "ready", "review", "working"][i % 4], long, i as i64)).collect();
    tasks[1]["summary"] = json!(summary);
    s.repo("alpha", json!([]), &tasks);
    let mut m = s.mission();
    m.cfg = Config::default_for_test().with(json!({"board_text_size": "large"}));
    m.look = Look::Image;
    let mut p = Picture::answered_for_test((19, 37));
    let out = frame(&mut m, &mut p, 211, 65);
    let sizes = bullpen_draw::cut::uploaded(&out);
    assert!(sizes.len() >= 4);
    for (w, h) in &sizes {
        assert!(bullpen_draw::cut::fits(*w, *h), "an image {w}×{h}: {} bytes", w * h * 4);
    }
    // the list's rows and the page's each covered once at most by what's placed under the marks
    let under: String = out.split("\x1b7").filter(|c| !c.contains(",z=")).collect::<Vec<_>>().join("\x1b7");
    let v = m.view.clone().unwrap();
    let mut list = vec![0; 65];
    let mut page = vec![0; 65];
    let field = |c: &str, k: &str| c.split(',').find_map(|f| f.strip_prefix(k)).and_then(|v| v.parse::<u16>().ok());
    for c in under.split("\x1b7\x1b[").skip(1) {
        let (Some(row), Some(col)) = (c.split(';').next().and_then(|r| r.parse::<u16>().ok()), c.split(';').nth(1).and_then(|r| r.split('H').next()?.parse::<u16>().ok())) else { continue };
        let Some(rows) = c.split("\x1b_G").nth(1).and_then(|k| field(k, "r=")) else { continue };
        let which = if col > v.side.as_ref().unwrap().col { &mut page } else { &mut list };
        (row - 1..row - 1 + rows).for_each(|r| which[r as usize] += 1);
    }
    assert!(list.iter().chain(&page).all(|k| *k <= 1), "{list:?} {page:?}");
}

#[test]
fn while_a_line_is_typed_or_quit_is_asked_the_bottom_rows_are_text() {
    let s = State::new("look-typed");
    let mut m = fleet(&s);
    let mut p = Picture::answered_for_test((10, 20));
    frame(&mut m, &mut p, 160, 50);
    press(&mut m, 'q');
    frame(&mut m, &mut p, 160, 50);
    assert!(m.view.as_ref().unwrap().foot.is_none());
    let rows = screen(&mut m, 160, 50);
    assert!(rows.last().unwrap().contains("y quits mission control"), "{rows:?}");
    key(&mut m, ratatui::crossterm::event::KeyCode::Esc);
    frame(&mut m, &mut p, 160, 50);
    assert!(m.view.as_ref().unwrap().foot.is_some());
}

#[test]
fn the_text_look_or_a_terminal_without_images_draws_mission_control_as_text() {
    let s = State::new("look-text");
    let mut m = fleet(&s);
    m.look = Look::Text;
    let mut p = Picture::answered_for_test((10, 20));
    assert_eq!(frame(&mut m, &mut p, 160, 50), "");
    assert!(m.view.is_none() && screen(&mut m, 160, 50)[0].starts_with(" MISSION CONTROL"));
    // a fall back says why, and is text from then on
    m.look = Look::Image;
    m.fall_back("Drawing the list as text: the terminal doesn't draw kitty graphics.".into());
    assert_eq!(m.look, Look::Text);
    assert!(m.msg.text.contains("kitty graphics"));
}

#[test]
fn render_writes_mission_control_as_the_image_look_draws_it() {
    let s = State::new("look-render");
    let mut m = fleet(&s);
    let path = s.0.join("mission.png");
    render(&mut m, 120, (10, 20), crate::image::Theme::light(), &path).unwrap();
    let png = std::fs::read(&path).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    assert_eq!(width, 1190);
}

#[test]
fn a_new_cell_size_draws_every_image_again_at_it() {
    // a floating pane opens at a placeholder cell and is sized a moment later: the
    // images are drawn at the cell the pane has each frame, never stretched
    let s = State::new("look-cell");
    let mut m = fleet(&s);
    let mut p = Picture::answered_for_test((8, 16));
    let first = frame(&mut m, &mut p, 160, 50);
    assert!(first.contains("w=1272,")); // the header, 159 columns of 8 pixels
    p.answered(&crate::image::term::Answers { done: true, cell: Some((19, 37)), ..Default::default() });
    let out = frame(&mut m, &mut p, 160, 50);
    assert!(sent(&out).len() >= 4 && out.contains("w=3021,"), "{}", sent(&out).len());
    assert!(!out.contains("w=1272,"));
}

#[test]
fn a_peek_over_mission_control_is_text_and_takes_the_images_off_the_screen() {
    let s = State::new("look-peek");
    let mut m = fleet(&s);
    let mut p = Picture::answered_for_test((10, 20));
    frame(&mut m, &mut p, 160, 50);
    let dir = m.sel.clone().unwrap().0;
    m.boards.get_mut(&dir).unwrap().mode = crate::app::Mode::Overlay { title: "BP-1".into(), lines: vec!["its screen".into()], peek: Some("BP-1".into()) };
    let out = frame(&mut m, &mut p, 160, 50);
    assert!(m.view.is_none() && sent(&out).is_empty());
    assert!(out.contains("a=d,d=i,"), "{out:?}");
    assert!(screen(&mut m, 160, 50).iter().any(|l| l.contains("its screen")));
}
