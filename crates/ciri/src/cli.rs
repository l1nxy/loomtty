#[derive(Debug)]
pub enum CliCommand {
    /// Create a new session with random name and connect.
    New,
    /// Connect to a session by name (create if not exists).
    Run {
        session_name: String,
    },
    /// Attach to an existing session (error if not exists).
    Attach {
        session_name: String,
    },
    List,
    Kill {
        session_name: String,
    },
    KillServer,
    Delete {
        session_name: String,
    },
    Help,
    /// IPC messaging commands for external scripting.
    Msg { subcommand: MsgSubcommand, json: bool },
}

#[derive(Debug)]
pub enum MsgSubcommand {
    SendKeys { session_name: String, pane_id: u64, keys: String },
    ListPanes { session_name: String },
    Info { session_name: String },
    FocusPane { session_name: String, pane_id: u64 },
    ClosePane { session_name: String, pane_id: u64 },
    CreatePane { session_name: String },
    GetLayout { session_name: String },
    RunCommand { session_name: String, command: String },
}

/// Reserved subcommand names that cannot be used as positional session names.
fn is_subcommand(arg: &str) -> bool {
    matches!(
        arg,
        "new"
            | "list"
            | "ls"
            | "kill"
            | "k"
            | "kill-server"
            | "ks"
            | "delete"
            | "rm"
            | "attach"
            | "a"
            | "help"
            | "msg"
    )
}

pub fn parse_args<I>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    let usage = usage();

    // Handle "msg" subcommand separately since it has variable-length arguments
    if args.first().map(|s| s.as_str()) == Some("msg") {
        return parse_msg_args(&args[1..]);
    }

    match args.as_slice() {
        [] => Ok(CliCommand::New),
        [cmd] if cmd == "new" => Ok(CliCommand::New),
        [cmd] if cmd == "list" || cmd == "ls" => Ok(CliCommand::List),
        [cmd] if cmd == "kill-server" || cmd == "ks" => Ok(CliCommand::KillServer),
        [cmd, name] if cmd == "kill" || cmd == "k" => Ok(CliCommand::Kill {
            session_name: name.clone(),
        }),
        [cmd, name] if cmd == "delete" || cmd == "rm" => Ok(CliCommand::Delete {
            session_name: name.clone(),
        }),
        [cmd, name] if cmd == "attach" || cmd == "a" => Ok(CliCommand::Attach {
            session_name: name.clone(),
        }),
        [cmd] if cmd == "attach" || cmd == "a" => {
            Err("attach requires a session name.\nUse `ciri ls` to list sessions.".to_string())
        }
        [flag, session_name] if flag == "--session" || flag == "-s" => Ok(CliCommand::Run {
            session_name: session_name.clone(),
        }),
        [arg] if is_help_flag(arg) => Ok(CliCommand::Help),
        [arg] if !is_subcommand(arg) => Ok(CliCommand::Run {
            session_name: arg.clone(),
        }),
        _ => Err(usage),
    }
}

fn parse_msg_args(args: &[String]) -> Result<CliCommand, String> {
    let msg_usage = msg_usage();

    if args.is_empty() {
        return Err(msg_usage);
    }

    // Check for --json flag anywhere in the args
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<&String> = args.iter().filter(|a| a.as_str() != "--json").collect();

    if args.is_empty() {
        return Err(msg_usage);
    }

    let subcmd = args[0].as_str();
    match subcmd {
        "send-keys" => {
            if args.len() < 4 {
                return Err(format!("Usage: ciri msg send-keys <session> <pane_id> <keys>\n\n{msg_usage}"));
            }
            let session_name = args[1].clone();
            let pane_id: u64 = args[2].parse().map_err(|_| format!("invalid pane_id: {}", args[2]))?;
            let keys = args[3].clone();
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::SendKeys { session_name, pane_id, keys },
                json,
            })
        }
        "list-panes" => {
            if args.len() < 2 {
                return Err(format!("Usage: ciri msg list-panes <session>\n\n{msg_usage}"));
            }
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::ListPanes { session_name: args[1].clone() },
                json,
            })
        }
        "info" => {
            if args.len() < 2 {
                return Err(format!("Usage: ciri msg info <session>\n\n{msg_usage}"));
            }
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::Info { session_name: args[1].clone() },
                json,
            })
        }
        "focus-pane" => {
            if args.len() < 3 {
                return Err(format!("Usage: ciri msg focus-pane <session> <pane_id>\n\n{msg_usage}"));
            }
            let pane_id: u64 = args[2].parse().map_err(|_| format!("invalid pane_id: {}", args[2]))?;
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::FocusPane { session_name: args[1].clone(), pane_id },
                json,
            })
        }
        "close-pane" => {
            if args.len() < 3 {
                return Err(format!("Usage: ciri msg close-pane <session> <pane_id>\n\n{msg_usage}"));
            }
            let pane_id: u64 = args[2].parse().map_err(|_| format!("invalid pane_id: {}", args[2]))?;
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::ClosePane { session_name: args[1].clone(), pane_id },
                json,
            })
        }
        "create-pane" => {
            if args.len() < 2 {
                return Err(format!("Usage: ciri msg create-pane <session>\n\n{msg_usage}"));
            }
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::CreatePane { session_name: args[1].clone() },
                json,
            })
        }
        "get-layout" => {
            if args.len() < 2 {
                return Err(format!("Usage: ciri msg get-layout <session>\n\n{msg_usage}"));
            }
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::GetLayout { session_name: args[1].clone() },
                json,
            })
        }
        "run-command" => {
            if args.len() < 3 {
                return Err(format!("Usage: ciri msg run-command <session> <command>\n\n{msg_usage}"));
            }
            Ok(CliCommand::Msg {
                subcommand: MsgSubcommand::RunCommand { session_name: args[1].clone(), command: args[2].clone() },
                json,
            })
        }
        _ => Err(format!("unknown msg subcommand: {subcmd}\n\n{msg_usage}")),
    }
}

fn msg_usage() -> String {
    "\
Usage: ciri msg <subcommand> [options]

Subcommands:
  send-keys <session> <pane_id> <keys>   Send keystrokes to a pane
  list-panes <session> [--json]           List all panes in a session
  info <session> [--json]                 Get session info
  focus-pane <session> <pane_id>          Focus a pane by ID
  close-pane <session> <pane_id>          Close a pane by ID
  create-pane <session> [--json]          Create a new pane
  get-layout <session> [--json]           Get the full layout state
  run-command <session> <command>          Run a command in a new pane"
        .to_string()
}

pub fn usage() -> String {
    "\
Usage:
  ciri                          Create new session and connect
  ciri new                      Create new session and connect
  ciri <name>                   Connect to session (create if needed)
  ciri attach|a <name>          Attach to existing session (must exist)
  ciri list|ls                  List all sessions
  ciri kill|k <name>            Kill a session
  ciri kill-server|ks           Kill the server
  ciri delete|rm <name>         Delete saved session
  ciri msg <subcommand>         IPC commands for scripting (see ciri msg --help)
  ciri --help|-h                Show this help"
        .to_string()
}

fn is_help_flag(arg: &str) -> bool {
    matches!(arg, "--help" | "-h" | "help")
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, parse_args};

    fn parse(args: &[&str]) -> CliCommand {
        parse_args(args.iter().map(|s| s.to_string())).expect("parse should succeed")
    }

    #[test]
    fn no_args_creates_new() {
        assert!(matches!(parse(&[]), CliCommand::New));
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
    fn attach_requires_name() {
        let err = parse_args(["attach"].into_iter().map(|s| s.to_string()));
        assert!(err.is_err());
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
    fn explicit_session_flag() {
        assert!(
            matches!(parse(&["--session", "qa"]), CliCommand::Run { session_name } if session_name == "qa")
        );
    }

    #[test]
    fn help_flag() {
        assert!(matches!(parse(&["--help"]), CliCommand::Help));
    }

    #[test]
    fn list_command() {
        assert!(matches!(parse(&["list"]), CliCommand::List));
        assert!(matches!(parse(&["ls"]), CliCommand::List));
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

    #[test]
    fn rejects_extra_arguments() {
        let err = parse_args(["attach", "foo", "bar"].into_iter().map(|s| s.to_string()))
            .expect_err("parse should fail");
        assert!(err.contains("Usage:"));
    }
}
