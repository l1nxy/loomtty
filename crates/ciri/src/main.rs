mod app;
mod cli;
mod connection;
mod control;
mod grid;

use anyhow::Result;
use app::App;
use ciri_config::config::CiriConfig;
use ciri_protocol::message::ClientMessage;
use cli::CliCommand;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
    )
    .init();

    let cli = match cli::parse_args(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    // Handle non-GUI commands first
    match cli {
        CliCommand::Help => {
            println!("{}", cli::usage());
            return Ok(());
        }
        CliCommand::List => {
            return control::run_control_command(ClientMessage::ListSessions);
        }
        CliCommand::Kill { session_name } => {
            return control::run_control_command(ClientMessage::KillSession { session_name });
        }
        CliCommand::KillServer => {
            return control::run_control_command(ClientMessage::KillServer);
        }
        CliCommand::Delete { session_name } => {
            let dir = ciri_protocol::transport::state_dir();
            match ciri_session::restore::delete_session(&session_name, &dir) {
                Ok(()) => println!("deleted session '{session_name}'"),
                Err(e) => eprintln!("failed to delete session '{session_name}': {e}"),
            }
            return Ok(());
        }
        _ => {}
    }

    // Resolve session name
    let session_name = match cli {
        CliCommand::New => {
            let existing =
                ciri_session::restore::list_sessions(&ciri_protocol::transport::state_dir())
                    .unwrap_or_default();
            let name = ciri_session::names::unique_name(&existing);
            log::info!("creating new session: {name}");
            name
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
