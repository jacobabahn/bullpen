//! A chat as the page gets it (`GET /api/chat/{repo}/{id}?who=&before=`, the `chat`
//! event): whose it is (a task's worker or reviewer, or a coordinator, named by its
//! repo's name and its ID), its transcript found from the snapshot (the session ID
//! bullpen recorded, else matched by title: bullpen_core::chat::find), and its items as
//! the page draws them: what the agent said as HTML (markdown.rs), an AskUserQuestion's
//! fields alone, never a record. A page is the last 200 items, or the 200 before one.

use crate::markdown;
use bullpen_core::chat::{self, Agent, Item, Kind, Source};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// How many items a page has.
pub const PAGE: usize = 200;

/// Whose chat is asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub repo: String,
    pub id: String,
    pub reviewer: bool,
}

impl Target {
    /// From the route's parts; `who` is `worker` (or nothing) or `reviewer`.
    pub fn new(repo: &str, id: &str, who: Option<&str>) -> Result<Target, String> {
        let reviewer = match who.unwrap_or("") {
            "" | "worker" => false,
            "reviewer" => true,
            w => return Err(format!("who is worker or reviewer, not {w:?}")),
        };
        Ok(Target {
            repo: repo.into(),
            id: id.into(),
            reviewer,
        })
    }

    /// From `/events?watch={repo}/{id}[/{who}]`.
    pub fn watched(watch: &str) -> Option<Target> {
        let mut parts = watch.split('/');
        let (repo, id) = (parts.next()?, parts.next()?);
        let who = parts.next();
        if repo.is_empty() || id.is_empty() || parts.next().is_some() {
            return None;
        }
        Target::new(repo, id, who).ok()
    }

    fn who(&self) -> &'static str {
        if self.reviewer {
            "reviewer"
        } else {
            "worker"
        }
    }
}

/// Where a chat's transcript is looked for: whose it is, the session ID recorded for
/// it, and the folder it runs in. The same Where finds the same file, so a stream looks
/// again only when it changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Where {
    /// `{repo, id, who}`: `who` is `coordinator` for a coordinator's
    pub agent: Value,
    whose: Agent,
    chat: String,
    cwd: PathBuf,
}

/// Why a chat can't be had: no such repo, task or coordinator in the snapshot (404), or
/// it was asked for wrongly (a coordinator's reviewer: 400).
#[derive(Debug, PartialEq)]
pub enum Missing {
    Unknown(String),
    Wrong(String),
}

/// Where the chat is, from the snapshot as the page has it (fleet.rs's JSON).
pub fn locate(snapshot: &Value, t: &Target) -> Result<Where, Missing> {
    let repos = snapshot.get("repos").and_then(Value::as_array);
    let repo = repos
        .into_iter()
        .flatten()
        .find(|r| text(r, "name") == t.repo)
        .ok_or_else(|| Missing::Unknown(format!("no repo named {}", t.repo)))?;
    let entries = repo
        .get("settings")
        .and_then(|s| s.get("coordinators"))
        .and_then(Value::as_array);
    if let Some(c) = entries.into_iter().flatten().find(|c| text(c, "id") == t.id) {
        if t.reviewer {
            return Err(Missing::Wrong(format!("{} is a coordinator: it has no reviewer", t.id)));
        }
        return Ok(Where {
            agent: json!({"repo": t.repo, "id": t.id, "who": "coordinator"}),
            whose: Agent::Coordinator(t.repo.clone()),
            chat: text(c, "chat").into(),
            cwd: text(repo, "root").into(),
        });
    }
    let tasks = repo.get("tasks").and_then(Value::as_array);
    let task = tasks
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("task").and_then(Value::as_object))
        .find(|task| task.get("id").and_then(Value::as_str) == Some(t.id.as_str()))
        .ok_or_else(|| Missing::Unknown(format!("no task or coordinator {} in {}", t.id, t.repo)))?;
    Ok(Where {
        agent: json!({"repo": t.repo, "id": t.id, "who": t.who()}),
        whose: match t.reviewer {
            true => Agent::Reviewer(t.id.clone()),
            false => Agent::Worker(t.id.clone()),
        },
        chat: chat::recorded(task, t.reviewer).into(),
        cwd: task.get("worktree").and_then(Value::as_str).unwrap_or("").into(),
    })
}

impl Where {
    /// Its transcript, and how it was found; None when there's none (yet).
    pub fn find(&self) -> Option<(PathBuf, Source)> {
        chat::find(&chat::projects(), &self.chat, &self.cwd, &self.whose)
    }
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The answer for a page of the chat: `{agent, source, items, cursor, more}`, the
/// agent with its transcript's title and permission mode. `page` is Open::page's (None:
/// no transcript, so no items).
pub fn answer(w: &Where, source: Option<Source>, page: Option<Value>) -> Value {
    let page = page.unwrap_or_else(|| json!({"title": null, "mode": null, "items": [], "cursor": 0, "more": false}));
    let mut agent = w.agent.clone();
    agent["title"] = page["title"].clone();
    agent["mode"] = page["mode"].clone();
    json!({
        "agent": agent,
        "source": source.map_or("none", Source::word),
        "items": page["items"],
        "cursor": page["cursor"],
        "more": page["more"],
    })
}

/// The items to send for a page: the last PAGE before the item `before` (or the end),
/// and whether there are earlier ones. None when `before` isn't one of them.
pub fn page<'a>(items: &[&'a Item], before: Option<&str>) -> Option<(Vec<&'a Item>, bool)> {
    let end = match before {
        Some(id) => items.iter().position(|i| i.id == id)?,
        None => items.len(),
    };
    let start = end.saturating_sub(PAGE);
    Some((items[start..end].to_vec(), start > 0))
}

/// An item as the page gets it (`Item::json`): what the agent said as HTML in place of
/// its Markdown, and an AskUserQuestion's questions and answers with their own fields
/// alone (nothing else the tool's input or result had).
pub fn item(i: &Item) -> Value {
    let mut v = i.json();
    match &i.kind {
        Kind::Said { text } => {
            if let Some(o) = v.as_object_mut() {
                o.remove("text");
                o.insert("html".into(), json!(markdown::html(text)));
            }
        }
        Kind::Question { questions, answers, .. } => {
            v["questions"] = json!(questions_only(questions));
            v["answers"] = answers.as_ref().map_or(Value::Null, |a| json!(strings(a)));
        }
        _ => {}
    }
    v
}

/// `[{question, header, multiSelect, options: [{label, description}]}]`.
fn questions_only(questions: &Value) -> Vec<Value> {
    let qs = questions.as_array().into_iter().flatten();
    qs.map(|q| {
        let options = q.get("options").and_then(Value::as_array).into_iter().flatten();
        let options: Vec<Value> = options
            .map(|o| json!({"label": text(o, "label"), "description": text(o, "description")}))
            .collect();
        json!({
            "question": text(q, "question"),
            "header": text(q, "header"),
            "multiSelect": q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
            "options": options,
        })
    })
    .collect()
}

/// The answers chosen, question to answer, the strings alone.
fn strings(answers: &Value) -> Map<String, Value> {
    let all = answers.as_object().into_iter().flatten();
    all.filter(|(_, a)| a.is_string())
        .map(|(q, a)| (q.clone(), a.clone()))
        .collect()
}

#[cfg(test)]
#[path = "tests/test_chat.rs"]
mod tests;
