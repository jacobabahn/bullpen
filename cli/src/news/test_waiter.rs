//! The waiter with a clock of the test's own: every look is given its time.

use super::*;
use bullpen_core::timeutil::parse;
use std::sync::atomic::AtomicU32;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A repo's state folder of the test's own, with coordinators c1 (pane blk1) and c2
/// (blk2) and news that isn't typed.
struct Home {
    dir: PathBuf,
}

impl Home {
    fn new() -> Home {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("bullpen-waiter-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let home = Home { dir };
        home.config("wake");
        set_news_cursor(&home.repo(), 0, "c1").unwrap();
        home
    }

    fn config(&self, news: &str) {
        let cfg = json!({"news": news, "coordinators": [
            {"id": "c1", "block": "blk1", "session": "s", "board": null},
            {"id": "c2", "block": "blk2", "session": "s", "board": null}]});
        std::fs::write(self.dir.join("config.json"), cfg.to_string()).unwrap();
    }

    fn repo(&self) -> Repo {
        Repo {
            root: "/proj".into(),
            dir: self.dir.clone(),
            cfg: load_config(&self.dir).unwrap(),
            own: Default::default(),
        }
    }

    /// A waiter for c1, started at second `at` by a hook event.
    fn waiter(&self, event: &str, at: i64) -> Option<Waiter> {
        let file = self.dir.join("limit.json");
        let mut w = Waiter::take(self.repo(), "blk1", event, "sess", TIMEOUT, t(at), file).unwrap();
        if let Some(w) = w.as_mut() {
            w.say = |_| true; // the test reads what it said from the step
        }
        w
    }

    /// Log a line of news for c1 at second `at`.
    fn log(&self, at: i64, task: &str, event: &str) {
        self.log_to("c1", at, task, event);
    }

    fn log_to(&self, to: &str, at: i64, task: &str, event: &str) {
        let line = format!(
            "{} [{to}] {task} {event}: {NEWS_MARK}{task} {event}\n",
            iso_seconds(&t(at))
        );
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("news.log"))
            .unwrap();
        f.write_all(line.as_bytes()).unwrap();
    }

    fn wait(&self) -> Obj {
        wait_file(&self.repo(), "c1")
    }

    fn set_wait(&self, change: impl FnOnce(&mut Obj)) {
        update_wait(&self.repo(), "c1", change).unwrap();
    }

    fn cursor(&self) -> u64 {
        news_cursor(&self.repo(), "c1").unwrap().unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Second n of the test's clock.
fn t(n: i64) -> Stamp {
    parse("2026-11-16T10:00:00-08:00").unwrap() + chrono::Duration::seconds(n)
}

/// Look once a second from..=to: the second it woke at, and what it said.
fn woke(w: &mut Waiter, from: i64, to: i64) -> Option<(i64, String)> {
    for at in from..=to {
        match w.step(t(at)).unwrap() {
            Step::Wait => {}
            Step::Quit => panic!("it quit at {at}"),
            Step::Wake(said) => return Some((at, said)),
        }
    }
    None
}

#[test]
fn news_wakes_it_once_the_batch_has_settled_with_what_rode_along() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    assert_eq!(woke(&mut w, 0, 100), None); // nothing logged: nothing happens
    h.log(100, "BP-1", "started");
    assert_eq!(woke(&mut w, 100, 200), None); // bookkeeping wakes nobody
    h.log(200, "BP-2", "ready");
    h.log_to("c2", 200, "BP-9", "ready"); // another coordinator's
    let (at, said) = woke(&mut w, 200, 300).unwrap();
    assert_eq!(at, 203);
    assert_eq!(said, format!("{NEWS_MARK}BP-1 started\n{NEWS_MARK}BP-2 ready"));
    assert_eq!(h.cursor(), news_size(&h.repo()));
    let wait = h.wait();
    assert!(!wait.contains_key("waiter"));
    assert_eq!(s(part(&wait, "woke"), "at"), iso_seconds(&t(203)));
}

#[test]
fn news_for_another_coordinator_wakes_nobody() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    h.log_to("c2", 1, "BP-9", "ready");
    assert_eq!(woke(&mut w, 1, 60), None);
}

#[test]
fn news_logged_before_it_started_wakes_it_too() {
    let h = Home::new();
    h.log(-30, "BP-2", "ready"); // during the turn
    let mut w = h.waiter("Stop", 0).unwrap();
    assert_eq!(woke(&mut w, 0, 60).unwrap().0, 3);
}

#[test]
fn a_batch_still_growing_waits_twenty_seconds_at_most() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    for at in (10..40).step_by(2) {
        h.log(at, "BP-2", "ready");
        assert_eq!(
            woke(&mut w, at, at + 1).map(|(at, _)| at),
            (at >= 30).then_some(30),
            "{at}"
        );
        if at >= 30 {
            return;
        }
    }
    panic!("it never woke");
}

#[test]
fn the_newest_waiter_wins_and_the_older_goes_quietly() {
    let h = Home::new();
    let mut old = h.waiter("Stop", 0).unwrap();
    assert_eq!(old.step(t(1)).unwrap(), Step::Wait);
    let mut new = h.waiter("Stop", 2).unwrap();
    h.log(3, "BP-2", "ready");
    assert_eq!(old.step(t(3)).unwrap(), Step::Quit);
    old.leave().unwrap(); // not its wait to give up
    assert_eq!(woke(&mut new, 3, 10).unwrap().0, 6);
}

#[test]
fn one_wake_a_minute() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    h.log(1, "BP-2", "ready");
    assert_eq!(woke(&mut w, 1, 10).unwrap().0, 4);
    let mut w = h.waiter("Stop", 10).unwrap(); // the woken turn ended
    h.log(11, "BP-3", "ready");
    let (at, said) = woke(&mut w, 11, 200).unwrap();
    assert_eq!(at, 64); // a minute after the last wake
    assert_eq!(said, format!("{NEWS_MARK}BP-3 ready"));
}

#[test]
fn after_an_api_error_it_waits_five_minutes_and_says_the_lost_news_again() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    h.log(1, "BP-2", "ready");
    assert_eq!(woke(&mut w, 1, 10).unwrap().0, 4);
    let mut w = h.waiter("StopFailure", 10).unwrap(); // the woken turn failed
    h.log(20, "BP-3", "ready");
    let (at, said) = woke(&mut w, 10, 1000).unwrap();
    assert_eq!(at, 304);
    let lines: Vec<&str> = said.lines().collect();
    assert!(lines[0].contains("ended in an error"), "{said}");
    assert_eq!(
        lines[1..],
        [format!("{NEWS_MARK}BP-2 ready"), format!("{NEWS_MARK}BP-3 ready")]
    );
    // it fails again, with nothing new: the same news, five minutes on, not sooner
    let mut w = h.waiter("StopFailure", 310).unwrap();
    let (at, said) = woke(&mut w, 310, 1000).unwrap();
    assert_eq!(at, 604);
    assert_eq!(said.lines().count(), 3);
    // a turn that ends well has heard it
    let mut w = h.waiter("Stop", 610).unwrap();
    assert_eq!(woke(&mut w, 610, 2000), None);
    let mut w = h.waiter("StopFailure", 2000).unwrap(); // an error with no wake before it
    assert_eq!(woke(&mut w, 2000, 3000), None);
}

#[test]
fn no_wake_before_a_usage_limit_resets() {
    let h = Home::new();
    let limit = json!({"at": iso_seconds(&t(0)), "resets": iso_seconds(&t(500)), "text": "limit"});
    std::fs::write(h.dir.join("limit.json"), limit.to_string()).unwrap();
    let mut w = h.waiter("Stop", 0).unwrap();
    h.log(1, "BP-2", "limit");
    assert_eq!(woke(&mut w, 1, 1000).unwrap().0, 500);
}

#[test]
fn a_usage_limit_with_no_known_reset_paces_wakes_as_an_error_does() {
    let h = Home::new();
    let limit = json!({"at": iso_seconds(&t(0)), "resets": null, "text": "limit"});
    std::fs::write(h.dir.join("limit.json"), limit.to_string()).unwrap();
    h.set_wait(|w| {
        w.insert("woke".into(), json!({"at": iso_seconds(&t(-10))}));
    });
    let mut w = h.waiter("Stop", 0).unwrap();
    h.log(1, "BP-2", "ready");
    assert_eq!(woke(&mut w, 1, 1000).unwrap().0, 290);
}

#[test]
fn a_repeat_of_triages_line_wakes_once_in_fifteen_minutes_and_rides_along() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    h.log(1, "BP-2", "quiet");
    assert_eq!(woke(&mut w, 1, 10).unwrap().0, 4);
    let mut w = h.waiter("Stop", 10).unwrap();
    h.log(180, "BP-2", "quiet"); // the same, 3 minutes on
    assert_eq!(woke(&mut w, 10, 400), None);
    h.log(400, "BP-3", "quiet"); // another task's wakes
    let (at, said) = woke(&mut w, 400, 500).unwrap();
    assert_eq!(at, 403);
    assert_eq!(said, format!("{NEWS_MARK}BP-2 quiet\n{NEWS_MARK}BP-3 quiet"));
    let mut w = h.waiter("Stop", 410).unwrap();
    assert_eq!(woke(&mut w, 410, 999), None);
    h.log(1000, "BP-2", "quiet"); // over 15 minutes after it last woke
    assert_eq!(woke(&mut w, 1000, 1100).unwrap().0, 1003);
    // `ready` twice is never a repeat
    let mut w = h.waiter("Stop", 1010).unwrap();
    assert_eq!(woke(&mut w, 1010, 1099), None);
    h.log(1100, "BP-4", "ready");
    assert_eq!(woke(&mut w, 1100, 1200).unwrap().0, 1103);
    let mut w = h.waiter("Stop", 1110).unwrap();
    assert_eq!(woke(&mut w, 1110, 1199), None);
    h.log(1200, "BP-4", "ready");
    assert_eq!(woke(&mut w, 1200, 1300).unwrap().0, 1203);
}

#[test]
fn it_ends_a_minute_before_its_timeout() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    let last = TIMEOUT as i64 - 60;
    assert_eq!(w.step(t(last - 1)).unwrap(), Step::Wait);
    assert_eq!(w.step(t(last)).unwrap(), Step::Quit);
    // with news held back by a bound, it hands it over rather than be killed with it
    h.set_wait(|w| {
        w.insert("woke".into(), json!({"at": iso_seconds(&t(last + 99))}));
    });
    let mut w = h.waiter("Stop", 100).unwrap();
    h.log(last + 99, "BP-2", "ready");
    assert_eq!(w.step(t(last + 99)).unwrap(), Step::Wait);
    assert!(matches!(w.step(t(last + 100)).unwrap(), Step::Wake(_)));
}

#[test]
fn a_pause_ends_it_and_none_starts_while_paused() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    assert!(live(&h.wait()));
    h.set_wait(|w| {
        w.insert("paused".into(), json!(iso_seconds(&t(1))));
    });
    h.log(1, "BP-2", "ready");
    assert_eq!(w.step(t(5)).unwrap(), Step::Quit);
    w.leave().unwrap();
    let wait = h.wait();
    assert!(wait.contains_key("paused") && !wait.contains_key("waiter")); // the pause stays
    assert!(h.waiter("Stop", 6).is_none());
    assert_eq!(h.cursor(), 0);
}

#[test]
fn none_starts_unless_news_is_wake_or_in_another_pane() {
    let h = Home::new();
    let file = h.dir.join("limit.json");
    assert!(Waiter::take(h.repo(), "blk9", "Stop", "", TIMEOUT, t(0), file)
        .unwrap()
        .is_none());
    for news in ["type", "monitor"] {
        h.config(news);
        assert!(h.waiter("Stop", 0).is_none());
        assert!(!h.wait().contains_key("waiter"));
    }
}

#[test]
fn it_goes_when_news_becomes_typed() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    h.config("type");
    h.log(1, "BP-2", "ready"); // typed as well: not this waiter's to say
    assert_eq!(w.step(t(1)).unwrap(), Step::Quit);
    assert_eq!(h.cursor(), 0);
}

#[test]
fn a_reader_holding_the_news_lock_is_left_to_read() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    h.log(1, "BP-2", "ready");
    let held = bullpen_core::lock::lock(&lock_path(&h.repo(), "c1")).unwrap(); // a `news --follow`
    assert_eq!(woke(&mut w, 1, 30), None);
    set_news_cursor(&h.repo(), news_size(&h.repo()), "c1").unwrap(); // it read the line
    drop(held);
    assert_eq!(woke(&mut w, 30, 60), None);
    h.log(60, "BP-3", "ready");
    assert!(woke(&mut w, 60, 130).is_some());
}

#[test]
fn a_waiter_is_live_while_its_process_runs_and_it_hasnt_expired() {
    let waiter = |pid: i64, until: Stamp| {
        let w = json!({"waiter": {"pid": pid, "started": "x", "until": iso_seconds(&until)}});
        w.as_object().unwrap().clone()
    };
    let soon = now() + chrono::Duration::seconds(60);
    assert!(live(&waiter(std::process::id().into(), soon)));
    assert!(!live(&waiter(
        std::process::id().into(),
        now() - chrono::Duration::seconds(60)
    )));
    assert!(!live(&waiter(i32::MAX.into(), soon))); // no such process
    assert!(!live(&Obj::new()));
}

#[test]
fn news_nobody_reads_stays_unread() {
    let h = Home::new();
    let mut w = h.waiter("Stop", 0).unwrap();
    w.say = |_| false; // the session died: its end of stderr is closed
    h.log(1, "BP-2", "ready");
    assert_eq!(w.step(t(1)).unwrap(), Step::Wait);
    assert_eq!(w.step(t(4)).unwrap(), Step::Quit);
    w.leave().unwrap();
    assert_eq!(h.cursor(), 0);
    assert!(h.wait().is_empty()); // no wake noted, and the wait given up
}
