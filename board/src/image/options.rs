//! (Tests only, for BP-223's choice.) Three directions for mission control drawn as
//! images, composed from the board's own pieces (render.rs's rows, cards, washes, marks,
//! chips and keycaps), at a pane's size: `MISSION_STATE=<state folder>
//! MISSION_OUT=<folder> bin/bullpen-cargo test -p bullpen-board mission_options --
//! --ignored`. A child of render.rs, so it draws with `Draw`'s own parts.

use super::*;
use crate::image::frame;
use crate::mission::items::{self, Item, Row as MRow};
use crate::mission::{MMsg, Mission};
use crate::ui::bottom::{Foot, FootLine};
use crate::ui::rows;
use bullpen_core::fleet::RepoState;
use tiny_skia::{PathBuilder, PixmapPaint};

type Now = bullpen_core::timeutil::Stamp;

/// A pane: its cells and a cell's pixels.
#[derive(Clone, Copy)]
struct Pane {
    cols: usize,
    rows: usize,
    cw: u32,
    ch: u32,
}

impl Pane {
    fn canvas(&self, cols: usize, theme: Theme) -> Canvas {
        Canvas::new(Grid { cols, cell_w: self.cw, cell_h: self.ch }, TextSize::Medium, theme)
    }
    fn wide(&self) -> bool {
        self.cols >= 80
    }
}

fn pixmap(w: u32, h: u32) -> Pixmap {
    Pixmap::new(w.max(1), h.max(1)).expect("an image")
}

fn blit(dst: &mut Pixmap, src: &Pixmap, x: u32, y: u32) {
    dst.draw_pixmap(x as i32, y as i32, src.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

/// Parts one under the other, from y, clipped at `bottom`.
fn stack(dst: &mut Pixmap, parts: &[Pixmap], x: u32, mut y: u32, bottom: u32) {
    for p in parts {
        if y >= bottom {
            break;
        }
        let h = p.height().min(bottom - y);
        if let Some(cut) = p.clone_rect(tiny_skia::IntRect::from_xywh(0, 0, p.width(), h).unwrap()) {
            blit(dst, &cut, x, y);
        }
        y += p.height();
    }
}

/// Board rows drawn whole, as tall as they are.
fn list_pm(rows: Vec<Row>, c: &Canvas, fonts: &mut Fonts) -> Pixmap {
    let rows = Rows { rows, idw: 0 };
    let laid = lay(&rows, c, fonts);
    let n = rows.rows.len();
    render(&rows, &laid, Cut::rows(0..n, laid.tall(0, n)), c, fonts).expect("drawn").pixmap
}

/// The data: mission control on the fixture, and the selected task.
struct Data {
    m: Mission,
    now: Now,
    sel: Item,
}

impl Data {
    fn repo(&self, i: Item) -> &RepoState {
        &self.m.fleet.repos[i.repo]
    }

    /// A task's board rows: its row and its lines, as its repo's board has them (the
    /// image's). In the inbox it names its repository; selected, its key is ⏎.
    fn task(&self, i: Item, inbox: bool) -> Vec<Row> {
        let r = self.repo(i);
        let app = &self.m.boards[&r.repo.dir];
        let id = items::id(items::task(&self.m.fleet, i));
        let t = app.tasks.iter().find(|t| t.id == id).expect("its task");
        let sel = i == self.sel;
        let mut tr = rows::task_row(app, t, sel, true, self.now);
        if inbox {
            tr.repo = r.repo.name();
        }
        if sel {
            tr.key = Some(("⏎".into(), "open".into()));
        }
        let mut out = vec![Row::Task(tr)];
        rows::details(&mut out, app, t, sel, false, 200, self.now);
        out
    }

    fn inbox(&self) -> Vec<(&'static str, Vec<Item>)> {
        let mut out: Vec<(&'static str, Vec<Item>)> = Vec::new();
        for r in self.m.rows() {
            match r {
                MRow::Head(h) => out.push((if h == "NEEDS YOU" { "Needs you" } else { "Ready to accept" }, vec![])),
                MRow::Task(i, true) => out.last_mut().unwrap().1.push(i),
                _ => {}
            }
        }
        out
    }

    /// Each repo with its other open tasks.
    fn repos(&self) -> Vec<(usize, Vec<Item>)> {
        let mut out: Vec<(usize, Vec<Item>)> = Vec::new();
        for r in self.m.rows() {
            match r {
                MRow::Repo(ri) => out.push((ri, vec![])),
                MRow::Task(i, false) => out.last_mut().unwrap().1.push(i),
                _ => {}
            }
        }
        out
    }

    fn inbox_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (k, (name, items)) in self.inbox().into_iter().enumerate() {
            if k > 0 {
                rows.push(Row::Gap);
            }
            rows.push(Row::Head { name: name.into(), count: items.len() });
            for i in items {
                rows.extend(self.task(i, true));
            }
        }
        rows
    }

    fn tasks_rows(&self, items: &[Item]) -> Vec<Row> {
        items.iter().flat_map(|i| self.task(*i, false)).collect()
    }
}

/// A coordinator's state's mark, drawn at (cx, cy), radius r: working the working
/// task's ring with a quarter drawn, blocked an amber disc, idle a dim ring, gone an
/// amber cross, unknown a dim dotted ring.
fn coordinator_mark(d: &mut Draw, word: &str, cx: f32, cy: f32) {
    match word {
        "working" => d.mark(&Mark::Working, cx, cy, 0.1),
        "blocked" => d.mark(&Mark::Still("needs".into()), cx, cy, 0.0),
        "gone" => {
            let r = d.mark_r() * 0.7;
            let lw = d.line() * 1.2;
            let amber = solid(lit(d.theme.yellow));
            let mut pb = PathBuilder::new();
            pb.move_to(cx - r, cy - r);
            pb.line_to(cx + r, cy + r);
            pb.move_to(cx + r, cy - r);
            pb.line_to(cx - r, cy + r);
            if let Some(p) = pb.finish() {
                d.stroke(&p, lw, amber);
            }
        }
        "unknown" => d.mark(&Mark::Still("todo".into()), cx, cy, 0.0),
        _ => d.mark(&Mark::Still("working".into()), cx, cy, 0.0),
    }
}

/// A coordinator's words' colour and weight: blocked bold amber, gone amber, working
/// the foreground, idle dim.
fn coordinator_ink(theme: &Theme, word: &str) -> (Weight, [u8; 4]) {
    match word {
        "blocked" => (Weight::Bold, solid(theme.yellow)),
        "gone" => (Weight::Regular, solid(theme.yellow)),
        "working" => (Weight::Regular, theme.soft()),
        _ => (Weight::Regular, theme.dim()),
    }
}

/// A repo's coordinators at the right of a line whose top is y, ending at `right`, in
/// type px: each its mark, its ID and its word. Returns where they start.
fn coordinators_at(d: &mut Draw, r: &RepoState, right: f32, y: f32, px: f32) -> f32 {
    let theme = d.theme;
    let mut x = right;
    let list: Vec<(String, &str)> = if r.coordinators.is_empty() {
        vec![("no coordinator".into(), "gone")]
    } else {
        r.coordinators.iter().map(|c| (format!("{} {}", c.id, c.word()), c.word())).collect()
    };
    for (text, word) in list.iter().rev() {
        let (weight, color) = coordinator_ink(&theme, word);
        let w = d.fonts.width(text, weight, px);
        x -= w;
        d.text_in(px, text, weight, x, y, w + 1.0, color);
        let mr = d.mark_r();
        let cx = x - 0.5 * px - mr;
        let cy = y + d.h() / 2.0;
        coordinator_mark(d, word, cx, cy);
        x = cx - mr - 1.2 * px;
    }
    x
}

/// The blocked coordinator's question, if one is.
fn blocked_text(r: &RepoState) -> Option<String> {
    r.coordinators.iter().find(|c| c.word() == "blocked").map(|c| {
        let q = c.text.trim();
        if q.is_empty() { format!("{} is waiting on you in its pane.", c.id) } else { format!("{} asks: {q}", c.id) }
    })
}

/// A repo's heading in the list (A, C): its name, its folder dim, its coordinators at
/// the right; blocked, on the amber wash with its bar, the question under it.
fn repo_heading(data: &Data, ri: usize, c: &Canvas, fonts: &mut Fonts, folder: bool) -> Pixmap {
    let r = &data.m.fleet.repos[ri];
    let m = c.m;
    let q = blocked_text(r);
    let top_room = m.gap + m.pad;
    let h = top_room + m.line + if q.is_some() { m.body_line + m.below } else { m.title_gap };
    let mut pm = pixmap(c.grid.width(), h.round() as u32);
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let y = top_room;
    if q.is_some() {
        d.wash(y, h - m.below + m.below, Flag::Needs);
        d.bar(y, h, Flag::Needs);
    }
    d.set(m.title, m.line);
    let left = c.left();
    let theme = d.theme;
    let name = r.repo.name();
    let end = d.text(&name, Weight::Bold, left, y, c.right() - left, theme.text());
    let start = coordinators_at(&mut d, r, c.right(), y, m.detail);
    if folder {
        let path = items::tilde(&r.repo.root_str());
        let open = items::open_words(r);
        let text = format!("{path}  ·  {open}");
        d.text_in(m.body, &text, Weight::Regular, end + 0.6 * m.title, y, start - end - 1.2 * m.title, theme.dim());
    }
    if let Some(q) = q {
        d.set(m.body, m.body_line);
        let x = d.title_x().min(left + 2.0 * c.c());
        d.text(&q, Weight::Regular, x, y + m.line, c.right() - x, theme.soft());
    }
    pm
}

/// The header: the name, chips at the right (what's blocked, needs you and is ready),
/// and the time.
fn header_line(data: &Data, d: &mut Draw, c: &Canvas, y: f32, line: f32) {
    let t = items::totals(&data.m.fleet);
    let theme = d.theme;
    let name_px = (frame_name() * c.m.title).round();
    d.set(name_px, line);
    let left = c.left();
    let px = c.m.label + 1.0;
    let cy = y + line / 2.0;
    let clock = data.now.format("%a %-d %b %H:%M").to_string();
    let cw = d.fonts.width(&clock, Weight::Regular, c.m.detail);
    let mut x = d.right();
    if c.grid.cols >= 80 {
        d.text_in(c.m.detail, &clock, Weight::Regular, x - cw, y, cw + 1.0, theme.dim());
        x -= cw + 1.2 * c.m.title;
    }
    let mut chips: Vec<(String, &str)> = Vec::new();
    let narrow = c.grid.cols < 80;
    if t.blocked > 0 {
        chips.push((if narrow { format!("c {}", t.blocked) } else { format!("{} coordinator blocked", t.blocked) }, "needs"));
    }
    if t.needs > 0 {
        chips.push((if narrow { t.needs.to_string() } else { format!("{} need you", t.needs) }, "needs"));
    }
    if t.ready > 0 {
        chips.push((if narrow { t.ready.to_string() } else { format!("{} ready", t.ready) }, "ready"));
    }
    for (text, st) in chips.iter().rev() {
        x = chip::chip(d, text, px, x, cy, theme.status(st).unwrap()) - 0.4 * px;
    }
    d.text(if narrow { "Mission" } else { "Mission control" }, Weight::Bold, left, y, x - left, theme.text());
}

fn frame_name() -> f32 {
    1.15
}

/// The status bar, a run of segments for each repository, its name under it: the
/// whole fleet at a glance (A).
fn fleet_bar(data: &Data, d: &mut Draw, c: &Canvas, top: f32) {
    let ch = c.grid.cell_h as f32;
    let (l, r) = (c.left(), c.right());
    let repos = &data.m.fleet.repos;
    let gap_repo = 1.5 * c.grid.cell_w as f32;
    let seg_gap = (0.18 * c.grid.cell_w as f32).round().max(2.0);
    let weights: Vec<f32> = repos.iter().map(|r| (r.open.len() as f32).max(1.0) + 2.0).collect();
    let total: f32 = weights.iter().sum();
    let room = r - l - gap_repo * (repos.len() as f32 - 1.0);
    let h = (0.24 * ch).round().max(2.0);
    let px = c.m.label;
    let theme = d.theme;
    let mut x = l;
    for (rs, w) in repos.iter().zip(&weights) {
        let width = room * w / total;
        let n = rs.open.len();
        if n == 0 {
            d.rounded_fill(x, top, width, h, h / 2.0, theme.rule());
        } else {
            let each = (width - seg_gap * (n as f32 - 1.0)) / n as f32;
            for (k, t) in rs.open.iter().enumerate() {
                let st = items::status(t);
                let color = theme.group(st);
                d.rounded_fill(x + k as f32 * (each + seg_gap), top, each, h, (h / 2.0).floor(), color);
            }
        }
        // its name, and its coordinator's mark
        let y = top + h + 0.12 * ch;
        d.set(px, ch);
        let blocked = items::coordinator_blocked(rs);
        let gone = rs.coordinators.is_empty() || rs.coordinators.iter().any(|c| c.word() == "gone");
        let word = rs.coordinators.first().map_or("gone", |c| c.word());
        let sc = d.scale;
        d.scale = 0.7;
        coordinator_mark(d, word, x + d.mark_r(), y + ch / 2.0);
        let mx = x + 2.0 * d.mark_r() + 0.4 * px;
        d.scale = sc;
        let color = if blocked || gone { solid(theme.yellow) } else { theme.heading() };
        let end = d.text(&rs.repo.name(), if blocked { Weight::Bold } else { Weight::Regular }, mx, y, width - (mx - x), color);
        let said = match (rs.counts.needs, rs.counts.ready) {
            (0, 0) if n == 0 => String::new(),
            (0, 0) => format!("{n} open"),
            (a, 0) => format!("{a} need you"),
            (0, b) => format!("{b} ready"),
            (a, b) => format!("{a} need you · {b} ready"),
        };
        if !said.is_empty() && end + 0.5 * px < x + width {
            d.text(&format!("  {said}"), Weight::Regular, end, y, x + width - end, theme.dim());
        }
        x += width + gap_repo;
    }
}

/// The selected task's detail, as a page in a column (A, B): its title, its repo and
/// folder, its status and age, its keys, what its agent is doing and where it stands,
/// its stat line, and what its worker said.
fn detail_page(data: &Data, c: &Canvas, fonts: &mut Fonts, h: u32) -> Pixmap {
    let mut pm = pixmap(c.grid.width(), h);
    let i = data.sel;
    let r = data.repo(i);
    let app = &data.m.boards[&r.repo.dir];
    let id = items::id(items::task(&data.m.fleet, i));
    let t = app.tasks.iter().find(|t| t.id == id).unwrap().clone();
    let ts = items::task(&data.m.fleet, i).clone();
    let m = c.m;
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let theme = d.theme;
    let (l, right) = (c.left(), c.right());
    let mut y = m.pad;
    // the ID and the repo, small, then the title large
    d.set(m.detail, m.detail_line);
    let end = d.text(&t.id, Weight::Bold, l, y, right - l, theme.dim());
    d.text(&format!("   {}  ·  {}", r.repo.name(), items::tilde(&r.repo.root_str())), Weight::Regular, end, y, right - end, theme.dim());
    y += m.detail_line + 0.2 * m.title;
    let big = (1.1 * m.title).round();
    let line = (1.32 * big).round();
    let lines = d.fonts.wrap(&t.title, Weight::Bold, big, (right - l, right - l), 3);
    d.set(big, line);
    for s in &lines {
        d.text(s, Weight::Bold, l, y, right - l, theme.text());
        y += line;
    }
    y += 0.4 * m.title;
    // the status: its mark and its words
    d.set(m.title, m.line);
    let mark = match items::status(&ts) {
        "working" => Mark::Working,
        "review" => Mark::Review,
        st => Mark::Still(st.into()),
    };
    let mr = d.mark_r();
    d.mark(&mark, l + mr, y + m.line / 2.0, 0.1);
    let age = crate::model::ago(&t, data.now);
    let status = status_word(&t.status);
    let color = theme.status(&t.status).map_or(theme.text(), |c| solid(lit(Rgb(c[0], c[1], c[2]))));
    let end = d.text_in(m.detail, status, Weight::Bold, l + 2.0 * mr + 0.6 * m.title, y, right - l, color);
    if !age.is_empty() {
        d.text_in(m.detail, &format!("  ·  {age}"), Weight::Regular, end, y, right - end, theme.dim());
    }
    y += m.line + 0.3 * m.title;
    // its keys: p and ⏎ first
    let px = m.label;
    let mut x = l;
    let mut keys: Vec<(&str, &str)> = vec![("p", "peek"), ("⏎", "go to its pane")];
    keys.extend(crate::model::task_keys(Some(&t)).into_iter().filter(|(k, _)| !matches!(*k, "p" | "⏎")).take(2));
    for (k, what) in keys {
        x = chip::keycap(&mut d, k, what, px, x, y + m.detail_line / 2.0) + 1.1 * px;
    }
    y += m.detail_line + 0.5 * m.title;
    // what its agent is doing, where it stands, its stat line
    let mut texts: Vec<(String, [u8; 4])> = Vec::new();
    if !ts.doing.is_empty() {
        texts.push((ts.doing.clone(), theme.text()));
    }
    let det = crate::model::detail_with_auto(&t, &app.cfg, data.now);
    if !det.is_empty() {
        texts.push((det, theme.soft()));
    }
    let stat = crate::model::stat_line(&t);
    if !stat.is_empty() {
        texts.push((stat, theme.dim()));
    }
    d.set(m.body, m.body_line);
    for (text, color) in texts {
        for s in d.fonts.wrap(&text, Weight::Regular, m.body, (right - l, right - l), 6) {
            d.text(&s, Weight::Regular, l, y, right - l, color);
            y += m.body_line;
        }
        y += 0.25 * m.body_line;
    }
    let summary = t.summary.as_deref().unwrap_or("").trim().to_string();
    if !summary.is_empty() && matches!(t.status.as_str(), "review" | "ready" | "finished") {
        y += 0.5 * m.title;
        d.rounded_fill(l, y, right - l, 1.5, 0.0, theme.rule());
        y += 0.6 * m.title;
        d.set(m.detail, m.detail_line);
        d.text("What its worker said", Weight::Bold, l, y, right - l, theme.heading());
        y += m.detail_line + 0.2 * m.title;
        d.set(m.body, m.body_line);
        'para: for para in summary.lines().filter(|p| !p.trim().is_empty()) {
            for s in d.fonts.wrap(para, Weight::Regular, m.body, (right - l, right - l), 40) {
                if y + m.body_line > h as f32 {
                    break 'para;
                }
                d.text(&s, Weight::Regular, l, y, right - l, theme.dim());
                y += m.body_line;
            }
            y += 0.4 * m.body_line;
        }
    }
    pm
}

fn status_word(st: &str) -> &str {
    match st {
        "needs" => "needs you",
        "stopped" => "stopped: its agent exited",
        "triage" => "its coordinator is on it",
        "ready" => "ready to accept",
        "review" => "in review",
        "todo" => "queued",
        other => other,
    }
}

/// The bottom rows: the selected task's keys, then mission control's own.
fn foot_pm(data: &Data, p: &Pane, c: &Canvas, fonts: &mut Fonts) -> Pixmap {
    let (app_t, _) = {
        let r = data.repo(data.sel);
        let app = &data.m.boards[&r.repo.dir];
        let id = items::id(items::task(&data.m.fleet, data.sel)).to_string();
        (app.tasks.iter().find(|t| t.id == id).cloned(), ())
    };
    let mut keys: Vec<(String, String)> =
        crate::model::task_keys(app_t.as_ref()).into_iter().map(|(k, w)| (k.to_string(), w.to_string())).collect();
    keys.extend([("n", "next"), ("/", "find"), ("q", "quit")].map(|(k, w)| (k.to_string(), w.to_string())));
    let _ = p;
    let foot = Foot { lines: vec![(1, FootLine::Keys(keys))] };
    frame::foot(&foot, 2, c, fonts).unwrap()
}

/// A vertical rule between two columns, at x, from y0 to y1.
fn vrule(pm: &mut Pixmap, theme: &Theme, x: f32, y0: f32, y1: f32) {
    let rule = theme.rule();
    if let Some(p) = rounded(x, y0, 1.5, y1 - y0, 0.0) {
        pm.fill_path(&p, &paint(rule), FillRule::Winding, Transform::identity(), None);
    }
}

/// A: one list as on the board, the repositories its groups, the detail beside it.
fn option_a(data: &Data, p: Pane, theme: Theme, fonts: &mut Fonts) -> Pixmap {
    let (w, h) = (p.cols as u32 * p.cw, p.rows as u32 * p.ch);
    let mut pm = pixmap(w, h);
    let full = p.canvas(p.cols, theme);
    let ch = p.ch as f32;
    // the header: name and chips, then the fleet's bar
    let head_rows = 4;
    {
        let mut hp = pixmap(w, head_rows * p.ch);
        let mut d = Draw::new(&mut hp, fonts, &full, 0.0);
        header_line(data, &mut d, &full, (0.2 * ch).round(), (1.3 * ch).round());
        fleet_bar(data, &mut d, &full, (1.75 * ch).round());
        blit(&mut pm, &hp, 0, 0);
    }
    let foot = foot_pm(data, &p, &full, fonts);
    blit(&mut pm, &foot, 0, h - foot.height());
    let body_top = head_rows * p.ch;
    let body_bottom = h - foot.height();
    let list_cols = if p.wide() { (p.cols * 45 / 100).clamp(60, 90) } else { p.cols };
    let lc = p.canvas(list_cols, theme);
    let mut parts = vec![list_pm(data.inbox_rows(), &lc, fonts)];
    for (ri, its) in data.repos() {
        parts.push(repo_heading(data, ri, &lc, fonts, p.wide()));
        if !its.is_empty() {
            parts.push(list_pm(data.tasks_rows(&its), &lc, fonts));
        }
    }
    stack(&mut pm, &parts, 0, body_top, body_bottom);
    if p.wide() {
        let dc = p.canvas(p.cols - list_cols, theme);
        let page = detail_page(data, &dc, fonts, body_bottom - body_top);
        blit(&mut pm, &page, list_cols as u32 * p.cw, body_top);
        vrule(&mut pm, &theme, list_cols as f32 * p.cw as f32, body_top as f32 + 0.5 * ch, body_bottom as f32 - 0.5 * ch);
    }
    pm
}

/// A repository's tile (B): its name and folder, its coordinators (blocked: the amber
/// wash and bar, the question), a bar of its open tasks and its counts. The selected
/// task's repository's tile has the selection's blue edge.
fn tile(data: &Data, ri: usize, c: &Canvas, fonts: &mut Fonts, h: f32, selected: bool) -> Pixmap {
    let r = &data.m.fleet.repos[ri];
    let m = c.m;
    let mut pm = pixmap(c.grid.width(), h.round() as u32);
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let theme = d.theme;
    let (x0, x1) = (0.5 * c.c(), c.grid.width() as f32 - 0.5 * c.c());
    let rad = d.radius();
    let q = blocked_text(r);
    let fill = if q.is_some() { theme.wash(Flag::Needs) } else { theme.tint() };
    d.rounded_fill(x0, 0.0, x1 - x0, h - 0.4 * c.c(), rad, fill);
    if q.is_some() {
        if let Some(p) = left_slice(x0, 0.0, (0.25 * c.c()).max(3.0), h - 0.4 * c.c(), rad) {
            d.fill(&p, solid(lit(theme.yellow)));
        }
    }
    if selected {
        d.rounded_edge(x0, 0.0, x1 - x0, h - 0.4 * c.c(), rad, RING, theme.edge());
    }
    let (l, right) = (x0 + 1.2 * c.c(), x1 - 1.0 * c.c());
    let mut y = 0.45 * m.title;
    d.set(m.title, m.line);
    let end = d.text(&r.repo.name(), Weight::Bold, l, y, right - l, theme.text());
    let open = items::open_words(r);
    let ow = d.fonts.width(&open, Weight::Regular, m.body);
    d.text_in(m.body, &open, Weight::Regular, right - ow, y, ow + 1.0, theme.dim());
    let _ = end;
    y += m.line;
    d.set(m.body, m.body_line);
    d.text(&items::tilde(&r.repo.root_str()), Weight::Regular, l, y, right - l, theme.dim());
    y += m.body_line + 0.25 * m.title;
    // each coordinator: its mark, its words, and what it's doing or asks
    d.set(m.detail, m.line);
    for co in &r.coordinators {
        let word = co.word();
        let mr = d.mark_r();
        coordinator_mark(&mut d, word, l + mr, y + m.line / 2.0);
        let (weight, color) = coordinator_ink(&theme, word);
        let x = l + 2.0 * mr + 0.5 * m.title;
        let end = d.text(&format!("{} {}", co.id, word), weight, x, y, right - x, color);
        let said = if word == "blocked" { co.text.trim().to_string() } else { String::new() };
        if !said.is_empty() {
            d.text_in(m.body, &format!("   {said}"), Weight::Regular, end, y, right - end, theme.soft());
        }
        y += m.line;
    }
    if r.coordinators.is_empty() {
        d.text("no coordinator", Weight::Regular, l, y, right - l, solid(theme.yellow));
        y += m.line;
    }
    // its open tasks as the board's bar, and its counts
    y += 0.2 * m.title;
    let bh = (0.22 * c.grid.cell_h as f32).round().max(2.0);
    let n = r.open.len();
    let gap = (0.18 * c.c()).round().max(2.0);
    if n == 0 {
        d.rounded_fill(l, y, right - l, bh, bh / 2.0, theme.rule());
    } else {
        let each = (right - l - gap * (n as f32 - 1.0)) / n as f32;
        for (k, t) in r.open.iter().enumerate() {
            let color = theme.group(items::status(t));
            d.rounded_fill(l + k as f32 * (each + gap), y, each, bh, (bh / 2.0).floor(), color);
        }
    }
    pm
}

/// B: a tile for each repository across the top, then the list (the inbox, and each
/// repository's other tasks under its name) and the detail beside it.
fn option_b(data: &Data, p: Pane, theme: Theme, fonts: &mut Fonts) -> Pixmap {
    let (w, h) = (p.cols as u32 * p.cw, p.rows as u32 * p.ch);
    let mut pm = pixmap(w, h);
    let full = p.canvas(p.cols, theme);
    let ch = p.ch as f32;
    let head_rows = 2;
    {
        let mut hp = pixmap(w, head_rows * p.ch);
        let mut d = Draw::new(&mut hp, fonts, &full, 0.0);
        header_line(data, &mut d, &full, (0.2 * ch).round(), (1.3 * ch).round());
        blit(&mut pm, &hp, 0, 0);
    }
    let sel_repo = data.sel.repo;
    let n = data.m.fleet.repos.len();
    let mut y = head_rows * p.ch;
    let m = full.m;
    if p.wide() {
        let tile_h = (0.9 * m.title + m.line + m.body_line + 0.25 * m.title + m.line + 0.2 * m.title + 0.6 * ch + 0.9 * m.title).round();
        let per = (p.cols - 1) / n;
        for ri in 0..n {
            let c = p.canvas(per, theme);
            let t = tile(data, ri, &c, fonts, tile_h, ri == sel_repo);
            blit(&mut pm, &t, (p.cw as f32 * (1.0 + (ri * per) as f32) - 0.5 * p.cw as f32) as u32, y);
        }
        y += tile_h as u32 + (0.3 * ch) as u32;
    } else {
        // narrow: a line each, the coordinators' marks at the right
        for (ri, rs) in data.m.fleet.repos.iter().enumerate() {
            let lh = (1.25 * ch).round();
            let mut lp = pixmap(w, lh as u32);
            let mut d = Draw::new(&mut lp, fonts, &full, 0.0);
            let blocked = items::coordinator_blocked(rs);
            if blocked || ri == sel_repo {
                let (x0, x1) = (full.left() - full.out(), full.right() + full.out());
                let fill = if blocked { theme.wash(Flag::Needs) } else { theme.tint() };
                d.rounded_fill(x0, 1.0, x1 - x0, lh - 2.0, d.radius(), fill);
                if ri == sel_repo {
                    d.rounded_edge(x0, 1.0, x1 - x0, lh - 2.0, d.radius(), RING, theme.edge());
                }
            }
            d.set(m.detail, lh);
            let end = d.text(&rs.repo.name(), Weight::Bold, full.left(), 0.0, full.right() - full.left(), theme.text());
            let start = coordinators_at(&mut d, rs, full.right(), 0.0, m.label);
            let said = match (rs.counts.needs, rs.counts.ready) {
                (0, 0) => items::open_words(rs),
                (a, 0) => format!("{a} need you"),
                (0, b) => format!("{b} ready"),
                (a, b) => format!("{a} need you · {b} ready"),
            };
            d.text_in(m.body, &format!("  {said}"), Weight::Regular, end, 0.0, start - end - m.title, theme.dim());
            blit(&mut pm, &lp, 0, y);
            y += lh as u32;
        }
        y += (0.3 * ch) as u32;
    }
    let foot = foot_pm(data, &p, &full, fonts);
    blit(&mut pm, &foot, 0, h - foot.height());
    let body_bottom = h - foot.height();
    let list_cols = if p.wide() { (p.cols * 45 / 100).clamp(60, 90) } else { p.cols };
    let lc = p.canvas(list_cols, theme);
    let mut rows = data.inbox_rows();
    for (ri, its) in data.repos() {
        if its.is_empty() {
            continue;
        }
        rows.push(Row::Gap);
        rows.push(Row::Head { name: data.m.fleet.repos[ri].repo.name(), count: its.len() });
        rows.extend(data.tasks_rows(&its));
    }
    stack(&mut pm, &[list_pm(rows, &lc, fonts)], 0, y, body_bottom);
    if p.wide() {
        let dc = p.canvas(p.cols - list_cols, theme);
        let page = detail_page(data, &dc, fonts, body_bottom - y);
        blit(&mut pm, &page, list_cols as u32 * p.cw, y);
        vrule(&mut pm, &theme, list_cols as f32 * p.cw as f32, y as f32 + 0.5 * ch, body_bottom as f32 - 0.5 * ch);
    }
    pm
}

/// The drawer (C): the selected task's detail along the bottom, `rows` high: its title,
/// repo, status and keys at the left; what its agent is doing, where it stands and what
/// its worker said at the right (under the left, narrow).
fn drawer(data: &Data, p: &Pane, c: &Canvas, fonts: &mut Fonts, h: u32) -> Pixmap {
    let mut pm = pixmap(c.grid.width(), h);
    let i = data.sel;
    let r = data.repo(i);
    let app = &data.m.boards[&r.repo.dir];
    let id = items::id(items::task(&data.m.fleet, i));
    let t = app.tasks.iter().find(|t| t.id == id).unwrap().clone();
    let ts = items::task(&data.m.fleet, i).clone();
    let m = c.m;
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let theme = d.theme;
    // a sheet: the card's fill, the full width, its top edge a faint rule
    let (x0, x1) = (c.left() - c.out(), c.right() + c.out());
    d.rounded_fill(x0, 0.3 * m.title, x1 - x0, h as f32, d.radius(), theme.tint());
    let (l, right) = (c.left(), c.right());
    let split = if p.wide() { l + 0.42 * (right - l) } else { right };
    let mut y = 0.3 * m.title + m.pad;
    d.set(m.detail, m.detail_line);
    let end = d.text(&t.id, Weight::Bold, l, y, split - l, theme.dim());
    d.text(&format!("   {}  ·  {}", r.repo.name(), items::tilde(&r.repo.root_str())), Weight::Regular, end, y, split - end, theme.dim());
    y += m.detail_line;
    d.set(m.title, m.line);
    for s in d.fonts.wrap(&t.title, Weight::Bold, m.title, (split - l - m.title, split - l - m.title), 2) {
        d.text(&s, Weight::Bold, l, y, split - l, theme.text());
        y += m.line;
    }
    let mark = match items::status(&ts) {
        "working" => Mark::Working,
        "review" => Mark::Review,
        st => Mark::Still(st.into()),
    };
    let mr = d.mark_r();
    d.mark(&mark, l + mr, y + m.line / 2.0, 0.1);
    let color = theme.status(&t.status).map_or(theme.text(), |c| solid(lit(Rgb(c[0], c[1], c[2]))));
    let end = d.text_in(m.detail, status_word(&t.status), Weight::Bold, l + 2.0 * mr + 0.6 * m.title, y, split - l, color);
    let age = crate::model::ago(&t, data.now);
    d.text_in(m.detail, &format!("  ·  {age}"), Weight::Regular, end, y, split - end, theme.dim());
    y += m.line + 0.2 * m.title;
    let mut x = l;
    for (k, what) in [("p", "peek"), ("⏎", "go to its pane")] {
        x = chip::keycap(&mut d, k, what, m.label, x, y + m.detail_line / 2.0) + 1.1 * m.label;
    }
    y += m.detail_line + 0.4 * m.title;
    let (tx, mut ty) = if p.wide() { (split + 1.5 * m.title, 0.3 * m.title + m.pad) } else { (l, y) };
    let mut texts: Vec<(String, [u8; 4])> = Vec::new();
    if !ts.doing.is_empty() {
        texts.push((ts.doing.clone(), theme.text()));
    }
    let det = crate::model::detail_with_auto(&t, &app.cfg, data.now);
    if !det.is_empty() {
        texts.push((det, theme.soft()));
    }
    let stat = crate::model::stat_line(&t);
    if !stat.is_empty() {
        texts.push((stat, theme.dim()));
    }
    let summary = t.summary.as_deref().unwrap_or("").trim().to_string();
    if !summary.is_empty() && matches!(t.status.as_str(), "review" | "ready" | "finished") {
        texts.push((summary.lines().next().unwrap_or("").to_string(), theme.dim()));
    }
    d.set(m.body, m.body_line);
    for (text, color) in texts {
        for s in d.fonts.wrap(&text, Weight::Regular, m.body, (right - tx, right - tx), 4) {
            if ty + m.body_line > h as f32 {
                break;
            }
            d.text(&s, Weight::Regular, tx, ty, right - tx, color);
            ty += m.body_line;
        }
        ty += 0.3 * m.body_line;
    }
    pm
}

/// C: what's yours at the left, every repository's coordinators and work at the right,
/// the selected task in a drawer along the bottom.
fn option_c(data: &Data, p: Pane, theme: Theme, fonts: &mut Fonts) -> Pixmap {
    let (w, h) = (p.cols as u32 * p.cw, p.rows as u32 * p.ch);
    let mut pm = pixmap(w, h);
    let full = p.canvas(p.cols, theme);
    let ch = p.ch as f32;
    let head_rows = 2;
    {
        let mut hp = pixmap(w, head_rows * p.ch);
        let mut d = Draw::new(&mut hp, fonts, &full, 0.0);
        header_line(data, &mut d, &full, (0.2 * ch).round(), (1.3 * ch).round());
        blit(&mut pm, &hp, 0, 0);
    }
    let foot = foot_pm(data, &p, &full, fonts);
    blit(&mut pm, &foot, 0, h - foot.height());
    let drawer_rows = if p.wide() { 10 } else { 12 };
    let drawer_top = h - foot.height() - drawer_rows * p.ch;
    let dr = drawer(data, &p, &full, fonts, drawer_rows * p.ch);
    blit(&mut pm, &dr, 0, drawer_top);
    let body_top = head_rows * p.ch;
    if p.wide() {
        let left_cols = p.cols * 52 / 100;
        let lc = p.canvas(left_cols, theme);
        stack(&mut pm, &[list_pm(data.inbox_rows(), &lc, fonts)], 0, body_top, drawer_top);
        let rc = p.canvas(p.cols - left_cols, theme);
        let mut parts = vec![];
        for (ri, its) in data.repos() {
            parts.push(repo_heading(data, ri, &rc, fonts, false));
            if !its.is_empty() {
                parts.push(list_pm(data.tasks_rows(&its), &rc, fonts));
            }
        }
        // the lane's top lines up with the inbox's first heading
        stack(&mut pm, &parts, left_cols as u32 * p.cw, body_top - (rc.m.gap as u32), drawer_top);
        vrule(&mut pm, &theme, left_cols as f32 * p.cw as f32, body_top as f32 + 0.5 * ch, drawer_top as f32 - 0.5 * ch);
    } else {
        let mut parts = vec![list_pm(data.inbox_rows(), &full, fonts)];
        for (ri, its) in data.repos() {
            parts.push(repo_heading(data, ri, &full, fonts, false));
            if !its.is_empty() {
                parts.push(list_pm(data.tasks_rows(&its), &full, fonts));
            }
        }
        stack(&mut pm, &parts, 0, body_top, drawer_top);
    }
    pm
}

fn fixture() -> Data {
    let state = std::path::PathBuf::from(std::env::var("MISSION_STATE").expect("MISSION_STATE"));
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut m = Mission::new(state.clone(), state.join("no-bullpen"), None, tx);
    m.session = None;
    m.reload();
    // jj-evolved's coordinator's pane is gone; the others are alive
    let alive: Vec<_> = m
        .fleet
        .repos
        .iter()
        .flat_map(|r| r.coordinators.iter().map(move |c| (r.repo.dir.clone(), c.id.clone(), !r.repo.dir.ends_with("home-jbahn-dev-jj-evolved"))))
        .collect();
    m.handle(MMsg::Alive(alive));
    for app in m.boards.values_mut() {
        app.image = true;
    }
    let pick = std::env::var("MISSION_SELECT").unwrap_or("BP-222".into());
    let sel = m.order().into_iter().find(|i| items::id(items::task(&m.fleet, *i)) == pick).expect("the selected task");
    Data { m, now: crate::model::now(), sel }
}

#[test]
#[ignore]
fn mission_options() {
    let mut data = fixture();
    let out = std::path::PathBuf::from(std::env::var("MISSION_OUT").expect("MISSION_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    // a ready task selected, at the large size, dark: what its worker said
    let ready = data.m.order().into_iter().find(|i| items::id(items::task(&data.m.fleet, *i)) == "BP-219").expect("BP-219");
    let was = std::mem::replace(&mut data.sel, ready);
    let mut fonts = Fonts::builtin();
    let p = Pane { cols: 211, rows: 65, cw: 19, ch: 37 };
    for (name, f) in [("A", option_a as fn(&Data, Pane, Theme, &mut Fonts) -> Pixmap), ("B", option_b), ("C", option_c)] {
        let theme = Theme::default();
        let pm = f(&data, p, theme, &mut fonts);
        let mut bg = pixmap(pm.width(), pm.height());
        bg.fill(tiny_skia::Color::from_rgba8(theme.bg.0, theme.bg.1, theme.bg.2, 255));
        blit(&mut bg, &pm, 0, 0);
        std::fs::write(out.join(format!("{name}-wide-dark-ready.png")), crate::image::png::encode(&bg)).unwrap();
    }
    data.sel = was;
    let out = std::path::PathBuf::from(std::env::var("MISSION_OUT").expect("MISSION_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let mut fonts = Fonts::builtin();
    let sizes = [("wide", Pane { cols: 211, rows: 65, cw: 19, ch: 37 }), ("narrow", Pane { cols: 64, rows: 48, cw: 19, ch: 37 })];
    for (theme_name, theme) in [("dark", Theme::default()), ("light", Theme::light())] {
        for (size, p) in sizes {
            for (name, f) in [("A", option_a as fn(&Data, Pane, Theme, &mut Fonts) -> Pixmap), ("B", option_b), ("C", option_c)] {
                let pm = f(&data, p, theme, &mut fonts);
                let mut bg = pixmap(pm.width(), pm.height());
                bg.fill(tiny_skia::Color::from_rgba8(theme.bg.0, theme.bg.1, theme.bg.2, 255));
                blit(&mut bg, &pm, 0, 0);
                std::fs::write(out.join(format!("{name}-{size}-{theme_name}.png")), crate::image::png::encode(&bg)).unwrap();
            }
        }
    }
}
