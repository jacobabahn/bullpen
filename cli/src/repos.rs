//! `repos`: every repository bullpen knows, one line each (its coordinators, alive or
//! gone, working or idle; what needs you, what's ready, what's running), or with --json
//! the whole snapshot (core/src/fleet.rs). It answers "is everything up?" from any shell.

use crate::args::{flag, Arg, Args};
use crate::show::print_all;
use bullpen_core::fleet::{self, Fleet, RepoState};
use bullpen_core::json::dumps;
use bullpen_core::text::width;
use bullpen_core::words::count_phrase;
use bullpen_core::Res;

const REPOS: &[Arg] = &[flag(
    "--json",
    "the whole snapshot: each repository's root, settings, coordinators, open tasks (with the board's words) and counts",
)];

pub fn cmd_repos(argv: &[String]) -> Res<i32> {
    let a = Args::parse("repos", argv, REPOS);
    let mut fleet = fleet::read()?;
    fleet.ask_alive(crate::rex::alive);
    if a.flag("--json") {
        print_all(&[dumps(&fleet.to_json())])?;
    } else {
        print_all(&repo_lines(&fleet))?;
    }
    Ok(0)
}

/// One line per repository, its name padded so the rest lines up:
/// "shop  c1 working · 1 needs you · 2 ready · 3 running".
pub fn repo_lines(fleet: &Fleet) -> Vec<String> {
    if fleet.repos.is_empty() {
        return vec!["no repositories yet: `bullpen up` in one starts its bullpen".into()];
    }
    let names: Vec<String> = fleet.repos.iter().map(|r| r.repo.name()).collect();
    let w = names.iter().map(|n| width(n)).max().unwrap_or(0);
    let line = |(name, r): (&String, &RepoState)| {
        let pad = " ".repeat(w - width(name));
        let mut parts = vec![coordinators_words(r)];
        parts.extend(count_words(r));
        format!("{name}{pad}  {}", parts.join(" · "))
    };
    names.iter().zip(&fleet.repos).map(line).collect()
}

/// "c1 working, c2 gone", or "no coordinator".
fn coordinators_words(r: &RepoState) -> String {
    if r.coordinators.is_empty() {
        return "no coordinator".into();
    }
    let each: Vec<String> = r
        .coordinators
        .iter()
        .map(|c| format!("{} {}", c.id, c.word()))
        .collect();
    each.join(", ")
}

/// "1 needs you", "2 ready", "3 running" (those that aren't 0), else "4 open" or
/// "nothing open"; and the task files that don't parse.
fn count_words(r: &RepoState) -> Vec<String> {
    let c = r.counts;
    let counts = [(c.needs, "needs"), (c.ready, "ready"), (c.running, "running")];
    let mut out: Vec<String> = counts
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|&(n, what)| count_phrase(n, what))
        .collect();
    if out.is_empty() {
        out.push(match r.open.len() {
            0 => "nothing open".into(),
            n => format!("{n} open"),
        });
    }
    match r.broken.len() {
        0 => {}
        1 => out.push("1 task file doesn't parse".into()),
        n => out.push(format!("{n} task files don't parse")),
    }
    out
}

#[cfg(test)]
#[path = "test_repos.rs"]
mod tests;
