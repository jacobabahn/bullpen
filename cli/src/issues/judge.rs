//! The coordinator's judgement of an issue (docs/issues.md, "Judging one"): `bullpen
//! issue N --score clarity,size,risk --why "..." [--area ...] [--brief-file FILE]`. The
//! score is three words from short lists; bullpen, not the coordinator, says whether
//! the issue is trusted (trust.rs). A trusted issue whose score is within `issues_start`
//! goes on the list (`kept`), so pickup starts it like any item; any other stays asking
//! the person, and the coordinators get one line with its score and why. A brief given
//! here (or to an issue's item with `later --brief-file`) is the coordinator's, with the
//! issue quoted at its end, fenced (`with_quote`).

use super::trust::{trusted, Trust};
use super::{repo, store, text, view};
use crate::args::Args;
use crate::backlog::pickup;
use crate::news::kind::Kind;
use crate::news::{news, Opts};
use bullpen_core::backlog::{self, is_open};
use bullpen_core::json::{obj, s, truthy, Obj};
use bullpen_core::text::strip;
use bullpen_core::timeutil::iso_now;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

pub const CLARITY: [&str; 2] = ["clear", "vague"];
pub const SIZE: [&str; 3] = ["small", "medium", "large"];
pub const RISK: [&str; 2] = ["low", "high"];

/// The coordinator's score of an issue: a word from each list.
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub clarity: String,
    pub size: String,
    pub risk: String,
}

impl Score {
    /// "clear, small, low"
    pub fn said(&self) -> String {
        format!("{}, {}, {}", self.clarity, self.size, self.risk)
    }
}

/// `--score`'s words, `clear,small,low`; else what's wrong, with the lists.
pub fn parse_score(given: &str) -> Result<Score, String> {
    let words: Vec<String> = given.split(',').map(|w| strip(w).to_ascii_lowercase()).collect();
    let lists = format!(
        "--score is three words, clarity,size,risk: clarity {}; size {}; risk {} (`--score clear,small,low`)",
        CLARITY.join(" or "),
        SIZE.join(", ").replacen(", large", " or large", 1),
        RISK.join(" or ")
    );
    let [c, z, r] = words.as_slice() else {
        return Err(lists);
    };
    if !CLARITY.contains(&c.as_str()) || !SIZE.contains(&z.as_str()) || !RISK.contains(&r.as_str()) {
        return Err(lists);
    }
    Ok(Score {
        clarity: c.clone(),
        size: z.clone(),
        risk: r.clone(),
    })
}

/// Is the score within issues_start (`off`, `small`, `medium`)? Trust is asked apart.
pub fn within(score: &Score, start: &str) -> bool {
    let sizes: &[&str] = match start {
        "small" => &["small"],
        "medium" => &["small", "medium"],
        _ => return false,
    };
    score.clarity == "clear" && score.risk == "low" && sizes.contains(&score.size.as_str())
}

/// Why an issue still asks the person, when it does: None when it starts by itself.
pub fn why_it_asks(score: &Score, start: &str, trust: &Trust) -> Option<String> {
    if start == "off" {
        return Some("issues_start is off: every issue asks the person".into());
    }
    if !within(score, start) {
        let what = if start == "medium" {
            "clear, small or medium, low-risk"
        } else {
            "clear, small, low-risk"
        };
        return Some(format!(
            "issues_start is {start}: only {what} issues start by themselves"
        ));
    }
    (!trust.yes).then(|| trust.why.clone())
}

fn issues_start(repo: &Repo) -> String {
    match repo.cfg_str("issues_start") {
        "" => "small".into(),
        v => v.to_string(),
    }
}

/// `issue N` with --score or --brief-file: record the judgement on the issue's item, and
/// its brief; start it by itself or ask the person.
pub fn judge(repo: &Repo, n: u32, a: &Args) -> Res<i32> {
    let score = match a.opt("--score") {
        Some(given) => Some(parse_score(given).or_else(|e| fail(e))?),
        None => None,
    };
    let why = a.opt("--why").map(|w| text::line(w, 200)).filter(|w| !w.is_empty());
    let area = a.opt("--area").map(|w| text::line(w, 200)).filter(|w| !w.is_empty());
    if score.is_none() && (why.is_some() || area.is_some()) {
        return fail("--why and --area go with --score: `bullpen issue 12 --score clear,small,low --why \"...\"`");
    }
    if score.is_some() && why.is_none() {
        return fail("--score needs --why: one line on what the issue is and why you scored it so");
    }
    let brief = match a.opt("--brief-file").filter(|f| !f.is_empty()) {
        Some(f) => Some(crate::backlog::brief_text_of(f)?),
        None => None,
    };
    let slug = repo::issues_repo(repo)?;
    let issue = view::fetch(repo, &slug, n)?;
    let id = item_of(repo, n)?;
    if s(&issue, "state").eq_ignore_ascii_case("closed") {
        return fail(format!("#{n} is closed on GitHub: there's nothing to judge"));
    }
    let trust = match &score {
        Some(_) => Some(trusted(repo, &slug, &issue)?),
        None => None,
    };
    let brief = brief.map(|b| with_quote(&b, &issue));
    let start = issues_start(repo);
    let judged = score.as_ref().zip(trust.as_ref()).map(|(sc, t)| {
        json!({"score": sc.said(), "area": area, "why": why, "trusted": t.yes, "trust": t.why, "at": iso_now()})
    });
    let asks = score
        .as_ref()
        .zip(trust.as_ref())
        .and_then(|(sc, t)| why_it_asks(sc, &start, t));
    let (item, was, by_itself) = backlog::change(repo, |b| {
        let item = b.item(&id)?;
        if !is_open(item) {
            return fail(crate::backlog::not_open(item));
        }
        let was = s(item, "state").to_string();
        // kept by an earlier judgement, not the person: a new one can take it back
        let by_itself = was == "kept" && truthy(obj(item, "judged").get("kept"));
        if let Some(text) = &brief {
            let name = crate::backlog::write_brief(repo, &id, text)?;
            b.item_mut(&id)?.insert("brief".into(), json!(name));
        }
        let it = b.item_mut(&id)?;
        if let Some(j) = &judged {
            let mut j = j.clone();
            if was == "asks" || by_itself {
                let kept = asks.is_none();
                it.insert("state".into(), json!(if kept { "kept" } else { "asks" }));
                if kept {
                    j["kept"] = json!(true);
                }
            }
            it.insert("judged".into(), j);
        }
        Ok((it.clone(), was, by_itself))
    })?;
    let mut out = Vec::new();
    if brief.is_some() {
        let path = repo.dir.join("briefs").join(s(&item, "brief"));
        out.push(format!(
            "{id} has a brief: {} (the issue quoted at its end)",
            path.display()
        ));
    }
    if let (Some(sc), Some(t)) = (&score, &trust) {
        out.push(format!("{id}: {}; {}", sc.said(), t.why));
        out.push(outcome(repo, &item, &was, by_itself, &asks, n)?);
        if (was == "asks" || by_itself) && asks.is_some() {
            tell(
                repo,
                &item,
                sc,
                why.as_deref().unwrap_or(""),
                asks.as_deref().unwrap_or(""),
            )?;
        }
    }
    println!("{}", out.join("\n"));
    super::decided(repo, &id)?; // kept by itself is a yes: never asked again
    if by_itself && asks.is_some() {
        super::undecided(repo, &id)?; // taken back: it asks again
    }
    pickup::soon(repo);
    Ok(0)
}

/// The item of issue n, which must be on the backlog.
fn item_of(repo: &Repo, n: u32) -> Res<String> {
    let seen = store::load(repo)?;
    match seen.get(&n.to_string()).map(store::item_of).filter(|i| !i.is_empty()) {
        Some(id) => Ok(id.to_string()),
        None => fail(format!(
            "#{n} isn't on the backlog: `bullpen issues --now` reads the open issues"
        )),
    }
}

/// What became of the item, in words.
fn outcome(repo: &Repo, item: &Obj, was: &str, by_itself: bool, asks: &Option<String>, n: u32) -> Res<String> {
    let id = s(item, "id");
    if was != "asks" && !by_itself {
        return Ok(format!("{id} is on the list already: the person said yes"));
    }
    if let Some(why) = asks {
        let what = if by_itself {
            "is off the list and asks the person again"
        } else {
            "still asks the person"
        };
        return Ok(format!(
            "{id} {what} ({why}): tell them in a line, with its score and why, and ask whether they want it"
        ));
    }
    let brief = if truthy(item.get("brief")) {
        String::new()
    } else {
        format!(". It needs a brief before it can start: `bullpen issue {n} --brief-file FILE`")
    };
    let how = if truthy(repo.cfg.get("pickup")) {
        "pickup starts it when there's room"
    } else {
        "pickup is off, so nothing starts it by itself: `bullpen later ID --start` does"
    };
    let what = if by_itself { "stays" } else { "is" };
    Ok(format!(
        "{id} {what} on the list, trusted and within issues_start: {}{brief}",
        how.replace("ID", id)
    ))
}

/// The news line for an issue that still asks the person: its item, score and why, and
/// why it asks. Never the issue's text: its title is the item's, clipped when it was read.
fn tell(repo: &Repo, item: &Obj, score: &Score, why: &str, asks: &str) -> Res<()> {
    let id = s(item, "id");
    let text = format!(
        "{id} {}: {}, {why}. It asks the person ({asks}): ask them in a line whether they want it. `bullpen later \
         {id} --keep` is yes, `bullpen later {id} --drop \"why\"` no",
        s(item, "title"),
        score.said()
    );
    let mut about = Obj::new();
    about.insert("id".into(), json!(id));
    news(
        repo,
        &about,
        Kind::IssueAsk,
        &text,
        Opts {
            everyone: true,
            ..Opts::default()
        },
    )
}

/// A brief for an issue's item: the coordinator's text, then the issue quoted.
pub fn with_quote(brief: &str, issue: &Obj) -> String {
    format!("{}\n\n{}", brief.trim_end(), text::quoted(issue))
}

/// A brief given to an item (`later --brief-file`, `--start`): for an issue's item, the
/// issue quoted at its end (read from GitHub now); any other's as it is.
pub fn brief_for(repo: &Repo, item: &Obj, brief: &str) -> Res<String> {
    if s(item, "by") != "issue" {
        return Ok(brief.to_string());
    }
    let n = obj(item, "issue").get("number").and_then(Value::as_u64).unwrap_or(0) as u32;
    let slug = repo::issues_repo(repo)?;
    Ok(with_quote(brief, &view::fetch(repo, &slug, n)?))
}

/// A yes from the person to an issue's item that has no brief: the coordinators hear
/// that it needs one before it can start.
pub fn yes_needs_brief(repo: &Repo, item: &Obj) -> Res<()> {
    if s(item, "by") != "issue" || truthy(item.get("brief")) || s(item, "state") != "kept" {
        return Ok(());
    }
    let id = s(item, "id");
    let n = obj(item, "issue").get("number").and_then(Value::as_i64).unwrap_or(0);
    let text = format!(
        "The person said yes to {id} {}: it needs a brief before it can start. Read it with `bullpen issue {n}`, \
         write the brief in your own words and give it with `bullpen issue {n} --brief-file <f>`",
        s(item, "title")
    );
    let mut about = Obj::new();
    about.insert("id".into(), json!(id));
    news(
        repo,
        &about,
        Kind::IssueAsk,
        &text,
        Opts {
            everyone: true,
            ..Opts::default()
        },
    )
}

#[cfg(test)]
#[path = "tests/test_judge.rs"]
mod tests;
