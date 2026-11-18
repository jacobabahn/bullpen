use super::*;

/// A diff with two hunks in a file, a deleted file, a binary one and a name with a space.
pub const DIFF: &str = "diff --git a/a.txt b/a.txt
index 1..2 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 x
-y
+Y
 z
@@ -20 +20,2 @@ ctx
-q
+Q
+R
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-a
-b
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
diff --git a/x y.txt b/x y.txt
--- a/x y.txt
+++ b/x y.txt
@@ -5,0 +6 @@
+new
";

fn r(v: serde_json::Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn diff_of_reads_files_hunks_and_headers() {
    let files = diff_of(DIFF);
    let names: Vec<&str> = files.iter().map(|f| f.file.as_str()).collect();
    assert_eq!(names, ["a.txt", "old.txt", "img.png", "x y.txt"]);
    let a = &files[0];
    assert_eq!(
        a.head,
        [
            "diff --git a/a.txt b/a.txt",
            "index 1..2 100644",
            "--- a/a.txt",
            "+++ b/a.txt"
        ]
    );
    assert_eq!((a.hunks[0].old, a.hunks[0].new), ((1, 3), (1, 3)));
    assert_eq!(a.hunks[0].text, "@@ -1,3 +1,3 @@\n x\n-y\n+Y\n z");
    assert_eq!((a.hunks[1].old, a.hunks[1].new), ((20, 1), (20, 2)));
    assert_eq!(a.hunks[1].text, "@@ -20 +20,2 @@ ctx\n-q\n+Q\n+R");
    assert!(files[1].deleted && !a.deleted);
    assert_eq!((files[1].hunks[0].old, files[1].hunks[0].new), ((1, 2), (0, 0)));
    assert!(files[2].hunks.is_empty());
    assert_eq!((files[3].hunks[0].old, files[3].hunks[0].new), ((5, 0), (6, 1)));
    assert_eq!(files[3].hunks[0].text, "@@ -5,0 +6 @@\n+new"); // without the diff's last newline
}

#[test]
fn a_rename_is_named_by_its_new_name() {
    let files = diff_of("diff --git a/p q b/r s\nsimilarity index 90%\nrename from p q\nrename to r s\n");
    assert_eq!(files[0].file, "r s");
    assert!(diff_of("").is_empty());
    assert!(diff_of("stray\n").is_empty());
}

#[test]
fn hunk_head_reads_ranges() {
    assert_eq!(hunk_head("@@ -3 +4,0 @@"), Some(((3, 1), (4, 0))));
    assert_eq!(hunk_head("@@ -3,2 +4,5 @@ fn x()"), Some(((3, 2), (4, 5))));
    assert_eq!(hunk_head("@@ -3,2 +4,5"), None);
    assert_eq!(hunk_head("@@@ -1 -1 +1 @@@"), None);
}

#[test]
fn lines_of_a_hunk_are_in_the_new_file_or_the_old_one_when_deleted() {
    let files = diff_of(DIFF);
    assert_eq!(lines_of(&files[0], &files[0].hunks[1]), (20, 21));
    assert_eq!(lines_of(&files[1], &files[1].hunks[0]), (1, 2));
    assert_eq!(span((0, 0)), (1, 1));
}

#[test]
fn covers_matches_lines_on_either_side_of_a_removal() {
    let files = diff_of(DIFF);
    let (a, x) = (&files[0], &files[3]);
    assert!(covers(&r(json!({"file": "a.txt"})), a, &a.hunks[1]));
    assert!(covers(&r(json!({"file": "a.txt", "lines": [21, 30]})), a, &a.hunks[1]));
    assert!(!covers(&r(json!({"file": "a.txt", "lines": [4, 19]})), a, &a.hunks[0]));
    assert!(!covers(&r(json!({"file": "b.txt"})), a, &a.hunks[0]));
    // a pure removal is named by its old lines too
    let del = &files[1];
    assert!(covers(
        &r(json!({"file": "old.txt", "lines": [2, 2]})),
        del,
        &del.hunks[0]
    ));
    assert!(covers(&r(json!({"file": "x y.txt", "lines": [6, 6]})), x, &x.hunks[0]));
}

#[test]
fn step_diff_gives_the_steps_files_in_its_order() {
    let files = diff_of(DIFF);
    let step = r(json!({"hunks": [{"file": "x y.txt"}, {"file": "a.txt", "lines": [20, 21]}, {"file": "img.png"}]}));
    assert_eq!(
        step_diff(&files, &step),
        "diff --git a/x y.txt b/x y.txt\n--- a/x y.txt\n+++ b/x y.txt\n@@ -5,0 +6 @@\n+new\n\
         diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -20 +20,2 @@ ctx\n-q\n+Q\n+R\n\
         diff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n"
    );
    assert_eq!(step_diff(&files, &r(json!({"hunks": []}))), "");
}

#[test]
fn step_files_names_files_and_lines() {
    let step = r(json!({"hunks": [{"file": "a", "lines": [1, 2]}, {"file": "b"}]}));
    assert_eq!(step_files(&step), "a 1-2, b");
    assert_eq!(step_files(&r(json!({"hunks": []}))), "(no code)");
}

/// A repo of its own in the temp folder, with no config, and task BP-1 on disk as given.
pub fn repo_with(name: &str, task: Value) -> Repo {
    let dir = std::env::temp_dir().join(format!("bullpen-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    std::fs::create_dir_all(dir.join("briefs")).unwrap();
    let repo = Repo {
        root: dir.clone(),
        dir,
        cfg: Obj::new(),
        own: Default::default(),
    };
    json::write_obj(&repo.task_path("BP-1"), &r(task)).unwrap();
    repo
}

/// The pid of a process that has ended.
pub fn dead_pid() -> u32 {
    let mut p = std::process::Command::new("true").spawn().unwrap();
    p.wait().unwrap();
    p.id()
}

#[test]
fn a_writer_that_finished_after_its_task_was_read_leaves_the_tour_written() {
    let written = json!({"id": "BP-1", "status": "ready", "tour": {"state": "written", "commit": "c1"}});
    let repo = repo_with("tour-state-written", written);
    json::write(
        &tour_path(&repo, &r(json!({"id": "BP-1"}))),
        &json!({"commit": "c1", "steps": []}),
    )
    .unwrap();
    // read while the writer still ran: it has since written the tour and ended
    let mut old = r(json!({"id": "BP-1", "status": "ready", "tour": {"state": "writing", "pid": dead_pid()}}));
    let (state, tour, why) = tour_state(&repo, &mut old).unwrap();
    assert_eq!((state, why), ("current", None));
    assert!(tour.is_some());
    assert_eq!(
        repo.task("BP-1").unwrap()["tour"],
        json!({"state": "written", "commit": "c1"})
    );
    assert_eq!(old["tour"]["state"], "written");
    let _ = std::fs::remove_dir_all(&repo.dir);
}

#[test]
fn a_tour_whose_writer_is_gone_has_failed_unless_a_new_one_writes_it() {
    let gone = json!({"id": "BP-1", "status": "ready", "tour": {"state": "writing", "pid": dead_pid()}});
    let repo = repo_with("tour-state-gone", gone.clone());
    let (state, _, why) = tour_state(&repo, &mut r(gone.clone())).unwrap();
    let why = why.unwrap();
    assert_eq!(
        (state, why.as_str()),
        ("failed", "its writer stopped before the tour was written")
    );
    assert_eq!(repo.task("BP-1").unwrap()["tour"]["state"], "failed");
    // a new writer took over since the task was read
    let alive = json!({"id": "BP-1", "status": "ready", "tour": {"state": "writing", "pid": std::process::id()}});
    json::write_obj(&repo.task_path("BP-1"), &r(alive)).unwrap();
    assert_eq!(tour_state(&repo, &mut r(gone)).unwrap().0, "writing");
    assert_eq!(repo.task("BP-1").unwrap()["tour"]["pid"], json!(std::process::id()));
    let _ = std::fs::remove_dir_all(&repo.dir);
}
