//! `web`: bullpen web, every coordinator and task as a page for a browser on the
//! tailnet (docs/web.md). The server is a program of its own (web/, built into
//! target/bullpen-web as the board is): this runs it in the foreground, on the state
//! folder, from any folder.

use crate::args::Args;
use bullpen_core::paths::{home, self_bin, state};
use bullpen_core::{fail, Res};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

/// The server `bullpen web` runs: BULLPEN_WEB, else the copy bullpen builds
/// (target/bullpen-web, renamed into place once it's whole), else cargo's own output.
pub fn web_exe() -> PathBuf {
    if let Some(w) = std::env::var_os("BULLPEN_WEB").filter(|w| !w.is_empty()) {
        return PathBuf::from(w);
    }
    let web = home().join("target/bullpen-web");
    let cargo = home().join("target/release/bullpen-web");
    if web.exists() || !cargo.exists() {
        web
    } else {
        cargo
    }
}

pub fn cmd_web(argv: &[String]) -> Res<i32> {
    Args::parse("web", argv, &[]);
    let exe = web_exe();
    let runs = std::ffi::CString::new(exe.as_os_str().as_encoded_bytes())
        .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0);
    if !runs {
        return fail(format!(
            "bullpen web isn't built: run `{} build` (needs cargo)",
            self_bin().display()
        ));
    }
    let err = std::process::Command::new(&exe)
        .arg("--state")
        .arg(state())
        .arg("--bullpen")
        .arg(self_bin())
        .exec();
    fail(format!("couldn't run {}: {err}", exe.display()))
}
