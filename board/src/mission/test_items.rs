use super::*;
use crate::mission::testing::*;

/// The rows as text: "H NEEDS YOU", "T shop BP-2" (inbox), "t BP-1", "R shop", "-".
fn names(m: &crate::mission::Mission) -> Vec<String> {
    rows(&m.fleet)
        .iter()
        .map(|r| match *r {
            Row::Head(h) => format!("H {h}"),
            Row::Gap => "-".into(),
            Row::Repo(i) => format!("R {}", m.fleet.repos[i].repo.name()),
            Row::Task(i, inbox) => {
                let (repo, id) = (m.fleet.repos[i.repo].repo.name(), self::id(task(&m.fleet, i)).to_string());
                if inbox { format!("T {repo} {id}") } else { format!("t {id}") }
            }
        })
        .collect()
}

#[test]
fn the_inbox_puts_what_needs_you_first_then_what_is_ready_longest_wait_first_across_repos() {
    let st = State::new("inbox");
    st.repo("shop", json!([]), &[
        t("BP-1", "working", "Build", 30),
        t("BP-2", "needs", "Asks", 5),
        t("BP-3", "ready", "Done one", 60),
        t("BP-4", "todo", "Later", 1),
    ]);
    st.repo("web", json!([]), &[
        t("BP-1", "stopped", "Exited", 20),
        t("BP-2", "finished", "Notes", 10),
        t("BP-3", "review", "Look", 2),
        t("BP-4", "done", "Old", 2),
    ]);
    let m = st.mission();
    assert_eq!(
        names(&m),
        [
            "H NEEDS YOU", "T web BP-1", "T shop BP-2",
            "H READY", "T shop BP-3", "T web BP-2",
            "-", "R shop", "t BP-1", "t BP-4",
            "-", "R web", "t BP-3",
        ]
    );
}

#[test]
fn with_nothing_in_the_inbox_the_repos_come_first() {
    let st = State::new("noinbox");
    st.repo("shop", json!([]), &[t("BP-1", "working", "Build", 3)]);
    st.repo("web", json!([]), &[]);
    let m = st.mission();
    assert_eq!(names(&m), ["R shop", "t BP-1", "-", "R web"]);
}

#[test]
fn the_totals_in_words_and_in_marks() {
    let st = State::new("totals");
    st.repo("shop", json!([]), &[t("BP-1", "working", "a", 1), t("BP-2", "needs", "b", 1), t("BP-3", "todo", "c", 1)]);
    st.repo("web", json!([]), &[t("BP-1", "ready", "d", 1), t("BP-2", "review", "e", 1)]);
    let all = totals(&st.mission().fleet);
    assert_eq!(all, Totals { needs: 1, ready: 1, running: 2, open: 5, repos: 2 });
    assert_eq!(totals_words(all), "1 needs you · 1 ready · 2 of 5 running · 2 repos");
    assert_eq!(totals_marks(all), "●1 ✔1 2/5");
    let quiet = Totals { open: 2, repos: 1, ..Default::default() };
    assert_eq!(totals_words(quiet), "2 open · 1 repo");
    assert_eq!(totals_words(Totals::default()), "0 repos");
}

#[test]
fn a_repos_coordinators_and_what_is_open_in_words() {
    let st = State::new("coords");
    let cs = json!([{"id": "c1", "block": "block:a", "session": "s"}, {"id": "c2", "block": "block:b", "session": "s"}]);
    st.repo("shop", cs, &[t("BP-1", "todo", "a", 1)]);
    st.repo("web", json!([]), &[]);
    std::fs::write(st.0.join("repos/shop/tasks/BP-9.json"), "{").unwrap();
    let mut m = st.mission();
    m.fleet.repos[0].coordinators[1].alive = Some(false);
    let (shop, web) = (&m.fleet.repos[0], &m.fleet.repos[1]);
    assert_eq!(coordinators_words(shop, false), "c1 ○ idle, c2 ✕ gone");
    assert_eq!(coordinators_words(shop, true), "c1 ○, c2 ✕");
    assert_eq!(coordinators_words(web, false), "no coordinator");
    assert_eq!(open_words(shop), "1 open · 1 file doesn't parse");
    assert_eq!(open_words(web), "nothing open");
}

#[test]
fn a_coordinator_whose_note_cant_be_read_is_unknown() {
    let st = State::new("unread");
    st.repo("shop", json!([{"id": "c1", "block": "block:a", "session": "s"}]), &[]);
    std::fs::create_dir_all(st.0.join("doing")).unwrap();
    std::fs::write(st.0.join("doing/block_a.json"), "{").unwrap();
    let m = st.mission();
    assert_eq!(coordinators_words(&m.fleet.repos[0], false), "c1 ? unknown");
}

#[test]
fn home_is_a_tilde() {
    let home = std::env::var("HOME").unwrap();
    assert_eq!(tilde(&format!("{home}/dev/shop")), "~/dev/shop");
    assert_eq!(tilde(&format!("{home}x/dev")), format!("{home}x/dev"));
    assert_eq!(tilde("/srv/shop"), "/srv/shop");
}
