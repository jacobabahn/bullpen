//! `tour-comment` (tours.py's cmd_tour_comment): the person's draft comments on a
//! tour's steps, added, listed or removed. `tour --send` (stepper/) sends them to the
//! worker.

use super::{comments_path, tour_path};
use crate::args::{flag, pos, usage_error, value, Arg, Args, Nargs};
use crate::news::read::py_int;
use bullpen_core::json::{self, py_str, read_obj, s, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::text::{rjust, strip};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

const TOUR_COMMENT: &[Arg] = &[
    pos("id", Nargs::One, "the task, like BP-3"),
    pos("step", Nargs::Maybe, "the step the comment is on"),
    pos("text", Nargs::Any, "the comment (mention a line in it to point at one)"),
    value("--remove", "N", "remove draft comment N"),
    flag("--json", "list them as JSON"),
];

/// The task's draft comments: {"next": the next one's number, "comments": [...]}.
pub fn read_comments(repo: &Repo, t: &Obj) -> Res<Obj> {
    let c = read_obj(&comments_path(repo, t))?.filter(|o| !o.is_empty());
    Ok(c.unwrap_or_else(|| {
        let empty = json!({"next": 1, "comments": []});
        empty.as_object().cloned().unwrap_or_default()
    }))
}

fn drafts(c: &Obj) -> Vec<Value> {
    c.get("comments").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// An int argument, as argparse's type=int reads it (a usage error if it isn't one).
fn int_arg(a: &Args, name: &str, shown: &str) -> Option<i64> {
    let v = a.opt(name).or_else(|| a.arg(name))?;
    match py_int(v) {
        Some(n) => Some(n),
        None => usage_error(
            "tour-comment",
            TOUR_COMMENT,
            &format!("argument {shown}: invalid int value: '{v}'"),
        ),
    }
}

/// `tour-comment`: draft comments on a tour's steps: add one, list them, or remove one.
pub fn cmd_tour_comment(argv: &[String]) -> Res<i32> {
    let a = Args::parse("tour-comment", argv, TOUR_COMMENT);
    let step = int_arg(&a, "step", "step");
    let remove = int_arg(&a, "--remove", "--remove");
    let repo = Repo::new(repo_root()?)?;
    let t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id");
    let mut c = read_comments(&repo, &t)?;
    let all = drafts(&c);
    if let Some(n) = remove {
        let left: Vec<Value> = all.iter().filter(|x| x.get("id") != Some(&json!(n))).cloned().collect();
        if left.len() == all.len() {
            return fail(format!("{id} has no draft comment {n}"));
        }
        c.insert("comments".into(), Value::Array(left));
        json::write_obj(&comments_path(&repo, &t), &c)?;
        println!("removed comment {n} from {id}");
        return Ok(0);
    }
    let Some(step) = step else {
        if a.flag("--json") {
            println!("{}", json::dumps(&Value::Array(all)));
            return Ok(0);
        }
        for x in &all {
            let f = |k: &str| x.get(k).map_or("None".into(), py_str);
            println!(
                "{}. step {} ({}): {}",
                rjust(&f("id"), 3),
                f("step"),
                f("title"),
                f("text")
            );
        }
        if all.is_empty() {
            println!("{id} has no draft comments");
        }
        return Ok(0);
    };
    let text = strip(&a.args("text").join(" ")).to_string();
    if text.is_empty() {
        return fail("say what the comment is");
    }
    println!("{}", add_comment(&repo, &t, step, &text)?);
    Ok(0)
}

/// Add a draft comment on step N of the task's tour; returns its number.
pub fn add_comment(repo: &Repo, t: &Obj, step: i64, text: &str) -> Res<i64> {
    let id = s(t, "id");
    let Some(tour) = read_obj(&tour_path(repo, t))?.filter(|o| !o.is_empty()) else {
        return fail(format!(
            "{id} has no tour to comment on: `bullpen tour {id}` writes one"
        ));
    };
    let steps = tour.get("steps").and_then(Value::as_array).cloned().unwrap_or_default();
    if step < 1 || step > steps.len() as i64 {
        return fail(format!("{id}'s tour has steps 1 to {}", steps.len()));
    }
    let mut c = read_comments(repo, t)?;
    let st = steps[step as usize - 1].as_object().cloned().unwrap_or_default();
    let next = c.get("next").and_then(Value::as_i64).unwrap_or(1);
    // the step as it was: a new tour may number its steps differently
    let comment = json!({"id": next, "step": step, "title": st.get("title"), "hunks": st.get("hunks"),
                         "commit": tour.get("commit"), "text": text, "at": iso_now()});
    let mut all = drafts(&c);
    all.push(comment);
    c.insert("comments".into(), Value::Array(all));
    c.insert("next".into(), json!(next + 1));
    json::write_obj(&comments_path(repo, t), &c)?;
    Ok(next)
}

#[cfg(test)]
#[path = "test_comments.rs"]
mod tests;
