//! File names out of git's output, exactly: every list of a diff's files is read with
//! `-z` (NUL after each name, nothing quoted), so a name with a non-ASCII character, a
//! space, a quote, a backslash, a tab or a newline is the name on disk. Without it git
//! prints such a name quoted and escaped (`"prompts/caf\303\251.md"`), and a check that
//! matches paths (`prompts/`, `docs/`, risk_paths) misses it.
//!
//! Renames are read with `--no-renames`: a renamed file is its old path deleted and its
//! new one added, so each check sees both.
//!
//! A name shown to people goes through `shown`, which escapes what isn't printable.

use bullpen_core::proc::git;
use bullpen_core::text::is_printable;
use bullpen_core::Res;

/// A changed file of a diff: its changed lines (None: a binary file) and its path.
#[derive(Debug, Clone, PartialEq)]
pub struct Changed {
    pub lines: Option<i64>,
    pub path: String,
}

/// The files `git diff --numstat` changes in range (`base...HEAD`), renames as a delete
/// and an add.
pub fn numstat(wt: &str, range: &str) -> Res<Vec<Changed>> {
    let out = git(wt, &["diff", "-z", "--numstat", "--no-renames", range], true)?.stdout;
    Ok(numstat_of(&out))
}

/// numstat from `git diff -z --numstat --no-renames`'s output: "added\tdeleted\tpath"
/// rows, each ended by a NUL ("-\t-" for a binary file).
pub fn numstat_of(out: &str) -> Vec<Changed> {
    out.split('\0')
        .filter_map(|row| {
            let mut p = row.splitn(3, '\t');
            let (added, deleted, path) = (p.next()?, p.next()?, p.next()?);
            let lines = added
                .parse::<i64>()
                .ok()
                .zip(deleted.parse::<i64>().ok())
                .map(|(a, d)| a + d);
            Some(Changed {
                lines,
                path: path.to_string(),
            })
        })
        .collect()
}

/// The names a git command lists (`diff --name-only`, `ls-files`), read with `-z`
/// (added last); None when git fails.
pub fn names(dir: &str, args: &[&str]) -> Res<Option<Vec<String>>> {
    let mut args = args.to_vec();
    args.push("-z");
    let p = git(dir, &args, false)?;
    Ok(p.ok().then(|| names_of(&p.stdout)))
}

/// The names in `-z` output, NUL-separated.
pub fn names_of(out: &str) -> Vec<String> {
    out.split('\0').filter(|f| !f.is_empty()).map(String::from).collect()
}

/// A file name for a line people read: what isn't printable (a tab, a newline, another
/// control character) is escaped, `\t`, `\n`, `\u{1b}`; the rest is as it is.
pub fn shown(path: &str) -> String {
    let mut out = String::new();
    for c in path.chars() {
        match c {
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if is_printable(c) => out.push(c),
            c => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
        }
    }
    out
}

/// Names for a line people read, each `shown`, with commas.
pub fn listed(files: &[String]) -> String {
    files.iter().map(|f| shown(f)).collect::<Vec<_>>().join(", ")
}

/// A path as git writes it in a patch's header: as it is, or quoted C-style
/// (`"a/caf\303\251.md"`: the quotes gone, its escapes and octal bytes decoded). The
/// rest of the text after a quoted path is returned too (`" b/..."`).
pub fn unquote(text: &str) -> (String, &str) {
    let Some(rest) = text.strip_prefix('"') else {
        return (text.to_string(), "");
    };
    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return (String::from_utf8_lossy(&out).into_owned(), &rest[i + 1..]),
            b'\\' if i + 1 < bytes.len() => {
                let e = bytes[i + 1];
                let octal = |b: u8| (b'0'..=b'7').contains(&b);
                if octal(e) && i + 3 < bytes.len() && octal(bytes[i + 2]) && octal(bytes[i + 3]) {
                    let n = (e - b'0') as u32 * 64 + (bytes[i + 2] - b'0') as u32 * 8 + (bytes[i + 3] - b'0') as u32;
                    out.push(n as u8);
                    i += 4;
                    continue;
                }
                out.push(match e {
                    b'a' => 7,
                    b'b' => 8,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 11,
                    b'f' => 12,
                    b'r' => b'\r',
                    other => other, // \" and \\
                });
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), "")
}

#[cfg(test)]
#[path = "test_git_names.rs"]
mod tests;
