//! Helpers the model's tests share.

use super::*;
pub(crate) use serde_json::{json, Value};

impl Config {
    /// The built-in defaults (rebase, 2 review rounds, workers float...).
    pub(crate) fn default_for_test() -> Config {
        Config::from_obj(bullpen_core::settings::defaults())
    }
}

pub(crate) fn task(id: &str, status: &str) -> Task {
    Task::from_obj(json!({"id": id, "status": status}).as_object().unwrap().clone()).unwrap()
}

pub(crate) fn merge(how: &str) -> Config {
    Config::default_for_test().with(json!({"merge": how}))
}

/// A review that asks for changes, in this round.
pub(crate) fn changes(round: u32) -> Value {
    json!({"round": round, "verdict": "changes"})
}
