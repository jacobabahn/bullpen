//! What the ported commands ask Rex (bullpen/rex.py).

use crate::core::{rex_program, run, Res};

/// Is the pane there? (`rex block inspect`, in its session when given.)
pub fn alive(session: &str, block: &str) -> Res<bool> {
    if block.is_empty() {
        return Ok(false);
    }
    let rex = rex_program();
    let mut argv = vec![rex.as_str(), "block", "inspect", block];
    if !session.is_empty() {
        argv.extend(["-s", session]);
    }
    Ok(run(&argv, false)?.code == 0)
}
