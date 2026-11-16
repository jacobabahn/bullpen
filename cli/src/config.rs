//! `config`: showing and changing settings, the repo's and the
//! global ones. Its flags come from core's settings table, one per setting that has one.

use crate::args::{flag, usage_error, value, Arg, Args};
use crate::coordinators::{coordinator, coordinator_at, coordinator_mut, coordinators};
use crate::news::news_mode_line;
use crate::news::read::py_int;
use crate::task_line::term_width;
use bullpen_core::config::{config_source, global_config_path, inherited_config, read_global, read_global_file};
use bullpen_core::json::{self, dumps, dumps_line, py_str, s, Obj};
use bullpen_core::paths::{home_path, homes, state};
use bullpen_core::repo::{reindex, repo_root};
use bullpen_core::settings::{defaults, is_internal, is_repo_only, Kind as SKind, Setting, SETTINGS};
use bullpen_core::task::model_phrase;
use bullpen_core::text::{len, ljust, shell_join, strip};
use bullpen_core::{fail, Repo, Res};
use serde_json::Value;

const ON_OFF: &[&str] = &["on", "off"];

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The settings that have a flag (all but the file-only ones), in the table's order.
fn flagged() -> impl Iterator<Item = &'static Setting> {
    SETTINGS.iter().filter(|s| s.kind != SKind::File)
}

/// argparse's metavar for an option with none given: its name in capitals.
fn metavar(s: &Setting) -> &'static str {
    match (s.metavar, s.kind) {
        (Some(m), _) => m,
        (None, SKind::Model) => "MODEL",
        (None, _) => leak(s.name.to_uppercase()),
    }
}

/// config's arguments: its own, a flag for each setting that has one, then --model,
/// --board-others and --coordinator, in config.py's order.
fn spec() -> Vec<Arg> {
    let mut out = vec![
        flag(
            "--global",
            "change the settings every repository gets unless it sets its own, in \
             $XDG_CONFIG_HOME/bullpen/config.json (default ~/.config/bullpen/config.json)",
        ),
        value(
            "--unset",
            "KEY",
            "drop this repo's own value for a setting, like merge or worker_model, so the global one (or the \
             default) applies; with --global, drop the global one (repeatable)",
        ),
        flag(
            "--json",
            "print the settings as JSON, with each one's source under \"sources\"",
        ),
    ];
    for s in flagged() {
        let (name, help) = (leak(s.flag()), leak(s.full_help()));
        out.push(match s.kind {
            SKind::Choice => value(name, "", help).choices(s.choices.unwrap_or(&[])),
            SKind::OnOff => value(name, "", help).choices(ON_OFF),
            _ => value(name, metavar(s), help),
        });
    }
    out.extend([
        value(
            "--model",
            "MODEL",
            "claude's --model for workers and reviewers, follow-up ones too (an alias like opus, or a model ID; \
             \"default\": let Claude Code pick)",
        ),
        value(
            "--board-others",
            "",
            "with several coordinators: this coordinator's board shows the other ones' tasks too (default off; \
             the board's o key)",
        )
        .choices(ON_OFF),
        value(
            "--coordinator",
            "ID",
            "the coordinator --board-others is for (default: the one whose pane or board runs it)",
        ),
    ]);
    out
}

/// What a setting's flag was given, as argparse stores it: an int for a number.
enum Given<'a> {
    Text(&'a str),
    Number(i64),
}

/// The value config's flag gives setting s, checked.
fn setting_value(s: &Setting, v: &Given) -> Res<Value> {
    match (s.kind, v) {
        (SKind::OnOff, Given::Text(v)) => return Ok(Value::Bool(*v == "on")),
        (_, Given::Number(n)) => {
            if s.least.is_some_and(|least| *n < least) {
                return fail(format!("{} takes {}", s.flag(), s.says.unwrap_or("")));
            }
            return Ok(Value::from(*n));
        }
        _ => {}
    }
    let Given::Text(v) = v else { unreachable!() };
    if s.pattern.is_some() && !s.matches(v) {
        return fail(format!("{} must be {}", s.name, s.says.unwrap_or("")));
    }
    if s.clears {
        let v = strip(v);
        return Ok(if v.is_empty() { Value::Null } else { Value::from(v) });
    }
    Ok(Value::from(*v))
}

/// {key: value} for the settings config's flags set, each checked.
fn config_changes(a: &Args) -> Res<Obj> {
    let mut ch = Obj::new();
    for s in flagged() {
        let flag = s.flag();
        let Some(v) = a.opt(&flag) else { continue };
        let given = if s.kind == SKind::Number {
            Given::Number(py_int(v).expect("check_ints read it"))
        } else {
            Given::Text(v)
        };
        if matches!(given, Given::Text("")) && !s.clears {
            continue;
        }
        ch.insert(s.name.into(), setting_value(s, &given)?);
    }
    if let Some(model) = a.opt("--model").filter(|m| !m.is_empty()) {
        // --model is each role's, unless its own flag says otherwise ("default" too: it drops the command's own)
        for key in ["worker_model", "reviewer_model", "reviewer_followup_model"] {
            ch.entry(key).or_insert_with(|| Value::from(model));
        }
    }
    Ok(ch)
}

/// The numbers' flags are ints to argparse: a bad one is a usage error, whatever else is wrong.
fn check_ints(a: &Args, sp: &[Arg]) {
    for s in flagged().filter(|s| s.kind == SKind::Number) {
        let flag = s.flag();
        for v in a.all(&flag) {
            if py_int(v).is_none() {
                usage_error("config", sp, &format!("argument {flag}: invalid int value: '{v}'"));
            }
        }
    }
}

/// The keys --unset names, checked: settings bullpen has (or this repo's config.json
/// has), and not one only a repository has, unless the repo's own is just switched off
/// (base, push_remote).
fn check_unset(keys: &[&str], repo: Option<&Repo>) -> Res<()> {
    let defaults = defaults();
    for &k in keys {
        // a repo-only setting can be dropped only to switch the repo's own off (base, push_remote)
        if is_repo_only(k) && !(repo.is_some() && defaults.get(k) == Some(&Value::Null)) {
            return fail(format!(
                "{k} is this repository's own: {}",
                if repo.is_none() {
                    "it can't be in the global config.json".to_string()
                } else {
                    format!("change it with --{}", k.replace('_', "-"))
                }
            ));
        }
        let known = defaults.contains_key(k) || repo.is_some_and(|r| r.own.contains(k));
        if is_internal(k) || (!known && !read_global_file()?.contains_key(k)) {
            let names: Vec<&str> = defaults
                .iter()
                .filter(|(k, v)| !is_repo_only(k) || v.is_null())
                .map(|(k, _)| k.as_str())
                .collect();
            return fail(format!("no setting {k}: it is one of {}", names.join(", ")));
        }
    }
    Ok(())
}

/// `config`: show or change this repo's settings (--global: every repo's).
pub fn cmd_config(argv: &[String]) -> Res<i32> {
    let sp = spec();
    let a = Args::parse("config", argv, &sp);
    check_ints(&a, &sp);
    if a.opt("--pickup") == Some("on") {
        // the watcher's timer looks within minutes (`_pickup` drops the flag if there's nothing)
        std::fs::create_dir_all(bullpen_core::paths::state())?;
        crate::news::touch(&crate::backlog::pickup::flag())?;
    }
    if a.opt("--issues") == Some("on") || a.opt("--issues-every").is_some() {
        // the watcher's timer looks at its next tick (`_issues` reads only where a read is due)
        crate::issues::timer::soon()?;
    }
    if a.flag("--global") {
        return config_global(&a);
    }
    let mut repo = Repo::new(repo_root()?)?;
    let ch = config_changes(&a)?;
    let unset = a.all("--unset");
    check_unset(&unset, Some(&repo))?;
    let news_was = repo.cfg_str("news").to_string();
    let changed = !ch.is_empty() || !unset.is_empty();
    for k in unset {
        repo.unset(k)?;
    }
    if let Some(on) = a.opt("--board-others") {
        let named = a.opt("--coordinator").filter(|c| !c.is_empty());
        let block = std::env::var("REX_BLOCK").unwrap_or_default();
        let found = match named {
            Some(cid) => coordinator(&repo, cid),
            None => coordinator_at(&repo, &block, true),
        };
        let Some(cid) = found.map(|c| s(c, "id").to_string()) else {
            return fail(match named {
                Some(cid) => format!("no coordinator {cid}"),
                None => "--board-others is per board: run it in a coordinator's pane or board, or add --coordinator ID"
                    .into(),
            });
        };
        if let Some(c) = coordinator_mut(&mut repo, &cid) {
            c.insert("board_others".into(), Value::Bool(on == "on"));
        }
    }
    for (k, v) in &ch {
        repo.set(k, v.clone());
    }
    if ch.contains_key("news") {
        let told = crate::news::switch::switched(&repo, &news_was)?; // before the save: `wake` can be refused
        eprintln!("{}", news_mode_line(&repo));
        crate::guide::told_by_switch(&mut repo, &told)?;
    }
    repo.save()?;
    reindex(&repo)?;
    if changed {
        crate::guide::tell_guide_changes_quietly(&mut repo); // a setting may have brought its guide a section
    }
    show_config(Some(&repo), a.flag("--json"))?;
    Ok(0)
}

/// The flags that are per repository, for --global's refusal: the repo-only settings',
/// and --board-others with its --coordinator (per board).
fn per_repo_flags() -> Vec<String> {
    let mut out: Vec<String> = flagged().filter(|s| s.repo_only).map(Setting::flag).collect();
    out.extend(["--board-others".into(), "--coordinator".into()]);
    out
}

/// `config --global`: change the global config.json, which every repository's settings
/// fall back on.
fn config_global(a: &Args) -> Res<i32> {
    let per_repo: Vec<String> = per_repo_flags().into_iter().filter(|f| a.opt(f).is_some()).collect();
    if !per_repo.is_empty() {
        return fail(format!(
            "{} {} per repository, so not for the global config: run it without --global, in that repository",
            per_repo.join(", "),
            if per_repo.len() == 1 { "is" } else { "are" }
        ));
    }
    let ch = config_changes(a)?;
    let keys = a.all("--unset");
    check_unset(&keys, None)?;
    let path = global_config_path();
    let mut g = read_global_file()?;
    if !ch.is_empty() || !keys.is_empty() {
        for k in &keys {
            g.shift_remove(*k);
        }
        for (k, v) in ch {
            g.insert(k, v);
        }
        json::write_obj(&path, &g)?;
        for cfg in config_files() {
            let root = json::read_obj(&cfg)?.unwrap_or_default();
            match root.get("root") {
                Some(r) if bullpen_core::json::truthy(Some(r)) => reindex(&Repo::new(py_str(r))?)?, // stuck_after can come from here
                _ => {}
            }
        }
        crate::guide::tell_guide_changes_everywhere();
    }
    let repo = repo_root().ok().map(Repo::new).transpose().ok().flatten();
    show_config(repo.as_ref(), a.flag("--json"))?;
    Ok(0)
}

/// <state>/repos/*/config.json.
fn config_files() -> Vec<std::path::PathBuf> {
    let dirs = std::fs::read_dir(state().join("repos")).into_iter().flatten().flatten();
    dirs.map(|e| e.path().join("config.json"))
        .filter(|p| p.is_file())
        .collect()
}

/// A setting's value as `config` lists it: a command as a shell would take it, text as
/// it is, the rest as JSON.
pub fn config_value(v: &Value) -> String {
    match v {
        Value::Array(a) if a.iter().all(Value::is_string) => {
            let words: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            shell_join(&words)
        }
        Value::String(s) => s.clone(),
        other => dumps_line(other),
    }
}

/// [(title, keys)]: keys in the settings' groups and order, the rest under Other.
pub fn config_groups(keys: &[String]) -> Vec<(String, Vec<String>)> {
    let mut titles: Vec<&str> = Vec::new();
    for g in SETTINGS.iter().filter_map(|s| s.group) {
        if !titles.contains(&g) {
            titles.push(g);
        }
    }
    let grouped = |k: &str| SETTINGS.iter().any(|s| s.name == k && s.group.is_some());
    let mut out: Vec<(String, Vec<String>)> = titles
        .iter()
        .map(|t| {
            let names: Vec<&str> = SETTINGS.iter().filter(|s| s.group == Some(t)).map(|s| s.name).collect();
            let ks = names
                .iter()
                .filter(|n| keys.iter().any(|k| k == *n))
                .map(|n| n.to_string())
                .collect();
            (t.to_string(), ks)
        })
        .collect();
    out.push(("Other".into(), keys.iter().filter(|k| !grouped(k)).cloned().collect()));
    out.retain(|(_, ks)| !ks.is_empty());
    out
}

fn cmd_words(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().map(py_str).collect())
        .unwrap_or_default()
}

/// Each setting, with where its value comes from: this repo's config.json ("repo"), the
/// global one ("global") or bullpen's own default ("default"). Without a repo, what a
/// repository gets when it sets nothing itself.
fn show_config(repo: Option<&Repo>, as_json: bool) -> Res<()> {
    let path = global_config_path();
    let (cfg, sources): (Obj, Vec<(String, &str)>) = match repo {
        Some(r) => {
            let mut sources = Vec::new();
            for k in r.cfg.keys().filter(|k| !is_internal(k)) {
                sources.push((k.clone(), config_source(r, k)?));
            }
            (r.cfg.clone(), sources)
        }
        None => {
            let glob = read_global()?;
            let cfg: Obj = inherited_config()?
                .into_iter()
                .filter(|(k, _)| !is_repo_only(k))
                .collect();
            let sources = cfg
                .keys()
                .map(|k| (k.clone(), if glob.contains_key(k) { "global" } else { "default" }))
                .collect();
            (cfg, sources)
        }
    };
    let model = |role: &str| {
        let m = cfg.get(&format!("{role}_model")).filter(|v| !v.is_null()).map(py_str);
        model_phrase(&cmd_words(cfg.get(role)), m.as_deref())
    };
    let (worker, reviewer) = (model("worker"), model("reviewer"));
    if as_json {
        let mut out = Obj::new();
        if let Some(r) = repo {
            out.insert("repo".into(), Value::from(r.root.to_string_lossy()));
            out.insert("state".into(), Value::from(r.dir.to_string_lossy()));
        }
        for (k, v) in &cfg {
            out.insert(k.clone(), v.clone());
        }
        out.insert(
            "models".into(),
            serde_json::json!({"worker": worker, "reviewer": reviewer}),
        );
        out.insert("global".into(), Value::from(path.to_string_lossy()));
        let src: Obj = sources.iter().map(|(k, v)| (k.clone(), Value::from(*v))).collect();
        out.insert("sources".into(), Value::Object(src));
        println!("{}", dumps(&Value::Object(out)));
        return Ok(());
    }
    let mut head = match repo {
        Some(r) => vec![
            ("repo", home_path(&r.root)),
            ("state", home_path(r.dir.join("config.json"))),
        ],
        None => vec![("repo", "none: not in a git repository".to_string())],
    };
    let none_yet = if path.exists() { "" } else { " (none yet)" };
    head.push(("global", home_path(&path) + none_yet));
    for (k, v) in head {
        println!("{}{v}", ljust(&format!("{k}:"), 8));
    }
    let keys: Vec<String> = sources
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| !["next", "next_coordinator", "coordinators"].contains(&k.as_str()))
        .collect();
    let source = |k: &str| sources.iter().find(|(n, _)| n == k).map_or("", |(_, v)| *v);
    let cols = term_width().filter(|c| *c != 0);
    for (title, group) in config_groups(&keys) {
        println!("\n{title}");
        let width = group.iter().map(|k| len(k)).max().unwrap_or(0);
        let vals: Vec<String> = group
            .iter()
            .map(|k| homes(&config_value(cfg.get(k).unwrap_or(&Value::Null))))
            .collect();
        // on a terminal, a value too long to fit beside the others isn't lined up with them
        let room = cols.map(|c| c - width as i64 - 6 - group.iter().map(|k| len(source(k))).max().unwrap_or(0) as i64);
        let vwidth = vals
            .iter()
            .map(|v| len(v))
            .filter(|n| room.is_none_or(|r| *n as i64 <= r))
            .max()
            .unwrap_or(0);
        for (k, v) in group.iter().zip(&vals) {
            println!("  {}  {}  {}", ljust(k, width), ljust(v, vwidth), source(k));
        }
    }
    println!();
    println!("models: worker {worker}; reviewer {reviewer}");
    if let Some(r) = repo {
        let cs: Vec<String> = coordinators(r)
            .iter()
            .map(|c| {
                let block = c.get("block").map_or("None".to_string(), py_str);
                format!("{} ({block})", s(c, "id"))
            })
            .collect();
        let cs = cs.join(", ");
        println!(
            "coordinators: {}",
            if cs.is_empty() {
                "none: `bullpen up` starts one"
            } else {
                &cs
            }
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "test_config.rs"]
mod tests;
