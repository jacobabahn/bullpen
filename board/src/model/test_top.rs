use super::*;
use crate::model::testing::*;

/// A state folder of its own under the system's temp folder, removed when dropped.
struct TempState(PathBuf);

impl TempState {
    fn new(name: &str) -> TempState {
        let d = std::env::temp_dir().join(format!("bullpen-board-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("repos")).unwrap();
        TempState(d)
    }

    /// A repository's state: its checkout (made unless gone) and tasks of these statuses.
    fn repo(&self, name: &str, gone: bool, statuses: &[&str]) -> PathBuf {
        let root = self.0.join("checkouts").join(name);
        if !gone {
            std::fs::create_dir_all(&root).unwrap();
        }
        let dir = self.0.join("repos").join(name);
        std::fs::create_dir_all(dir.join("tasks")).unwrap();
        std::fs::write(dir.join("config.json"), serde_json::json!({"root": root}).to_string()).unwrap();
        for (i, st) in statuses.iter().enumerate() {
            let t = serde_json::json!({"id": format!("BP-{}", i + 1), "status": st});
            std::fs::write(dir.join("tasks").join(format!("BP-{}.json", i + 1)), t.to_string()).unwrap();
        }
        dir
    }
}

impl Drop for TempState {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_other_repos_are_counted_without_this_one() {
    let st = TempState::new("others");
    let me = st.repo("proj", false, &["needs", "ready"]);
    st.repo("shop", false, &["needs", "stopped", "ready", "finished", "working", "triage"]);
    st.repo("quiet", false, &["working", "todo", "done"]); // nothing for you: left out
    st.repo("gone", true, &["needs"]); // its checkout is gone: left out
    st.repo("web", false, &["ready"]);
    let mut o = OtherRepos::default();
    o.look(&me);
    assert_eq!(
        o.counts,
        vec![
            RepoCount { name: "shop".into(), needs: 2, ready: 2 },
            RepoCount { name: "web".into(), needs: 0, ready: 1 },
        ]
    );
    // a task changes: the next look sees it
    std::fs::write(st.0.join("repos/web/tasks/BP-2.json"), r#"{"id": "BP-2", "status": "needs"}"#).unwrap();
    o.look(&me);
    assert_eq!(o.counts[1], RepoCount { name: "web".into(), needs: 1, ready: 1 });
    // refresh looks at most every OTHERS_EVERY
    std::fs::remove_dir_all(st.0.join("repos/web")).unwrap();
    o.refresh(&me);
    o.refresh(&me);
    assert_eq!(o.counts.len(), 1);
    std::fs::remove_dir_all(st.0.join("repos/shop")).unwrap();
    o.refresh(&me);
    assert_eq!(o.counts.len(), 1);
}

#[test]
fn the_other_repos_shrink_to_marks_when_the_words_dont_fit() {
    let counts = vec![
        RepoCount { name: "shop".into(), needs: 1, ready: 2 },
        RepoCount { name: "web".into(), needs: 0, ready: 1 },
    ];
    assert_eq!(others_words(&counts), "shop: 1 needs you, 2 ready · web: 1 ready");
    assert_eq!(others_marks(&counts), "shop ●1 ✔2 · web ✔1");
    let o = |room| tr(0, 0, &counts, room);
    assert_eq!(o(80).0, "shop: 1 needs you, 2 ready · web: 1 ready");
    assert_eq!(o(30).0, "shop ●1 ✔2 · web ✔1");
    assert_eq!(width(&o(13).0), 10);
    assert_eq!(o(10).0, ""); // under 8 columns: left out
}

/// The top line's right as text: the other repos, and this one's counts.
fn tr(needs: usize, ready: usize, others: &[RepoCount], room: usize) -> (String, String) {
    let t = top_right(needs, ready, others, room);
    let counts: Vec<String> = t.counts.into_iter().map(|(s, _)| s).collect();
    (t.others, counts.join(t.sep))
}

fn mine(counts: &str) -> (String, String) {
    (String::new(), counts.into())
}

#[test]
fn the_top_line_counts_what_you_act_on_in_words_when_they_fit() {
    let parts = |n, r, words| act_parts(n, r, words).into_iter().map(|(s, _)| s).collect::<Vec<_>>();
    assert_eq!(parts(2, 3, true), ["2 need you", "3 ready"]);
    assert_eq!(parts(1, 0, true), ["1 needs you"]);
    assert_eq!(parts(2, 3, false), ["●2", "✔3"]);
    assert!(parts(0, 0, true).is_empty());
    // "2 need you · 3 ready" is 20 wide, "●2 ✔3" 5, each with 2 columns before it
    assert_eq!(tr(2, 3, &[], 22), mine("2 need you · 3 ready"));
    assert_eq!(tr(2, 3, &[], 21), mine("●2 ✔3"));
    assert_eq!(tr(2, 3, &[], 7), mine("●2 ✔3"));
    assert_eq!(tr(2, 3, &[], 6), mine("●2")); // then only what needs you
    assert!(top_right(2, 3, &[], 7).whole && !top_right(2, 3, &[], 6).whole);
    assert_eq!(tr(2, 3, &[], 3), mine(""));
    assert_eq!(tr(0, 3, &[], 4), mine("✔3"));
    assert_eq!(tr(0, 3, &[], 3), mine(""));
    assert!(top_right(0, 0, &[], 0).whole);
    let ts = [task("BP-1", "needs"), task("BP-2", "stopped"), task("BP-3", "ready"), task("BP-4", "finished"),
              task("BP-5", "working"), task("BP-6", "todo"), task("BP-7", "triage"), task("BP-8", "review")];
    assert_eq!(acts(&ts.iter().collect::<Vec<_>>()), (2, 2));
}

#[test]
fn the_other_repos_get_the_room_before_this_ones_counts() {
    let shop = |needs, ready| vec![RepoCount { name: "shop".into(), needs, ready }];
    let pair = |o: &str, c: &str| (o.to_string(), c.to_string());
    // "shop: 1 needs you" is 17 wide and " · " before it: 20
    assert_eq!(tr(2, 3, &shop(1, 0), 42), pair("shop: 1 needs you", "2 need you · 3 ready"));
    assert_eq!(tr(2, 3, &shop(1, 0), 41), pair("shop: 1 needs you", "●2 ✔3"));
    assert_eq!(tr(2, 3, &shop(1, 0), 27), pair("shop: 1 needs you", "●2 ✔3"));
    // then the other repos shrink to marks
    assert_eq!(tr(2, 3, &shop(1, 0), 26), pair("shop ●1", "●2 ✔3"));
    // their marks and this one's words, when their words are what's long
    let two = vec![RepoCount { name: "shop".into(), needs: 2, ready: 4 }, RepoCount { name: "web".into(), needs: 0, ready: 1 }];
    assert_eq!(tr(2, 3, &two, 44), pair("shop ●2 ✔4 · web ✔1", "2 need you · 3 ready"));
    // then cut, never under 8 columns, and this one's marks stay
    let long = vec![RepoCount { name: "jj-evolved-sandbox".into(), needs: 1, ready: 2 }];
    assert_eq!(tr(2, 3, &long, 20), pair("jj-evolve…", "●2 ✔3"));
    assert!(!top_right(2, 3, &long, 20).whole);
    // without room for both, the other repos' marks stay whole and this one's counts go
    let jj = vec![RepoCount { name: "jj-evolved".into(), needs: 0, ready: 1 }];
    assert_eq!(tr(1, 1, &jj, 16), pair("jj-evolved ✔1", ""));
    assert!(!top_right(1, 1, &jj, 16).whole);
    // and with no room for even those, this one's counts come back
    assert_eq!(tr(1, 1, &jj, 15), pair("", "●1 ✔1"));
    assert!(top_right(1, 1, &jj, 23).whole);
}

#[test]
fn the_top_line_counts_in_the_order_of_the_groups() {
    // what the top line counts, in the order the board lists their groups
    let groups: Vec<&str> = GROUPS.iter().flat_map(|(_, sts)| sts.iter().copied()).collect();
    let counted: Vec<&str> = act_parts(1, 1, true).into_iter().map(|(_, st)| st).collect();
    let at: Vec<usize> = counted.iter().map(|st| groups.iter().position(|g| g == st).unwrap()).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{counted:?} against {groups:?}");
    assert_eq!(counted, ["needs", "ready"]);
}
