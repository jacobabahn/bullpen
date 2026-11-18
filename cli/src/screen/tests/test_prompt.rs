use super::*;

/// Claude Code's AskUserQuestion menu, as `rex capture` shows it (a real one).
const CHOICE: &str = "\
────────────────────────────────────────
 ☐ Drink

Do you prefer tea or coffee?

❯ 1. Tea
     You prefer tea.
  2. Coffee
     You prefer coffee.
  3. Type something.
────────────────────────────────────────
  4. Chat about this

Enter to select · ↑/↓ to navigate · Esc to cancel
";

/// The boxed layout of older Claude Code versions.
const BOXED: &str = "\
╭──────────────────────────────────────╮
│ Bash command                         │
│                                      │
│   npm test                           │
│                                      │
│ Do you want to proceed?              │
│ ❯ 1. Yes                             │
│   2. No, and tell Claude what to do  │
│      differently (esc)               │
╰──────────────────────────────────────╯
";

fn opts(p: &Prompt) -> Vec<(&str, &str)> {
    p.options.iter().map(|(k, l)| (k.as_str(), l.as_str())).collect()
}

#[test]
fn test_a_choice_menu_is_read_without_type_something() {
    let p = read_prompt(CHOICE).unwrap();
    assert_eq!(p.question, "Do you prefer tea or coffee?");
    assert_eq!(opts(&p), [("1", "Tea"), ("2", "Coffee"), ("4", "Chat about this")]);
}

#[test]
fn test_the_boxed_layout_is_read_too() {
    let p = read_prompt(BOXED).unwrap();
    assert_eq!(p.question, "Do you want to proceed?");
    assert_eq!(opts(&p), [("1", "Yes"), ("2", "No, and tell Claude what to do")]);
}

#[test]
fn test_a_yes_no_question_is_answered_by_y_or_n() {
    let p = read_prompt("Some output\nOverwrite it? [y/N]\n\n").unwrap();
    assert_eq!(p.question, "Overwrite it? [y/N]");
    assert_eq!(opts(&p), [("y", "Yes"), ("n", "No")]);
}

#[test]
fn test_what_the_agent_wrote_is_no_prompt() {
    let listed = "● Two ways:\n  1. Patch it\n  2. Rewrite it\n\n────\n❯ \n────\n  ⏸ manual mode on\n";
    assert_eq!(read_prompt(listed), None); // its input box is under it
    assert_eq!(read_prompt("● Two ways:\n  1. Patch it\n  2. Rewrite it\n"), None); // no ❯ on one
    let more = format!("Do you want to proceed?\n❯ 1. Yes\n  2. No\n{}", "more\n".repeat(4));
    assert_eq!(read_prompt(&more), None);
    assert_eq!(read_prompt("❯ 2. Yes\n  3. No\n"), None); // not counted from 1
    assert_eq!(read_prompt(""), None);
}

#[test]
fn test_a_prompt_id_tells_one_command_from_another() {
    let a = read_prompt(BOXED).unwrap();
    let again = read_prompt(&format!("{}{BOXED}", "old output\n".repeat(20))).unwrap();
    assert_eq!(a.id, again.id);
    let b = read_prompt(&BOXED.replace("npm test", "rm -rf /")).unwrap();
    assert_ne!(a.id, b.id);
}

#[test]
fn the_ids_are_the_pythons() {
    // hashlib.sha1 of the Python's joined lines, cut to 12
    assert_eq!(read_prompt(BOXED).unwrap().id, "b9a2aaa4da41");
    assert_eq!(read_prompt(CHOICE).unwrap().id, "cfdaf6973775");
    assert_eq!(
        read_prompt("Some output\nOverwrite it? [y/N]\n\n").unwrap().id,
        "d9da7b4b598c"
    );
}

#[test]
fn the_matchers_are_the_regexes() {
    assert_eq!(unboxed("  │ x │  "), " x");
    assert_eq!(unboxed("│ a │ b │"), " a │ b");
    assert_eq!(unboxed("a │"), "a");
    assert!(is_rule(" ╰───╯ ") && is_rule("---") && !is_rule("--") && !is_rule("─── x"));
    assert!(is_top_rule("╭───╮") && !is_top_rule("╌╌╌") && !is_top_rule("╰───╯"));
    assert!(is_yes_no("go on? ( Y / n ]") && !is_yes_no("y/n") && !is_yes_no("(yes/no)"));
    assert_eq!(option("  ❯ 12. Yes "), Some((2, true, "12".into(), "Yes ".into())));
    assert_eq!(option("> 1.  x"), Some((0, true, "1".into(), "x".into())));
    assert_eq!(option("123. x"), None);
    assert_eq!(option("1.x"), None);
    assert_eq!(option("1.   "), None);
}
