//! The tests' fake gh. It logs each call (and what it got on stdin, which must be
//! nothing) to gh.log in $FAKE_DIR and answers from the files the tests put there:
//!
//!   gh_signed_out    `auth status` says it isn't signed in
//!   pr.json          what `pr view` says GitHub has on the PR (else an open PR)
//!   meanwhile        a command (a JSON list) `pr view` runs before it answers: a slow gh
//!   merge_refused    `pr merge` fails, saying this file's text

use std::io::Read;
use std::process::Command;

use bullpen_fakes::{append, broke, dumps, exit_msg, fake_dir, finish, print, read_json, read_text, Res};
use serde_json::{json, Value};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    finish(run(&argv))
}

fn run(argv: &[String]) -> Res<()> {
    let d = fake_dir()?;
    let mut body = String::new(); // like a gh that waits for input: it must get none
    std::io::stdin()
        .read_to_string(&mut body)
        .map_err(|e| broke(&format!("stdin: {e}")))?;
    append(&d.join("gh.log"), &dumps(&json!({"argv": argv, "body": body})))?;
    let two = |x: &str, y: &str| argv.len() >= 2 && argv[0] == x && argv[1] == y;
    if two("auth", "status") && d.join("gh_signed_out").exists() {
        return Err(exit_msg("You are not logged into any GitHub hosts."));
    }
    if two("pr", "create") {
        print("https://github.com/me/repo/pull/7");
    } else if two("pr", "view") {
        let meanwhile = d.join("meanwhile");
        if meanwhile.exists() {
            run_meanwhile(&read_json(&meanwhile)?)?;
        }
        let pr = d.join("pr.json");
        print(&if pr.exists() {
            read_text(&pr)?
        } else {
            dumps(&json!({"state": "OPEN"}))
        });
    } else if two("pr", "merge") {
        let refused = d.join("merge_refused");
        if refused.exists() {
            return Err(exit_msg(&read_text(&refused)?));
        }
    }
    Ok(())
}

/// Run the command the tests say happens while gh answers; it must work.
fn run_meanwhile(cmd: &Value) -> Res<()> {
    let words: Vec<&str> = cmd.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let (prog, args) = words.split_first().ok_or_else(|| broke("meanwhile: no command"))?;
    let status = Command::new(prog)
        .args(args)
        .status()
        .map_err(|e| broke(&format!("meanwhile: {e}")))?;
    if !status.success() {
        return Err(broke(&format!("meanwhile: {prog} failed")));
    }
    Ok(())
}
