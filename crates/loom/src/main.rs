//! `loomtty` — the CLI entry point (console subsystem on Windows).
//!
//! Keeping this binary on the console subsystem means CLI subcommands print
//! synchronously and the shell waits for them — required for `loomtty init`
//! (interactive wizard) and `loomtty web` (foreground server). The GUI is
//! launched from the separate `loomtty-gui` binary; see `lib.rs`.

use anyhow::Result;
use clap::Parser;
use loomtty::cli::{self, Cli};

fn main() -> Result<()> {
    loomtty::init_logging();
    let cli = cli::resolve(Cli::parse());
    loomtty::run(cli)
}
