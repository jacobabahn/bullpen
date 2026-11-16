//! bullpen's entry (bin/bullpen runs target/bullpen). It looks the command up in the
//! table (commands.rs): no command or `--help` prints the help, a command runs, and a
//! name bullpen doesn't have is one line (an internal one first catches up with a newer
//! build: build::catch_up).

mod accept;
mod agents;
mod answer;
mod args;
mod auto_accept;
mod backlog;
mod board;
mod brief;
mod build;
mod check;
mod close;
mod commands;
mod config;
mod coordinators;
mod diff;
mod doctor;
mod events;
mod go;
mod guide;
mod help;
mod hooks;
mod jobs;
mod keep;
mod keys;
mod limit;
mod line_edit;
mod mission;
mod modes;
mod news;
mod notes;
mod notifications;
mod overlap;
mod panes;
mod pick;
mod prs;
mod push_remote;
mod ready;
mod repos;
mod review;
mod rex;
mod risk;
mod rules;
mod saves;
mod say;
mod screen;
mod show;
mod start;
mod stats;
mod stepper;
mod stops;
mod task_line;
mod tours;
mod triage;
mod up;
mod watcher;

use bullpen_core::paths;
use std::ffi::OsString;

/// Run a command: its error is "bullpen: …", exit 1; what its saves set off runs after.
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
    // bullpen's checkout, from this program's path while it still has one (`build` renames a new one over it)
    let _ = paths::home();
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(first) = argv.first().map(|a| a.to_string_lossy().into_owned()) else {
        print!("{}", help::top_help(help::term_width()));
        std::process::exit(2);
    };
    if first == "-h" || first == "--help" {
        print!("{}", help::top_help(help::term_width()));
        std::process::exit(0);
    }
    let Some(cmd) = commands::find(&first) else {
        // returns only when it can't run the call with a newer build
        match build::catch_up(&argv) {
            Ok(true) => eprintln!("{}", build::not_in_build(&first)),
            Ok(false) => eprintln!("{}", help::unknown_command(&first)),
            Err(e) => {
                eprintln!("bullpen: {e}");
                std::process::exit(1);
            }
        }
        std::process::exit(2);
    };
    std::process::exit(run(cmd.run, &argv));
}
