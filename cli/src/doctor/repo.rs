//! doctor's checks of this repository's coordinators.

use super::{Check, DoctorCheck};
use crate::board::old_board;
use crate::coordinators::{alive, coordinators};
use bullpen_core::json::{obj, py_str, truthy, Obj};
use bullpen_core::Repo;

pub(super) const CHECKS: &[DoctorCheck] = &[COORDINATORS, COPY, RISK];

/// The copy of bullpen this repo's coordinator uses (copies.rs).
const COPY: DoctorCheck = DoctorCheck {
    name: "copy",
    run: |c| crate::copies::check_copy(c.repo, c.why_not),
    ..DoctorCheck::NEW
};

/// The repository's sensitive paths (risk_paths.rs).
const RISK: DoctorCheck = DoctorCheck {
    name: "risk",
    slow: true,
    run: |c| crate::risk_paths::check_risk(c.repo, c.why_not),
    ..DoctorCheck::NEW
};

/// f"{d.get(key)}": the value as Python prints it, None when it isn't there.
fn shown(o: &Obj, key: &str) -> String {
    o.get(key).map_or_else(|| "None".into(), py_str)
}

const COORDINATORS: DoctorCheck = DoctorCheck {
    name: "coordinators",
    slow: true,
    run: |c| check_coordinators(c.repo, c.why_not),
    ..DoctorCheck::NEW
};

/// The coordinators config.json lists still have their panes (and boards).
pub fn check_coordinators(repo: Option<&Repo>, why_not: Option<&str>) -> Check {
    if let (None, Some(why)) = (repo, why_not) {
        return Ok(("skip", format!("can't tell: {why}")));
    }
    let Some(repo) = repo.filter(|r| truthy(r.cfg.get("coordinators"))) else {
        return Ok(("skip", "none yet: run `bullpen up`".into()));
    };
    let (mut gone, mut old, mut ok) = (Vec::new(), Vec::new(), Vec::new());
    for c in coordinators(repo) {
        let id = shown(c, "id");
        if !alive(Some(c))? {
            gone.push(format!("{id}'s pane ({}) is gone", shown(c, "block")));
            continue;
        }
        let b = obj(c, "board");
        let board_block = b.get("block").filter(|v| truthy(Some(v))).map(py_str);
        if let Some(bb) = &board_block {
            let session = b
                .get("session")
                .filter(|v| !v.is_null())
                .map(py_str)
                .unwrap_or_default();
            if !crate::rex::alive(&session, bb)? {
                gone.push(format!("{id}'s board ({bb}) is gone"));
                continue;
            }
            if old_board(b)?.is_some_and(|o| !o.is_empty()) {
                old.push(format!("{id}'s board is from an older build"));
                continue;
            }
        }
        ok.push(format!("{id} ({})", shown(c, "block")));
    }
    if gone.is_empty() && old.is_empty() {
        return Ok(("ok", ok.join(", ")));
    }
    let mut parts = Vec::new();
    if !gone.is_empty() {
        parts.push(format!(
            "{}: run `bullpen up`, which drops gone coordinators and reopens boards",
            gone.join("; ")
        ));
    }
    if !old.is_empty() {
        let them = if old.len() == 1 { "it" } else { "them" };
        parts.push(format!("{}: `bullpen up` restarts {them}", old.join(", ")));
    }
    Ok(("fail", parts.join("; ")))
}
