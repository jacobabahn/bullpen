//! The coordinator's guide, and telling running coordinators when it changed
//!.
//!
//! A coordinator gets its guide once, in its system prompt or by reading `bullpen guide`
//! (`up --here`), and can't load it again. Each records the hash of the guide it was
//! given (c["guide"]), and the text itself is kept in <state>/guides/<hash>.md: one
//! folder for every repo, since they all run this bullpen's guide. `up` and `accept`
//! compare each running coordinator's hash, in every repo, with the guide now. A
//! coordinator whose guide is out of date gets one news line, only it, with the diff
//! after the line when it is short, and its hash moves on, so each change is told once.
//! One from before hashes, or whose old text is gone, is told to read the guide again.

use crate::args::Args;
use crate::coordinators::alive;
use crate::news::{follow_cmd, news, Opts};
use bullpen_core::json::{s, Obj};
use bullpen_core::paths::{prompts, state};
use bullpen_core::repo::known_repos;
use bullpen_core::text::{splitlines, strip};
use bullpen_core::{Fail, Repo, Res};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A longer diff is summed up by its sections instead.
const GUIDE_DIFF_LINES: usize = 40;

/// Where the guides coordinators were given are kept, by hash.
pub fn guides() -> PathBuf {
    state().join("guides")
}

/// An OSError as Python words it: "[Errno 2] No such file or directory: '/x'".
fn os_error(e: std::io::Error, path: &Path) -> Fail {
    let said = e.to_string();
    match e.raw_os_error() {
        Some(n) => Fail(format!(
            "[Errno {n}] {}: '{}'",
            said.trim_end_matches(&format!(" (os error {n})")),
            path.display()
        )),
        None => Fail(said),
    }
}

/// A UnicodeDecodeError as Python words it.
fn decode_error(bytes: &[u8], e: std::str::Utf8Error) -> Fail {
    let at = e.valid_up_to();
    let byte = bytes[at];
    let (why, end) = match e.error_len() {
        None => ("unexpected end of data", bytes.len()),
        Some(n) if (0x80..0xc2).contains(&byte) || byte > 0xf4 => ("invalid start byte", at + n),
        Some(n) => ("invalid continuation byte", at + n),
    };
    let what = if end - at == 1 {
        format!("byte 0x{byte:02x} in position {at}")
    } else {
        format!("bytes in position {at}-{}", end - 1)
    };
    Fail(format!("'utf-8' codec can't decode {what}: {why}"))
}

/// Path.read_text(): the file as UTF-8, with universal newlines.
fn read_text(path: &Path) -> Res<String> {
    decode(std::fs::read(path).map_err(|e| os_error(e, path))?)
}

/// A file's bytes as Python's text mode reads them: UTF-8, with universal newlines.
fn decode(bytes: Vec<u8>) -> Res<String> {
    let text = String::from_utf8(bytes).map_err(|e| decode_error(e.as_bytes(), e.utf8_error()))?;
    if !text.contains('\r') {
        return Ok(text);
    }
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Path.write_text().
fn write_text(path: &Path, text: &str) -> Res<()> {
    std::fs::write(path, text).map_err(|e| os_error(e, path))
}

/// Path.mkdir(parents=True, exist_ok=True).
fn mkdirs(path: &Path) -> Res<()> {
    std::fs::create_dir_all(path).map_err(|e| os_error(e, path))
}

/// The coordinator's guide, with this bullpen's own path in its commands.
pub fn guide_text() -> Res<String> {
    let text = read_text(&prompts().join("coordinator.md"))?;
    Ok(text.replace("{{follow}}", &follow_cmd()))
}

/// The guide as a file, for the coordinator's system prompt.
pub fn guide_file(repo: &Repo) -> Res<PathBuf> {
    let path = repo.dir.join("coordinator.md");
    mkdirs(&repo.dir)?;
    write_text(&path, &guide_text()?)?;
    Ok(path)
}

/// The guide's hash: sha256, in hex.
pub fn guide_hash(text: &str) -> String {
    bullpen_core::sha256::hex(text.as_bytes())
}

/// Record that coordinator c has this guide: its hash, and the text, for the diff when it changes.
pub fn give_guide(c: &mut Obj, text: &str) -> Res<()> {
    let h = guide_hash(text);
    let dir = guides();
    let path = dir.join(format!("{h}.md"));
    if !path.exists() {
        mkdirs(&dir)?;
        let tmp = dir.join(format!(".{h}.md.{}.tmp", std::process::id()));
        write_text(&tmp, text)?;
        std::fs::rename(&tmp, &path).map_err(|e| os_error(e, &tmp))?;
    }
    c.insert("guide".into(), Value::String(h));
    Ok(())
}

/// The guide text with hash h, if it was kept (a file that isn't UTF-8 is an error).
pub fn old_guide(h: &str) -> Res<Option<String>> {
    if h.is_empty() {
        return Ok(None);
    }
    let Ok(bytes) = std::fs::read(guides().join(format!("{h}.md"))) else {
        return Ok(None);
    };
    decode(bytes).map(Some)
}

/// The heading of the section line i is in: the nearest "#" line at or above it.
fn heading(lines: &[&str], i: usize) -> String {
    let upto = &lines[..(i + 1).min(lines.len())];
    match upto.iter().rev().find(|l| l.starts_with('#')) {
        Some(l) => strip(l.trim_start_matches('#')).to_string(),
        None => "the top".into(),
    }
}

/// The headings of the guide's sections that changed, in order: those around changed
/// lines in the new guide, or in the old one for a section that's gone.
pub fn guide_sections(old: &str, new: &str) -> Vec<String> {
    let (a, b) = (splitlines(old), splitlines(new));
    let mut out: Vec<String> = Vec::new();
    for (op, i1, i2, j1, j2) in Matcher::new(&a, &b, false).opcodes() {
        if op == "equal" {
            continue;
        }
        let mut hs: Vec<String> = Vec::new();
        if !b.is_empty() {
            hs.extend((j1..j2.max(j1 + 1)).map(|j| heading(&b, j)));
        }
        hs.extend((i1..i2).filter(|&i| a[i].starts_with('#')).map(|i| heading(&a, i))); // removed sections
        for h in hs {
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out
}

/// (line, lines after it) telling a coordinator given the guide old (None: not known)
/// that it is now new.
pub fn guide_news(old: Option<&str>, new: &str) -> (String, Vec<String>) {
    let follow = "follow it from now on";
    let Some(old) = old else {
        return (
            format!(
                "Your coordinator guide may have changed since you were given it: run \
                 `bullpen guide` to read it all, and {follow}."
            ),
            vec![],
        );
    };
    let diff = unified_diff(&splitlines(old), &splitlines(new), "guide (yours)", "guide (now)", 3);
    let where_ = guide_sections(old, new);
    let where_ = match where_.len() {
        0 => String::new(),
        1 => format!(" (section: {})", where_[0]),
        _ => format!(" (sections: {})", where_.join(", ")),
    };
    if diff.len() <= GUIDE_DIFF_LINES {
        return (
            format!("Your coordinator guide changed{where_}. Read the change below and {follow}:"),
            diff,
        );
    }
    let line = format!(
        "Your coordinator guide changed{where_}, {} lines of diff: run `bullpen guide` to read it all, and {follow}.",
        diff.len()
    );
    (line, vec![])
}

/// Tell each running coordinator, in this repo and every other, whose guide isn't the
/// current one what changed. Never fails the command that called it.
pub fn tell_guide_changes(repo: &mut Repo) {
    if let Err(e) = tell_all(repo) {
        eprintln!("bullpen: couldn't tell the coordinators about their guide: {e}");
    }
}

fn tell_all(repo: &mut Repo) -> Res<()> {
    let text = guide_text()?;
    let h = guide_hash(&text);
    let mut others: Vec<Repo> = known_repos()?.into_iter().filter(|o| o.dir != repo.dir).collect();
    tell_repo(repo, &text, &h, false)?;
    for r in &mut others {
        tell_repo(r, &text, &h, true)?;
    }
    Ok(())
}

/// Tell repo r's running coordinators whose guide isn't h (the guide text); other: r
/// isn't the repo the command runs in, so the lines name its root.
fn tell_repo(r: &mut Repo, text: &str, h: &str, other: bool) -> Res<()> {
    let list = r
        .cfg
        .get("coordinators")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut told = Vec::new(); // by place in the list, all found before any is told
    for (n, c) in list.iter().enumerate() {
        let Some(c) = c.as_object() else { continue };
        if c.get("guide").and_then(Value::as_str) != Some(h) && alive(Some(c))? {
            told.push(n);
        }
    }
    let task = json!({"id": "guide"}).as_object().cloned().unwrap_or_default();
    for &n in &told {
        let c = list[n].as_object().cloned().unwrap_or_default();
        let old = old_guide(c.get("guide").and_then(Value::as_str).unwrap_or(""))?;
        let (line, more) = guide_news(old.as_deref(), text);
        let to = Some(vec![s(&c, "id").to_string()]);
        news(
            r,
            &task,
            "guide",
            &line,
            Opts {
                to,
                more,
                ..Default::default()
            },
        )?;
        if let Some(c) = coordinator_at_mut(r, n) {
            give_guide(c, text)?;
        }
        let of = if other {
            format!(" of {}", r.root.display())
        } else {
            String::new()
        };
        println!("coordinator {}{of}: told that its guide changed", s(&c, "id"));
    }
    if !told.is_empty() {
        r.save()?;
    }
    Ok(())
}

/// The coordinator at place n in the repo's list, to change.
fn coordinator_at_mut(r: &mut Repo, n: usize) -> Option<&mut Obj> {
    r.cfg
        .get_mut("coordinators")?
        .as_array_mut()?
        .get_mut(n)?
        .as_object_mut()
}

/// `guide`: print the coordinator's guide.
pub fn cmd_guide(argv: &[String]) -> Res<i32> {
    Args::parse("guide", argv, &[]);
    print!("{}\n", guide_text()?);
    Ok(0)
}

// ---------- difflib: SequenceMatcher and unified_diff, as Python's ----------

/// An opcode: (tag, i1, i2, j1, j2), the tag "equal", "replace", "delete" or "insert".
type Op = (&'static str, usize, usize, usize, usize);

/// difflib.SequenceMatcher(None, a, b, autojunk) over lines: no junk, and with
/// autojunk, b's popular lines (in 1% of a b of 200 or more) start no match.
struct Matcher<'a> {
    a: &'a [&'a str],
    b: &'a [&'a str],
    b2j: HashMap<&'a str, Vec<usize>>,
}

impl<'a> Matcher<'a> {
    fn new(a: &'a [&'a str], b: &'a [&'a str], autojunk: bool) -> Matcher<'a> {
        let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
        for (j, l) in b.iter().enumerate() {
            b2j.entry(l).or_default().push(j);
        }
        if autojunk && b.len() >= 200 {
            let most = b.len() / 100 + 1;
            b2j.retain(|_, js| js.len() <= most);
        }
        Matcher { a, b, b2j }
    }

    /// find_longest_match: (i, j, k), a[i..i+k] == b[j..j+k], the longest, earliest in a,
    /// then in b, extended over popular lines.
    fn longest(&self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> (usize, usize, usize) {
        let (a, b) = (self.a, self.b);
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut next: HashMap<usize, usize> = HashMap::new();
            for &j in self.b2j.get(a[i]).map_or(&[][..], |v| &v[..]) {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = if j == 0 {
                    0
                } else {
                    j2len.get(&(j - 1)).copied().unwrap_or(0)
                } + 1;
                next.insert(j, k);
                if k > bestsize {
                    (besti, bestj, bestsize) = (i + 1 - k, j + 1 - k, k);
                }
            }
            j2len = next;
        }
        while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
            (besti, bestj, bestsize) = (besti - 1, bestj - 1, bestsize + 1);
        }
        while besti + bestsize < ahi && bestj + bestsize < bhi && a[besti + bestsize] == b[bestj + bestsize] {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }

    /// get_matching_blocks: the matches in order, adjacent ones joined, then (len a, len b, 0).
    fn matching_blocks(&self) -> Vec<(usize, usize, usize)> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut blocks = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.longest(alo, ahi, blo, bhi);
            if k > 0 {
                blocks.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        let mut out = Vec::new();
        for (i2, j2, k2) in blocks {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    out.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 > 0 {
            out.push((i1, j1, k1));
        }
        out.push((la, lb, 0));
        out
    }

    /// get_opcodes: how to turn a into b.
    fn opcodes(&self) -> Vec<Op> {
        let (mut i, mut j) = (0, 0);
        let mut out = Vec::new();
        for (ai, bj, size) in self.matching_blocks() {
            let tag = match (i < ai, j < bj) {
                (true, true) => "replace",
                (true, false) => "delete",
                (false, true) => "insert",
                _ => "",
            };
            if !tag.is_empty() {
                out.push((tag, i, ai, j, bj));
            }
            (i, j) = (ai + size, bj + size);
            if size > 0 {
                out.push(("equal", ai, i, bj, j));
            }
        }
        out
    }

    /// get_grouped_opcodes(n): the changes in hunks, with up to n lines around each.
    fn grouped_opcodes(&self, n: usize) -> Vec<Vec<Op>> {
        let mut codes = self.opcodes();
        if codes.is_empty() {
            codes.push(("equal", 0, 1, 0, 1));
        }
        if let Some(c) = codes.first_mut().filter(|c| c.0 == "equal") {
            *c = (
                c.0,
                c.1.max(c.2.saturating_sub(n)),
                c.2,
                c.3.max(c.4.saturating_sub(n)),
                c.4,
            );
        }
        if let Some(c) = codes.last_mut().filter(|c| c.0 == "equal") {
            *c = (c.0, c.1, c.2.min(c.1 + n), c.3, c.4.min(c.3 + n));
        }
        let mut groups = Vec::new();
        let mut group = Vec::new();
        for (tag, mut i1, i2, mut j1, j2) in codes {
            if tag == "equal" && i2 - i1 > 2 * n {
                group.push((tag, i1, i2.min(i1 + n), j1, j2.min(j1 + n)));
                groups.push(std::mem::take(&mut group));
                (i1, j1) = (i1.max(i2.saturating_sub(n)), j1.max(j2.saturating_sub(n)));
            }
            group.push((tag, i1, i2, j1, j2));
        }
        if !group.is_empty() && !(group.len() == 1 && group[0].0 == "equal") {
            groups.push(group);
        }
        groups
    }
}

/// A hunk's range in a unified diff: "start,length", "start" for one line, and the
/// line before for none.
fn unified_range(start: usize, stop: usize) -> String {
    let length = stop - start;
    match length {
        1 => format!("{}", start + 1),
        0 => format!("{start},0"),
        _ => format!("{},{length}", start + 1),
    }
}

/// difflib.unified_diff(a, b, from, to, n=n, lineterm=""), as its lines.
pub fn unified_diff(a: &[&str], b: &[&str], from: &str, to: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for group in Matcher::new(a, b, true).grouped_opcodes(n) {
        if out.is_empty() {
            out.push(format!("--- {from}"));
            out.push(format!("+++ {to}"));
        }
        let (first, last) = (group[0], group[group.len() - 1]);
        out.push(format!(
            "@@ -{} +{} @@",
            unified_range(first.1, last.2),
            unified_range(first.3, last.4)
        ));
        for (tag, i1, i2, j1, j2) in group {
            if tag == "equal" {
                out.extend(a[i1..i2].iter().map(|l| format!(" {l}")));
                continue;
            }
            if tag == "replace" || tag == "delete" {
                out.extend(a[i1..i2].iter().map(|l| format!("-{l}")));
            }
            if tag == "replace" || tag == "insert" {
                out.extend(b[j1..j2].iter().map(|l| format!("+{l}")));
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "test_guide.rs"]
mod tests;
