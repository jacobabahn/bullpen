//! doctor's checks of bullpen's own builds: the command line, the board, and the cargo
//! that builds them.

use super::{Check, DoctorCheck};
use crate::build::{build_log, building, bullpen_cargo, programs, Program, BOARD, CLI};
use bullpen_core::paths::{home, self_bin};
use bullpen_core::proc::run_in;
use bullpen_core::text::{splitlines, strip};
use serde_json::Value;
use std::path::Path;

pub(super) const CHECKS: &[DoctorCheck] = &[BULLPEN, BOARD_CHECK, CARGO];

/// os.access(path, X_OK): a folder counts too.
pub(super) fn executable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: c is a valid NUL-terminated path for the length of the call
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

const BOARD_CHECK: DoctorCheck = DoctorCheck {
    name: "board",
    shared: true,
    run: |_| check_board(),
    ..DoctorCheck::NEW
};

/// Built from the board's source as it is now (see build::Program).
pub fn check_board() -> Check {
    if let Some(b) = std::env::var_os("BULLPEN_BOARD").filter(|b| !b.is_empty()) {
        let exe: std::path::PathBuf = Path::new(&b).components().collect();
        if executable(&exe) {
            return Ok(("ok", format!("{} (BULLPEN_BOARD)", exe.display())));
        }
        return Ok((
            "fail",
            format!("BULLPEN_BOARD is {}, which isn't an executable file", exe.display()),
        ));
    }
    check_build(&BOARD, "running boards reload when it's done")
}

const BULLPEN: DoctorCheck = DoctorCheck {
    name: "bullpen",
    shared: true,
    run: |_| check_cli(),
    ..DoctorCheck::NEW
};

/// bin/bullpen's Rust build, built from its source as it is now.
pub fn check_cli() -> Check {
    check_build(&CLI, "commands run it from their next call")
}

/// A program bullpen builds from its own checkout: built from its source as it is now, or being built.
fn check_build(prog: &Program, then: &str) -> Check {
    let build = format!("run `{} build`", self_bin().display());
    let src = prog
        .crates
        .iter()
        .map(|c| format!("{c}/"))
        .collect::<Vec<_>>()
        .join(", ");
    let log = build_log();
    let log = log.display();
    if building()? {
        return Ok(("ok", format!("being rebuilt now (log: {log}); {then}")));
    }
    if !executable(&prog.installed()) {
        return Ok(("fail", format!("not built: {build}")));
    }
    let st = prog.stamp()?;
    let hash = Value::String(prog.source_hash()?);
    if st.get("source") == Some(&hash) {
        return Ok((
            "ok",
            format!("{}, built from {src} as they are", prog.installed().display()),
        ));
    }
    if st.get("failed") == Some(&hash) {
        return Ok((
            "fail",
            format!("older than {src}: its build failed (log: {log}). Fix that, then {build}"),
        ));
    }
    if prog.dirty()? {
        return Ok((
            "fail",
            format!(
                "older than {src}, which have uncommitted changes, so bullpen won't build it by \
                 itself: commit them, or {build} to build them as they are"
            ),
        ));
    }
    Ok((
        "fail",
        format!("older than {src}: `bullpen up` rebuilds it in the background, or {build}"),
    ))
}

const CARGO: DoctorCheck = DoctorCheck {
    name: "cargo",
    shared: true,
    slow: true,
    run: |_| check_cargo(),
    ..DoctorCheck::NEW
};

/// The cargo bullpen's build would use (bin/bullpen-cargo), found the way a build
/// started from here would find it: a background rebuild inherits this environment.
pub fn check_cargo() -> Check {
    if programs(false).is_empty() {
        return Ok((
            "skip",
            format!("{} has no Cargo.toml: nothing to build", home().display()),
        ));
    }
    let lookup = bullpen_cargo().display().to_string();
    let p = run_in(&[lookup.as_str(), "--which"], &home(), false)?;
    if p.ok() {
        return Ok(("ok", strip(&p.stdout).to_string()));
    }
    let said = splitlines(&p.stderr)
        .into_iter()
        .rev()
        .find(|l| !strip(l).is_empty())
        .unwrap_or_default();
    let said = said.strip_prefix("bullpen-cargo: ").unwrap_or(said);
    Ok((
        "fail",
        format!("{said}: bullpen's build can't run (from here, or a rebuild started from here)"),
    ))
}
