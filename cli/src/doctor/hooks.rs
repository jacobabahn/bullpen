//! doctor's checks of the setup and teardown hooks a new task would get.

use super::builds::executable;
use super::{Check, DoctorCheck};
use crate::modes::repo_base_ref;
use bullpen_core::json::{py_str, truthy};
use bullpen_core::proc::{git, run, run_in, run_input, which};
use bullpen_core::text::{split, splitlines, strip};
use bullpen_core::{Repo, Res};
use std::path::Path;

pub(super) const CHECKS: &[DoctorCheck] = &[SETUP, TEARDOWN];

const SETUP: DoctorCheck = DoctorCheck {
    name: "setup",
    run: |c| check_hook(c.repo, "setup", c.why_not),
    ..DoctorCheck::NEW
};

const TEARDOWN: DoctorCheck = DoctorCheck {
    name: "teardown",
    run: |c| check_hook(c.repo, "teardown", c.why_not),
    ..DoctorCheck::NEW
};

/// The program a #! line runs: its first word, or for env, the first word after
/// env's options (-S splits its string) and NAME=value settings.
pub fn shebang_program(argv: &[&str]) -> Option<String> {
    let first = *argv.first()?;
    if Path::new(first).file_name().is_none_or(|n| n != "env") {
        return Some(first.into());
    }
    let mut skip = false;
    for w in &argv[1..] {
        if skip {
            skip = false;
        } else if ["-u", "-C", "--unset", "--chdir"].contains(w) {
            skip = true; // their argument follows
        } else if w.starts_with("-S") && w.chars().count() > 2 {
            return Some(w[2..].into()); // -Sbash
        } else if !w.starts_with('-') && !w.contains('=') {
            return Some((*w).into());
        }
    }
    None
}

/// shlex.split (POSIX, no comments): the words, or None where it raises ValueError
/// (an unclosed quote, or a backslash at the end).
pub fn shlex_split(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut it = text.chars();
    while let Some(c) = it.next() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\\' => {
                cur.push(it.next()?);
                in_word = true;
            }
            '\'' => {
                in_word = true;
                loop {
                    match it.next()? {
                        '\'' => break,
                        q => cur.push(q),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match it.next()? {
                        '"' => break,
                        '\\' => {
                            let n = it.next()?;
                            if n != '"' && n != '\\' {
                                cur.push('\\');
                            }
                            cur.push(n);
                        }
                        q => cur.push(q),
                    }
                }
            }
            _ => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Some(words)
}

/// Is word something sh can run here: a program on PATH, a path, or a builtin?
fn shell_word_found(word: &str, cwd: &Path) -> Res<bool> {
    Ok(run_in(&["sh", "-c", "command -v \"$1\" >/dev/null", "sh", word], cwd, false)?.ok())
}

/// The setup (or teardown) a new task would get: config.json's command, else
/// .bullpen/<name>.sh as committed on the base. It must parse and find what it runs.
pub fn check_hook(repo: Option<&Repo>, name: &str, why_not: Option<&str>) -> Check {
    let Some(repo) = repo else {
        return Ok((
            "skip",
            format!("can't tell: {}", why_not.unwrap_or("not in a git repository")),
        ));
    };
    let path = format!(".bullpen/{name}.sh");
    if truthy(repo.cfg.get(name)) {
        let cmd = py_str(&repo.cfg[name]);
        let source = format!("config.json's {name}");
        if !run(&["sh", "-n", "-c", cmd.as_str()], false)?.ok() {
            return Ok((
                "fail",
                format!("{source} isn't valid sh: fix it with `bullpen config --{name} \"<cmd>\"`"),
            ));
        }
        let word = shlex_split(&cmd).and_then(|w| w.into_iter().next()).unwrap_or_default();
        if !word.is_empty() && !word.contains('=') && !shell_word_found(&word, &repo.root)? {
            return Ok((
                "fail",
                format!("{source} runs {word}, which isn't on PATH: install it, or fix the command"),
            ));
        }
        return Ok(("ok", format!("{source}: {cmd}")));
    }
    let r = repo_base_ref(repo)?;
    let p = git(&repo.root, &["show", &format!("{r}:{path}")], false)?;
    if !p.ok() || strip(&p.stdout).is_empty() {
        if repo.root.join(&path).exists() {
            return Ok((
                "fail",
                format!("{path} isn't committed on {r}, so it won't run: commit it (and push it)"),
            ));
        }
        return Ok(("skip", format!("none ({path} or `bullpen config --{name}`)")));
    }
    let text = p.stdout;
    let first = splitlines(&text).first().copied().unwrap_or("");
    let bang = first.starts_with("#!");
    let shell = if bang {
        let argv = split(&first[2..]);
        if argv.is_empty() {
            return Ok((
                "fail",
                format!("{path} on {r} has an empty #! line: name its interpreter, or remove the line"),
            ));
        }
        let Some(prog) = shebang_program(&argv) else {
            return Ok((
                "fail",
                format!("{path} on {r} has a #! line that names no program: fix it"),
            ));
        };
        let here = if prog.contains('/') {
            executable(Path::new(&prog))
        } else {
            which(&prog).is_some()
        };
        if !here {
            return Ok((
                "fail",
                format!("{path} on {r} runs under {prog}, which isn't here: install it, or fix its #! line"),
            ));
        }
        Path::new(&prog)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        "sh".to_string()
    };
    if ["sh", "bash", "dash", "zsh"].contains(&shell.as_str())
        && !run_input(&[shell.as_str(), "-n"], &text, false)?.ok()
    {
        return Ok((
            "fail",
            format!("{path} on {r} isn't valid {shell}: `{shell} -n {path}` says where"),
        ));
    }
    Ok((
        "ok",
        format!("{path} on {r}{}", if bang { "" } else { ", run under sh" }),
    ))
}
