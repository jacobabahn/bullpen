//! Talking to Rex (bullpen/rex.py): what the commands moved so far need of it.

use bullpen_core::paths;
use bullpen_core::proc::run;
use bullpen_core::Res;

/// rex_alive: is the pane there? (`rex block inspect`, in its session when given.)
pub fn alive(session: &str, block: &str) -> Res<bool> {
    if block.is_empty() {
        return Ok(false);
    }
    let rex = paths::rex();
    let mut argv = vec![rex.as_str(), "block", "inspect", block];
    if !session.is_empty() {
        argv.extend(["-s", session]);
    }
    Ok(run(&argv, false)?.ok())
}
