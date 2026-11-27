//! Each Claude Code agent's conversation: bullpen picks its session ID when it starts one
//! (`--session-id`) and keeps it as `chat` (on the task for its worker, on the task's
//! `review` for its reviewer, on a coordinator's entry), so its transcript is
//! `~/.claude/projects/*/<chat>.jsonl` and resuming takes that conversation
//! (`--resume <chat>`), never the newest in the folder. An agent that isn't Claude Code
//! gets neither flag. The Stop hooks move a `chat` that changed (stop_hook/whose.rs).

use bullpen_core::json::{s, Obj};
use bullpen_core::task::is_claude;
use std::io::Read;

/// A new random UUID (version 4), as Claude Code's `--session-id` takes it.
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_err()
    {
        // no /dev/urandom: the time and the process are unique enough here
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
        b[..8].copy_from_slice(&nanos.to_le_bytes());
        b[8..12].copy_from_slice(&std::process::id().to_le_bytes());
    }
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // the RFC 4122 variant
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// For an agent started with cmd: a new session ID and its flags, when it is Claude Code.
pub fn start(cmd: &[String]) -> Option<(String, Vec<String>)> {
    is_claude(cmd).then(|| {
        let id = new_id();
        (id.clone(), vec!["--session-id".into(), id])
    })
}

/// The flags that resume the task's worker (started with cmd) and the `chat` it then has:
/// `--resume` with its own recorded conversation. A worker with none recorded (started
/// before bullpen recorded it, and no turn of its own has ended since) continues the
/// newest conversation in its worktree, as does a worker that isn't Claude Code.
pub fn resume(cmd: &[String], t: &Obj) -> (Option<String>, Vec<String>) {
    match s(t, "chat") {
        id if is_claude(cmd) && !id.is_empty() => (Some(id.into()), vec!["--resume".into(), id.into()]),
        _ => (None, vec!["--continue".into()]),
    }
}

#[cfg(test)]
#[path = "tests/test_chat_ids.rs"]
mod tests;
