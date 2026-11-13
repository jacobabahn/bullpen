//! bullpen-core: what bullpen's command line and its board share: paths, the settings
//! table, a repo's config and tasks, a task's status, and the Python's ways with JSON,
//! text, time and programs (bullpen was Python first), so the files it writes stay the same.

pub mod config;
pub mod fail;
pub mod json;
pub mod lock;
pub mod paths;
pub mod printable;
pub mod proc;
pub mod repo;
pub mod settings;
pub mod sha1;
pub mod sha256;
pub mod task;
pub mod text;
pub mod timeutil;
pub mod words;

#[cfg(test)]
mod testenv;

pub use fail::{fail, Fail, Res};
pub use json::Obj;
pub use repo::Repo;
