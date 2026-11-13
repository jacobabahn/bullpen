//! The rules every worker and reviewer gets (bullpen/rules.py): the repo's
//! .bullpen/rules.md as committed, then the person's own, and the `rules` command.
//! Python keeps rules.py's helpers (a twin) for review.py's `task_rules`.

use crate::args::{flag, Args};
use bullpen_core::proc::git;
use bullpen_core::repo::repo_root;
use bullpen_core::text::strip;
use bullpen_core::{Repo, Res};
use std::path::PathBuf;

pub const RULES_FILE: &str = ".bullpen/rules.md";

/// The person's own rules for the repo.
pub fn personal_rules(repo: &Repo) -> PathBuf {
    repo.dir.join("rules.md")
}

/// [(where it came from, text)]: the repo's rules file first, then the personal one.
pub fn rule_sources(repo: &Repo, committed: Option<&str>, at: &str) -> Res<Vec<(String, String)>> {
    let mut out = Vec::new();
    if let Some(text) = committed.filter(|c| !strip(c).is_empty()) {
        out.push((
            strip(&format!("{RULES_FILE} {at}")).to_string(),
            strip(text).to_string(),
        ));
    }
    let p = personal_rules(repo);
    if p.exists() {
        let text = std::fs::read_to_string(&p)?;
        if !strip(&text).is_empty() {
            out.push((p.display().to_string(), strip(&text).to_string()));
        }
    }
    Ok(out)
}

/// The repo's rules file as committed on git_ref, or None.
pub fn committed_rules_on(repo: &Repo, git_ref: &str) -> Res<Option<String>> {
    let p = git(&repo.root, &["show", &format!("{git_ref}:{RULES_FILE}")], false)?;
    Ok(p.ok().then_some(p.stdout))
}

pub fn rules_text(sources: &[(String, String)]) -> String {
    sources
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

const RULES: &[crate::args::Arg] = &[flag("--edit", "edit your own rules for this repo in $EDITOR")];

/// `rules`: print the rules every worker and reviewer gets, and where they're from; or
/// edit the person's own with --edit.
pub fn cmd_rules(argv: &[String]) -> Res<i32> {
    let a = Args::parse("rules", argv, RULES);
    let repo = Repo::new(repo_root()?)?;
    if a.flag("--edit") {
        let p = personal_rules(&repo);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let editor = ["VISUAL", "EDITOR"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_else(|| "vi".into());
        let words = bullpen_core::text::split(&editor);
        let Some((prog, rest)) = words.split_first() else {
            return Ok(0);
        };
        // subprocess.call: what the editor says is its own; a failed editor isn't ours
        if let Err(e) = std::process::Command::new(prog).args(rest).arg(&p).status() {
            return Err(e.into());
        }
        return Ok(0);
    }
    let git_ref = crate::modes::repo_base_ref(&repo)?;
    let committed = committed_rules_on(&repo, &git_ref)?;
    let sources = rule_sources(&repo, committed.as_deref(), &format!("on {git_ref}"))?;
    if sources.is_empty() {
        println!(
            "No rules. Commit them as {RULES_FILE}, or keep your own in {} (`bullpen rules --edit`).",
            personal_rules(&repo).display()
        );
        return Ok(0);
    }
    let parts: Vec<String> = sources
        .iter()
        .map(|(at, text)| format!("# From {at}\n\n{text}"))
        .collect();
    println!("{}", parts.join("\n\n"));
    Ok(0)
}
