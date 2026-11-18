use super::*;
use std::path::PathBuf;

fn repo(cfg: Value) -> Repo {
    Repo {
        root: PathBuf::from("/x/proj"),
        dir: PathBuf::from("/s/repos/x-proj"),
        cfg: cfg.as_object().unwrap().clone(),
        own: Default::default(),
    }
}

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

fn at(kind: &str) -> Where {
    let mut layer = Obj::new();
    layer.insert("kind".into(), json!(kind));
    Where {
        window: Some("w1".into()),
        layer,
        block: "blk3".into(),
    }
}

#[test]
fn test_the_diff_pane_gets_a_tab_when_the_task_has_a_tab_each() {
    let r = repo(json!({"workers": "float"}));
    assert_eq!(for_new(&r, &task(json!({"placement": "tabs"})), "diff"), Tabs);
    assert_eq!(for_new(&r, &task(json!({"placement": "tab"})), "diff"), Float);
    // never in the background, since it's made to be looked at
    assert_eq!(
        for_new(&repo(json!({"workers": "background"})), &task(json!({})), "diff"),
        Float
    );
}

#[test]
fn a_new_panes_placement_follows_the_task_then_the_settings() {
    let r = repo(json!({"workers": "tab", "reviewers": "background"}));
    assert_eq!(for_new(&r, &task(json!({})), "worker"), Tab);
    assert_eq!(
        for_new(&r, &task(json!({"placement": "background"})), "worker"),
        Background
    );
    assert_eq!(for_new(&r, &task(json!({"placement": "bogus"})), "worker"), Tab);
    assert_eq!(for_new(&r, &task(json!({"placement": "tabs"})), "reviewer"), Background); // the reviewers setting
    let same = repo(json!({"workers": "tab", "reviewers": "same"}));
    assert_eq!(for_new(&same, &task(json!({"placement": "tabs"})), "reviewer"), Tabs);
    assert_eq!(for_new(&repo(json!({})), &task(json!({})), "worker"), Float);
    assert_eq!(for_new(&repo(json!({})), &task(json!({})), "reviewer"), Background);
}

#[test]
fn a_pane_is_shown_the_way_it_was_made() {
    let t = task(json!({"pane": "background", "review": {"pane": "tab"}}));
    assert_eq!(made_with(&repo(json!({})), &t, "worker"), Background);
    assert_eq!(made_with(&repo(json!({})), &t, "reviewer"), Tab);
    // before bullpen recorded it, both followed `workers`
    assert_eq!(
        made_with(&repo(json!({"workers": "tabs"})), &task(json!({})), "reviewer"),
        Tabs
    );
    assert_eq!(
        made_with(&repo(json!({"workers": "background"})), &task(json!({})), "worker"),
        Float
    );
}

#[test]
fn placement_at_goes_by_where_the_pane_is_now() {
    assert_eq!(placement_at(None, Some(Float)), None);
    assert_eq!(placement_at(Some(&detached("blk3")), Some(Float)), Some(Background));
    assert_eq!(placement_at(Some(&at("floating")), Some(Background)), Some(Background)); // up, floating
    assert_eq!(placement_at(Some(&at("tiled")), Some(Background)), Some(Background));
    assert_eq!(placement_at(Some(&at("floating")), Some(Tab)), Some(Tab));
    assert_eq!(placement_at(Some(&at("floating")), Some(Tabs)), Some(Float)); // dragged into a float
    assert_eq!(placement_at(Some(&at("floating")), None), Some(Float));
    assert_eq!(placement_at(Some(&at("tiled")), Some(Float)), Some(Tabs)); // moved into a tab by hand
}

#[test]
fn test_a_second_coordinator_has_its_own_workers_tab() {
    let r = repo(json!({}));
    let c = |id: &str| task(json!({"id": id}));
    assert_eq!(tab_label(&r, &c("c1")), "bullpen: proj workers");
    assert_eq!(tab_label(&r, &c("c2")), "bullpen: proj workers c2");
    assert_eq!(tab_label(&r, &Obj::new()), "bullpen: proj workers");
}

#[test]
fn an_agents_pane_says_how_to_hide_it() {
    let t = task(json!({"worktree": "/wt"}));
    let s = spec(&t, "bullpen: BP-1", &["true".to_string()], true);
    assert_eq!(s["label"], "bullpen: BP-1 · ctrl+esc hides");
    assert_eq!(s["options"]["cwd"], "/wt");
    assert_eq!(
        spec(&t, "bullpen: BP-1 diff", &[], false)["label"],
        "bullpen: BP-1 diff"
    );
    assert_eq!(Placement::named("tabs"), Some(Tabs));
    assert_eq!(Placement::named(""), None);
}
