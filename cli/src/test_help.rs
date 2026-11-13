use super::*;

const NAMES: [&str; 16] = [
    "accept", "close", "drop", "answer", "add", "after", "ask", "build", "doctor", "diff", "go", "show", "ls", "start",
    "review", "verdict",
];

#[test]
fn textwrap_as_python() {
    // each expected value is Python's textwrap.wrap(text, width, initial_indent, subsequent_indent)
    assert_eq!(
        wrap_with("build bullpen and the board now and put them in place (install.sh runs it)", 30, "  build  ", "         "),
        ["  build  build bullpen and the", "         board now and put", "         them in place", "         (install.sh runs it)"]
    );
    assert_eq!(
        wrap_with("a well-known self-contained thing--really, x--y", 12, "", ""),
        ["a well-known", "self-", "contained", "thing--", "really, x--y"]
    );
    assert_eq!(wrap_with("abcdefghij-klmnop qrs", 7, "", ""), ["abcdefg", "hij-", "klmnop", "qrs"]);
    assert_eq!(
        wrap_with(
            "merge a ready task's PR, or its local branch without a remote (config --merge), and clean up",
            40,
            "  accept        ",
            "                "
        ),
        [
            "  accept        merge a ready task's PR,",
            "                or its local branch",
            "                without a remote (config",
            "                --merge), and clean up"
        ]
    );
    assert_eq!(
        wrap_with("supercalifragilistic", 5, "> ", "  "),
        ["> sup", "  erc", "  ali", "  fra", "  gil", "  ist", "  ic"]
    );
    assert_eq!(wrap_with("tab\there and\tthere", 10, "", ""), ["tab", "here and", "there"]);
    assert_eq!(wrap_with("one-two-three-four-five", 9, "", ""), ["one-two-", "three-", "four-five"]);
    assert!(wrap_with("   ", 10, "", "").is_empty());
}

#[test]
fn close_matches_as_difflib() {
    // each expected value is Python's difflib.get_close_matches(word, NAMES, n=1)
    assert_eq!(close_match("acept", NAMES.iter().copied()), Some("accept"));
    assert_eq!(close_match("shw", NAMES.iter().copied()), Some("show"));
    assert_eq!(close_match("bild", NAMES.iter().copied()), Some("build"));
    assert_eq!(close_match("frobnicate", NAMES.iter().copied()), None);
    assert_eq!(close_match("stat", NAMES.iter().copied()), Some("start"));
    assert_eq!(close_match("revew", NAMES.iter().copied()), Some("review"));
    assert_eq!(close_match("ad", NAMES.iter().copied()), Some("add"));
    assert_eq!(close_match("dif", NAMES.iter().copied()), Some("diff"));
    assert_eq!(close_match("l", NAMES.iter().copied()), Some("ls"));
    assert_eq!(close_match("go2", NAMES.iter().copied()), Some("go"));
    assert_eq!(close_match("aftr", NAMES.iter().copied()), Some("after"));
}

#[test]
fn an_unknown_command_never_offers_an_internal_one_or_an_alias() {
    assert_eq!(unknown_command("_evnt"), "bullpen: no command `_evnt`. `bullpen --help` lists them.");
    assert_eq!(unknown_command("build-bord"), "bullpen: no command `build-bord`. Did you mean `build`? `bullpen --help` lists them.");
    assert_eq!(unknown_command("--bogus"), "bullpen: `--bogus` goes after a command. `bullpen --help` lists them.");
}

#[test]
fn the_help_fits_its_width_and_lists_every_group() {
    let out = top_help(42);
    assert!(out.starts_with(USAGE));
    assert!(out.lines().all(|l| l.chars().count() <= 40), "{out}");
    for (title, _) in GROUPS {
        assert!(out.contains(&format!("\n{title}:\n")));
    }
    assert!(!out.contains("build-board") && !out.contains("_nop"));
}
