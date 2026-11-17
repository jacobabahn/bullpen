//! The settings the board reads, as bullpen-core layers them: the repo's config.json
//! over the global one, over the defaults (core's `load_config`), with the board's typed
//! view (`Config`). A setting of the wrong shape counts as its default.

use bullpen_core::config::{load_config, read_own};
use bullpen_core::coordinators::sees_to;
use bullpen_core::json::{self, Obj};
use bullpen_core::settings::defaults;
use serde_json::Value;
use std::path::Path;
use super::*;

/// The repo's settings, and the board's typed view of the ones it reads itself.
#[derive(Debug, Clone)]
pub struct Config {
    /// every setting, for bullpen-core's sentences
    obj: Obj,
    pub layout: Layout,
    pub merge: String,
    /// approved tasks accept themselves (`bullpen config --auto-accept on`)
    pub auto_accept: bool,
    /// the IDs of the repo's coordinators
    pub coordinators: Vec<String>,
    /// the coordinators whose boards show the other coordinators' tasks too
    /// (their entry's `board_others`, set by the board's `o`)
    pub board_others: Vec<String>,
    /// rows the board leaves empty at its bottom, for Rex's minimized-pane capsules
    /// (`bullpen config --board-bottom-gap`; 0: none; unset: default_gap)
    pub bottom_gap: usize,
    /// the top line counts what needs you or is ready in other repositories
    /// (`bullpen config --board-repos`)
    pub other_repos: bool,
    /// a reviewer checks each task before it's ready (`bullpen config --review`)
    pub review: bool,
    /// minutes a task waits on its coordinator before it needs you
    /// (`bullpen config --triage-minutes`)
    pub triage_minutes: u64,
}

impl Config {
    /// Is the task another coordinator's: its owner, not me, sees to it (core's
    /// `sees_to`)? Only when there are several: a task with no owner, or one whose owner
    /// is gone, is everyone's. bullpen drops a coordinator once its pane closes, so a
    /// listed one is taken as running, without asking Rex about panes.
    pub fn theirs(&self, t: &Task, me: Option<&str>) -> bool {
        let (Some(me), Some(owner)) = (me, t.coordinator.as_deref()) else { return false };
        if self.coordinators.len() < 2 || owner == me {
            return false;
        }
        let cs: Vec<Obj> = self.coordinators.iter().map(|id| Obj::from_iter([("id".into(), Value::from(id.as_str()))])).collect();
        let sees = sees_to(cs.iter().collect(), t.obj(), |_| Ok(true)).unwrap_or_default();
        matches!(sees.as_slice(), [c] if json::s(c, "id") == owner)
    }

    /// Does the board beside coordinator me show the other coordinators' tasks?
    pub fn shows_others(&self, me: Option<&str>) -> bool {
        me.is_some_and(|me| self.board_others.iter().any(|c| c == me))
    }

    /// Is the task left off this board? Another coordinator's, while the board shows
    /// only its own.
    pub fn hides(&self, t: &Task, me: Option<&str>) -> bool {
        !self.shows_others(me) && self.theirs(t, me)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Grouped,
    Compact,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Grouped => "grouped",
            Layout::Compact => "compact",
        }
    }

    pub fn other(self) -> Layout {
        match self {
            Layout::Grouped => Layout::Compact,
            Layout::Compact => Layout::Grouped,
        }
    }
}

impl Config {
    /// The repo's settings. A config.json that doesn't parse counts as empty: the board
    /// shows what it can (bullpen's commands say what's wrong).
    pub fn load(dir: &Path) -> Config {
        Config::from_obj(load_config(dir).unwrap_or_else(|_| {
            let mut cfg = defaults();
            cfg.extend(read_own(dir).unwrap_or_default());
            cfg
        }))
    }

    /// The board's view of these settings. A value of the wrong shape counts as the
    /// default (bullpen-core's settings table has them).
    pub fn from_obj(obj: Obj) -> Config {
        let d = defaults();
        let get = |k: &str| [obj.get(k), d.get(k)];
        let flag = |k: &str| get(k).into_iter().flatten().find_map(Value::as_bool).unwrap_or(false);
        let num = |k: &str| get(k).into_iter().flatten().find_map(Value::as_u64).unwrap_or(0);
        let text = |k: &str| get(k).into_iter().flatten().find_map(Value::as_str).unwrap_or("").to_string();
        let coordinators = obj.get("coordinators").and_then(Value::as_array).cloned().unwrap_or_default();
        let ids = |only_others: bool| {
            coordinators
                .iter()
                .filter(|c| !only_others || c["board_others"].as_bool() == Some(true))
                .filter_map(|c| c["id"].as_str().map(String::from))
                .collect()
        };
        Config {
            layout: if text("board_layout") == "compact" { Layout::Compact } else { Layout::Grouped },
            merge: text("merge"),
            auto_accept: flag("auto_accept"),
            coordinators: ids(false),
            board_others: ids(true),
            bottom_gap: obj.get("board_bottom_gap").and_then(Value::as_u64).map_or_else(|| default_gap(&text), |n| n as usize),
            other_repos: flag("board_repos"),
            review: flag("review"),
            triage_minutes: num("triage_minutes"),
            obj,
        }
    }

    /// Every setting, as bullpen-core reads them.
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// These settings with these set too (for tests).
    #[cfg(test)]
    pub fn with(&self, more: Value) -> Config {
        let mut obj = self.obj.clone();
        obj.extend(more.as_object().cloned().unwrap_or_default());
        Config::from_obj(obj)
    }
}

/// The bottom gap when the config sets none: a row while workers or reviewers float as
/// capsules over the board's tab, else none.
fn default_gap(text: &dyn Fn(&str) -> String) -> usize {
    usize::from(text("workers") == "float" || text("reviewers") == "float")
}

#[cfg(test)]
#[path = "test_config.rs"]
mod tests;
