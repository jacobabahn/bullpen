//! What the board shows, apart from how it draws it: tasks and config as bullpen
//! stores them, the groups, and the sentences and keys each task gets; the backlog's
//! items, the same.

mod backlog;
mod confirm;
mod config;
mod detail;
mod groups;
mod task;
mod text;
mod top;
#[cfg(test)]
pub(crate) mod testing;

pub use backlog::*;
pub use confirm::*;
pub use config::*;
pub use detail::*;
pub use groups::*;
pub use task::*;
pub use text::*;
pub use top::*;
