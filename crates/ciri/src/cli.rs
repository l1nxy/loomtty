const DEFAULT_SESSION: &str = "default";

#[derive(Debug)]
pub enum CliCommand {
    Run { session_name: String },
    Help,
}

pub fn parse_args<I>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    let usage = usage();

    match args.as_slice() {
        [] => Ok(CliCommand::Run {
            session_name: DEFAULT_SESSION.to_string(),
        }),
        [cmd] if cmd == "attach" => Ok(CliCommand::Run {
            session_name: DEFAULT_SESSION.to_string(),
        }),
        [cmd, session_name] if cmd == "attach" => Ok(CliCommand::Run {
            session_name: session_name.clone(),
        }),
        [flag, session_name] if flag == "--session" || flag == "-s" => Ok(CliCommand::Run {
            session_name: session_name.clone(),
        }),
        [arg] if is_help_flag(arg) => Ok(CliCommand::Help),
        [arg] => Ok(CliCommand::Run {
            session_name: arg.clone(),
        }),
        _ => Err(usage),
    }
}

pub fn usage() -> String {
    format!(
        "Usage:\n  ciri\n  ciri <session>\n  ciri attach [session]\n  ciri --session <session>\n  ciri --help\n\nDefault session: {DEFAULT_SESSION}"
    )
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
    fn default_session_without_args() {
        match parse(&[]) {
            CliCommand::Run { session_name } => assert_eq!(session_name, "default"),
            CliCommand::Help => panic!("expected run command"),
        }
    }

    #[test]
    fn positional_session_name() {
        match parse(&["work"]) {
            CliCommand::Run { session_name } => assert_eq!(session_name, "work"),
            CliCommand::Help => panic!("expected run command"),
        }
    }

    #[test]
    fn attach_subcommand_defaults_to_default() {
        match parse(&["attach"]) {
            CliCommand::Run { session_name } => assert_eq!(session_name, "default"),
            CliCommand::Help => panic!("expected run command"),
        }
    }

    #[test]
    fn attach_subcommand_accepts_name() {
        match parse(&["attach", "ops"]) {
            CliCommand::Run { session_name } => assert_eq!(session_name, "ops"),
            CliCommand::Help => panic!("expected run command"),
        }
    }

    #[test]
    fn explicit_session_flag() {
        match parse(&["--session", "qa"]) {
            CliCommand::Run { session_name } => assert_eq!(session_name, "qa"),
            CliCommand::Help => panic!("expected run command"),
        }
    }

    #[test]
    fn help_flag() {
        assert!(matches!(parse(&["--help"]), CliCommand::Help));
    }

    #[test]
    fn rejects_extra_arguments() {
        let err = parse_args(["attach", "foo", "bar"].into_iter().map(|s| s.to_string()))
            .expect_err("parse should fail");
        assert!(err.contains("Usage:"));
    }
}
