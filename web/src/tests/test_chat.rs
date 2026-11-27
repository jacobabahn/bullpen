use super::*;

fn snapshot() -> Value {
    json!({"repos": [{
        "name": "proj",
        "root": "/src/proj",
        "settings": {"coordinators": [{"id": "c1", "chat": "c-chat"}]},
        "tasks": [{"task": {"id": "BP-1", "worktree": "/wt/BP-1", "chat": "w-chat", "review": {"chat": "r-chat"}}}],
    }]})
}

#[test]
fn a_watch_names_a_repo_an_id_and_maybe_who() {
    let t = |repo: &str, id: &str, reviewer| Target {
        repo: repo.into(),
        id: id.into(),
        reviewer,
    };
    assert_eq!(Target::watched("proj/BP-1"), Some(t("proj", "BP-1", false)));
    assert_eq!(Target::watched("proj/BP-1/reviewer"), Some(t("proj", "BP-1", true)));
    assert_eq!(Target::watched("proj/c1/worker"), Some(t("proj", "c1", false)));
    for bad in ["", "proj", "proj/", "/BP-1", "proj/BP-1/boss", "proj/BP-1/reviewer/x"] {
        assert_eq!(Target::watched(bad), None, "{bad}");
    }
}

#[test]
fn a_chat_is_located_from_the_snapshot() {
    let snap = snapshot();
    let w = locate(&snap, &Target::new("proj", "BP-1", None).unwrap()).unwrap();
    assert_eq!(w.agent, json!({"repo": "proj", "id": "BP-1", "who": "worker"}));
    assert_eq!((w.chat.as_str(), w.cwd.to_str().unwrap()), ("w-chat", "/wt/BP-1"));
    assert_eq!(w.whose, Agent::Worker("BP-1".into()));
    let r = locate(&snap, &Target::new("proj", "BP-1", Some("reviewer")).unwrap()).unwrap();
    assert_eq!((r.chat.as_str(), r.whose), ("r-chat", Agent::Reviewer("BP-1".into())));
    let c = locate(&snap, &Target::new("proj", "c1", None).unwrap()).unwrap();
    assert_eq!(c.agent["who"], "coordinator");
    assert_eq!((c.chat.as_str(), c.cwd.to_str().unwrap()), ("c-chat", "/src/proj"));
    assert_eq!(c.whose, Agent::Coordinator("proj".into()));
    assert!(matches!(
        locate(&snap, &Target::new("proj", "c1", Some("reviewer")).unwrap()),
        Err(Missing::Wrong(_))
    ));
    assert!(matches!(
        locate(&snap, &Target::new("proj", "BP-2", None).unwrap()),
        Err(Missing::Unknown(_))
    ));
    assert!(matches!(
        locate(&snap, &Target::new("other", "BP-1", None).unwrap()),
        Err(Missing::Unknown(_))
    ));
}

fn you(n: usize) -> Item {
    Item {
        id: format!("i{n}"),
        at: String::new(),
        kind: Kind::You { text: format!("{n}") },
    }
}

#[test]
fn a_page_is_the_last_200_or_the_200_before_one() {
    let all: Vec<Item> = (0..450).map(you).collect();
    let refs: Vec<&Item> = all.iter().collect();
    let ids = |p: &[&Item]| (p.first().unwrap().id.clone(), p.last().unwrap().id.clone(), p.len());
    let (last, more) = page(&refs, None).unwrap();
    assert_eq!((ids(&last), more), (("i250".into(), "i449".into(), 200), true));
    let (before, more) = page(&refs, Some("i250")).unwrap();
    assert_eq!((ids(&before), more), (("i50".into(), "i249".into(), 200), true));
    let (first, more) = page(&refs, Some("i50")).unwrap();
    assert_eq!((ids(&first), more), (("i0".into(), "i49".into(), 50), false));
    assert_eq!(page(&refs, Some("i0")).unwrap(), (vec![], false));
    assert_eq!(page(&refs, Some("nothing")), None);
}

#[test]
fn what_was_said_goes_out_as_html_alone() {
    let i = Item {
        id: "a".into(),
        at: "t".into(),
        kind: Kind::Said {
            text: "**Done** <b>".into(),
        },
    };
    assert_eq!(
        item(&i),
        json!({"kind": "said", "id": "a", "at": "t", "html": "<p><strong>Done</strong> &lt;b&gt;</p>"})
    );
}

#[test]
fn a_question_keeps_its_own_fields_alone() {
    let i = Item {
        id: "q".into(),
        at: "t".into(),
        kind: Kind::Question {
            questions: json!([{"question": "Which?", "header": "Pick", "multiSelect": false, "extra": "x",
                "options": [{"label": "A", "description": "a", "secret": "y"}]}]),
            answers: Some(json!({"Which?": "A", "odd": {"n": 1}})),
            error: None,
        },
    };
    let v = item(&i);
    assert_eq!(
        v["questions"],
        json!([{"question": "Which?", "header": "Pick", "multiSelect": false,
            "options": [{"label": "A", "description": "a"}]}])
    );
    assert_eq!(v["answers"], json!({"Which?": "A"}));
}
