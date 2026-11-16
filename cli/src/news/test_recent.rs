use super::*;

fn at(stamp: &str) -> Stamp {
    parse(stamp).unwrap()
}

#[test]
fn a_line_in_its_parts() {
    let it = parse_line("2026-11-15T14:03:00-06:00 [c1,c2] BP-3 ready: bullpen · BP-3 is ready: Fix it").unwrap();
    assert_eq!(it.stamp, "2026-11-15T14:03:00-06:00");
    assert_eq!(it.to, vec!["c1", "c2"]);
    assert_eq!((it.task.as_str(), it.event.as_str()), ("BP-3", "ready"));
    assert_eq!(it.text, "BP-3 is ready: Fix it");
    let it = parse_line("2026-11-15T14:03:00-06:00 - guide: bullpen · Your guide changed").unwrap();
    assert!(it.to.is_empty());
    assert_eq!(it.text, "Your guide changed");
    assert_eq!(parse_line("\t+ a diff line"), None);
    assert_eq!(parse_line("junk"), None);
}

#[test]
fn times_for_a_person() {
    let now = at("2026-11-15T14:04:00-06:00"); // a minute after: the same day in any time zone
    let today = at("2026-11-15T14:03:00-06:00").with_timezone(&Local);
    assert_eq!(
        when("2026-11-15T14:03:00-06:00", now),
        today.format("%-I:%M %p").to_string()
    );
    assert_eq!(when("not a time", now), "not a time");
    let old = when("2026-10-01T14:03:00-06:00", now);
    assert!(old.starts_with("Oct ") || old.starts_with("Sep 30 "), "{old}");
}

fn log(lines: &[String]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bullpen-recent-{}-{}", std::process::id(), lines.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("news.log");
    std::fs::write(&path, lines.concat()).unwrap();
    path
}

#[test]
fn the_last_lines_of_a_long_log_with_their_diffs() {
    let mut lines: Vec<String> = (0..5000)
        .map(|i| {
            format!(
                "2026-11-15T14:03:00-06:00 BP-{i} blocked: bullpen · line {i} {}\n",
                "x".repeat(40)
            )
        })
        .collect();
    lines.push("2026-11-15T14:04:00-06:00 - guide: bullpen · Your guide changed\n".into());
    lines.push("\t--- guide (yours)\n".into());
    lines.push("\t+++ guide (now)\n".into());
    let path = log(&lines);
    let items = last_items(&path, 3).unwrap();
    assert_eq!(
        items.iter().map(|i| i.task.as_str()).collect::<Vec<_>>(),
        ["BP-4998", "BP-4999", "-"]
    );
    assert_eq!(items[2].more, ["--- guide (yours)", "+++ guide (now)"]);
    // a line still being written waits, and so does the one before it (its diff may be coming), as for `news`
    lines.push("2026-11-15T14:05:00-06:00 BP-9 bloc".into());
    std::fs::write(&path, lines.concat()).unwrap();
    let items = last_items(&path, 1).unwrap();
    assert_eq!(items[0].task, "BP-4999");
    assert_eq!(last_items(&path, 10_000).unwrap().len(), 5000); // fewer than asked: all there is (but the two waiting)
    assert!(last_items(&path, 0).unwrap().is_empty());
    assert!(last_items(&path.with_file_name("none.log"), 3).unwrap().is_empty());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

fn repo(name: &str) -> Repo {
    let mut r = Repo::new(std::env::temp_dir().join(name)).unwrap();
    r.cfg = Default::default();
    r
}

#[test]
fn lines_for_a_terminal() {
    let now = at("2026-11-15T14:04:00-06:00");
    let line = |stamp: &str, text: &str| parse_line(&format!("{stamp} BP-1 blocked: bullpen · {text}")).unwrap();
    let mut diffed = line("2026-11-15T14:03:00-06:00", "guide changed");
    diffed.more = vec!["+a".into(), "-b".into()];
    let items = vec![(repo("shop"), line("garbled", "first")), (repo("proj-long"), diffed)];
    let today = when("2026-11-15T14:03:00-06:00", now);
    let w = len("garbled").max(len(&today)); // the times' column
    let pad = |t: &str| format!("{t}{}", " ".repeat(w - len(t)));
    assert_eq!(
        text_lines(&items, true, now),
        [
            format!("{}  shop       first", pad("garbled")),
            format!(
                "{}  proj-long  guide changed (2 more lines: --json prints them)",
                pad(&today)
            ),
        ]
    );
    assert_eq!(text_lines(&items[..1], false, now), ["garbled  first"]);
    assert_eq!(text_lines(&[], true, now), ["no news in any repository yet"]);
}
