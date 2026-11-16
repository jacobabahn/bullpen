//! `config --news <value>`: what switching how news reaches the coordinators sets off.
//!
//! - To `wake`: refused, with the reason, unless the coordinator is Claude Code; the
//!   hook is written (hook.rs). Running coordinators get one last typed line, which
//!   also gives each a turn: the hook is read by the running session, so that turn's
//!   end parks its first waiter.
//! - Away from `wake`: the hook is removed, and a parked waiter ends.
//! - To `monitor`: running coordinators get one last typed line telling them to start it.
//! - From typed news to news that isn't: every cursor goes to the end of the log, since
//!   what was typed already isn't news again.

use super::read::{news_size, set_news_cursor};
use super::wake::typed_mode;
use super::{follow_cmd, hook, type_news, NEWS_MARK};
use crate::coordinators::{alive, calling_coordinator, coordinators};
use bullpen_core::json::{s, Obj};
use bullpen_core::{fail, Repo, Res};

/// The repo's `news` was `was`, and is what repo.cfg says now (not saved yet).
pub fn switched(repo: &Repo, was: &str) -> Res<()> {
    let now = repo.cfg_str("news");
    if now == "wake" {
        // first, so a refusal changes nothing
        if let Some(why) = hook::refusal(repo) {
            return fail(why);
        }
        hook::install(repo)?;
    }
    if now == was {
        return Ok(());
    }
    let ids: Vec<String> = coordinators(repo).iter().map(|c| s(c, "id").to_string()).collect();
    if typed_mode(was) && !typed_mode(now) {
        for cid in std::iter::once("").chain(ids.iter().map(String::as_str)) {
            set_news_cursor(repo, news_size(repo), cid)?;
        }
    }
    if was == "wake" {
        if let Err(e) = hook::remove(repo) {
            eprintln!("bullpen: the news hook wasn't removed: {e}. `bullpen doctor --fix` removes it.");
        }
        for cid in &ids {
            super::wake::end_waiter(repo, cid)?;
        }
    }
    let line = match now {
        "monitor" => format!(
            "{NEWS_MARK}news now comes by monitor and is no longer typed: start the Monitor tool on `{}` now, \
             as your guide says",
            follow_cmd()
        ),
        "wake" => format!(
            "{NEWS_MARK}news now wakes you by itself and is no longer typed: there is nothing to start, and a \
             news monitor that still runs can be stopped. It comes as \"Stop hook feedback\" when your turn has \
             ended: that is bullpen's news, not an error and not the person's words."
        ),
        _ => return Ok(()),
    };
    // The last typed line: running coordinators have to hear of the switch. Not the one
    // that ran `config` itself: it sees the answer.
    let me = calling_coordinator(repo).map(|c| c as *const Obj);
    let mut told = Vec::new();
    for c in coordinators(repo) {
        if Some(c as *const Obj) != me && alive(Some(c))? {
            told.push(s(c, "id").to_string());
        }
    }
    if !told.is_empty() {
        type_news(repo, &line, &told)?;
    }
    Ok(())
}
