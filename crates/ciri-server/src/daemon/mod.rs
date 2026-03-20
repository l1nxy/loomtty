mod client;
mod connection;
mod damage;
mod server;
mod session;
mod tick;

use anyhow::Result;
use ciri_layout::column::ColumnWidth;
use ciri_protocol::transport;
use std::sync::Arc;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::sync::{Mutex, Notify};

use server::Server;

pub async fn run_daemon() -> Result<()> {
    let config = ciri_config::config::CiriConfig::load().unwrap_or_default();
    let shell = config.terminal.shell.clone();

    let sock_path = transport::server_socket_path();
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    #[cfg(unix)]
    {
        if sock_path.exists() {
            std::fs::remove_file(&sock_path)?;
        }
    }

    #[cfg(unix)]
    let listener = UnixListener::bind(&sock_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        if let Err(e) = std::fs::set_permissions(&sock_path, perms) {
            log::warn!("failed to set socket permissions: {e}");
        }
    }
    #[cfg(windows)]
    let pipe_name = transport::server_pipe_name();
    #[cfg(windows)]
    let mut pipe_server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)?;

    #[cfg(unix)]
    log::info!("ciri-server listening on {}", sock_path.display());
    #[cfg(windows)]
    log::info!("ciri-server listening on {}", pipe_name);

    let mut server = Server::new(&shell, config.appearance.column_gap);
    // Apply default_column_width from config (falls back to 0.5 proportion)
    if let Some(ref pw) = config.layout.default_column_width {
        use ciri_config::config::PresetWidth;
        server.default_column_width = match pw {
            PresetWidth::Proportion { proportion } => ColumnWidth::Proportion(*proportion),
            PresetWidth::Fixed { fixed } => ColumnWidth::Fixed(*fixed),
        };
    }
    server.pane_inset = (config.appearance.padding + config.appearance.border_width) * 2.0;
    let state = Arc::new(Mutex::new(server));

    // Shutdown signal shared between tick loop, signal handler, and accept loop
    let shutdown = Arc::new(Notify::new());

    // Spawn tick loop (16ms = ~60fps)
    let tick_state = state.clone();
    let tick_shutdown = shutdown.clone();
    tokio::spawn(async move {
        tick::run_tick_loop(tick_state, tick_shutdown).await;
    });

    // Signal handling - SIGTERM and SIGINT
    let signal_state = state.clone();
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            let mut sigint =
                signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");
            tokio::select! {
                _ = sigterm.recv() => {
                    log::info!("received SIGTERM");
                }
                _ = sigint.recv() => {
                    log::info!("received SIGINT");
                }
            }
            connection::graceful_shutdown(&signal_state).await;
            signal_shutdown.notify_one();
        }
        #[cfg(windows)]
        {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("received Ctrl-C");
            connection::graceful_shutdown(&signal_state).await;
            signal_shutdown.notify_one();
        }
    });

    // Accept connections, with graceful shutdown via select!
    loop {
        #[cfg(unix)]
        {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result?;
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown));
                }
                _ = shutdown.notified() => {
                    log::info!("accept loop shutting down");
                    break;
                }
            }
        }

        #[cfg(windows)]
        {
            tokio::select! {
                result = pipe_server.connect() => {
                    if let Err(e) = result {
                        log::error!("named pipe accept error: {e}");
                        continue;
                    }
                }
                _ = shutdown.notified() => {
                    log::info!("accept loop shutting down");
                    break;
                }
            }
            // After select!, the borrow from connect() is released
            let connected = pipe_server;
            pipe_server = ServerOptions::new().create(&pipe_name)?;
            let state = state.clone();
            let client_shutdown = shutdown.clone();
            let (reader, writer) = tokio::io::split(connected);
            tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown));
        }
    }

    Ok(())
}
