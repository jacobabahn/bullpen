//! The image look's benchmark: how long a j or a k takes, from the key reaching the
//! board to the last byte of its frame written, where that time goes and what it
//! sends, on a fixed board (a state folder on disk, read as the board reads it), for
//! every width, cell, text size and motion; then a held-down j, keys 30 ms apart. The
//! text look's numbers are beside them. No terminal: the output is counted, not shown.
//!
//!     bin/bullpen-cargo test -p bullpen-board --release bench -- --ignored --nocapture --test-threads 1
//!
//! BENCH_ONLY="80×45 32px,text" runs only the cases whose names have one of those in them.

use super::meter::{self, Tally};
use super::*;
use crate::Msg;
use std::collections::BTreeMap;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde_json::json;
use std::sync::mpsc;
use std::time::Instant;

/// The terminal's output, counted.
#[derive(Default, Clone)]
struct Out {
    bytes: std::rc::Rc<std::cell::Cell<usize>>,
}

impl Write for Out {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.bytes.set(self.bytes.get() + b.len());
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const ROWS: u16 = 45;
const LONG: &str = "Image board: make j and k feel instant, measured first and then climbed one change at a time";

/// A state folder with a repo whose board has a task in every status, long titles, a
/// Later group, and 200 closed tasks (a repo's history, which a reload reads too).
fn state(root: &Path) -> PathBuf {
    let dir = root.join("repos").join("bench");
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    let at = "2026-11-18T09:00:00-08:00";
    let mut tasks = vec![
        json!({"status": "needs", "ask": true, "title": "Architecture round 2: review", "why": crate::ui::testing::QUESTION}),
        json!({"status": "needs", "why": "Asks: python3 or python? The tests run both and the docs name one.", "title": LONG}),
        json!({"status": "stopped", "title": "Release notes for the board's image look", "why": "The worker stopped: its pane closed."}),
        json!({"status": "triage", "title": "Tours: say what a tour shows before it opens", "coordinator": "c1"}),
        json!({"status": "ready", "title": LONG, "diffstat": "3 files, +13 −16", "pr": 41}),
        json!({"status": "ready", "title": "Doctor: warn when /tmp is nearly full", "diffstat": "1 file, +40 −2"}),
        json!({"status": "finished", "title": "Report: what the reviewers asked for most this month"}),
        json!({"status": "review", "title": LONG, "pr": 42}),
        json!({"status": "working", "title": "Mission control: a repo's coordinators in one line each"}),
        json!({"status": "working", "title": LONG}),
        json!({"status": "todo", "title": "Accept: rebase the branch first when main moved"}),
        json!({"status": "todo", "title": LONG, "after": ["BP-10"]}),
        json!({"status": "todo", "title": "Later: a kept item's brief opens in the pager"}),
        json!({"status": "todo", "title": "The board's help lists the image look's keys"}),
        json!({"status": "todo", "title": "Settings: board_font takes a font's family name too"}),
    ];
    tasks.extend((0..200).map(|i| json!({"status": if i % 5 == 0 { "dropped" } else { "done" }, "title": format!("An old task, number {i}")})));
    for (i, t) in tasks.iter_mut().enumerate() {
        let id = format!("BP-{}", i + 1);
        let o = t.as_object_mut().unwrap();
        o.insert("id".into(), json!(id));
        o.insert("created".into(), json!(at));
        o.insert("updated".into(), json!(at));
        o.insert("since".into(), json!(at));
        std::fs::write(dir.join("tasks").join(format!("{id}.json")), serde_json::to_string_pretty(t).unwrap()).unwrap();
    }
    let items: Vec<_> = (1..=6)
        .map(|i| json!({"id": format!("L{i}"), "title": format!("An idea for later, number {i}: {LONG}"), "state": if i % 2 == 0 { "kept" } else { "asks" }, "by": "c1", "added": at}))
        .collect();
    std::fs::write(dir.join("backlog.json"), json!({"next": 7, "items": items}).to_string()).unwrap();
    dir
}

#[derive(Debug, Clone, Copy)]
struct Case {
    cols: u16,
    cell: (u32, u32),
    size: TextSize,
    motion: bool,
    look: Look,
}

impl Case {
    fn name(&self) -> String {
        let look = if self.look == Look::Text { " text".into() } else { format!(" {:?} {}", self.size, if self.motion { "motion" } else { "still" }) };
        format!("{}×{} {}px{look}", self.cols, ROWS, self.cell.0)
    }
}

/// The board in a case: the app on the state folder, its picture, and a terminal whose
/// output is counted.
struct Board {
    app: App,
    picture: Picture,
    term: Terminal<CrosstermBackend<Out>>,
    out: Out,
}

impl Board {
    fn new(dir: &Path, case: Case) -> Board {
        let config = json!({
            "board_look": if case.look == Look::Image { "image" } else { "text" },
            "board_text_size": format!("{:?}", case.size).to_lowercase(),
            "board_motion": case.motion,
            "board_bottom_gap": 0,
        });
        std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
        let arg = |p: &Path| p.to_string_lossy().into_owned();
        let args = crate::app::parse_args(["--dir".into(), arg(dir), "--root".into(), arg(dir), "--bullpen".into(), "/nonexistent".into()]).unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(args, None, tx);
        app.reload();
        app.later_open = true;
        let picture = Picture { probe: Probe::Yes, on: true, cell: Some(case.cell), window: || None, ..Picture::new(Arc::new(term::Gate::default())) };
        let viewport = Viewport::Fixed(Rect::new(0, 0, case.cols, ROWS));
        let out = Out::default();
        let term = Terminal::with_options(CrosstermBackend::new(out.clone()), TerminalOptions { viewport }).unwrap();
        let mut b = Board { app, picture, term, out };
        b.frame();
        b
    }

    /// Draw a frame as the main loop does, after reading the tasks again: what it wrote.
    fn frame(&mut self) -> usize {
        let before = self.out.bytes.get();
        meter::time(|| "reload".into(), || self.app.reload());
        loop {
            let again = meter::time(|| "frame".into(), || crate::draw(&mut self.term, &mut self.app, &mut self.picture)).unwrap();
            if !again {
                break;
            }
        }
        self.out.bytes.get() - before
    }

    /// A key, as the main loop takes it: handled, then a frame.
    fn key(&mut self, c: char) -> (Tally, usize) {
        meter::start();
        let t = Instant::now();
        meter::time(|| "key".into(), || self.app.handle(Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))));
        let n = self.frame();
        let mut tally = meter::take();
        tally.ms.insert("total".into(), t.elapsed().as_secs_f64() * 1000.0);
        tally.bytes.insert("total".into(), n);
        meter::stop();
        (tally, n)
    }

    /// The tasks and items j moves through.
    fn entries(&self) -> usize {
        self.app.order().len() + self.app.later.len()
    }
}

/// The value at fraction q of the sorted values.
fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

/// Each key from the top to the bottom and back, after a pass to warm the glyphs: the
/// tallies of the measured keys.
fn sweep(b: &mut Board) -> Vec<Tally> {
    let n = b.entries() - 1;
    let mut out = Vec::new();
    for pass in 0..3 {
        for c in std::iter::repeat_n('j', n).chain(std::iter::repeat_n('k', n)) {
            let (t, _) = b.key(c);
            if pass > 0 {
                out.push(t);
            }
        }
    }
    out
}

/// One case's line: a key's time (median, 90th percentile, most) and bytes (median,
/// most); how many keys sent the list's image again (it scrolled), and the median
/// time and bytes of those that didn't; then where the time and bytes go (each part's
/// mean over all the keys).
fn report(case: Case, tallies: &[Tally]) -> String {
    let ms = |t: &Tally| t.ms["total"];
    let kb = |t: &Tally| t.bytes["total"] as f64 / 1024.0;
    let list = |t: &Tally| t.bytes.contains_key("upload List");
    let stays: Vec<&Tally> = tallies.iter().filter(|t| !list(t)).collect();
    let mut all_ms: Vec<f64> = tallies.iter().map(ms).collect();
    let mut all_kb: Vec<f64> = tallies.iter().map(kb).collect();
    let mut stay_ms: Vec<f64> = stays.iter().map(|t| ms(t)).collect();
    let mut stay_kb: Vec<f64> = stays.iter().map(|t| kb(t)).collect();
    let (stay_ms, stay_kb) = if stays.is_empty() { (f64::NAN, f64::NAN) } else { (pct(&mut stay_ms, 0.5), pct(&mut stay_kb, 0.5)) };
    let n = tallies.len() as f64;
    let mut parts: BTreeMap<String, f64> = BTreeMap::new();
    let mut sent: BTreeMap<String, f64> = BTreeMap::new();
    for t in tallies {
        t.ms.iter().filter(|(k, _)| *k != "total").for_each(|(k, v)| *parts.entry(k.clone()).or_default() += v / n);
        t.bytes.iter().filter(|(k, _)| *k != "total").for_each(|(k, v)| *sent.entry(k.clone()).or_default() += *v as f64 / n);
    }
    let parts: Vec<String> = parts.iter().map(|(k, v)| format!("{k} {v:.2}")).collect();
    let sent: Vec<String> = sent.iter().map(|(k, v)| format!("{k} {:.1}", v / 1024.0)).collect();
    format!(
        "| {} | {:.2} | {:.2} | {:.2} | {:.1} | {:.1} | {}/{} | {stay_ms:.2} | {stay_kb:.1} | {} | {} |",
        case.name(),
        pct(&mut all_ms, 0.5),
        pct(&mut all_ms, 0.9),
        pct(&mut all_ms, 1.0),
        pct(&mut all_kb, 0.5),
        pct(&mut all_kb, 1.0),
        tallies.len() - stays.len(),
        tallies.len(),
        parts.join(", "),
        sent.join(", ")
    )
}

/// A held-down j: `keys` keys `gap` apart from a thread, as the key reader sends them,
/// and the board's loop as main.rs runs it (a key's handled with every one waiting, then
/// a frame; a tick draws a frame too). The frames drawn, the time from the first key to
/// the last frame, and how long after the last key the last frame ended.
fn held(b: &mut Board, keys: usize, gap: Duration) -> (usize, f64, f64) {
    b.app.sel = b.app.order().first().map(|t| t.id.clone());
    b.frame();
    let (tx, rx) = mpsc::channel();
    let sender = std::thread::spawn(move || {
        let mut sent = Vec::new();
        for _ in 0..keys {
            sent.push(Instant::now());
            tx.send(Msg::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))).unwrap();
            std::thread::sleep(gap);
        }
        sent
    });
    let (mut got, mut frames, mut last) = (0, 0, Instant::now());
    while got < keys {
        let tick = b.picture.tick().map_or(b.app.tick(), |t| t.min(b.app.tick()));
        match rx.recv_timeout(tick) {
            Ok(m) => {
                got += 1;
                b.app.handle(m);
                while let Ok(m) = rx.try_recv() {
                    got += 1;
                    b.app.handle(m);
                }
            }
            Err(_) => {}
        }
        b.frame();
        frames += 1;
        last = Instant::now();
    }
    let sent = sender.join().unwrap();
    let first = sent[0];
    let end = *sent.last().unwrap();
    (frames, (last - first).as_secs_f64() * 1000.0, last.saturating_duration_since(end).as_secs_f64() * 1000.0)
}

#[test]
#[ignore = "a benchmark: run it by name with --release (docs/code.md)"]
fn bench() {
    let scratch = crate::app::testing::Scratch::new("bench");
    // the global config and the other repos are the scratch folder's, not this machine's
    std::env::set_var("BULLPEN_STATE", &scratch.0);
    let dir = state(&scratch.0);
    let mut cases = Vec::new();
    for cols in [48, 80] {
        for scale in [1, 2] {
            let cell = (16 * scale, 34 * scale);
            cases.push(Case { cols, cell, size: TextSize::Medium, motion: true, look: Look::Text });
            for size in [TextSize::Small, TextSize::Medium, TextSize::Large] {
                for motion in [true, false] {
                    cases.push(Case { cols, cell, size, motion, look: Look::Image });
                }
            }
        }
    }
    // BENCH_ONLY="80×45 32px,text" runs the cases whose names have any of these in them
    let only = std::env::var("BENCH_ONLY").unwrap_or_default();
    cases.retain(|c| only.is_empty() || only.split(',').any(|o| c.name().contains(o)));
    println!("\nA j or k: the key handled, the tasks read again, the frame drawn and written\n");
    println!("| case | ms median | ms p90 | ms most | KB median | KB most | keys sending the list | list stays: ms median | list stays: KB median | where the time goes (mean ms) | what's sent (mean KB) |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for &case in &cases {
        let mut b = Board::new(&dir, case);
        println!("{}", report(case, &sweep(&mut b)));
    }
    println!("\nA held-down j: 20 keys 30 ms apart\n");
    println!("| case | frames | first key to last frame ms | last frame after last key ms |");
    println!("|---|---|---|---|");
    for &case in &cases {
        let mut b = Board::new(&dir, case);
        let (frames, total, behind) = held(&mut b, 20, Duration::from_millis(30));
        println!("| {} | {frames} | {total:.1} | {behind:.1} |", case.name());
    }
}
