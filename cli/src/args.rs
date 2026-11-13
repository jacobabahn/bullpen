//! Each command's arguments, read as argparse reads them for the common cases: long
//! options by any unambiguous prefix, `--opt value` and `--opt=value`, negative
//! numbers as values. Anything else (help, a usage error, an odd mix of positionals and
//! options) goes to the Python bullpen, whose argparse says it exactly.

use crate::core::{Fail, Res};

pub struct Spec {
    /// store_true options
    pub flags: &'static [&'static str],
    /// options with a value (repeatable: every value is kept)
    pub opts: &'static [&'static str],
    /// how many positionals, at least and at most
    pub min: usize,
    pub max: usize,
}

#[derive(Default)]
pub struct Args {
    flags: Vec<&'static str>,
    opts: Vec<(&'static str, String)>,
    pub pos: Vec<String>,
}

impl Args {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }

    /// The option's last value, as argparse keeps it.
    pub fn opt(&self, name: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    /// Every value of a repeatable option (argparse's append).
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.opts.iter().filter(|(n, _)| *n == name).map(|(_, v)| v.as_str()).collect()
    }

    /// The nth positional, if given.
    pub fn arg(&self, n: usize) -> Option<&str> {
        self.pos.get(n).map(String::as_str)
    }
}

/// argparse's test for a negative number, which it takes as a value, not an option.
fn negative_number(a: &str) -> bool {
    let b = &a[1..];
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    digits(b) || b.split_once('.').is_some_and(|(i, f)| (i.is_empty() || digits(i)) && digits(f))
}

/// The option an argument names: an exact match, else the one it is a prefix of.
fn option(name: &str, spec: &Spec) -> Option<&'static str> {
    let all = spec.flags.iter().chain(spec.opts.iter());
    if let Some(o) = all.clone().find(|o| **o == name) {
        return Some(o);
    }
    let mut hits = all.filter(|o| o.starts_with(name));
    match (hits.next(), hits.next()) {
        (Some(o), None) => Some(o),
        _ => None,
    }
}

/// argv after the command's name. Err(Fail::Python) for whatever the port leaves to argparse.
pub fn parse(argv: &[String], spec: &Spec) -> Res<Args> {
    let mut a = Args::default();
    let mut runs = 0; // runs of positionals: argparse matches a later one its own way
    let mut last_pos = false;
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if arg.starts_with('-') && arg.len() > 1 && !negative_number(arg) {
            last_pos = false;
            if !arg.starts_with("--") || arg == "--" {
                return Err(Fail::Python); // -h, short options, --
            }
            let (name, value) = match arg.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (arg.as_str(), None),
            };
            let o = option(name, spec).ok_or(Fail::Python)?;
            if spec.flags.contains(&o) {
                if value.is_some() {
                    return Err(Fail::Python);
                }
                a.flags.push(o);
            } else {
                let v = match value {
                    Some(v) => v,
                    None => {
                        let v = argv.get(i).ok_or(Fail::Python)?;
                        if v.starts_with('-') && v.len() > 1 && !negative_number(v) {
                            return Err(Fail::Python);
                        }
                        i += 1;
                        v.clone()
                    }
                };
                a.opts.push((o, v));
            }
        } else {
            if !last_pos {
                runs += 1;
            }
            last_pos = true;
            a.pos.push(arg.clone());
        }
    }
    if runs > 1 || a.pos.len() < spec.min || a.pos.len() > spec.max {
        return Err(Fail::Python);
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: Spec = Spec { flags: &["--all", "--all-repos", "--vt"], opts: &["--lines", "--after"], min: 0, max: 2 };

    fn p(args: &[&str]) -> Res<Args> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &SPEC)
    }

    #[test]
    fn as_argparse() {
        let a = p(&["BP-3", "--lines", "-28", "--all-r", "--after=x", "--after", "y"]).unwrap();
        assert_eq!(a.pos, vec!["BP-3"]);
        assert_eq!(a.opt("--lines"), Some("-28"));
        assert!(a.flag("--all-repos") && !a.flag("--all"));
        assert_eq!(a.all("--after"), vec!["x", "y"]);
        assert!(p(&["--all"]).unwrap().flag("--all"));
        for bad in [&["-h"][..], &["--al"], &["--lines"], &["--vt=1"], &["a", "--vt", "b"], &["a", "b", "c"], &["--nope"]] {
            assert!(matches!(p(bad), Err(Fail::Python)), "{bad:?}");
        }
    }
}
