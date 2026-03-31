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

    // Terminal metadata env vars (TERM_PROGRAM, COLORTERM, etc.) are set in
    // main() before the tokio runtime is created to avoid data races with
    // worker threads.  See CVE fix: std::env::set_var is unsound with threads.

    // Write shell integration scripts and set env var for child processes.
    // SAFETY: This runs early in run_daemon before any tasks that read
    // CIRI_SHELL_INTEGRATION_DIR are spawned.  The tokio worker threads do
    // not access this variable.
    match crate::shell_integration::ensure_integration_dir() {
        Ok(dir) => {
            unsafe { std::env::set_var("CIRI_SHELL_INTEGRATION_DIR", &dir) };
            log::info!("shell integration scripts at {}", dir.display());
        }
        Err(e) => {
            log::warn!("failed to write shell integration scripts: {e}");
        }
    }

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
    let listener = {
        match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // Socket already exists.  Instead of check-then-remove (which has
                // a TOCTOU gap allowing symlink attacks), use the OpenSSH approach:
                // bind to a temporary path in the same directory, then rename()
                // atomically over the target.  rename(2) atomically replaces the
                // old entry — there is never a moment where the path is missing.
                let tmp_path = sock_path.with_extension("tmp");
                // Clean up any leftover temp socket from a prior crash.
                let _ = std::fs::remove_file(&tmp_path);
                let listener = UnixListener::bind(&tmp_path)?;
                // rename(2) on Unix atomically replaces the target.  If the
                // existing entry is a symlink, we overwrite it (safe: we are
                // replacing it with our own socket, not following it).
                std::fs::rename(&tmp_path, &sock_path)?;
                listener
            }
            Err(e) => return Err(e.into()),
        }
    };
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

    // Build terminal palette from resolved theme.
    let theme = &config.theme;
    let parse = ciri_term::pane::TerminalColors::parse_hex;
    let terminal_colors = ciri_term::pane::TerminalColors {
        ansi: [
            parse(&theme.black),
            parse(&theme.red),
            parse(&theme.green),
            parse(&theme.yellow),
            parse(&theme.blue),
            parse(&theme.magenta),
            parse(&theme.cyan),
            parse(&theme.white),
            parse(&theme.bright_black),
            parse(&theme.bright_red),
            parse(&theme.bright_green),
            parse(&theme.bright_yellow),
            parse(&theme.bright_blue),
            parse(&theme.bright_magenta),
            parse(&theme.bright_cyan),
            parse(&theme.bright_white),
        ],
        foreground: parse(&theme.foreground),
        background: parse(&theme.background),
        cursor: parse(&theme.foreground), // cursor defaults to foreground
    };
    let mut server = Server::new(&shell, config.appearance.column_gap, terminal_colors);
    // Apply default_column_width from config (falls back to 0.5 proportion)
    if let Some(ref pw) = config.layout.default_column_width {
        use ciri_config::config::PresetWidth;
        server.default_column_width = match pw {
            PresetWidth::Proportion { proportion } => ColumnWidth::Proportion(*proportion),
            PresetWidth::Fixed { fixed } => ColumnWidth::Fixed(*fixed),
        };
    }
    server.pane_inset = (config.appearance.padding + config.appearance.border_width) * 2.0;
    server.idle_timeout = std::time::Duration::from_secs(config.server.idle_timeout_secs);
    server.session_config = config.session.clone();
    let state = Arc::new(Mutex::new(server));

    // Shutdown signal shared between tick loop, signal handler, and accept loop
    let shutdown = Arc::new(Notify::new());
    let input_notify = Arc::new(Notify::new());

    // Spawn tick loop (16ms = ~60fps)
    let tick_state = state.clone();
    let tick_shutdown = shutdown.clone();
    let tick_input_notify = input_notify.clone();
    tokio::spawn(async move {
        tick::run_tick_loop(tick_state, tick_shutdown, tick_input_notify).await;
    });

    // Signal handling - SIGTERM and SIGINT
    let signal_state = state.clone();
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            let mut sigint =
                signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");
            let mut sighup =
                signal(SignalKind::hangup()).expect("failed to register SIGHUP handler");
            loop {
                tokio::select! {
                    _ = sigterm.recv() => {
                        log::info!("received SIGTERM");
                        break;
                    }
                    _ = sigint.recv() => {
                        log::info!("received SIGINT");
                        break;
                    }
                    _ = sighup.recv() => {
                        // Ignore — keeps daemon alive after SSH disconnect.
                        log::info!("received SIGHUP, ignoring");
                    }
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

    // Optionally bind TCP listener for remote connections.
    // WARNING: The TCP listener has NO authentication — any process that can
    // reach this port can execute commands as the current user.  It is intended
    // to be used behind an SSH tunnel (see `connect_remote`).  Binding to
    // 127.0.0.1 limits exposure to localhost, but any local user can connect.
    let tcp_listener = if config.remote.enabled {
        let addr = format!("127.0.0.1:{}", config.remote.port);
        let tcp = tokio::net::TcpListener::bind(&addr).await?;
        log::warn!(
            "ciri-server TCP listener on {addr} (remote enabled) — \
             WARNING: no authentication, intended for SSH tunnel use only"
        );
        Some(tcp)
    } else {
        None
    };

    // Accept connections, with graceful shutdown via select!
    loop {
        #[cfg(unix)]
        {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result?;
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
                }
                result = tcp_accept(&tcp_listener) => {
                    let (stream, addr) = result?;
                    log::info!("TCP client connected from {addr}");
                    stream.set_nodelay(true).ok();
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
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
                result = tcp_accept(&tcp_listener) => {
                    let (stream, addr) = result?;
                    log::info!("TCP client connected from {addr}");
                    stream.set_nodelay(true).ok();
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
                    continue;
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
            let client_input_notify = input_notify.clone();
            let (reader, writer) = tokio::io::split(connected);
            tokio::spawn(connection::handle_client(
                reader,
                writer,
                state,
                client_shutdown,
                client_input_notify,
            ));
        }
    }

    Ok(())
}

/// Accept from an optional TCP listener, or pend forever if None.
async fn tcp_accept(
    listener: &Option<tokio::net::TcpListener>,
) -> std::io::Result<(tokio::net::TcpStream, std::net::SocketAddr)> {
    match listener {
        Some(l) => l.accept().await,
        None => std::future::pending().await,
    }
}
