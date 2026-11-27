use super::*;
use std::io::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A folder of the test's own.
fn folder() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("bullpen-web-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn typed(uuid: &str, text: &str) -> String {
    let r = json!({"type": "user", "uuid": uuid, "timestamp": "2026-11-27T10:00:00Z",
        "message": {"role": "user", "content": text}});
    format!("{r}\n")
}

fn append(path: &Path, text: &str) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn texts(v: &Value) -> Vec<&str> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["text"].as_str().unwrap())
        .collect()
}

#[test]
fn a_read_sends_what_it_made_and_the_first_sends_nothing() {
    let dir = folder();
    let path = dir.join("s.jsonl");
    append(&path, &typed("u1", "one"));
    let all = Transcripts::start();
    let open = all.open(&path, Source::Recorded);
    let mut rx = open.subscribe();
    open.read_on();
    assert!(rx.try_recv().is_err(), "the whole file isn't an update");
    let page = open.page(None).unwrap();
    assert_eq!(texts(&page), ["one"]);
    let cursor = page["cursor"].as_u64().unwrap();
    append(&path, &format!("{}{}", typed("u2", "two"), typed("u3", "three")));
    append(&path, "{\"type\": \"user\", \"half");
    open.read_on();
    let Update::Items(v) = &*rx.try_recv().unwrap() else {
        panic!("not items")
    };
    assert_eq!(texts(v), ["two", "three"]);
    assert!(rx.try_recv().is_err(), "one update a read");
    let Missed::Items(m) = open.since(cursor) else {
        panic!("nothing missed")
    };
    assert_eq!(texts(&m), ["two", "three"]);
    assert_eq!(open.since(v["cursor"].as_u64().unwrap()), Missed::Nothing);
    assert!(
        v["cursor"].as_u64().unwrap() < std::fs::metadata(&path).unwrap().len(),
        "the half line waits"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn joining_with_a_cursor_has_what_was_missed_once() {
    let dir = folder();
    let path = dir.join("s.jsonl");
    append(&path, &typed("u1", "one"));
    let all = Transcripts::start();
    let open = all.read(&path, Source::Recorded);
    let cursor = open.page(None).unwrap()["cursor"].as_u64().unwrap();
    append(&path, &typed("u2", "two")); // written after the page, before the stream
    let (mut rx, missed) = open.join(Some(cursor));
    let Missed::Items(m) = missed else {
        panic!("nothing missed")
    };
    assert_eq!(texts(&m), ["two"]);
    assert!(rx.try_recv().is_err(), "what was missed isn't heard as well");
    append(&path, &typed("u3", "three"));
    open.read_on();
    let Update::Items(v) = &*rx.try_recv().unwrap() else {
        panic!("not items")
    };
    assert_eq!(texts(v), ["three"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_cursor_from_before_the_server_read_the_file_has_the_chat_whole() {
    let dir = folder();
    let path = dir.join("s.jsonl");
    append(&path, &typed("u1", "one"));
    let cursor = std::fs::metadata(&path).unwrap().len(); // a page from the server before this one
    append(&path, &typed("u2", "two"));
    let all = Transcripts::start();
    let open = all.read(&path, Source::Recorded);
    assert_eq!(open.since(cursor), Missed::Whole);
    assert_eq!(open.since(cursor + 10_000), Missed::Whole, "past where it has read to");
    let now = std::fs::metadata(&path).unwrap().len();
    assert_eq!(open.since(now), Missed::Nothing);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_file_written_anew_starts_its_chat_again() {
    let dir = folder();
    let path = dir.join("s.jsonl");
    append(&path, &format!("{}{}", typed("u1", "one"), typed("u2", "two")));
    let all = Transcripts::start();
    let open = all.read(&path, Source::Title);
    let mut rx = open.subscribe();
    std::fs::write(&path, typed("u9", "nine")).unwrap();
    open.read_on();
    assert!(matches!(&*rx.try_recv().unwrap(), Update::Anew));
    assert_eq!(texts(&open.page(None).unwrap()), ["nine"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_transcript_is_watched_while_a_stream_watches_it() {
    let dir = folder();
    let path = dir.join("s.jsonl");
    let all = Transcripts::start();
    let a = all.watch(&path);
    let b = all.watch(&path);
    assert_eq!(all.watched.lock().unwrap().get(&path), Some(&2));
    assert_eq!(all.folders.len(), 1);
    drop(a);
    assert_eq!(all.watched.lock().unwrap().get(&path), Some(&1));
    drop(b);
    assert!(all.watched.lock().unwrap().is_empty());
    assert_eq!(all.folders.len(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_watched_transcript_is_read_each_second_whatever_is_heard() {
    let dir = folder();
    let (a, b) = (dir.join("a.jsonl"), dir.join("b.jsonl"));
    append(&a, &typed("u1", "one"));
    let (tx, rx) = channel();
    let all = Arc::new(Transcripts {
        open: Mutex::default(),
        watched: Mutex::default(),
        folders: crate::watch::Folders::new(channel().0),
    });
    let _w = all.watch(&a);
    let mut heard = all.read(&a, Source::Recorded).subscribe();
    append(&a, &typed("u2", "two")); // its change is missed: only b's is heard
    tx.send(b.clone()).unwrap();
    let mut swept = Instant::now();
    assert!(all.read_changed(&rx, &mut swept));
    assert!(heard.try_recv().is_err(), "b heard, a not due yet");
    tx.send(b.clone()).unwrap();
    swept = Instant::now() - EVERY;
    assert!(all.read_changed(&rx, &mut swept));
    let Update::Items(v) = &*heard.try_recv().unwrap() else {
        panic!("not items")
    };
    assert_eq!(texts(v), ["two"]);
    assert!(swept.elapsed() < EVERY, "the sweep's time is kept");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hearing_writes_waits_a_tenth_of_a_second_at_most() {
    let (tx, rx) = channel();
    let all = Arc::new(Transcripts {
        open: Mutex::default(),
        watched: Mutex::default(),
        folders: crate::watch::Folders::new(channel().0),
    });
    std::thread::spawn(move || {
        while tx.send(PathBuf::from("/x")).is_ok() {
            std::thread::sleep(Duration::from_millis(5)); // writes keep coming
        }
    });
    let t = Instant::now();
    assert!(all.read_changed(&rx, &mut Instant::now()));
    assert!(
        t.elapsed() < SETTLE_AT_MOST + Duration::from_millis(50),
        "{:?}",
        t.elapsed()
    );
}
