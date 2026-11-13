//! A command's error (core.Fail): what main prints as "bullpen: …" before it exits 1.

use std::fmt;

/// Why a command stopped, in words for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fail(pub String);

pub type Res<T> = Result<T, Fail>;

/// Stop with msg.
pub fn fail<T>(msg: impl Into<String>) -> Res<T> {
    Err(Fail(msg.into()))
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Fail {}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Fail {
        Fail(e.to_string())
    }
}

impl From<String> for Fail {
    fn from(msg: String) -> Fail {
        Fail(msg)
    }
}

impl From<&str> for Fail {
    fn from(msg: &str) -> Fail {
        Fail(msg.into())
    }
}

#[cfg(test)]
#[path = "test_fail.rs"]
mod tests;
