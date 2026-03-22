#[derive(Debug)]
pub enum CliCommand {
    /// Create a new session with random name and connect.
    New,
    /// Connect to a session by name (create if not exists).
    Run { session_name: String },
    /// Attach to an existing session (error if not exists).
    Attach { session_name: String },
    List,
    Kill { session_name: String },
    KillServer,
    Delete { session_name: String },
    /// Connect to a remote ciri-server via SSH tunnel.
    Remote {
        host: String,
        session_name: Option<String>,
        port: u16,
        ssh_port: u16,
    },
    Help,
}

/// Reserved subcommand names that cannot be used as positional session names.
fn is_subcommand(arg: &str) -> bool {
    matches!(arg, "new" | "list" | "ls" | "kill" | "k" | "kill-server" | "ks" | "delete" | "rm" | "attach" | "a" | "remote" | "help")
}

pub fn parse_args<I>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    let usage = usage();

    // Handle "remote" subcommand with flags
    if args.first().map(|s| s.as_str()) == Some("remote") {
        return parse_remote_args(&args[1..]);
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
        [cmd] if cmd == "attach" || cmd == "a" => Err("attach requires a session name.\nUse `ciri ls` to list sessions.".to_string()),
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

/// Parse arguments for `ciri remote <user@host> [session_name] [--port PORT] [--ssh-port PORT]`.
fn parse_remote_args(args: &[String]) -> Result<CliCommand, String> {
    if args.is_empty() {
        return Err("remote requires a host argument.\nUsage: ciri remote <user@host> [session] [--port PORT] [--ssh-port PORT]".to_string());
    }

    let mut host: Option<String> = None;
    let mut session_name: Option<String> = None;
    let mut port: u16 = ciri_protocol::transport::DEFAULT_REMOTE_PORT;
    let mut ssh_port: u16 = 22;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--port" {
            i += 1;
            if i >= args.len() {
                return Err("--port requires a value".to_string());
            }
            port = args[i].parse::<u16>().map_err(|_| format!("invalid port: {}", args[i]))?;
        } else if arg == "--ssh-port" {
            i += 1;
            if i >= args.len() {
                return Err("--ssh-port requires a value".to_string());
            }
            ssh_port = args[i].parse::<u16>().map_err(|_| format!("invalid ssh-port: {}", args[i]))?;
        } else if arg.starts_with('-') {
            return Err(format!("unknown flag: {arg}"));
        } else if host.is_none() {
            host = Some(arg.clone());
        } else if session_name.is_none() {
            session_name = Some(arg.clone());
        } else {
            return Err(format!("unexpected argument: {arg}"));
        }
        i += 1;
    }

    let host = host.ok_or_else(|| "remote requires a host argument".to_string())?;

    Ok(CliCommand::Remote {
        host,
        session_name,
        port,
        ssh_port,
    })
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
  ciri remote <host> [session]  Connect to remote server via SSH tunnel
        [--port PORT]           Remote TCP port (default: 7890)
        [--ssh-port PORT]       SSH port (default: 22)
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
        assert!(matches!(parse(&["work"]), CliCommand::Run { session_name } if session_name == "work"));
    }

    #[test]
    fn attach_requires_name() {
        let err = parse_args(["attach"].into_iter().map(|s| s.to_string()));
        assert!(err.is_err());
    }

    #[test]
    fn attach_with_name() {
        assert!(matches!(parse(&["attach", "ops"]), CliCommand::Attach { session_name } if session_name == "ops"));
        assert!(matches!(parse(&["a", "ops"]), CliCommand::Attach { session_name } if session_name == "ops"));
    }

    #[test]
    fn explicit_session_flag() {
        assert!(matches!(parse(&["--session", "qa"]), CliCommand::Run { session_name } if session_name == "qa"));
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
        assert!(matches!(parse(&["kill", "dev"]), CliCommand::Kill { session_name } if session_name == "dev"));
        assert!(matches!(parse(&["k", "dev"]), CliCommand::Kill { session_name } if session_name == "dev"));
    }

    #[test]
    fn kill_server_command() {
        assert!(matches!(parse(&["kill-server"]), CliCommand::KillServer));
        assert!(matches!(parse(&["ks"]), CliCommand::KillServer));
    }

    #[test]
    fn delete_command() {
        assert!(matches!(parse(&["delete", "old"]), CliCommand::Delete { session_name } if session_name == "old"));
        assert!(matches!(parse(&["rm", "old"]), CliCommand::Delete { session_name } if session_name == "old"));
    }

    #[test]
    fn rejects_extra_arguments() {
        let err = parse_args(["attach", "foo", "bar"].into_iter().map(|s| s.to_string()))
            .expect_err("parse should fail");
        assert!(err.contains("Usage:"));
    }
}
