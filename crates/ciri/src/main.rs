mod app;
mod cli;
mod connection;
mod control;
mod grid;

use anyhow::Result;
use app::App;
use ciri_config::config::CiriConfig;
use cli::CliCommand;
use ciri_protocol::message::ClientMessage;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
    )
    .init();

    let cli = match cli::parse_args(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(usage) => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
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
        CliCommand::Run { ref session_name } => {
            log::info!("session: {session_name}");
        }
    }

    let session_name = match cli {
        CliCommand::Run { session_name } => session_name,
        _ => unreachable!(),
    };

    let config = CiriConfig::load().unwrap_or_default();
    log::info!(
        "config: font={} size={}",
        config.font.family,
        config.font.size
    );

    let event_loop = EventLoop::new()?;
    let mut app = App::new(config, session_name);
    event_loop.run_app(&mut app)?;
    Ok(())
}
