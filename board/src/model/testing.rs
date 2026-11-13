//! Helpers the model's tests share.

use super::*;

impl Config {
    pub(crate) fn default_for_test() -> Config {
        Config {
            layout: Layout::Grouped,
            review_rounds: 2,
            merge: "rebase".into(),
            auto_accept: false,
            coordinators: Vec::new(),
            board_others: Vec::new(),
            worker_model: None,
            bottom_gap: 1,
            other_repos: true,
            review: true,
            triage_minutes: 10,
        }
    }
}

pub(crate) fn task(id: &str, status: &str) -> Task {
    Task { id: id.into(), status: status.into(), ..Default::default() }
}

pub(crate) fn merge(how: &str) -> Config {
    Config { merge: how.into(), ..Config::default_for_test() }
}

pub(crate) fn changes(round: u32) -> Option<Review> {
    Some(Review { round: Some(round), verdict: Some("changes".into()), kind: None, block: None })
}
