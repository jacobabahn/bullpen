//! The top line's counts: this repo's and the other repositories' (from core's fleet.rs
//! snapshot).

use bullpen_core::fleet::Reader;
use bullpen_core::words::count_phrase;
use std::path::Path;
use std::time::{Duration, Instant};
use super::*;

/// Another repository with tasks that need you or are ready, for the top line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoCount {
    pub name: String,
    /// needs you (needs or stopped)
    pub needs: usize,
    /// waits on your accept or close (ready or finished)
    pub ready: usize,
}

/// How often the board looks at the other repositories: their tasks aren't watched.
pub const OTHERS_EVERY: Duration = Duration::from_secs(10);

/// The other repositories' counts, kept between looks. A look takes the snapshot of
/// every repo (bullpen_core::fleet), whose reader is kept: it reads a task file again
/// only when the file changed.
#[derive(Default)]
pub struct OtherRepos {
    pub counts: Vec<RepoCount>,
    reader: Reader,
    looked: Option<Instant>,
}

impl OtherRepos {
    /// Look again if the last look is OTHERS_EVERY old.
    pub fn refresh(&mut self, me: &Path) {
        if self.looked.is_some_and(|t| t.elapsed() < OTHERS_EVERY) {
            return;
        }
        self.looked = Some(Instant::now());
        self.look(me);
    }

    /// The repositories beside me (<state>/repos/*) whose checkout is still there,
    /// each with what needs you and what's ready; those with neither are left out.
    /// A snapshot that can't be read (a broken global config) keeps the last counts.
    pub fn look(&mut self, me: &Path) {
        let Some(state) = me.parent().and_then(Path::parent) else { return };
        let Ok(fleet) = self.reader.read(state) else { return };
        self.counts = fleet
            .repos
            .iter()
            .filter(|r| r.repo.dir.file_name() != me.file_name())
            .filter(|r| r.counts.needs + r.counts.ready > 0)
            .map(|r| RepoCount { name: r.repo.name(), needs: r.counts.needs, ready: r.counts.ready })
            .collect();
    }
}

/// What you act on among these tasks: how many need you (needs or stopped) and
/// how many wait on your accept or close (ready or finished).
pub fn acts(tasks: &[&Task]) -> (usize, usize) {
    let n = |pick: fn(&Task) -> bool| tasks.iter().filter(|t| pick(t)).count();
    (n(Task::needs_you), n(Task::awaits_accept))
}

/// This repository's part of the top line: what needs you and what's ready, each
/// with the status whose colour it takes, in words ("2 need you", "3 ready") or
/// marks ("●2", "✔3"). Words go between " · ", marks between spaces.
pub fn act_parts(needs: usize, ready: usize, words: bool) -> Vec<(String, &'static str)> {
    [(needs, "needs"), (ready, "ready")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, st)| (if words { count_phrase(n, st) } else { format!("{}{n}", mark(st)) }, st))
        .collect()
}

pub fn act_sep(words: bool) -> &'static str {
    if words { " · " } else { " " }
}

fn act_width(needs: usize, ready: usize, words: bool) -> usize {
    let parts = act_parts(needs, ready, words);
    parts.iter().map(|(s, _)| width(s)).sum::<usize>() + width(act_sep(words)) * parts.len().saturating_sub(1)
}

/// The other repositories on the top line: "shop: 1 needs you, 2 ready · web: 1 ready".
fn others_words(counts: &[RepoCount]) -> String {
    let one = |c: &RepoCount| {
        let parts: Vec<String> = [(c.needs, "needs"), (c.ready, "ready")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, st)| count_phrase(n, st))
            .collect();
        format!("{}: {}", c.name, parts.join(", "))
    };
    counts.iter().map(one).collect::<Vec<_>>().join(" · ")
}

/// The same in marks: "shop ●1 ✔2 · web ✔1".
fn others_marks(counts: &[RepoCount]) -> String {
    let one = |c: &RepoCount| {
        let parts: Vec<String> = act_parts(c.needs, c.ready, false).into_iter().map(|(s, _)| s).collect();
        format!("{} {}", c.name, parts.join(" "))
    };
    counts.iter().map(one).collect::<Vec<_>>().join(" · ")
}

/// The right of the top line: the other repositories' text (put after " · "),
/// and this repository's counts at the right edge, two columns after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Top {
    pub others: String,
    /// each part with the status whose colour it takes, between `sep`
    pub counts: Vec<(String, &'static str)>,
    pub sep: &'static str,
    /// nothing was cut or left out
    pub whole: bool,
}

/// How the top line's `room` columns after the repository's name (and its
/// coordinator and hidden tasks, which come first) are shared. The other
/// repositories get the room first: their words, else their marks, else their
/// marks cut to no less than 8 columns. This repository's counts take words when
/// they fit beside them, else marks, else only what needs you ("●2"). Without room
/// for both, the other repositories' marks stay whole and this one's counts go:
/// they are on the group headings below.
pub fn top_right(needs: usize, ready: usize, others: &[RepoCount], room: usize) -> Top {
    let forms = [(needs, ready, true), (needs, ready, false), (needs, 0, false)];
    let gap = |w: usize| if w == 0 { 0 } else { w + 2 };
    let mine = |taken: usize, all: bool| {
        let forms = if all { &forms[..2] } else { &forms[..] };
        forms.iter().find(|&&(n, r, words)| taken + gap(act_width(n, r, words)) <= room).map(|&(n, r, words)| Top {
            others: String::new(),
            counts: act_parts(n, r, words),
            sep: act_sep(words),
            whole: r == ready,
        })
    };
    let with = |others: String, top: Option<Top>, cut: bool| {
        top.map(|t| Top { whole: t.whole && !cut, others, ..t })
    };
    let mut tries: Vec<Option<Top>> = Vec::new();
    if !others.is_empty() {
        for o in [others_words(others), others_marks(others)] {
            tries.push(with(o.clone(), mine(3 + width(&o), true), false));
        }
        let left = room.saturating_sub(3 + gap(act_width(needs, ready, false)));
        if left >= 8 {
            let cut = fit(&others_marks(others), left);
            // "shop ●1…", not "shop ●1 …"
            let cut = match cut.strip_suffix('…') {
                Some(head) if head.ends_with(' ') => format!("{}…", head.trim_end()),
                _ => cut,
            };
            tries.push(with(cut.clone(), mine(3 + width(&cut), true), true));
        }
        let o = others_marks(others);
        if 3 + width(&o) <= room {
            tries.push(Some(Top { others: o, counts: Vec::new(), sep: "", whole: needs + ready == 0 }));
        }
    }
    tries.push(with(String::new(), mine(0, false), !others.is_empty()));
    let none = Top { others: String::new(), counts: Vec::new(), sep: "", whole: false };
    tries.into_iter().flatten().next().unwrap_or(none)
}

#[cfg(test)]
#[path = "test_top.rs"]
mod tests;
