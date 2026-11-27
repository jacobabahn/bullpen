//! `bullpen web`: every coordinator and task bullpen runs, as a page (docs/web.md,
//! docs/prd/web.md). This is its command line, the Unix socket it serves on
//! (`<state>/web.sock`, 0600 in a folder only the person can open: never a TCP port),
//! and reloading itself when its program is rebuilt. `tailscale serve` is what reaches
//! it from the tailnet; it prints that line and never runs it.

mod auth;
mod events;
mod fleet;
mod routes;
mod watch;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// What it was started with.
struct Args {
    /// the state folder: every repo's tasks, and the socket
    state: PathBuf,
    /// the `bullpen` program, run for what only it can say (coordinators' liveness)
    bullpen: PathBuf,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let (mut state, mut bullpen) = (None, None);
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        let mut next = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--state" => state = Some(PathBuf::from(next()?)),
            "--bullpen" => bullpen = Some(PathBuf::from(next()?)),
            _ => return Err(format!("unknown argument {a}: it's run by `bullpen web`")),
        }
    }
    match (state, bullpen) {
        (Some(state), Some(bullpen)) => Ok(Args { state, bullpen }),
        _ => Err("--state and --bullpen are needed: it's run by `bullpen web`".into()),
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&argv) {
        eprintln!("bullpen web: {e}");
        std::process::exit(1);
    }
}

fn run(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    private_folder(&args.state)?;
    let gate = Arc::new(auth::Gate::new(&args.state)?);
    let sock = args.state.join("web.sock");
    let listener = std::os::unix::net::UnixListener::bind(free(&sock)?)
        .map_err(|e| format!("couldn't make the socket {}: {e}", sock.display()))?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    say_how_to_reach(&sock, &gate);
    let snapshot = fleet::start(&args.state, &args.bullpen);
    reload_on_rebuild(sock.clone());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let listener = tokio::net::UnixListener::from_std(listener).map_err(|e| e.to_string())?;
        let app = routes::app(gate, snapshot);
        axum::serve(listener, app).await.map_err(|e| e.to_string())
    })
}

/// The state folder, which holds the socket: made 0700 if it's new, and refused when
/// anyone but the person can open it (a socket there could be reached by them).
fn private_folder(state: &Path) -> Result<(), String> {
    if !state.exists() {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(state)
            .map_err(|e| format!("couldn't make {}: {e}", state.display()))?;
    }
    let mode = std::fs::metadata(state)
        .map_err(|e| e.to_string())?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 != 0 {
        return Err(format!(
            "{} can be opened by others (its mode is {mode:o}), and its socket would be too: run `chmod 700 {}` \
             (bullpen needs no one else in it), then `bullpen web` again",
            state.display(),
            state.display()
        ));
    }
    Ok(())
}

/// The socket's path, free to bind: a socket left by a server that's gone is removed;
/// one a server still answers on is refused.
fn free(sock: &Path) -> Result<&Path, String> {
    if std::fs::symlink_metadata(sock).is_err() {
        return Ok(sock);
    }
    if std::os::unix::net::UnixStream::connect(sock).is_ok() {
        return Err(format!("it's already running: {} answers", sock.display()));
    }
    std::fs::remove_file(sock).map_err(|e| format!("couldn't remove the old {}: {e}", sock.display()))?;
    Ok(sock)
}

/// Where it listens, and the line that reaches it from the tailnet (never run here).
fn say_how_to_reach(sock: &Path, gate: &auth::Gate) {
    println!(
        "bullpen web: serving on {} (only you can connect to it)",
        sock.display()
    );
    println!("To reach it from your tailnet, run this once (it stays until `tailscale serve reset`):");
    println!("  tailscale serve --bg unix:{}", sock.display());
    println!("then open https://{}/ (for {})", gate.host(), gate.users().join(", "));
}

/// Its program's file as it was started (`bullpen web` runs target/bullpen-web, which a
/// rebuild renames a new one onto): when that file changes, exec the new one with the
/// same arguments. Its socket goes with the old process; the new one makes it again,
/// and the page's event stream reconnects by itself.
fn reload_on_rebuild(sock: PathBuf) {
    let Some(exe) = std::env::args_os()
        .next()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    else {
        return;
    };
    let seen = |p: &Path| -> Option<(SystemTime, u64)> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(p).ok()?;
        Some((m.modified().ok()?, m.ino()))
    };
    let Some(was) = seen(&exe) else {
        return;
    };
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        match seen(&exe) {
            Some(now) if now != was => {
                use std::os::unix::process::CommandExt;
                eprintln!("bullpen web: its program was rebuilt: reloading");
                let _ = std::fs::remove_file(&sock);
                let err = std::process::Command::new(&exe)
                    .args(std::env::args_os().skip(1))
                    .exec();
                eprintln!("bullpen web: couldn't reload the rebuilt program: {err}");
                std::process::exit(1);
            }
            _ => {}
        }
    });
}
