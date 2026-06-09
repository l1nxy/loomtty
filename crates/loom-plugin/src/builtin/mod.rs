//! Built-in plugins embedded into the binary.

pub(crate) const SESSION_RESTORE: &str = include_str!("session_restore.lua");
pub(crate) const USAGE: &str = include_str!("usage.lua");
