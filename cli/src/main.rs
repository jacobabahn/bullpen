//! bullpen's entry (bin/bullpen runs target/bullpen). It looks the command up in the
//! table (commands.rs): no command or `--help` prints the help, a name bullpen doesn't
//! have is one line, a Rust command runs here, and a Python one is handed over whole
//! (`exec` of bin/bullpen-py, never bin/bullpen, so it can't loop) before anything is
//! read or written.

#[allow(dead_code)] // what later commands read their arguments with
mod args;
mod commands;
mod coordinators;
mod help;
mod rex;
mod saves;
mod show;
mod task_line;

use bullpen_core::paths;
use commands::Owner;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;

/// Exec the Python bullpen with these arguments; returns only when that fails.
/// BULLPEN_RUST_STRICT=1 (cli/check/compare.sh) fails instead, so what it compares is Rust's.
fn hand_to_python(argv: &[OsString]) -> i32 {
    if std::env::var_os("BULLPEN_RUST_STRICT").is_some_and(|v| !v.is_empty()) {
        let words: Vec<_> = argv.iter().map(|a| a.to_string_lossy()).collect();
        eprintln!("bullpen-rust: not in Rust: {}", words.join(" "));
        return 3;
    }
    let py = paths::python_entry();
    let err = std::process::Command::new("python3")
        .arg("-S")
        .arg(&py)
        .args(argv)
        .exec();
    eprintln!("bullpen: couldn't run {}: {err}", py.display());
    1
}

/// Run a Rust command: its error is "bullpen: …", exit 1; what its saves set off runs after.
fn run(f: fn(&[String]) -> bullpen_core::fail::Res<i32>, argv: &[OsString]) -> i32 {
    let rest: Vec<String> = argv[1..].iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let code = match f(&rest) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("bullpen: {e}");
            1
        }
    };
    saves::run_later();
    code
}

fn main() {
    // die quietly on a closed pipe, as a shell tool does, rather than panic
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let mut argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(first) = argv.first().map(|a| a.to_string_lossy().into_owned()) else {
        print!("{}", help::top_help(help::term_width()));
        std::process::exit(2);
    };
    if first == "-h" || first == "--help" {
        print!("{}", help::top_help(help::term_width()));
        std::process::exit(0);
    }
    let Some(mut cmd) = commands::find(&first) else {
        eprintln!("{}", help::unknown_command(&first));
        std::process::exit(2);
    };
    if let Owner::Alias(of) = cmd.owner {
        cmd = commands::find(of).expect("an alias names a command");
        argv[0] = of.into();
    }
    let code = match cmd.owner {
        Owner::Rust(f) => run(f, &argv),
        _ => hand_to_python(&argv),
    };
    std::process::exit(code);
}
