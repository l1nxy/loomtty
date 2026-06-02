use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Parser, Subcommand};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default())
    .valid(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD));

#[derive(Parser, Debug)]
#[command(name = "ciritty", about = "GPU-accelerated terminal multiplexer")]
#[command(arg_required_else_help = false, styles = STYLES)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Connect to session by name (create if needed)
    #[arg(value_name = "SESSION")]
    pub session_name: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create a new session and connect
    New,

    /// Interactive setup wizard — generate config file
    Init,

    /// Attach to an existing session (error if not exists)
    #[command(visible_alias = "a")]
    Attach {
        /// Session name
        session_name: String,
    },

    /// List sessions
    #[command(visible_alias = "ls")]
    List {
        /// Include saved (inactive) sessions
        #[arg(short, long)]
        all: bool,
    },

    /// Kill a session
    #[command(visible_alias = "k")]
    Kill {
        /// Session name
        session_name: String,
    },

    /// Kill the server
    #[command(visible_alias = "ks")]
    KillServer,

    /// Delete saved session
    #[command(visible_alias = "rm")]
    Delete {
        /// Session name
        session_name: String,
    },

    /// Connect to remote server via SSH tunnel
    Remote {
        /// Host (user@host)
        host: String,

        /// Session name
        session_name: Option<String>,

        /// Remote TCP port
        #[arg(long, default_value_t = ciri_protocol::transport::DEFAULT_REMOTE_PORT)]
        port: u16,

        /// SSH port
        #[arg(long, default_value_t = 22)]
        ssh_port: u16,
    },

    /// IPC commands for scripting
    Msg {
        #[command(subcommand)]
        subcommand: MsgCommand,

        /// Output as JSON
        #[arg(long, global = true)]
        json: bool,
    },

    /// Layout template management
    #[command(visible_alias = "tpl")]
    Template {
        #[command(subcommand)]
        subcommand: TemplateCommand,
    },

    /// Serve the browser web UI (HTTP + WebSocket) and run the server in
    /// the foreground. Enables `[web]` and mints a token on first run,
    /// persisting both to your config so the desktop app shares them.
    Web {
        /// Listen port (default: web.port from config, else 7891)
        #[arg(long)]
        port: Option<u16>,
        /// Bind address (default: 127.0.0.1). Use 0.0.0.0 to expose on
        /// the LAN — then also set web.allowed_origins in your config.
        #[arg(long)]
        bind: Option<String>,
        /// Auth token. Default: reuse the configured one, or generate a
        /// fresh 32-hex-char token. Either way it's saved to your config.
        /// NOTE: a value passed here is visible to other users in process
        /// listings — on a shared host prefer the CIRITTY_WEB_TOKEN env var.
        #[arg(long)]
        token: Option<String>,
        /// Directory of the built web bundle (default: <server-dir>/web)
        #[arg(long)]
        static_dir: Option<String>,
        /// Open the URL in your default browser after starting
        #[arg(long)]
        open: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum MsgCommand {
    /// Send keystrokes to a pane
    SendKeys {
        session_name: String,
        pane_id: u64,
        keys: String,
    },
    /// List all panes in a session
    ListPanes { session_name: String },
    /// Get session info
    Info { session_name: String },
    /// Focus a pane by ID
    FocusPane { session_name: String, pane_id: u64 },
    /// Close a pane by ID
    ClosePane { session_name: String, pane_id: u64 },
    /// Create a new pane
    CreatePane { session_name: String },
    /// Get the full layout state
    GetLayout { session_name: String },
    /// Run a command in a new pane
    RunCommand {
        session_name: String,
        command: String,
    },
    /// Capture a pane's active grid (and optional scrollback) as text to stdout.
    ///
    /// Note: when an alt-screen TUI is foregrounded (vim/less/htop), the alt
    /// buffer is captured and `--scrollback-rows` has no effect (the primary
    /// buffer's history is not reachable). Only ASCII spaces are trimmed
    /// from row tails by default; pass `--preserve-trailing-spaces` to keep them.
    CapturePane {
        session_name: String,
        pane_id: u64,
        /// Include this many rows of scrollback above the viewport (0 = viewport only).
        /// Clamped to: the pane's actual history, an absolute row cap (100k),
        /// and a byte budget (~900 KiB) to fit a single control frame. Wide panes
        /// with deep scrollback get fewer rows than requested.
        /// Intentionally an unsigned count, NOT the signed `start-line` semantics
        /// of `tmux capture-pane -S`.
        #[arg(long, default_value_t = 0)]
        scrollback_rows: u32,
        /// Join soft-wrapped lines (omit the newline between rows that the
        /// terminal soft-wrapped). Equivalent to tmux's `capture-pane -J`:
        /// trailing spaces on wrapping rows are kept (so content connects
        /// correctly across the join), while non-wrap rows still get trimmed.
        #[arg(long)]
        join_wrapped: bool,
        /// Keep trailing ASCII-space cells on each row (default trims them).
        /// Only ASCII U+0020 is trimmed; tabs, NBSP, U+3000 and other
        /// non-ASCII whitespace are preserved either way.
        #[arg(long)]
        preserve_trailing_spaces: bool,
    },
    /// List recorded OSC 133 prompt boundaries (shell-integration history).
    ///
    /// Each entry reports the absolute line of OSC 133;A (prompt start),
    /// OSC 133;C (output start), and OSC 133;D (done), plus exit code and
    /// duration. Empty when the pane has never observed OSC 133 — that's
    /// the signal that shell integration isn't active in this pane.
    ListPrompts {
        session_name: String,
        pane_id: u64,
    },
}

#[derive(Subcommand, Debug)]
pub enum TemplateCommand {
    /// List layout templates
    #[command(visible_alias = "ls")]
    List,
    /// Apply a template
    Apply {
        template_name: String,
        session_name: Option<String>,
    },
    /// Save session layout as template
    Save {
        template_name: String,
        session_name: String,
    },
}

/// Resolve the CLI into the internal command enum used by main.rs.
pub fn resolve(cli: Cli) -> CliCommand {
    match cli.command {
        None => {
            if let Some(name) = cli.session_name {
                CliCommand::Run { session_name: name }
            } else {
                CliCommand::Default
            }
        }
        Some(Command::New) => CliCommand::New,
        Some(Command::Init) => CliCommand::Init,
        Some(Command::Attach { session_name }) => CliCommand::Attach { session_name },
        Some(Command::List { all }) => CliCommand::List { all },
        Some(Command::Kill { session_name }) => CliCommand::Kill { session_name },
        Some(Command::KillServer) => CliCommand::KillServer,
        Some(Command::Delete { session_name }) => CliCommand::Delete { session_name },
        Some(Command::Remote {
            host,
            session_name,
            port,
            ssh_port,
        }) => CliCommand::Remote {
            host,
            session_name,
            port,
            ssh_port,
        },
        Some(Command::Msg { subcommand, json }) => {
            let sub = match subcommand {
                MsgCommand::SendKeys {
                    session_name,
                    pane_id,
                    keys,
                } => MsgSubcommand::SendKeys {
                    session_name,
                    pane_id,
                    keys,
                },
                MsgCommand::ListPanes { session_name } => MsgSubcommand::ListPanes { session_name },
                MsgCommand::Info { session_name } => MsgSubcommand::Info { session_name },
                MsgCommand::FocusPane {
                    session_name,
                    pane_id,
                } => MsgSubcommand::FocusPane {
                    session_name,
                    pane_id,
                },
                MsgCommand::ClosePane {
                    session_name,
                    pane_id,
                } => MsgSubcommand::ClosePane {
                    session_name,
                    pane_id,
                },
                MsgCommand::CreatePane { session_name } => {
                    MsgSubcommand::CreatePane { session_name }
                }
                MsgCommand::GetLayout { session_name } => MsgSubcommand::GetLayout { session_name },
                MsgCommand::RunCommand {
                    session_name,
                    command,
                } => MsgSubcommand::RunCommand {
                    session_name,
                    command,
                },
                MsgCommand::CapturePane {
                    session_name,
                    pane_id,
                    scrollback_rows,
                    join_wrapped,
                    preserve_trailing_spaces,
                } => MsgSubcommand::CapturePane {
                    session_name,
                    pane_id,
                    scrollback_rows,
                    join_wrapped,
                    preserve_trailing_spaces,
                },
                MsgCommand::ListPrompts {
                    session_name,
                    pane_id,
                } => MsgSubcommand::ListPrompts {
                    session_name,
                    pane_id,
                },
            };
            CliCommand::Msg {
                subcommand: sub,
                json,
            }
        }
        Some(Command::Template { subcommand }) => {
            let sub = match subcommand {
                TemplateCommand::List => TemplateSubcommand::List,
                TemplateCommand::Apply {
                    template_name,
                    session_name,
                } => TemplateSubcommand::Apply {
                    template_name,
                    session_name,
                },
                TemplateCommand::Save {
                    template_name,
                    session_name,
                } => TemplateSubcommand::Save {
                    template_name,
                    session_name,
                },
            };
            CliCommand::Template { subcommand: sub }
        }
        Some(Command::Web {
            port,
            bind,
            token,
            static_dir,
            open,
        }) => CliCommand::Web {
            port,
            bind,
            token,
            static_dir,
            open,
        },
    }
}

// Internal command types (unchanged interface for main.rs)

#[derive(Debug)]
pub enum CliCommand {
    /// No subcommand given: auto-attach to existing session or create new.
    Default,
    New,
    Init,
    Run {
        session_name: String,
    },
    Attach {
        session_name: String,
    },
    List {
        all: bool,
    },
    Kill {
        session_name: String,
    },
    KillServer,
    Delete {
        session_name: String,
    },
    Remote {
        host: String,
        session_name: Option<String>,
        port: u16,
        ssh_port: u16,
    },
    Msg {
        subcommand: MsgSubcommand,
        json: bool,
    },
    Template {
        subcommand: TemplateSubcommand,
    },
    Web {
        port: Option<u16>,
        bind: Option<String>,
        token: Option<String>,
        static_dir: Option<String>,
        open: bool,
    },
}

#[derive(Debug)]
pub enum MsgSubcommand {
    SendKeys {
        session_name: String,
        pane_id: u64,
        keys: String,
    },
    ListPanes {
        session_name: String,
    },
    Info {
        session_name: String,
    },
    FocusPane {
        session_name: String,
        pane_id: u64,
    },
    ClosePane {
        session_name: String,
        pane_id: u64,
    },
    CreatePane {
        session_name: String,
    },
    GetLayout {
        session_name: String,
    },
    RunCommand {
        session_name: String,
        command: String,
    },
    CapturePane {
        session_name: String,
        pane_id: u64,
        scrollback_rows: u32,
        join_wrapped: bool,
        preserve_trailing_spaces: bool,
    },
    ListPrompts {
        session_name: String,
        pane_id: u64,
    },
}

#[derive(Debug)]
pub enum TemplateSubcommand {
    List,
    Apply {
        template_name: String,
        session_name: Option<String>,
    },
    Save {
        template_name: String,
        session_name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> CliCommand {
        let mut full_args = vec!["ciritty"];
        full_args.extend_from_slice(args);
        let cli = Cli::try_parse_from(full_args).expect("parse should succeed");
        resolve(cli)
    }

    #[test]
    fn no_args_creates_default() {
        assert!(matches!(parse(&[]), CliCommand::Default));
    }

    #[test]
    fn new_command() {
        assert!(matches!(parse(&["new"]), CliCommand::New));
    }

    #[test]
    fn positional_session_name() {
        assert!(
            matches!(parse(&["work"]), CliCommand::Run { session_name } if session_name == "work")
        );
    }

    #[test]
    fn attach_with_name() {
        assert!(
            matches!(parse(&["attach", "ops"]), CliCommand::Attach { session_name } if session_name == "ops")
        );
        assert!(
            matches!(parse(&["a", "ops"]), CliCommand::Attach { session_name } if session_name == "ops")
        );
    }

    #[test]
    fn attach_rejects_positional_session_name() {
        let err = Cli::try_parse_from(["ciritty", "attach"])
            .expect_err("attach without a session name should fail");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn no_args_help_flag_stays_on_safe_help_path() {
        let cli = Cli::try_parse_from(["ciritty", "--help"])
            .expect_err("clap should exit after rendering help");
        assert_eq!(cli.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn positional_session_name_then_help_stays_on_safe_help_path() {
        let cli = Cli::try_parse_from(["ciritty", "work", "--help"])
            .expect_err("clap should exit after rendering help");
        assert_eq!(cli.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn list_command() {
        assert!(matches!(parse(&["list"]), CliCommand::List { all: false }));
        assert!(matches!(parse(&["ls"]), CliCommand::List { all: false }));
        assert!(matches!(
            parse(&["ls", "--all"]),
            CliCommand::List { all: true }
        ));
        assert!(matches!(
            parse(&["ls", "-a"]),
            CliCommand::List { all: true }
        ));
    }

    #[test]
    fn kill_command() {
        assert!(
            matches!(parse(&["kill", "dev"]), CliCommand::Kill { session_name } if session_name == "dev")
        );
        assert!(
            matches!(parse(&["k", "dev"]), CliCommand::Kill { session_name } if session_name == "dev")
        );
    }

    #[test]
    fn kill_server_command() {
        assert!(matches!(parse(&["kill-server"]), CliCommand::KillServer));
        assert!(matches!(parse(&["ks"]), CliCommand::KillServer));
    }

    #[test]
    fn delete_command() {
        assert!(
            matches!(parse(&["delete", "old"]), CliCommand::Delete { session_name } if session_name == "old")
        );
        assert!(
            matches!(parse(&["rm", "old"]), CliCommand::Delete { session_name } if session_name == "old")
        );
    }
}
