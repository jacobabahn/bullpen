//! doctor's check of this repository's push remote (push_remote.rs): asked itself by
//! `doctor`, and from local refs, without the network, by `doctor --all-repos`.

use super::{DoctorCheck, Run};
use crate::push_remote::{check_push, check_push_seen};

pub(super) const CHECKS: &[DoctorCheck] = &[PUSH];

const PUSH: DoctorCheck = DoctorCheck {
    name: "push",
    slow: true,
    run: |c| match c.run {
        Run::EachRepo => check_push_seen(c.repo, c.why_not),
        _ => check_push(c.repo, c.why_not),
    },
    ..DoctorCheck::NEW
};
