//! `bullpen later L7 --start`: the item becomes a task (`add` with its title and brief,
//! the task's `from` and `origin` saying where it came from) and its worker starts. The
//! item records its task.

use super::{write_brief, Caller};
use bullpen_core::backlog::{self, is_open};
use bullpen_core::json::{s, truthy, Obj};
use bullpen_core::{fail, Repo, Res};
use serde_json::json;

/// What a task made from the item says it came from: the backlog, or (later) a check,
/// something bullpen found, an issue.
fn origin(item: &Obj) -> &'static str {
    match s(item, "by") {
        b if b.starts_with("check:") => "check",
        b if b.starts_with("found:") => "found",
        "issue" => "issue",
        _ => "backlog",
    }
}

/// Make item id a task, with brief as its brief (else the item's own), and start it.
pub fn start(repo: &Repo, id: &str, brief: Option<&str>, who: &Caller) -> Res<i32> {
    let started_by = match who {
        Caller::Coordinator(c) => c.clone(),
        _ => "person".into(),
    };
    let tid = make_task(repo, id, brief, &started_by, &[])?;
    match crate::start::launch::start(std::slice::from_ref(&tid)) {
        Ok(t) => {
            println!(
                "{id} is {tid} now, started in {} (branch {}, pane {})",
                s(&t, "worktree"),
                s(&t, "branch"),
                s(&t, "block")
            );
            Ok(0)
        }
        Err(e) => fail(format!("{id} is {tid} now, but it didn't start: {}", e.0)),
    }
}

/// Make item id a task (todo), with brief as its brief (else the item's own), started_by
/// on it and `add`'s flags given; its ID. The item records it.
pub fn make_task(repo: &Repo, id: &str, brief: Option<&str>, started_by: &str, flags: &[String]) -> Res<String> {
    // under the backlog's lock, so two starts of one item make one task
    let tid = backlog::change(repo, |b| {
        let item = b.item(id)?;
        if !is_open(item) {
            return fail(super::not_open(item));
        }
        if let Some(text) = brief {
            let name = write_brief(repo, id, text)?;
            b.item_mut(id)?.insert("brief".into(), json!(name));
        }
        let item = b.item(id)?.clone();
        if !truthy(item.get("brief")) {
            return fail(format!(
                "{id} has no brief: a worker needs one. `bullpen later {id} --start --brief-file FILE`"
            ));
        }
        let path = repo.dir.join("briefs").join(s(&item, "brief"));
        let mut extra = Obj::new();
        extra.insert("from".into(), json!(id));
        extra.insert("origin".into(), json!(origin(&item)));
        extra.insert("started_by".into(), json!(started_by));
        let mut argv = vec!["--brief-file".into(), path.display().to_string()];
        argv.extend(flags.iter().cloned());
        // `--` last: a title may start with a dash
        argv.extend(["--".into(), s(&item, "title").to_string()]);
        let tid = crate::start::add_in(Repo::new(&repo.root)?, &argv, extra)?;
        let it = b.item_mut(id)?;
        it.insert("state".into(), json!("started"));
        it.insert("task".into(), json!(tid));
        it.shift_remove("skipped");
        Ok(tid)
    })?;
    crate::issues::decided(repo, id)?; // an issue's item started is a yes
    Ok(tid)
}
