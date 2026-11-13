//! The board's program, and a coordinator's board pane beside it:
//! `board`, and `ensure_board`, which `go` uses to start a board that was quit.

use crate::args::{value, Arg, Args};
use crate::coordinators::coordinator_mut;
use crate::rex::{board_label, first_block, pane_process, rex_close, with_state};
use bullpen_core::json::{py_str, s, Obj};
use bullpen_core::paths::rex;
use bullpen_core::paths::{home, self_bin};
use bullpen_core::proc::run;
use bullpen_core::repo::{index_block, repo_root, unindex_block};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

const BOARD: &[Arg] = &[value(
    "--coordinator",
    "COORDINATOR",
    "the coordinator it sits beside: its tasks are marked",
)];

/// Coordinator cid's task board, in a narrow pane to its right. A board running an older
/// build than `bullpen board` runs now (old_board) is replaced: it never reloads. Says
/// what it did on stderr (or into said, for `up`).
pub fn ensure_board(
    repo: &mut Repo,
    session: &str,
    cid: &str,
    said: Option<&mut Vec<(String, String, String)>>,
) -> Res<()> {
    let Some(c) = crate::coordinators::coordinator(repo, cid).cloned() else {
        return fail(format!("no coordinator {cid}"));
    };
    let b = c
        .get("board")
        .and_then(Value::as_object)
        .filter(|b| !b.is_empty())
        .cloned();
    let mut old = None;
    if let Some(b) = &b {
        if crate::rex::alive(s(b, "session"), s(b, "block"))? {
            if old_board(b)?.is_none() {
                return Ok(());
            }
            old = Some(b.clone());
        }
    }
    let mut argv: Vec<String> = ["split", "--json", "--focus=false", "-s", session, "-b"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    argv.insert(0, rex());
    argv.extend([py_str(c.get("block").unwrap_or(&Value::Null)), "--split=right".into()]);
    argv.extend(["--ratio".into(), "35".into(), "--label".into(), board_label(repo)]);
    argv.extend(["-c".into(), repo.root_str(), "--".into()]);
    let me = vec![
        self_bin().display().to_string(),
        "board".into(),
        "--coordinator".into(),
        cid.into(),
    ];
    argv.extend(with_state(&me));
    let block = first_block(&run(&argv, true)?.stdout)?;
    if let Some(b) = &b {
        unindex_block(s(b, "block"))?;
    }
    if let Some(c) = coordinator_mut(repo, cid) {
        c.insert("board".into(), json!({"block": block, "session": session}));
    }
    index_block(&block, repo, "board")?;
    repo.save()?;
    if let Some(old) = &old {
        rex_close(s(old, "session"), s(old, "block"))?; // only once the new one is up
    }
    let what = if old.is_some() {
        "restarted (it ran an older build)"
    } else {
        "started"
    };
    match said {
        Some(said) => said.push((what.into(), "board".into(), block)),
        None => eprintln!("board {what}: {block}"),
    }
    Ok(())
}

/// The program a board's pane runs, as it was started (the path a board reloads from),
/// or None when Rex can't say or it isn't the board yet.
fn board_program(b: &Obj) -> Res<Option<String>> {
    let Some(p) = pane_process(s(b, "session"), s(b, "block"))? else {
        return Ok(None);
    };
    let child = p.get("child").and_then(Value::as_object).cloned().unwrap_or_default();
    let text = |k: &str| child.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut prog = Some(text("invoked_path"))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| text("argv0"));
    if !prog.starts_with('/') {
        prog = text("path");
    }
    let name = Path::new(&prog).file_name().map(|n| n.to_string_lossy().into_owned());
    let exe = board_exe();
    let exe_name = exe.file_name().map(|n| n.to_string_lossy().into_owned());
    if prog.is_empty() || !(name.as_deref() == Some("bullpen-board") || name == exe_name) {
        return Ok(None); // still `bullpen board`, about to exec it, or Rex doesn't say
    }
    Ok(Some(prog))
}

/// A running board's program, when it isn't the one `bullpen board` runs now: an older
/// install's path, which a rebuild never replaces, so the board never reloads. (A board
/// on the right path whose file was rebuilt reloads by itself.) Else None.
pub fn old_board(b: &Obj) -> Res<Option<String>> {
    Ok(board_program(b)?.filter(|prog| realpath(Path::new(prog)) != realpath(&board_exe())))
}

/// os.path.realpath: the path with its links resolved, as far as it exists.
pub fn realpath(p: &Path) -> PathBuf {
    if p.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            return realpath(&cwd.join(p));
        }
    }
    if let Ok(r) = std::fs::canonicalize(p) {
        return r;
    }
    match (p.parent(), p.file_name()) {
        (Some(dir), Some(name)) if !dir.as_os_str().is_empty() => realpath(dir).join(name),
        _ => p.to_path_buf(),
    }
}

/// The board program `bullpen board` runs: BULLPEN_BOARD, else the copy bullpen builds
/// (target/bullpen-board, renamed into place once it's whole, so a board never runs a
/// half-written file), else cargo's own output.
pub fn board_exe() -> PathBuf {
    if let Some(b) = std::env::var_os("BULLPEN_BOARD").filter(|b| !b.is_empty()) {
        return PathBuf::from(b);
    }
    let board = home().join("target/bullpen-board");
    let cargo = home().join("target/release/bullpen-board");
    if board.exists() || !cargo.exists() {
        board
    } else {
        cargo
    }
}

/// `board`: the board is a Rust program (board/): it reads the repo's state directory
/// and does everything through this command. It execs itself again when its program is
/// replaced (a rebuild), keeping what it shows.
pub fn cmd_board(argv: &[String]) -> Res<i32> {
    let a = Args::parse("board", argv, BOARD);
    let repo = Repo::new(repo_root()?)?;
    let exe = board_exe();
    let runs = std::ffi::CString::new(exe.as_os_str().as_encoded_bytes())
        .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0);
    if !runs {
        return fail(format!(
            "the board isn't built: run {} (needs cargo)",
            home().join("install.sh").display()
        ));
    }
    let mut cmd = std::process::Command::new(&exe);
    cmd.args([
        "--dir".as_ref(),
        repo.dir.as_os_str(),
        "--root".as_ref(),
        repo.root.as_os_str(),
    ]);
    cmd.args(["--bullpen".as_ref(), self_bin().as_os_str()]);
    if let Some(c) = a.opt("--coordinator") {
        cmd.args(["--coordinator", c]);
    }
    let err = cmd.exec();
    fail(format!("couldn't run {}: {err}", exe.display()))
}
