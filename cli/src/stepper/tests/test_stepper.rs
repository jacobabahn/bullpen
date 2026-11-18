use super::*;
use crate::tours::tests::{dead_pid, repo_with};

fn r(v: Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn g_on_a_tour_whose_writer_has_just_finished_leaves_it_written() {
    let written = json!({"id": "BP-1", "status": "ready", "tour": {"state": "written", "commit": "c1"}});
    let repo = repo_with("pane-written", written.clone());
    // read while the writer still ran: it has since written the tour and ended
    let mut old = r(json!({"id": "BP-1", "status": "ready", "tour": {"state": "writing", "pid": dead_pid()}}));
    let opener = json!({"session": "s1", "from": "blk1"});
    assert!(!open_once_written(&repo, &mut old, &opener).unwrap());
    assert_eq!(repo.task("BP-1").unwrap(), r(written.clone()));
    assert_eq!(old, r(written)); // the task as it is, for opening the tour now
    let _ = std::fs::remove_dir_all(&repo.dir);
}

#[test]
fn g_on_a_tour_being_written_asks_its_writer_to_open_it() {
    let writing = json!({"id": "BP-1", "status": "ready", "note": "n", "tour": {"state": "writing", "pid": 7}});
    let repo = repo_with("pane-writing", writing);
    let mut old = r(json!({"id": "BP-1", "status": "review", "tour": {"state": "writing"}}));
    let opener = json!({"session": "s1", "from": "blk1"});
    assert!(open_once_written(&repo, &mut old, &opener).unwrap());
    let t = repo.task("BP-1").unwrap();
    assert_eq!(t["tour"], json!({"state": "writing", "pid": 7, "open": opener}));
    assert_eq!((t["status"].clone(), t["note"].clone()), (json!("ready"), json!("n"))); // not the old copy's
    let _ = std::fs::remove_dir_all(&repo.dir);
}
