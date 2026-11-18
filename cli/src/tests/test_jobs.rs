use super::*;

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bullpen-cli-jobs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("jobs.log")
}

#[test]
fn a_failure_is_the_last_line_the_job_wrote() {
    let log = temp("reason");
    std::fs::write(&log, "before\nstart\nTraceback\n  IsADirectoryError: x  \n\n").unwrap();
    assert_eq!(reason(&log, 0, 0), None);
    assert_eq!(reason(&log, 0, 1).as_deref(), Some("IsADirectoryError: x (exit 1)"));
    assert_eq!(reason(&log, 0, -9).as_deref(), Some("it was killed (signal 9)"));
    let len = std::fs::metadata(&log).unwrap().len();
    assert_eq!(reason(&log, len, 1), None); // non-zero with no output: it said so itself
}

#[test]
fn a_long_log_keeps_its_last_half_from_a_lines_start() {
    let log = temp("trim");
    let line = "x".repeat(99) + "\n";
    std::fs::write(&log, line.repeat(3000)).unwrap(); // 300,000 bytes
    trim(&log);
    let data = std::fs::read_to_string(&log).unwrap();
    assert!(data.len() <= (LOG_MAX / 2) as usize);
    assert!(data.starts_with('x') && data.ends_with('\n'));
    assert_eq!(data.len() % 100, 0);
    std::fs::write(&log, "short\n").unwrap();
    trim(&log);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "short\n");
}

#[test]
fn a_job_waited_for_writes_its_output_to_the_log() {
    let log = temp("wait");
    let dir = log.parent().unwrap();
    let job = Job {
        argv: vec!["sh".into(), "-c".into(), "echo out; echo err >&2".into()],
        cwd: dir,
        log: &log,
        what: "Testing".into(),
        task: None,
        pass_fds: vec![],
    };
    assert!(background(&job, true).is_some());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.lines()
            .next()
            .unwrap()
            .ends_with(" sh -c 'echo out; echo err >&2'"),
        "{text}"
    );
    assert!(text.ends_with("out\nerr\n"), "{text}");
}

#[test]
fn lines_from_jobs_that_run_at_once_are_whole() {
    // BP-198: the supervisors' lines and the jobs' output, all appended to one log at once
    let log = temp("at-once");
    let dir = log.parent().unwrap().to_path_buf();
    let long = "y".repeat(3000);
    let runs: Vec<_> = (0..8)
        .map(|i| {
            let (dir, log, long) = (dir.clone(), log.clone(), long.clone());
            std::thread::spawn(move || {
                for _ in 0..10 {
                    let job = Job {
                        argv: vec!["sh".into(), "-c".into(), format!("echo out{i}"), long.clone()],
                        cwd: &dir,
                        log: &log,
                        what: "Testing".into(),
                        task: None,
                        pass_fds: vec![],
                    };
                    assert!(background(&job, true).is_some());
                }
            })
        })
        .collect();
    for run in runs {
        run.join().unwrap();
    }
    let text = std::fs::read_to_string(&log).unwrap();
    let start = format!("' {long}");
    for line in text.lines() {
        let whole = line.len() == 3 + 1 && line.starts_with("out")
            || line.contains(" sh -c 'echo out") && line.ends_with(&start) && line.matches('y').count() == 3000;
        assert!(whole, "a line cut by another: {}", &line[..line.len().min(200)]);
    }
    assert_eq!(text.lines().filter(|l| l.starts_with("out")).count(), 80);
}
