//! The settings the board reads: the repo's config.json over the global one.

use std::path::{Path, PathBuf};
use super::*;

/// The settings the board reads from the repo's config.json.
#[derive(Debug, Clone)]
pub struct Config {
    pub layout: Layout,
    pub review_rounds: u32,
    pub merge: String,
    /// approved tasks accept themselves (`bullpen config --auto-accept on`)
    pub auto_accept: bool,
    /// the IDs of the repo's coordinators
    pub coordinators: Vec<String>,
    /// the coordinators whose boards show the other coordinators' tasks too
    /// (their entry's `board_others`, set by the board's `o`)
    pub board_others: Vec<String>,
    /// the worker model for all tasks (`bullpen config --worker-model`)
    pub worker_model: Option<String>,
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
    /// Is the task another coordinator's? Only when there are several: a task with
    /// no owner, or one whose owner is gone, is everyone's. bullpen drops a coordinator
    /// once its pane closes, so a listed one is running: this agrees with bullpen's
    /// `sees_to` without asking Rex about panes.
    pub fn theirs(&self, t: &Task, me: Option<&str>) -> bool {
        match (me, t.coordinator.as_deref()) {
            (Some(me), Some(o)) => self.coordinators.len() > 1 && o != me && self.coordinators.iter().any(|c| c == o),
            _ => false,
        }
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

/// A JSON file's object, or an empty one when it is missing or doesn't parse.
pub(super) fn read_object(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// The config.json with every repository's defaults: $XDG_CONFIG_HOME/bullpen/config.json,
/// else ~/.config/bullpen/config.json.
pub fn global_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bullpen").join("config.json"))
}

/// Settings only a repository can have: the global config.json's are ignored.
/// A copy of core/src/settings.rs's repo-only settings: a test in tests/test_config.py holds it to its table.
const REPO_ONLY: [&str; 7] = ["prefix", "next", "base", "push_remote", "news", "coordinators", "next_coordinator"];

/// The repo's settings over the global ones, as bullpen layers them.
pub fn layered(
    global: serde_json::Map<String, serde_json::Value>,
    repo: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    let mut out: serde_json::Map<String, serde_json::Value> =
        global.into_iter().filter(|(k, _)| !REPO_ONLY.contains(&k.as_str())).collect();
    out.extend(repo);
    serde_json::Value::Object(out)
}

impl Config {
    pub fn load(dir: &Path) -> Config {
        let global = global_config_path().map(|p| read_object(&p)).unwrap_or_default();
        Config::from_value(&layered(global, read_object(&dir.join("config.json"))))
    }

    /// The defaults here are copies of core/src/settings.rs's: a test in tests/test_config.py holds them to its table.
    pub fn from_value(v: &serde_json::Value) -> Config {
        Config {
            layout: if v["board_layout"].as_str().unwrap_or("grouped") == "compact" {
                Layout::Compact
            } else {
                Layout::Grouped
            },
            review_rounds: v["review_rounds"].as_u64().unwrap_or(2) as u32,
            merge: v["merge"].as_str().unwrap_or("rebase").to_string(),
            auto_accept: v["auto_accept"].as_bool().unwrap_or(false),
            coordinators: v["coordinators"]
                .as_array()
                .map(|cs| cs.iter().filter_map(|c| c["id"].as_str().map(String::from)).collect())
                .unwrap_or_default(),
            board_others: v["coordinators"]
                .as_array()
                .map(|cs| {
                    cs.iter()
                        .filter(|c| c["board_others"].as_bool() == Some(true))
                        .filter_map(|c| c["id"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            worker_model: v["worker_model"].as_str().map(String::from),
            bottom_gap: v["board_bottom_gap"].as_u64().map_or_else(|| default_gap(v), |n| n as usize),
            other_repos: v["board_repos"].as_bool().unwrap_or(true),
            review: v["review"].as_bool().unwrap_or(true),
            triage_minutes: v["triage_minutes"].as_u64().unwrap_or(10),
        }
    }
}

/// The bottom gap when the config sets none: a row while workers or reviewers float as
/// capsules over the board's tab, else none. The defaults (workers float, reviewers
/// in the background) are core/src/settings.rs's.
fn default_gap(v: &serde_json::Value) -> usize {
    let workers = v["workers"].as_str().unwrap_or("float");
    let reviewers = v["reviewers"].as_str().unwrap_or("background");
    usize::from(workers == "float" || reviewers == "float")
}

#[cfg(test)]
#[path = "test_config.rs"]
mod tests;
