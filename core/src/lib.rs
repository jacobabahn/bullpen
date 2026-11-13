//! bullpen-core: what bullpen's command line and its board share. The Rust twin of
//! bullpen/core.py: paths, the settings table, a repo's config and tasks, a task's
//! status, and Python's ways with JSON, text, time and programs, so both write the same.

pub mod config;
pub mod fail;
pub mod json;
pub mod lock;
pub mod paths;
pub mod proc;
pub mod repo;
pub mod settings;
pub mod sha256;
pub mod task;
pub mod text;
pub mod timeutil;
#[rustfmt::skip]
pub mod widths;

#[cfg(test)]
mod testenv;

pub use fail::{fail, Fail, Res};
pub use json::Obj;
pub use repo::Repo;
