//! Mission control's benchmark (docs/code.md): how long a j or a k takes, from the key
//! to its frame's last byte, and what it sends, on a fixed fleet (four repositories on
//! disk, a task in every status, long titles, a blocked coordinator), at a floating
//! pane's sizes, its image look beside its text. As the board's (image/tests/test_bench.rs),
//! each key is handled and drawn, the snapshot read again and drawn, then what the next
//! key would want drawn ahead. No terminal: the output is counted, not shown.
//!
//!     bin/bullpen-cargo test -p bullpen-board --release bench_mission -- --ignored --nocapture --test-threads 1

use super::*;
use crate::app::Msg;
use crate::image::meter;
use crate::image::Picture;
use crate::mission::testing::State;
use crate::mission::MMsg;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde_json::json;
use std::io::Write;
use std::time::{Duration, Instant};

/// The terminal's output, counted.
#[derive(Default, Clone)]
struct Out(std::rc::Rc<std::cell::Cell<usize>>);

impl Write for Out {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.set(self.0.get() + b.len());
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const LONG: &str = "Mission control: make j and k feel instant across every repository, measured first";

/// Four repositories: one whose coordinator is blocked, a task in every open status in
/// each, long titles, and a ready task's long summary.
fn fleet(s: &State) {
    let statuses = ["needs", "stopped", "triage", "ready", "finished", "review", "working", "working", "todo", "todo", "todo"];
    for (r, name) in ["bullpen", "ai-overview-searxng", "chat-egw", "jj-evolved"].iter().enumerate() {
        let tasks: Vec<_> = statuses
            .iter()
            .enumerate()
            .map(|(i, st)| {
                let mut t = crate::mission::testing::t(&format!("BP-{}", i + 1), st, if i % 2 == 0 { LONG } else { "A short title" }, (i * 7 + r) as i64);
                t["why"] = json!("Asks: should the inbox show a coordinator's question above the tasks that need you?");
                t["summary"] = json!(format!("{LONG}.\n\n{LONG}, and what changed, at length.\n\nHow it was checked."));
                t["diffstat"] = json!("12 files, +455 −228");
                t
            })
            .collect();
        let co = json!([{"id": format!("c{r}"), "block": format!("block:c{r}"), "session": "s"}]);
        s.repo(name, co, &tasks);
    }
    std::fs::create_dir_all(s.0.join("doing")).unwrap();
    std::fs::write(s.0.join("doing/block_c1.json"), json!({"state": "blocked", "text": "Allow `git push origin main`?"}).to_string()).unwrap();
    std::fs::write(s.0.join("doing/block_c0.json"), json!({"state": "working", "text": ""}).to_string()).unwrap();
}

#[derive(Debug, Clone, Copy)]
struct Case {
    cols: u16,
    rows: u16,
    cell: (u32, u32),
    look: Look,
}

impl Case {
    fn name(&self) -> String {
        format!("mission {}×{} {}px {}", self.cols, self.rows, self.cell.0, if self.look == Look::Image { "image" } else { "text" })
    }
}

/// Mission control in a case: its state, its picture (with drawing ahead's thread), and a
/// terminal whose output is counted.
struct Bench {
    m: Mission,
    picture: Picture,
    term: Terminal<CrosstermBackend<Out>>,
    out: Out,
    rx: std::sync::mpsc::Receiver<Msg>,
}

impl Bench {
    fn new(s: &State, case: Case) -> Bench {
        let mut m = s.mission();
        m.look = case.look;
        let mut picture = Picture::answered_for_test(case.cell);
        let (tx, rx) = std::sync::mpsc::channel();
        picture.use_thread(tx);
        let viewport = Viewport::Fixed(Rect::new(0, 0, case.cols, case.rows));
        let out = Out::default();
        let term = Terminal::with_options(CrosstermBackend::new(out.clone()), TerminalOptions { viewport }).unwrap();
        let mut b = Bench { m, picture, term, out, rx };
        b.frame();
        b
    }

    fn draw(&mut self) -> usize {
        let before = self.out.0.get();
        while super::super::run::draw(&mut self.term, &mut self.m, &mut self.picture).unwrap() {}
        self.out.0.get() - before
    }

    fn frame(&mut self) -> usize {
        self.m.reload();
        self.draw()
    }

    /// Draw ahead, as the loop does while it waits, and send what the thread drew.
    fn ahead(&mut self) {
        let area = self.term.get_frame().area();
        while self.picture.ahead(&mut self.m, area, self.term.backend_mut()) {}
        while self.picture.busy() {
            if let Ok(Msg::Ready(r)) = self.rx.recv_timeout(Duration::from_secs(5)) {
                self.picture.ready(r, self.term.backend_mut());
            }
        }
    }

    /// A key: handled and its frame drawn (its time and bytes), then the snapshot read
    /// again and drawn, then what the next key would want drawn ahead.
    fn key(&mut self, c: char) -> (f64, usize, bool) {
        meter::start();
        let t = Instant::now();
        self.m.handle(MMsg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
        let n = self.draw();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let list = meter::take().bytes.contains_key("upload List");
        meter::stop();
        self.frame();
        self.ahead();
        (ms, n, list)
    }
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

#[test]
#[ignore = "a benchmark: run it by name with --release (docs/code.md)"]
fn bench_mission() {
    let s = State::new("bench");
    // the global settings are the scratch folder's, not this machine's
    let config = s.0.join("config");
    std::fs::create_dir_all(config.join("bullpen")).unwrap();
    std::fs::write(config.join("bullpen/config.json"), json!({"board_text_size": "medium", "board_motion": true}).to_string()).unwrap();
    std::env::set_var("XDG_CONFIG_HOME", &config);
    fleet(&s);
    let mut cases = Vec::new();
    for (cols, rows, cell) in [(160, 46, (16, 34)), (211, 65, (19, 37)), (64, 48, (19, 37))] {
        for look in [Look::Text, Look::Image] {
            cases.push(Case { cols, rows, cell, look });
        }
    }
    let only = std::env::var("BENCH_ONLY").unwrap_or_default();
    cases.retain(|c| only.is_empty() || only.split(',').any(|o| c.name().contains(o)));
    println!("\nMission control, a j or k: the key handled and its frame drawn and written\n");
    println!("| case | ms median | ms p90 | ms most | KB median | KB most | keys sending the list | first frame ms | first frame KB |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for &case in &cases {
        let t = Instant::now();
        let mut b = Bench::new(&s, case);
        let first = (t.elapsed().as_secs_f64() * 1000.0, b.out.0.get() as f64 / 1024.0);
        b.ahead();
        let n = b.m.order().len() - 1;
        let (mut ms, mut kb, mut lists) = (Vec::new(), Vec::new(), 0);
        for pass in 0..3 {
            for c in std::iter::repeat_n('j', n).chain(std::iter::repeat_n('k', n)) {
                let (t, bytes, list) = b.key(c);
                if pass > 0 {
                    ms.push(t);
                    kb.push(bytes as f64 / 1024.0);
                    lists += usize::from(list);
                }
            }
        }
        let keys = ms.len();
        println!(
            "| {} | {:.2} | {:.2} | {:.2} | {:.1} | {:.1} | {lists}/{keys} | {:.1} | {:.1} |",
            case.name(),
            pct(&mut ms, 0.5),
            pct(&mut ms, 0.9),
            pct(&mut ms, 1.0),
            pct(&mut kb, 0.5),
            pct(&mut kb, 1.0),
            first.0,
            first.1
        );
    }
}
