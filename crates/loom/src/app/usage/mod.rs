//! Background OAuth-driven usage probes for Claude Code and Codex CLI.
//!
//! See `crates/loom/src/app/usage/poller.rs` for the polling loop, and
//! `snapshot.rs` for the shared data type rendered by the status bar.

mod claude;
mod codex;
pub(crate) mod poller;
pub(crate) mod snapshot;
mod tokens;

pub(crate) use poller::{spawn, PollerConfig};
pub(crate) use snapshot::{SharedSnapshot, UsageSnapshot};
