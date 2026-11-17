//! The tests' fake gh. It logs each call (and what it got on stdin, which must be
//! nothing) to gh.log in $FAKE_DIR and answers from the files the tests put there.
//!
//! The files:
//!
//!   gh_signed_out    `auth status` says it isn't signed in
//!   gh_scopes        the token scopes `auth status` names (else gist, read:org and repo)
//!   pr.json          what `pr view` says GitHub has on the PR (else an open PR)
//!   meanwhile        a command (a JSON list) `pr view` runs before it answers: a slow gh
//!   merge_refused    `pr merge` fails, saying this file's text
//!   issues.json      the repository's issues (a JSON list): `issue list` prints the ones
//!                    not CLOSED (at most --limit), `issue view N` the one numbered N.
//!                    Signed out, both fail as gh does

use std::io::Read;
use std::path::Path;
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
    if two("auth", "status") {
        if d.join("gh_signed_out").exists() {
            return Err(exit_msg("You are not logged into any GitHub hosts."));
        }
        let scopes = d.join("gh_scopes");
        let scopes = if scopes.exists() {
            read_text(&scopes)?
        } else {
            "'gist', 'read:org', 'repo'".into()
        };
        print(&format!(
            "github.com\n  ✓ Logged in to github.com account me (keyring)\n  - Active account: true\n  - Token scopes: {}",
            scopes.trim()
        ));
        return Ok(());
    }
    if argv.first().is_some_and(|a| a == "issue") {
        return issue(&d, argv);
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

/// `issue list` and `issue view N`, from issues.json.
fn issue(d: &Path, argv: &[String]) -> Res<()> {
    if d.join("gh_signed_out").exists() {
        return Err(exit_msg("To get started with GitHub CLI, please run:  gh auth login"));
    }
    let file = d.join("issues.json");
    let all = if file.exists() { read_json(&file)? } else { json!([]) };
    let all = all.as_array().cloned().unwrap_or_default();
    let after = |flag: &str| argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1));
    match argv.get(1).map(String::as_str) {
        Some("list") => {
            let limit = after("--limit").and_then(|n| n.parse().ok()).unwrap_or(30);
            let open = all.into_iter().filter(|i| i["state"] != "CLOSED").take(limit);
            print(&dumps(&Value::Array(open.collect())));
        }
        Some("view") => {
            let n = argv.get(2).cloned().unwrap_or_default();
            match all.iter().find(|i| i["number"].to_string() == n) {
                Some(i) => print(&dumps(i)),
                None => {
                    return Err(exit_msg(&format!(
                        "GraphQL: Could not resolve to an Issue with the number of {n}. (repository.issue)"
                    )))
                }
            }
        }
        _ => {}
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
