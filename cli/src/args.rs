//! A command's arguments, read by argparse's rules: long options by any unambiguous
//! prefix, `--opt value` and `--opt=value`, `--` before positionals, negative numbers as
//! values, the last of a repeated option winning, and `-h`/`--help`. A usage error is
//! argparse's two lines (`usage: …` and `bullpen CMD: error: …`) on stderr, exit 2, but
//! for unrecognized arguments, which argparse says from the top parser (`usage: bullpen
//! COMMAND [ARGS]`, `bullpen: error: …`). The help is in argparse's layout, though not
//! its every byte (the plan's hard spot 1).

use crate::help::{term_width, wrap};

/// argparse.SUPPRESS as an argument's help: it is read, but left out of the usage and the help.
pub const SUPPRESS: &str = "==SUPPRESS==";

/// How many values a positional takes, as argparse's nargs.
#[derive(Clone, Copy, PartialEq)]
pub enum Nargs {
    One,
    /// "?"
    Maybe,
    /// "*"
    Any,
    /// "+"
    Some,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// store_true (or store_false: `Args::last` says which of two came last)
    Flag,
    /// an option with a value, its metavar; repeated, every value is kept (append) and the last wins
    Value(&'static str),
    /// a positional, by its name
    Pos(Nargs),
}

/// One argument a command takes.
pub struct Arg {
    /// `--all`, or a positional's name (`tid`)
    pub name: &'static str,
    pub kind: Kind,
    pub help: &'static str,
    /// the values it may have (an option's or a positional's), else any
    pub choices: &'static [&'static str],
}

pub const fn flag(name: &'static str, help: &'static str) -> Arg {
    Arg {
        name,
        kind: Kind::Flag,
        help,
        choices: &[],
    }
}

pub const fn value(name: &'static str, metavar: &'static str, help: &'static str) -> Arg {
    Arg {
        name,
        kind: Kind::Value(metavar),
        help,
        choices: &[],
    }
}

pub const fn pos(name: &'static str, nargs: Nargs, help: &'static str) -> Arg {
    Arg {
        name,
        kind: Kind::Pos(nargs),
        help,
        choices: &[],
    }
}

impl Arg {
    pub const fn choices(self, choices: &'static [&'static str]) -> Arg {
        Arg { choices, ..self }
    }

    fn is_option(&self) -> bool {
        !matches!(self.kind, Kind::Pos(_))
    }

    fn shown(&self) -> bool {
        self.help != SUPPRESS
    }

    /// How argparse shows it in the usage line.
    fn usage(&self) -> String {
        match self.kind {
            Kind::Flag => format!("[{}]", self.name),
            Kind::Value(m) => format!("[{} {}]", self.name, self.metavar(m)),
            Kind::Pos(Nargs::One) => self.name.into(),
            Kind::Pos(Nargs::Maybe) => format!("[{}]", self.name),
            Kind::Pos(Nargs::Any) => format!("[{} ...]", self.name),
            Kind::Pos(Nargs::Some) => format!("{0} [{0} ...]", self.name),
        }
    }

    fn metavar(&self, m: &'static str) -> String {
        if self.choices.is_empty() {
            m.into()
        } else {
            format!("{{{}}}", self.choices.join(","))
        }
    }

    /// How argparse names it in the help's left column.
    fn invocation(&self) -> String {
        match self.kind {
            Kind::Flag => self.name.into(),
            Kind::Value(m) => format!("{} {}", self.name, self.metavar(m)),
            Kind::Pos(_) if !self.choices.is_empty() => format!("{{{}}}", self.choices.join(",")),
            Kind::Pos(_) => self.name.into(),
        }
    }
}

/// The arguments a command was given, read against its spec.
#[derive(Default)]
pub struct Args {
    flags: Vec<&'static str>,
    opts: Vec<(&'static str, String)>,
    pos: Vec<(&'static str, String)>,
}

impl Args {
    /// Read argv (after the command's name). A usage error or `--help` prints and exits,
    /// as argparse does.
    pub fn parse(cmd: &str, argv: &[String], spec: &[Arg]) -> Args {
        match read(argv, spec) {
            Ok(a) => a,
            Err(Stop::Help) => {
                print!("{}", help(cmd, "", spec, term_width()));
                std::process::exit(0)
            }
            Err(Stop::Usage(msg)) => usage_error(cmd, spec, &msg),
            Err(Stop::Unrecognized(words)) => {
                eprint!(
                    "{}bullpen: error: unrecognized arguments: {words}\n",
                    crate::help::USAGE
                );
                std::process::exit(2)
            }
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }

    /// Which of these flags came last (store_true and store_false on one dest), if any.
    pub fn last(&self, names: &[&str]) -> Option<&'static str> {
        self.flags.iter().rev().find(|f| names.contains(f)).copied()
    }

    /// The option's last value.
    pub fn opt(&self, name: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every value of a repeated option, in order (argparse's append).
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.opts
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// A positional's value (its first, for one with several).
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.pos.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    /// Every value of a positional with nargs * or +.
    pub fn args(&self, name: &str) -> Vec<&str> {
        self.pos
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// Why reading stopped.
#[derive(Debug, PartialEq)]
enum Stop {
    Help,
    Usage(String),
    /// Arguments no option or positional takes: argparse says so from the top parser.
    Unrecognized(String),
}

/// argparse's test for a negative number, which it reads as a value, not an option.
fn negative_number(a: &str) -> bool {
    let Some(b) = a.strip_prefix('-') else { return false };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    digits(b)
        || b.split_once('.')
            .is_some_and(|(i, f)| (i.is_empty() || digits(i)) && digits(f))
}

fn looks_like_option(a: &str) -> bool {
    a.starts_with('-') && a.len() > 1 && !negative_number(a)
}

/// The option a `--name` names: an exact match, else the one option it is a prefix of.
fn option<'a>(name: &str, spec: &'a [Arg]) -> Result<Option<&'a Arg>, Stop> {
    if name == "--help" || name == "-h" {
        return Err(Stop::Help);
    }
    let opts = || spec.iter().filter(|a| a.is_option());
    if let Some(a) = opts().find(|a| a.name == name) {
        return Ok(Some(a));
    }
    if !name.starts_with("--") {
        return Ok(None);
    }
    let mut hits: Vec<&str> = opts().map(|a| a.name).filter(|n| n.starts_with(name)).collect();
    if "--help".starts_with(name) {
        hits.insert(0, "--help");
    }
    match hits.as_slice() {
        [] => Ok(None),
        ["--help"] => Err(Stop::Help),
        [one] => Ok(opts().find(|a| a.name == *one)),
        many => Err(Stop::Usage(format!(
            "ambiguous option: {name} could match {}",
            many.join(", ")
        ))),
    }
}

fn check_choice(a: &Arg, v: &str) -> Result<(), Stop> {
    if a.choices.is_empty() || a.choices.contains(&v) {
        return Ok(());
    }
    Err(Stop::Usage(format!(
        "argument {}: invalid choice: '{v}' (choose from {})",
        a.name,
        a.choices.join(", ")
    )))
}

fn read(argv: &[String], spec: &[Arg]) -> Result<Args, Stop> {
    let mut a = Args::default();
    let mut positionals: Vec<(usize, &String)> = Vec::new();
    let mut extras: Vec<(usize, &str)> = Vec::new();
    let mut i = 0;
    let mut only_pos = false;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if only_pos || !looks_like_option(arg) {
            positionals.push((i - 1, arg));
            continue;
        }
        if arg == "--" {
            only_pos = true;
            continue;
        }
        let (name, given) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v)),
            _ => (arg.as_str(), None),
        };
        let Some(o) = option(name, spec)? else {
            extras.push((i - 1, arg));
            continue;
        };
        match o.kind {
            Kind::Flag => {
                if let Some(v) = given {
                    return Err(Stop::Usage(format!(
                        "argument {}: ignored explicit argument '{v}'",
                        o.name
                    )));
                }
                a.flags.push(o.name);
            }
            Kind::Value(_) => {
                let v = match given {
                    Some(v) => v.to_string(),
                    None => match argv.get(i) {
                        Some(v) if !looks_like_option(v) => {
                            i += 1;
                            v.clone()
                        }
                        _ => return Err(Stop::Usage(format!("argument {}: expected one argument", o.name))),
                    },
                };
                check_choice(o, &v)?;
                a.opts.push((o.name, v));
            }
            Kind::Pos(_) => unreachable!(),
        }
    }
    assign(&mut a, &positionals, spec, &mut extras)?;
    if !extras.is_empty() {
        extras.sort();
        let words: Vec<&str> = extras.iter().map(|(_, w)| *w).collect();
        return Err(Stop::Unrecognized(words.join(" ")));
    }
    Ok(a)
}

/// Give the positionals, in order, to the spec's positional arguments: each takes what
/// it needs, a `*` or `+` what the ones after it leave. What's left over is unrecognized.
fn assign<'a>(
    a: &mut Args,
    given: &[(usize, &'a String)],
    spec: &[Arg],
    extras: &mut Vec<(usize, &'a str)>,
) -> Result<(), Stop> {
    let wanted: Vec<&Arg> = spec.iter().filter(|s| !s.is_option()).collect();
    let need = |from: usize| -> usize {
        wanted[from..]
            .iter()
            .filter(|w| matches!(w.kind, Kind::Pos(Nargs::One | Nargs::Some)))
            .count()
    };
    let mut at = 0;
    let mut missing = Vec::new();
    for (k, w) in wanted.iter().enumerate() {
        let left = given.len() - at;
        let after = need(k + 1);
        let take = match w.kind {
            Kind::Pos(Nargs::One) => usize::from(left > 0),
            Kind::Pos(Nargs::Maybe) => usize::from(left > after),
            Kind::Pos(Nargs::Any) | Kind::Pos(Nargs::Some) => left.saturating_sub(after),
            _ => 0,
        };
        if take == 0 && matches!(w.kind, Kind::Pos(Nargs::One | Nargs::Some)) {
            missing.push(w.name);
        }
        for (_, v) in &given[at..at + take] {
            check_choice(w, v)?;
            a.pos.push((w.name, (*v).clone()));
        }
        at += take;
    }
    if !missing.is_empty() {
        return Err(Stop::Usage(format!(
            "the following arguments are required: {}",
            missing.join(", ")
        )));
    }
    extras.extend(given[at..].iter().map(|(i, s)| (*i, s.as_str())));
    Ok(())
}

/// argparse's usage for a command, wrapped as argparse wraps it for `width` columns: the
/// options on lines of their own, then the positionals, under the command's name.
pub fn usage(cmd: &str, spec: &[Arg], width: usize) -> String {
    let prog = format!("bullpen {cmd}");
    let mut opts = vec!["[-h]".to_string()];
    opts.extend(spec.iter().filter(|a| a.is_option() && a.shown()).map(Arg::usage));
    let poss: Vec<String> = spec.iter().filter(|a| !a.is_option()).map(Arg::usage).collect();
    let all: Vec<&String> = opts.iter().chain(poss.iter()).collect();
    let line = format!(
        "{prog} {}",
        all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
    );
    const PREFIX: &str = "usage: ";
    if PREFIX.len() + line.chars().count() <= width || (PREFIX.len() + prog.len()) as f64 > 0.75 * width as f64 {
        return format!("{PREFIX}{line}\n");
    }
    let indent = " ".repeat(PREFIX.len() + prog.len() + 1);
    let mut first = vec![prog.clone()];
    first.extend(opts);
    let mut lines = usage_lines(&first, &indent, PREFIX.len(), width);
    lines[0] = lines[0][indent.len()..].to_string();
    lines.extend(usage_lines(&poss, &indent, indent.len(), width));
    format!("{PREFIX}{}\n", lines.join("\n"))
}

/// argparse's get_lines: parts joined by spaces, a new line (indented) when one won't fit.
fn usage_lines(parts: &[String], indent: &str, start: usize, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line: Vec<&str> = Vec::new();
    let mut len = start as i64 - 1;
    for part in parts {
        let n = part.chars().count() as i64;
        if len + 1 + n > width as i64 && !line.is_empty() {
            lines.push(format!("{indent}{}", line.join(" ")));
            line.clear();
            len = indent.len() as i64 - 1;
        }
        line.push(part);
        len += n + 1;
    }
    if !line.is_empty() {
        lines.push(format!("{indent}{}", line.join(" ")));
    }
    lines
}

/// A usage error, as argparse prints it: exit 2.
pub fn usage_error(cmd: &str, spec: &[Arg], msg: &str) -> ! {
    let width = term_width().saturating_sub(2);
    eprint!("{}bullpen {cmd}: error: {msg}\n", usage(cmd, spec, width));
    std::process::exit(2)
}

/// A command's `--help`, in argparse's layout, for a terminal `cols` wide.
pub fn help(cmd: &str, about: &str, spec: &[Arg], cols: usize) -> String {
    let width = cols.saturating_sub(2).max(11);
    let mut out = usage(cmd, spec, width);
    if !about.is_empty() {
        out += &format!("\n{}\n", wrap(about, width, "", ""));
    }
    let help_arg = flag("-h, --help", "show this help message and exit");
    let opts: Vec<&Arg> = std::iter::once(&help_arg)
        .chain(spec.iter().filter(|a| a.is_option() && a.shown()))
        .collect();
    let poss: Vec<&Arg> = spec.iter().filter(|a| !a.is_option()).collect();
    let longest = opts
        .iter()
        .chain(poss.iter())
        .map(|a| a.invocation().chars().count() + 2)
        .max()
        .unwrap_or(0);
    let at = (longest + 2).min(24.min((width.saturating_sub(20)).max(4)));
    for (title, args) in [("positional arguments", &poss), ("options", &opts)] {
        if args.is_empty() {
            continue;
        }
        out += &format!("\n{title}:\n");
        for a in args.iter() {
            out += &entry(&a.invocation(), a.help, at, width);
        }
    }
    out
}

/// One argument in the help: its invocation, and its help from column `at`.
fn entry(invocation: &str, text: &str, at: usize, width: usize) -> String {
    let left = format!("  {invocation}");
    let indent = " ".repeat(at);
    if text.is_empty() {
        return left + "\n";
    }
    let room = width.saturating_sub(at).max(11);
    let lines = crate::help::wrap_lines(text, room);
    let mut out = if left.chars().count() + 2 <= at {
        format!("{left:<at$}{}\n", lines[0])
    } else {
        format!("{left}\n{indent}{}\n", lines[0])
    };
    for l in &lines[1..] {
        out += &format!("{indent}{l}\n");
    }
    out
}

#[cfg(test)]
#[path = "test_args.rs"]
mod tests;
