use super::*;

#[test]
fn a_held_lock_isnt_taken_twice() {
    let dir = std::env::temp_dir().join(format!("bullpen-core-lock-{}", std::process::id()));
    let path = dir.join("sub/news.lock");
    let held = take(&path, true).unwrap().expect("a free lock is taken");
    assert!(try_lock(&path).unwrap().is_none()); // another open file can't have it
    assert!(take(&path, false).unwrap().is_none());
    drop(held);
    let again = try_lock(&path).unwrap();
    assert!(again.is_some());
    // what Python's flock says of it: held, while ours is
    let py = "import fcntl,sys\nf=open(sys.argv[1],'a')\ntry:\n fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)\n print('free')\nexcept BlockingIOError:\n print('held')";
    let said = crate::proc::run(&["python3", "-c", py, path.to_str().unwrap()], false);
    if let Ok(d) = said {
        assert_eq!(d.stdout, "held\n");
    }
    drop(again);
    let _ = std::fs::remove_dir_all(&dir);
}
