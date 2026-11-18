//! What mission control lists, apart from how it draws it: every repo's tasks as one
//! inbox (what needs you, then what's ready, longest wait first), then each repo with
//! its coordinators and its other open tasks in the board's order. All of it comes
//! from the fleet snapshot (bullpen_core::fleet).

use bullpen_core::fleet::{Coordinator, Fleet, RepoState, TaskState};
use bullpen_core::json::s;
use bullpen_core::task::{awaits_accept, needs_you};
use bullpen_core::words::count_phrase;
use bullpen_core::timeutil::{parse, Stamp};

/// A task on the screen: fleet.repos[repo].open[task].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub repo: usize,
    pub task: usize,
}

/// A row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// "NEEDS YOU", "READY"
    Head(&'static str),
    /// a task; in the inbox it names its repo
    Task(Item, bool),
    /// a repo's line: its name, root, coordinators and how many are open
    Repo(usize),
    Gap,
}

/// The inbox's two parts: what needs you (needs, stopped), then what waits on your
/// accept or close (ready, finished): core's rules, by status.
pub const INBOX: &[(&str, fn(&str) -> bool)] = &[("NEEDS YOU", needs_you), ("READY", awaits_accept)];

pub fn status(t: &TaskState) -> &str {
    s(&t.task, "status")
}

pub fn id(t: &TaskState) -> &str {
    s(&t.task, "id")
}

/// Is it in the inbox?
pub fn in_inbox(t: &TaskState) -> bool {
    INBOX.iter().any(|(_, part)| part(status(t)))
}

/// When it entered its current status, for "longest wait first"; None sorts last.
fn since(t: &TaskState) -> Option<Stamp> {
    bullpen_core::task::stamp(&t.task).and_then(parse)
}

/// The inbox's tasks whose status is in this part, every repo's, longest wait first
/// (the order `go --next --all-repos` takes), then by repo and the board's order.
fn inbox_part(fleet: &Fleet, part: fn(&str) -> bool) -> Vec<Item> {
    let mut items: Vec<(Option<Stamp>, Item)> = Vec::new();
    for (ri, r) in fleet.repos.iter().enumerate() {
        for (ti, t) in r.open.iter().enumerate() {
            if part(status(t)) {
                items.push((since(t), Item { repo: ri, task: ti }));
            }
        }
    }
    // a stable sort: same-time tasks keep the repos' and the board's order
    items.sort_by_key(|(at, _)| (at.is_none(), *at));
    items.into_iter().map(|(_, i)| i).collect()
}

/// The list's rows: the inbox's parts that have tasks, then each repo's line and its
/// open tasks that aren't in the inbox, a gap between each.
pub fn rows(fleet: &Fleet) -> Vec<Row> {
    let mut rows = Vec::new();
    for (head, part) in INBOX {
        let items = inbox_part(fleet, *part);
        if !items.is_empty() {
            rows.push(Row::Head(head));
            rows.extend(items.into_iter().map(|i| Row::Task(i, true)));
        }
    }
    for (ri, r) in fleet.repos.iter().enumerate() {
        if !rows.is_empty() {
            rows.push(Row::Gap);
        }
        rows.push(Row::Repo(ri));
        for (ti, t) in r.open.iter().enumerate() {
            if !in_inbox(t) {
                rows.push(Row::Task(Item { repo: ri, task: ti }, false));
            }
        }
    }
    rows
}

/// The tasks in the order the list shows them: what the selection moves through.
pub fn order(rows: &[Row]) -> Vec<Item> {
    rows.iter()
        .filter_map(|r| match r {
            Row::Task(i, _) => Some(*i),
            _ => None,
        })
        .collect()
}

/// The task an item names.
pub fn task(fleet: &Fleet, i: Item) -> &TaskState {
    &fleet.repos[i.repo].open[i.task]
}

/// The totals on the top line: needs you, ready, running (working or in review), open,
/// and how many repos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub needs: usize,
    pub ready: usize,
    pub running: usize,
    pub open: usize,
    pub repos: usize,
    /// coordinators blocked on a question in their panes
    pub blocked: usize,
}

pub fn totals(fleet: &Fleet) -> Totals {
    let mut t = Totals { repos: fleet.repos.len(), ..Default::default() };
    for r in &fleet.repos {
        t.needs += r.counts.needs;
        t.ready += r.counts.ready;
        t.running += r.counts.running;
        t.open += r.open.len();
        t.blocked += r.coordinators.iter().filter(|c| c.word() == "blocked").count();
    }
    t
}

/// Is one of a repo's coordinators blocked on a question in its pane: it waits on you.
pub fn coordinator_blocked(r: &RepoState) -> bool {
    r.coordinators.iter().any(|c| c.word() == "blocked")
}

/// The top line's totals in words: "1 coordinator blocked · 3 need you · 2 ready · 4
/// of 9 running · 2 repos"; what's 0 is left out, but the repos always show.
pub fn totals_words(t: Totals) -> String {
    let mut parts: Vec<String> = match t.blocked {
        0 => Vec::new(),
        1 => vec!["1 coordinator blocked".into()],
        n => vec![format!("{n} coordinators blocked")],
    };
    let counts = [(t.needs, "needs"), (t.ready, "ready")].into_iter().filter(|(n, _)| *n > 0);
    parts.extend(counts.map(|(n, what)| count_phrase(n, what)));
    if t.running > 0 {
        parts.push(format!("{} of {} running", t.running, t.open));
    } else if t.open > 0 {
        parts.push(format!("{} open", t.open));
    }
    parts.push(match t.repos {
        1 => "1 repo".into(),
        n => format!("{n} repos"),
    });
    parts.join(" · ")
}

/// The same in marks, for a narrow screen: "c●1 ●3 ✔2 4/9".
pub fn totals_marks(t: Totals) -> String {
    let mut parts = Vec::new();
    if t.blocked > 0 {
        parts.push(format!("c●{}", t.blocked));
    }
    if t.needs > 0 {
        parts.push(format!("●{}", t.needs));
    }
    if t.ready > 0 {
        parts.push(format!("✔{}", t.ready));
    }
    if t.open > 0 {
        parts.push(format!("{}/{}", t.running, t.open));
    }
    parts.join(" ")
}

/// A coordinator's mark and word: "●" working, "●" blocked (at a question in its pane),
/// "○" idle, "✕" gone, "?" when its note can't be read.
pub fn coordinator_mark(c: &Coordinator) -> &'static str {
    match c.word() {
        "working" | "blocked" => "●",
        "gone" => "✕",
        "unknown" => "?",
        _ => "○",
    }
}

/// A repo's coordinators in words: "c3 ● working", "c1 ○ idle, c2 ✕ gone", or
/// "no coordinator". `short`: marks only ("c3 ●").
pub fn coordinators_words(r: &RepoState, short: bool) -> String {
    if r.coordinators.is_empty() {
        return "no coordinator".into();
    }
    let each = r.coordinators.iter().map(|c| {
        if short {
            format!("{} {}", c.id, coordinator_mark(c))
        } else {
            format!("{} {} {}", c.id, coordinator_mark(c), c.word())
        }
    });
    each.collect::<Vec<_>>().join(", ")
}

/// How many of a repo's tasks are open, in words: "5 open", "nothing open"; with the
/// task files that don't parse: "5 open · 1 file doesn't parse".
pub fn open_words(r: &RepoState) -> String {
    let open = match r.open.len() {
        0 => "nothing open".to_string(),
        n => format!("{n} open"),
    };
    match r.broken.len() {
        0 => open,
        1 => format!("{open} · 1 file doesn't parse"),
        n => format!("{open} · {n} files don't parse"),
    }
}

/// A path with the home folder as "~".
pub fn tilde(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => format!("~{rest}"),
        _ => path.to_string(),
    }
}

#[cfg(test)]
#[path = "tests/test_items.rs"]
mod tests;
