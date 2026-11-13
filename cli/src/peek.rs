//! `peek`: the end of a worker's screen (bullpen/start.py cmd_peek).

use crate::args::Args;
use crate::core::{fail, repo_root, Fail, Repo, Res};
use crate::json::s;
use crate::rex;

pub fn cmd_peek(a: &Args) -> Res<()> {
    // argparse's int(): a value Rust doesn't read plainly is Python's to take or refuse
    let n: i64 = match a.opt("--lines") {
        Some(v) => v.parse().map_err(|_| Fail::Python)?,
        None => 40,
    };
    let repo = Repo::new(&repo_root()?)?;
    let t = repo.task(a.arg(0).unwrap_or(""))?;
    if !rex::alive(s(&t, "session"), s(&t, "block"))? {
        return fail(format!("{}'s agent isn't running", s(&t, "id")));
    }
    println!("{}", rex::screen_end(s(&t, "session"), s(&t, "block"), n, a.flag("--vt"))?.join("\n"));
    Ok(())
}
