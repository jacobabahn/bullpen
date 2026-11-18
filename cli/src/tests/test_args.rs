use super::*;

const SPEC: &[Arg] = &[
    flag("--all", "every task"),
    flag("--all-repos", "every repository"),
    value("--lines", "N", "how many"),
    value("--after", "TID", "what it waits on"),
    value("--review", "MODE", "how").choices(&["full", "light", "auto"]),
    pos("tid", Nargs::One, "the task"),
    pos("more", Nargs::Any, "more tasks"),
];

fn p(args: &[&str]) -> Result<Args, Stop> {
    read(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), SPEC)
}

fn err(args: &[&str]) -> String {
    match p(args) {
        Err(Stop::Usage(m)) => m,
        Err(Stop::Unrecognized(w)) => format!("unrecognized arguments: {w}"),
        Err(Stop::Help) => "help".into(),
        Ok(_) => "ok".into(),
    }
}

#[test]
fn prefixes_values_and_the_last_wins() {
    let a = p(&[
        "BP-3",
        "--lines",
        "-28",
        "--all-r",
        "--after=x",
        "--after",
        "y",
        "BP-4",
        "--lines=5",
    ])
    .unwrap();
    assert_eq!(a.arg("tid"), Some("BP-3"));
    assert_eq!(a.args("more"), ["BP-4"]);
    assert_eq!(a.opt("--lines"), Some("5"));
    assert!(a.flag("--all-repos") && !a.flag("--all"));
    assert_eq!(a.all("--after"), ["x", "y"]);
    assert_eq!(a.last(&["--all", "--all-repos"]), Some("--all-repos"));
    let a = p(&["--", "--all", "-x"]).unwrap();
    assert_eq!((a.arg("tid"), a.args("more")), (Some("--all"), vec!["-x"]));
}

#[test]
fn usage_errors_in_argparses_words() {
    assert_eq!(
        err(&["--al", "BP-1"]),
        "ambiguous option: --al could match --all, --all-repos"
    );
    assert_eq!(err(&["BP-1", "--lines"]), "argument --lines: expected one argument");
    assert_eq!(
        err(&["BP-1", "--lines", "--all"]),
        "argument --lines: expected one argument"
    );
    assert_eq!(
        err(&["BP-1", "--all=1"]),
        "argument --all: ignored explicit argument '1'"
    );
    assert_eq!(err(&["--all"]), "the following arguments are required: tid");
    assert_eq!(err(&["BP-1", "--nope", "u", "-q"]), "unrecognized arguments: --nope -q");
    let one: Vec<String> = ["BP-1", "--nope", "u", "-q"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        read(&one, &SPEC[..6]).err(),
        Some(Stop::Unrecognized("--nope u -q".into()))
    );
    assert_eq!(
        err(&["BP-1", "--review", "sometimes"]),
        "argument --review: invalid choice: 'sometimes' (choose from full, light, auto)"
    );
    assert_eq!(err(&["-h"]), "help");
    assert_eq!(err(&["BP-1", "--he"]), "help");
}

#[test]
fn usage_and_help_in_argparses_layout() {
    assert_eq!(
        usage("x", SPEC, 200),
        "usage: bullpen x [-h] [--all] [--all-repos] [--lines N] [--after TID] [--review {full,light,auto}] tid [more ...]\n"
    );
    // as argparse wraps it at 60 columns
    assert_eq!(
        usage("x", SPEC, 60),
        "usage: bullpen x [-h] [--all] [--all-repos] [--lines N]\n                 [--after TID] [--review {full,light,auto}]\n                 tid [more ...]\n"
    );
    let h = help("x", "", &SPEC[..3], 80);
    assert!(
        h.contains("\noptions:\n  -h, --help   show this help message and exit\n  --all        every task\n"),
        "{h}"
    );
}
