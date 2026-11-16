use super::*;
use bullpen_core::fleet::{Coordinator, Counts, TaskState};
use bullpen_core::{Obj, Repo};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_state(name: &str, coordinators: Vec<Coordinator>, counts: Counts, open: usize) -> RepoState {
    let repo = Repo {
        root: PathBuf::from("/dev").join(name),
        dir: PathBuf::from("/nowhere"),
        cfg: Obj::new(),
        own: BTreeSet::new(),
    };
    let t = TaskState {
        task: Obj::new(),
        mark: "◌",
        words: String::new(),
        doing: String::new(),
    };
    RepoState {
        repo,
        coordinators,
        open: vec![t; open],
        all: vec![],
        every: Default::default(),
        counts,
        broken: vec![],
    }
}

fn coordinator(id: &str, alive: bool, state: &str) -> Coordinator {
    Coordinator {
        id: id.into(),
        alive: Some(alive),
        state: state.into(),
        ..Default::default()
    }
}

#[test]
fn a_line_per_repository() {
    let shop = repo_state(
        "shop",
        vec![coordinator("c1", true, "working"), coordinator("c2", false, "working")],
        Counts {
            needs: 1,
            ready: 2,
            running: 3,
        },
        6,
    );
    let mut web = repo_state(
        "web-site",
        vec![coordinator("c3", true, "")],
        Counts {
            needs: 2,
            ..Default::default()
        },
        2,
    );
    web.broken = vec!["BP-4.json doesn't parse".into()];
    let quiet = repo_state("quiet", vec![coordinator("c1", true, "blocked")], Counts::default(), 2);
    let none = repo_state("new", vec![], Counts::default(), 0);
    let fleet = Fleet {
        repos: vec![shop, web, quiet, none],
    };
    assert_eq!(
        repo_lines(&fleet),
        [
            "shop      c1 working, c2 gone · 1 needs you · 2 ready · 3 running",
            "web-site  c3 idle · 2 need you · 1 task file doesn't parse",
            "quiet     c1 blocked · 2 open",
            "new       no coordinator · nothing open",
        ]
    );
    assert_eq!(
        repo_lines(&Fleet::default()),
        ["no repositories yet: `bullpen up` in one starts its bullpen"]
    );
}
