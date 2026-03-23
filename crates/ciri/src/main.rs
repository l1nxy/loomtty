mod app;
mod cli;
mod connection;
mod control;
mod grid;
mod init;

use anyhow::Result;
use app::App;
use clap::Parser;
use ciri_config::config::CiriConfig;
use ciri_protocol::message::ClientMessage;
use cli::CliCommand;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
    )
    .init();

    let cli = cli::resolve(cli::Cli::parse());

    // Handle non-GUI commands first
    match cli {
        CliCommand::Init => {
            return init::run_init();
        }
        CliCommand::List { all } => {
            return control::run_control_command(ClientMessage::ListSessions { all }, false);
        }
        CliCommand::Kill { session_name } => {
            return control::run_control_command(ClientMessage::KillSession { session_name }, false);
        }
        CliCommand::KillServer => {
            return control::run_control_command(ClientMessage::KillServer, false);
        }
        CliCommand::Msg { subcommand, json } => {
            use cli::MsgSubcommand;
            let msg = match subcommand {
                MsgSubcommand::SendKeys { session_name, pane_id, keys } => {
                    ClientMessage::SendKeys { session_name, pane_id, keys: keys.into_bytes() }
                }
                MsgSubcommand::ListPanes { session_name } => {
                    ClientMessage::ListPanes { session_name }
                }
                MsgSubcommand::Info { session_name } => {
                    ClientMessage::GetSessionInfo { session_name }
                }
                MsgSubcommand::FocusPane { session_name, pane_id } => {
                    ClientMessage::FocusPaneById { session_name, pane_id }
                }
                MsgSubcommand::ClosePane { session_name, pane_id } => {
                    ClientMessage::ClosePaneById { session_name, pane_id }
                }
                MsgSubcommand::CreatePane { session_name } => {
                    ClientMessage::CreatePaneIn { session_name }
                }
                MsgSubcommand::GetLayout { session_name } => {
                    ClientMessage::GetLayout { session_name }
                }
                MsgSubcommand::RunCommand { session_name, command } => {
                    ClientMessage::RunCommand { session_name, command, cwd: None }
                }
            };
            return control::run_control_command(msg, json);
        }
        CliCommand::Delete { session_name } => {
            let dir = ciri_protocol::transport::state_dir();
            match ciri_session::restore::delete_session(&session_name, &dir) {
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
                TemplateSubcommand::Apply { template_name, session_name } => {
                    let session = session_name.unwrap_or_else(|| {
                        let existing = ciri_session::restore::list_sessions(
                            &ciri_protocol::transport::state_dir(),
                        ).unwrap_or_default();
                        ciri_session::names::unique_name(&existing)
                    });
                    return control::run_control_command(ClientMessage::ApplyTemplate {
                        template_name,
                        session_name: session,
                    }, false);
                }
                TemplateSubcommand::Save { template_name, session_name } => {
                    return control::run_control_command(ClientMessage::SaveTemplate {
                        template_name,
                        session_name,
                    }, false);
                }
            }
        }
        _ => {}
    }

    // Handle remote connection separately (no local server spawn)
    if let CliCommand::Remote { host, session_name, port, ssh_port } = cli {
        let session_name = session_name.unwrap_or_else(|| {
            let existing = ciri_session::restore::list_sessions(&ciri_protocol::transport::state_dir())
                .unwrap_or_default();
            ciri_session::names::unique_name(&existing)
        });

        let config = CiriConfig::load().unwrap_or_default();
        log::info!(
            "config: font={} size={}, remote={}:{}, session={}",
            config.font.family,
            config.font.size,
            host,
            port,
            session_name
        );

        let event_loop = EventLoop::new()?;
        let mut app = App::new(config, session_name);
        app.remote_config = Some(app::RemoteConnectionConfig {
            host,
            port,
            ssh_port,
        });
        event_loop.run_app(&mut app)?;
        return Ok(());
    }

    // Resolve session name
    let session_name = match cli {
        CliCommand::New => {
            // If there are active sessions on the server, attach to the most recent one.
            let active = control::query_active_sessions();
            if let Some(name) = active.first() {
                log::info!("attaching to most recent session: {name}");
                name.clone()
            } else {
                let existing =
                    ciri_session::restore::list_sessions(&ciri_protocol::transport::state_dir())
                        .unwrap_or_default();
                let name = ciri_session::names::unique_name(&existing);
                log::info!("creating new session: {name}");
                name
            }
        }
        CliCommand::Run { session_name } => {
            if let Err(e) = ciri_session::names::validate_name(&session_name) {
                eprintln!("invalid session name: {e}");
                std::process::exit(2);
            }
            session_name
        }
        CliCommand::Attach { session_name } => {
            if let Err(e) = ciri_session::names::validate_name(&session_name) {
                eprintln!("invalid session name: {e}");
                std::process::exit(2);
            }
            // Verify the session exists: check saved state on disk first,
            // then probe the running server for live sessions that haven't
            // been saved yet (autosave triggers on layout changes only).
            let state_dir = ciri_protocol::transport::state_dir();
            let saved = ciri_session::restore::list_sessions(&state_dir).unwrap_or_default();
            let has_saved = saved.iter().any(|n| n == &session_name);
            let has_running = if !has_saved {
                control::session_exists_on_server(&session_name)
            } else {
                false
            };
            if !has_saved && !has_running {
                eprintln!(
                    "session '{}' does not exist.\nUse `ciri ls` to list sessions, or `ciri {}` to create it.",
                    session_name, session_name
                );
                std::process::exit(1);
            }
            session_name
        }
        _ => unreachable!(),
    };

    if !ciri_config::config::config_path().exists() {
        eprintln!("warning: No config file found. Run `ciri init` to set up your configuration.");
    }
    let config = CiriConfig::load().unwrap_or_default();
    log::info!(
        "config: font={} size={}, session={}",
        config.font.family,
        config.font.size,
        session_name
    );

    let event_loop = EventLoop::new()?;
    let mut app = App::new(config, session_name);
    event_loop.run_app(&mut app)?;
    Ok(())
}
