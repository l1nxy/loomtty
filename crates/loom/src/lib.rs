//! loomtty client core.
//!
//! This crate ships two binaries that share all of the logic below:
//! - `loomtty` (`src/main.rs`) — console subsystem, the CLI entry point.
//! - `loomtty-gui` (`src/bin/loomtty-gui.rs`) — Windows GUI subsystem, what
//!   Start Menu / desktop shortcuts launch so no console window appears.
//!
//! Both call [`init_logging`] then [`run`]. Keeping the GUI launcher in a
//! separate, console-less binary (the approach WezTerm uses) is what lets the
//! desktop shortcut open the window without flashing a terminal, while the CLI
//! binary keeps the console subsystem so `loomtty init` (interactive) and
//! `loomtty web` (foreground) behave correctly.

mod app;
pub mod cli;
mod connection;
mod control;
mod grid;
mod init;
mod recent_hosts;
mod remote_validate;
mod web;

use anyhow::Result;
use app::App;
use cli::CliCommand;
use loom_config::config::LoomConfig;
use loom_protocol::message::{CapturePaneOpts, ClientMessage};
use std::path::PathBuf;
use winit::event_loop::EventLoop;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionLaunchChoice {
    session_name: String,
    remembers_last_session: bool,
}

/// Initialize the env-logger. Shared by both binaries.
pub fn init_logging() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
    )
    .init();
}

/// Run a resolved CLI command: dispatch non-GUI subcommands, otherwise launch
/// the GUI client (local session or remote attach).
pub fn run(cli: CliCommand) -> Result<()> {
    // Handle non-GUI commands first
    match cli {
        CliCommand::Init => {
            return init::run_init();
        }
        CliCommand::List { all } => {
            return control::run_control_command(ClientMessage::ListSessions { all }, false);
        }
        CliCommand::Kill { session_name } => {
            return control::run_control_command(
                ClientMessage::KillSession { session_name },
                false,
            );
        }
        CliCommand::KillServer => {
            return control::run_control_command(ClientMessage::KillServer, false);
        }
        CliCommand::Msg { subcommand, json } => {
            use cli::MsgSubcommand;
            let msg = match subcommand {
                MsgSubcommand::SendKeys {
                    session_name,
                    pane_id,
                    keys,
                } => ClientMessage::SendKeys {
                    session_name,
                    pane_id,
                    keys: keys.into_bytes(),
                },
                MsgSubcommand::ListPanes { session_name } => {
                    ClientMessage::ListPanes { session_name }
                }
                MsgSubcommand::Info { session_name } => {
                    ClientMessage::GetSessionInfo { session_name }
                }
                MsgSubcommand::FocusPane {
                    session_name,
                    pane_id,
                } => ClientMessage::FocusPaneById {
                    session_name,
                    pane_id,
                },
                MsgSubcommand::ClosePane {
                    session_name,
                    pane_id,
                } => ClientMessage::ClosePaneById {
                    session_name,
                    pane_id,
                },
                MsgSubcommand::CreatePane { session_name } => {
                    ClientMessage::CreatePaneIn { session_name }
                }
                MsgSubcommand::GetLayout { session_name } => {
                    ClientMessage::GetLayout { session_name }
                }
                MsgSubcommand::RunCommand {
                    session_name,
                    command,
                } => ClientMessage::RunCommand {
                    session_name,
                    command,
                    cwd: None,
                },
                MsgSubcommand::CapturePane {
                    session_name,
                    pane_id,
                    scrollback_rows,
                    join_wrapped,
                    preserve_trailing_spaces,
                } => ClientMessage::CapturePane {
                    session_name,
                    pane_id,
                    opts: CapturePaneOpts {
                        scrollback_rows,
                        join_wrapped,
                        preserve_trailing_spaces,
                    },
                },
                MsgSubcommand::ListPrompts {
                    session_name,
                    pane_id,
                } => ClientMessage::ListPrompts {
                    session_name,
                    pane_id,
                },
            };
            return control::run_control_command(msg, json);
        }
        CliCommand::Delete { session_name } => {
            let dir = loom_protocol::transport::state_dir();
            match loom_session::restore::delete_session(&session_name, &dir) {
                Ok(()) => println!("deleted session '{session_name}'"),
                Err(e) => eprintln!("failed to delete session '{session_name}': {e}"),
            }
            return Ok(());
        }
        CliCommand::Template { subcommand } => {
            use cli::TemplateSubcommand;
            match subcommand {
                TemplateSubcommand::List => {
                    return control::run_control_command(ClientMessage::ListTemplates, false);
                }
                TemplateSubcommand::Apply {
                    template_name,
                    session_name,
                } => {
                    let session = session_name.unwrap_or_else(|| {
                        let existing = loom_session::restore::list_sessions(
                            &loom_protocol::transport::state_dir(),
                        )
                        .unwrap_or_default();
                        loom_session::names::unique_name(&existing)
                    });
                    return control::run_control_command(
                        ClientMessage::ApplyTemplate {
                            template_name,
                            session_name: session,
                        },
                        false,
                    );
                }
                TemplateSubcommand::Save {
                    template_name,
                    session_name,
                } => {
                    return control::run_control_command(
                        ClientMessage::SaveTemplate {
                            template_name,
                            session_name,
                        },
                        false,
                    );
                }
            }
        }
        CliCommand::Web {
            port,
            bind,
            token,
            static_dir,
            open,
        } => {
            return web::run_web(port, bind, token, static_dir, open);
        }
        _ => {}
    }

    // Handle remote connection separately (no local server spawn)
    if let CliCommand::Remote {
        host,
        session_name,
        port,
        ssh_port,
    } = cli
    {
        if let Err(e) = remote_validate::validate_fields(&host, port, ssh_port) {
            eprintln!("error: invalid remote target: {e}");
            std::process::exit(2);
        }

        let session_name = session_name.unwrap_or_else(|| {
            // For remote connections, probe the remote server for existing sessions
            // and attach to the most recently active one.  Generating a random local
            // name would create a new session on the remote every time.
            let remote_sessions =
                crate::connection::probe_remote_sessions_blocking(&host, port, ssh_port);
            if let Some(first) = remote_sessions.first() {
                log::info!("attaching to existing remote session: {}", first);
                first.clone()
            } else {
                // No sessions on remote — use "default" (deterministic, not random).
                "default".to_string()
            }
        });

        let mut config = LoomConfig::load()?;
        sanitize_remote_hosts(&mut config);
        log::info!(
            "config: font={} size={}, remote={}:{}, session={}",
            config.font.family,
            config.font.size,
            host,
            port,
            session_name
        );
        remember_last_session(&session_name);

        let event_loop = EventLoop::new()?;
        let mut app = App::new(config, session_name);
        app.event_loop_proxy = Some(event_loop.create_proxy());
        app.core.recent_hosts = recent_hosts::load();
        app.core.remote_config = Some(app::RemoteConnectionConfig {
            host: host.clone(),
            port,
            ssh_port,
        });
        // Recent-host recording is deferred to connect_remote_session()
        // so that only successfully initiated connections get persisted.
        event_loop.run_app(&mut app)?;
        return Ok(());
    }

    // Resolve session name
    let launch = resolve_session_launch(cli);
    let session_name = launch.session_name;

    if !loom_config::config::config_path().exists() {
        eprintln!(
            "warning: No config file found. Run `loomtty init` to set up your configuration."
        );
    }
    let mut config = LoomConfig::load()?;
    sanitize_remote_hosts(&mut config);
    log::info!(
        "config: font={} size={}, session={}",
        config.font.family,
        config.font.size,
        session_name
    );
    if launch.remembers_last_session {
        remember_last_session(&session_name);
    }

    let event_loop = EventLoop::new()?;
    let mut app = App::new(config, session_name);
    app.event_loop_proxy = Some(event_loop.create_proxy());
    app.core.recent_hosts = recent_hosts::load();
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn last_session_path() -> PathBuf {
    loom_protocol::transport::state_dir().join("last-session")
}

/// Drop any `[[remote.hosts]]` entries whose fields would be rejected at
/// connect time. We log a warning per bad entry but never refuse to start —
/// a broken config section shouldn't prevent local sessions from working.
fn sanitize_remote_hosts(config: &mut LoomConfig) {
    config.remote.hosts.retain(|h| {
        match remote_validate::validate_fields(&h.host, h.port, h.ssh_port) {
            Ok(()) => true,
            Err(e) => {
                log::warn!(
                    "config: dropping invalid [[remote.hosts]] entry {:?} ({}:{}): {e}",
                    h.name,
                    h.host,
                    h.port
                );
                false
            }
        }
    });
}

fn resolve_session_launch(cli: CliCommand) -> SessionLaunchChoice {
    match cli {
        CliCommand::Default => choose_default_session(),
        CliCommand::New => choose_new_session(),
        CliCommand::Run { session_name } => SessionLaunchChoice {
            session_name: validated_session_name(session_name),
            remembers_last_session: true,
        },
        CliCommand::Attach { session_name } => choose_existing_session(session_name),
        _ => unreachable!(),
    }
}

fn choose_default_session() -> SessionLaunchChoice {
    let state_dir = loom_protocol::transport::state_dir();
    let saved = loom_session::restore::list_sessions(&state_dir).unwrap_or_default();
    if let Some(name) = control::query_active_sessions().into_iter().next() {
        log::info!("attaching to most recent session: {name}");
        return SessionLaunchChoice {
            session_name: name,
            remembers_last_session: true,
        };
    }

    if let Some(name) = read_last_session().filter(|name| {
        saved.iter().any(|saved_name| saved_name == name) || session_is_running(name)
    }) {
        log::info!("attaching to last local session: {name}");
        return SessionLaunchChoice {
            session_name: name,
            remembers_last_session: true,
        };
    }

    let session_name = loom_session::names::unique_name(&saved);
    log::info!("creating new session: {session_name}");
    SessionLaunchChoice {
        session_name,
        remembers_last_session: true,
    }
}

fn choose_new_session() -> SessionLaunchChoice {
    let existing = loom_session::restore::list_sessions(&loom_protocol::transport::state_dir())
        .unwrap_or_default();
    let session_name = loom_session::names::unique_name(&existing);
    log::info!("creating new session: {session_name}");
    SessionLaunchChoice {
        session_name,
        remembers_last_session: true,
    }
}

fn choose_existing_session(session_name: String) -> SessionLaunchChoice {
    let session_name = validated_session_name(session_name);
    let state_dir = loom_protocol::transport::state_dir();
    let saved = loom_session::restore::list_sessions(&state_dir).unwrap_or_default();
    let has_saved = saved.iter().any(|name| name == &session_name);
    let has_running = !has_saved && session_is_running(&session_name);
    if !has_saved && !has_running {
        eprintln!(
            "session '{}' does not exist.\nUse `loomtty ls` to list sessions, or `loomtty {}` to create it.",
            session_name, session_name
        );
        std::process::exit(1);
    }

    SessionLaunchChoice {
        session_name,
        remembers_last_session: true,
    }
}

fn validated_session_name(session_name: String) -> String {
    if let Err(e) = loom_session::names::validate_name(&session_name) {
        eprintln!("invalid session name: {e}");
        std::process::exit(2);
    }
    session_name
}

fn session_is_running(session_name: &str) -> bool {
    control::session_exists_on_server(session_name)
}

fn read_last_session() -> Option<String> {
    let path = last_session_path();
    let name = std::fs::read_to_string(path).ok()?.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn remember_last_session(session_name: &str) {
    let path = last_session_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, session_name);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choose_default_session_for_test(
        active_sessions: Vec<&str>,
        remembered: Option<&str>,
        saved_sessions: Vec<&str>,
    ) -> SessionLaunchChoice {
        let saved = saved_sessions
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if let Some(name) = active_sessions.into_iter().next() {
            return SessionLaunchChoice {
                session_name: name.to_owned(),
                remembers_last_session: true,
            };
        }

        if let Some(name) =
            remembered.filter(|name| saved.iter().any(|saved_name| saved_name == name))
        {
            return SessionLaunchChoice {
                session_name: name.to_owned(),
                remembers_last_session: true,
            };
        }

        SessionLaunchChoice {
            session_name: loom_session::names::unique_name(&saved),
            remembers_last_session: true,
        }
    }

    #[test]
    fn validated_session_name_returns_input() {
        assert_eq!(validated_session_name("ops".into()), "ops");
    }

    #[test]
    fn run_launch_preserves_requested_name() {
        assert_eq!(
            resolve_session_launch(CliCommand::Run {
                session_name: "ops".into(),
            }),
            SessionLaunchChoice {
                session_name: "ops".into(),
                remembers_last_session: true,
            }
        );
    }

    #[test]
    fn new_launch_remembers_last_session() {
        let launch = choose_new_session();
        assert!(launch.remembers_last_session);
        assert!(!launch.session_name.is_empty());
    }

    #[test]
    fn remember_choice_tracks_flag() {
        let launch = SessionLaunchChoice {
            session_name: "saved".into(),
            remembers_last_session: true,
        };
        assert!(launch.remembers_last_session);
    }

    #[test]
    fn default_launch_prefers_running_session_before_remembered_session() {
        assert_eq!(
            choose_default_session_for_test(
                vec!["running-now"],
                Some("remembered-old"),
                vec!["remembered-old"],
            ),
            SessionLaunchChoice {
                session_name: "running-now".into(),
                remembers_last_session: true,
            }
        );
    }
}
