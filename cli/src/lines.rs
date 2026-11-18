//! Lines written whole: each in one write. std's stderr is unbuffered, and so is a File:
//! eprintln! and writeln! write each piece of their format apart ("bullpen: `", the
//! name, the rest), so in a log several processes append to at once (jobs.log: a job's
//! output, another job's, their supervisors' lines) one line could land in the middle of
//! another (BP-198). This module comes first in main.rs: its eprint!, eprintln!, print! and
//! println! are the ones the whole crate uses, and a line to a log is written with line_to!.

/// eprint!, its text formatted first and written in one go.
macro_rules! eprint {
    ($($arg:tt)*) => {
        std::eprint!("{}", format!($($arg)*))
    };
}

/// print!, the same.
macro_rules! print {
    ($($arg:tt)*) => {
        std::print!("{}", format!($($arg)*))
    };
}

/// eprintln!, its line formatted first and written in one go.
macro_rules! eprintln {
    () => {
        std::eprint!("\n")
    };
    ($($arg:tt)*) => {
        std::eprint!("{}", format!($($arg)*) + "\n")
    };
}

/// println!, the same (stdout's line buffer would split a line longer than itself).
macro_rules! println {
    () => {
        std::print!("\n")
    };
    ($($arg:tt)*) => {
        std::print!("{}", format!($($arg)*) + "\n")
    };
}

/// writeln!, the line in one write: `line_to!(file, "{} {}", a, b)` is an io::Result.
macro_rules! line_to {
    ($w:expr, $($arg:tt)*) => {
        std::io::Write::write_all(&mut $w, (format!($($arg)*) + "\n").as_bytes())
    };
}

#[cfg(test)]
#[path = "tests/test_lines.rs"]
mod tests;
