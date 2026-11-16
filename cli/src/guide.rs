//! The coordinator's guide, and telling running coordinators when it changed
//!.
//!
//! The guide is `prompts/coordinator.md` with the sections that apply to the repo put
//! in at its `{{sections}}` line, each from a file of its own in `prompts/coordinator/`
//! (SECTIONS): how news arrives when it isn't typed, pull requests, auto-accept, pickup,
//! other coordinators. So a repo's guide changes with its settings.
//!
//! A coordinator gets its guide once, in its system prompt or by reading `bullpen guide`
//! (`up --here`), and can't load it again. Each records the hash of the guide it was
//! given (c["guide"]), and the text itself is kept in <state>/guides/<hash>.md: one
//! folder for every repo. `up`, `accept` and `config` compare each running coordinator's
//! hash, in every repo, with its repo's guide now. A coordinator whose guide is out of
//! date gets one news line, only it: with the new section after the line when a section
//! came or went, else with the diff when it is short. Its hash moves on, so each change
//! is told once. One from before hashes, or whose old text is gone, is told to read the
//! guide again.

use crate::args::{flag, Arg, Args};
use crate::coordinators::{alive, coordinators};
use crate::news::{follow_cmd, news, Opts};
use bullpen_core::json::{s, truthy, Obj};
use bullpen_core::paths::{prompts, state};
use bullpen_core::repo::{known_repos, repo_root};
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

/// Where the guide's sections go in `prompts/coordinator.md`: a line of its own.
const SECTIONS_MARK: &str = "{{sections}}\n";

/// The sections a guide has only when they apply, in the guide's order: the file in
/// `prompts/coordinator/`, and whether the repo's guide has it.
const SECTIONS: &[(&str, fn(&Repo) -> Res<bool>)] = &[
    ("news-wake", |r| Ok(r.cfg_str("news") == "wake")),
    ("news-monitor", |r| Ok(r.cfg_str("news") == "monitor")),
    ("prs", |r| r.has_origin()),
    ("auto-accept", |r| {
        Ok(on(r, "auto_accept") || (on(r, "pickup") && on(r, "pickup_auto_accept")))
    }),
    ("pickup", |r| Ok(on(r, "pickup"))),
    ("others", |r| Ok(coordinators(r).len() > 1)),
];

fn on(repo: &Repo, key: &str) -> bool {
    truthy(repo.cfg.get(key))
}

/// The coordinator's guide for the repo, with the sections that apply to it (None:
/// every section) and this bullpen's own path in its commands.
pub fn guide_text(repo: Option<&Repo>) -> Res<String> {
    let mut sections = String::new();
    for (name, applies) in SECTIONS {
        if match repo {
            Some(r) => applies(r)?,
            None => true,
        } {
            let text = read_text(&prompts().join("coordinator").join(format!("{name}.md")))?;
            sections += &format!("{}\n\n", text.trim_end());
        }
    }
    let text = read_text(&prompts().join("coordinator.md"))?;
    Ok(text
        .replace(SECTIONS_MARK, &sections)
        .replace("{{follow}}", &follow_cmd()))
}

/// The guide as a file, for the coordinator's system prompt.
pub fn guide_file(repo: &Repo) -> Res<PathBuf> {
    let path = repo.dir.join("coordinator.md");
    mkdirs(&repo.dir)?;
    write_text(&path, &guide_text(Some(repo))?)?;
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
/// that it is now new. None when there is nothing to tell: the only change is sections
/// that no longer apply, and following one of those does no harm.
pub fn guide_news(old: Option<&str>, new: &str) -> Option<(String, Vec<String>)> {
    let follow = "follow it from now on";
    let Some(old) = old else {
        return Some((
            format!(
                "Your coordinator guide may have changed since you were given it: run \
                 `bullpen guide` to read it all, and {follow}."
            ),
            vec![],
        ));
    };
    if let Some((gone, came)) = section_change(old, new) {
        return sections_news(&gone, &came);
    }
    let diff = unified_diff(&splitlines(old), &splitlines(new), "guide (yours)", "guide (now)", 3);
    let where_ = guide_sections(old, new);
    let where_ = match where_.len() {
        0 => String::new(),
        1 => format!(" (section: {})", where_[0]),
        _ => format!(" (sections: {})", where_.join(", ")),
    };
    if diff.len() <= GUIDE_DIFF_LINES {
        return Some((
            format!("Your coordinator guide changed{where_}. Read the change below and {follow}:"),
            diff,
        ));
    }
    let line = format!(
        "Your coordinator guide changed{where_}, {} lines of diff: run `bullpen guide` to read it all, and {follow}.",
        diff.len()
    );
    Some((line, vec![]))
}

/// A guide as (heading, lines) for each `## ` section, the text above the first under "".
fn by_section(text: &str) -> Vec<(&str, Vec<&str>)> {
    let mut out: Vec<(&str, Vec<&str>)> = vec![("", Vec::new())];
    for line in splitlines(text) {
        match line.strip_prefix("## ") {
            Some(h) => out.push((strip(h), vec![line])),
            None => out.last_mut().expect("never empty").1.push(line),
        }
    }
    out
}

/// "X" for one name, "X and Y" for two, "X, Y and Z".
fn listed(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

/// A section of a guide: its heading and its lines.
type Section = (String, Vec<String>);

/// (the headings that went, the sections that came) when the guides differ only in
/// whole sections (a setting changed: a section applies now, or no longer does). None
/// when they are the same, or anything else changed too.
fn section_change(old: &str, new: &str) -> Option<(Vec<String>, Vec<Section>)> {
    let own = |text: &str| -> Vec<Section> {
        let each = by_section(text).into_iter();
        each.map(|(h, l)| (h.to_string(), l.iter().map(|x| x.to_string()).collect()))
            .collect()
    };
    let (a, b) = (own(old), own(new));
    let has = |list: &[Section], h: &str| list.iter().any(|(x, _)| x == h);
    let gone: Vec<Section> = a.iter().filter(|(h, _)| !has(&b, h)).cloned().collect();
    let came: Vec<Section> = b.iter().filter(|(h, _)| !has(&a, h)).cloned().collect();
    let kept = |list: &[Section], out: &[Section]| -> Vec<Section> {
        list.iter().filter(|s| !out.contains(s)).cloned().collect()
    };
    if (gone.is_empty() && came.is_empty()) || kept(&a, &gone) != kept(&b, &came) {
        return None;
    }
    Some((gone.into_iter().map(|(h, _)| h).collect(), came))
}

/// The news of sections that came (with those that went): the line, and the new
/// sections after it, as they are. None when sections only went.
fn sections_news(gone: &[String], came: &[Section]) -> Option<(String, Vec<String>)> {
    if came.is_empty() {
        return None;
    }
    let names = |list: Vec<&String>| listed(&list.iter().map(|h| h.as_str()).collect::<Vec<_>>());
    let and = match gone.len() {
        0 => String::new(),
        1 => format!(", and its section {} no longer applies", names(gone.iter().collect())),
        _ => format!(", and its sections {} no longer apply", names(gone.iter().collect())),
    };
    let one = came.len() == 1;
    let line = format!(
        "Your coordinator guide changed with this repo's settings: it has the section{} {} now{and}. Read {} \
         below and follow it from now on:",
        if one { "" } else { "s" },
        names(came.iter().map(|(h, _)| h).collect()),
        if one { "it" } else { "them" }
    );
    let mut more: Vec<String> = came.iter().flat_map(|(_, lines)| lines.clone()).collect();
    while more.last().is_some_and(|l| l.is_empty()) {
        more.pop();
    }
    Some((line, more))
}

/// A section's lines as a guide has them (`prompts/coordinator/<name>.md`), for a
/// message that carries it.
pub fn section_lines(name: &str) -> Res<Vec<String>> {
    let text = read_text(&prompts().join("coordinator").join(format!("{name}.md")))?;
    let text = text.replace("{{follow}}", &follow_cmd());
    Ok(splitlines(text.trim_end()).into_iter().map(String::from).collect())
}

/// `config --news` told these coordinators of the switch itself, with the section for
/// the new way: each whose guide differs from the repo's now only in its "News by"
/// sections has the guide as it is now, with no news of it.
pub fn told_by_switch(repo: &mut Repo, cids: &[String]) -> Res<()> {
    let text = guide_text(Some(repo))?;
    for cid in cids {
        let had = crate::coordinators::coordinator(repo, cid).map(|c| s(c, "guide").to_string());
        let Some(old) = old_guide(&had.unwrap_or_default())? else {
            continue;
        };
        let news_only = |(gone, came): (Vec<String>, Vec<Section>)| {
            let came = came.iter().map(|(h, _)| h);
            gone.iter().chain(came).all(|h| h.starts_with("News by "))
        };
        if section_change(&old, &text).is_some_and(news_only) {
            if let Some(c) = crate::coordinators::coordinator_mut(repo, cid) {
                give_guide(c, &text)?;
            }
        }
    }
    Ok(())
}

/// Tell each running coordinator, in this repo and every other, whose guide isn't the
/// current one what changed. Never fails the command that called it.
pub fn tell_guide_changes(repo: &mut Repo) {
    tell_changes(repo, true)
}

/// The same, for `config`: nothing is printed (its --json is all it prints).
pub fn tell_guide_changes_quietly(repo: &mut Repo) {
    tell_changes(repo, false)
}

fn tell_changes(repo: &mut Repo, says: bool) {
    if let Err(e) = tell_all(repo, says) {
        eprintln!("bullpen: couldn't tell the coordinators about their guide: {e}");
    }
}

fn tell_all(repo: &mut Repo, says: bool) -> Res<()> {
    let mut others: Vec<Repo> = known_repos()?.into_iter().filter(|o| o.dir != repo.dir).collect();
    tell_repo(repo, says.then_some(false))?;
    for r in &mut others {
        tell_repo(r, says.then_some(true))?;
    }
    Ok(())
}

/// `config --global` changed what every repo falls back on: tell the coordinators of
/// each repo whose guide that changed. Never fails the command.
pub fn tell_guide_changes_everywhere() {
    let told = || -> Res<()> {
        for r in &mut known_repos()? {
            tell_repo(r, None)?;
        }
        Ok(())
    };
    if let Err(e) = told() {
        eprintln!("bullpen: couldn't tell the coordinators about their guide: {e}");
    }
}

/// Tell repo r's running coordinators whose guide isn't r's guide now. says: print who
/// was told (None: nothing), and whether r isn't the repo the command runs in, so the
/// lines name its root.
fn tell_repo(r: &mut Repo, says: Option<bool>) -> Res<()> {
    let text = &guide_text(Some(r))?;
    let h = &guide_hash(text);
    let list = r
        .cfg
        .get("coordinators")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut told = Vec::new(); // by place in the list, all found before any is told
    for (n, c) in list.iter().enumerate() {
        let Some(c) = c.as_object() else { continue };
        // a coordinator another copy of bullpen started has that copy's guide: it tells its own
        if c.get("guide").and_then(Value::as_str) != Some(h.as_str()) && crate::copies::ours(c) && alive(Some(c))? {
            told.push(n);
        }
    }
    let task = json!({"id": "guide"}).as_object().cloned().unwrap_or_default();
    for &n in &told {
        let c = list[n].as_object().cloned().unwrap_or_default();
        let old = old_guide(c.get("guide").and_then(Value::as_str).unwrap_or(""))?;
        let told = guide_news(old.as_deref(), text);
        if let Some((line, more)) = &told {
            let to = Some(vec![s(&c, "id").to_string()]);
            let more = more.clone();
            news(
                r,
                &task,
                "guide",
                line,
                Opts {
                    to,
                    more,
                    ..Default::default()
                },
            )?;
        }
        if let Some(c) = coordinator_at_mut(r, n) {
            give_guide(c, text)?;
        }
        if let (Some(_), Some(other)) = (&told, says) {
            let of = if other {
                format!(" of {}", r.root.display())
            } else {
                String::new()
            };
            println!("coordinator {}{of}: told that its guide changed", s(&c, "id"));
        }
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

const GUIDE: &[Arg] = &[flag(
    "--all",
    "with every section, not only those that apply to this repository",
)];

/// `guide`: print the coordinator's guide for this repository (outside one, or with
/// --all: with every section).
pub fn cmd_guide(argv: &[String]) -> Res<i32> {
    let a = Args::parse("guide", argv, GUIDE);
    let repo = match a.flag("--all") {
        true => None,
        false => repo_root().ok().map(Repo::new).transpose()?,
    };
    print!("{}\n", guide_text(repo.as_ref())?);
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
