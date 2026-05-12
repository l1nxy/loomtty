pub(crate) mod client;
mod connection;
mod damage;
pub(crate) mod server;
pub(crate) mod session;
mod tick;
mod ws;

use anyhow::{Context, Result};
use ciri_layout::column::ColumnWidth;
use ciri_protocol::transport;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::sync::{Mutex, Notify, Semaphore};

use server::Server;

fn parse_cursor_shape_config(name: &str) -> u8 {
    use ciri_protocol::message::{
        CURSOR_BEAM, CURSOR_BLOCK, CURSOR_HOLLOW_BLOCK, CURSOR_UNDERLINE,
    };
    match name.trim().to_ascii_lowercase().as_str() {
        "beam" | "bar" | "ibeam" => CURSOR_BEAM,
        "underline" | "underscore" => CURSOR_UNDERLINE,
        "hollow_block" | "hollow" => CURSOR_HOLLOW_BLOCK,
        // Empty string (or any unknown value) means "use the alacritty default".
        _ => CURSOR_BLOCK,
    }
}

/// Shared daemon state returned by `prepare_daemon`.
pub struct DaemonState {
    pub state: Arc<Mutex<Server>>,
    pub shutdown: Arc<Notify>,
    /// Set to true when the daemon loop exits (for any reason).
    /// The tray thread checks this to know when to exit.
    pub server_exited: Arc<AtomicBool>,
    input_notify: Arc<Notify>,
    #[cfg(unix)]
    listener: UnixListener,
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(windows)]
    pipe_server: tokio::net::windows::named_pipe::NamedPipeServer,
    config: ciri_config::config::CiriConfig,
}

/// Initialize the server, bind the socket, start tick loop and signal handlers.
/// Returns shared state that can be passed to `run_daemon_loop` or the tray.
pub async fn prepare_daemon() -> Result<DaemonState> {
    let config = ciri_config::config::CiriConfig::load()?;
    let shell = config.terminal.shell.clone();

    // Apply user's preferred cursor shape as alacritty's default. TUI apps
    // (vim/neovim/etc.) that send DECSCUSR will still override this per-mode;
    // it only fills in when no app override is active.
    ciri_term::pane::set_default_cursor_shape(parse_cursor_shape_config(
        &config.terminal.cursor_shape,
    ));

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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // Create parent directory with restrictive permissions atomically:
        // set umask before mkdir so the directory is never world-accessible.
        if let Some(parent) = sock_path.parent() {
            let old_umask = unsafe { libc::umask(0o077) };
            let mkdir_result = std::fs::create_dir_all(parent);
            unsafe { libc::umask(old_umask) };
            mkdir_result?;

            // Verify permissions are correct (defense-in-depth against pre-existing dirs)
            let meta = std::fs::metadata(parent)?;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    #[cfg(windows)]
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    let listener = {
        // Set restrictive umask so the socket file is created with 0o700
        let old_umask = unsafe { libc::umask(0o077) };

        let result: Result<UnixListener> = match UnixListener::bind(&sock_path) {
            Ok(l) => Ok(l),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // Symlink-safe removal: check that the existing path is a socket,
                // not a symlink planted by an attacker.
                let tmp_path = sock_path.with_extension("tmp");
                if tmp_path.exists() {
                    let meta = tmp_path.symlink_metadata()?;
                    if meta.file_type().is_symlink() {
                        unsafe { libc::umask(old_umask) };
                        anyhow::bail!(
                            "refusing to remove {}: path is a symlink (possible attack)",
                            tmp_path.display()
                        );
                    }
                    std::fs::remove_file(&tmp_path)?;
                }
                let listener = UnixListener::bind(&tmp_path)?;
                std::fs::rename(&tmp_path, &sock_path)?;
                Ok(listener)
            }
            Err(e) => Err(e.into()),
        };

        // Restore original umask regardless of outcome
        unsafe { libc::umask(old_umask) };
        result?
    };
    #[cfg(windows)]
    let pipe_name = transport::server_pipe_name();
    #[cfg(windows)]
    let pipe_server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&pipe_name)?;

    #[cfg(unix)]
    log::info!("ciritty-server listening on {}", sock_path.display());
    #[cfg(windows)]
    log::info!("ciritty-server listening on {}", pipe_name);

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
        cursor: parse(&theme.foreground),
    };
    let mut server = Server::new(&shell, config.appearance.column_gap, terminal_colors);
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
    let shutdown = Arc::new(Notify::new());
    let input_notify = Arc::new(Notify::new());

    // Wire PTY output → tick loop wakeup via the same Notify
    let pty_wake = input_notify.clone();
    server.pty_notify = Some(Arc::new(move || pty_wake.notify_one()));

    let state = Arc::new(Mutex::new(server));

    // Spawn tick loop
    let tick_state = state.clone();
    let tick_shutdown = shutdown.clone();
    let tick_input_notify = input_notify.clone();
    tokio::spawn(async move {
        tick::run_tick_loop(tick_state, tick_shutdown, tick_input_notify).await;
    });

    // Signal handling
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

    Ok(DaemonState {
        state,
        shutdown,
        server_exited: Arc::new(AtomicBool::new(false)),
        input_notify,
        #[cfg(unix)]
        listener,
        #[cfg(windows)]
        pipe_name,
        #[cfg(windows)]
        pipe_server,
        config,
    })
}

/// Run the accept loop. Call after `prepare_daemon`.
pub async fn run_daemon_loop(ds: DaemonState) -> Result<()> {
    let state = ds.state;
    let shutdown = ds.shutdown;
    let server_exited = ds.server_exited;
    let input_notify = ds.input_notify;
    #[cfg(unix)]
    let listener = ds.listener;
    #[cfg(windows)]
    let pipe_name = ds.pipe_name;
    #[cfg(windows)]
    let mut pipe_server = ds.pipe_server;

    let tcp_listener = if ds.config.remote.enabled {
        let addr = format!("127.0.0.1:{}", ds.config.remote.port);
        let tcp = tokio::net::TcpListener::bind(&addr).await?;
        log::warn!(
            "ciritty-server TCP listener on {addr} (remote enabled) — \
             WARNING: no authentication, intended for SSH tunnel use only"
        );
        Some(tcp)
    } else {
        None
    };

    let (ws_listener, ws_token, ws_allowed_origins) = if ds.config.web.enabled {
        // Token: trimmed, zeroized-on-drop, refcounted. The helper enforces
        // the 16-byte floor and any surrounding whitespace is stripped so
        // a trailing newline in the config does not silently break auth.
        let token = ws::prepare_web_token(&ds.config.web.token)?;
        // `bind` has already passed schema validation (accepts empty or a
        // parseable IpAddr) — empty means "use loopback".
        let parsed_bind: std::net::IpAddr = if ds.config.web.bind.is_empty() {
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        } else {
            ds.config.web.bind.parse().with_context(|| {
                format!(
                    "[web] bind {:?} passed config validation but failed to parse here",
                    ds.config.web.bind,
                )
            })?
        };
        // Origin policy: an empty allowlist is only safe on loopback,
        // because only same-host pages can reach the listener. On a
        // non-loopback bind, an unscoped allowlist invites CSRF from any
        // page in the browser — refuse to start.
        if !parsed_bind.is_loopback() && ds.config.web.allowed_origins.is_empty() {
            anyhow::bail!(
                "[web] bind={parsed_bind} is not loopback but allowed_origins is empty. \
                 Add the browser origin(s) you intend to serve (e.g. \
                 allowed_origins = [\"https://terminal.example.com\"]) before exposing \
                 the gateway."
            );
        }
        // Construct the listener from the canonical IpAddr so the logged
        // address and the loopback check agree on a single normalised
        // form (matters for IPv6: `::0001` and `::1` parse to the same
        // address but compare as different strings).
        let addr = std::net::SocketAddr::new(parsed_bind, ds.config.web.port);
        let tcp = tokio::net::TcpListener::bind(&addr).await?;
        log::info!("ciritty-server WS listener on {addr}");
        if !parsed_bind.is_loopback() {
            log::warn!(
                "ws bind={parsed_bind} is not loopback — terminate TLS upstream \
                 and ensure the token is rotated; the gateway speaks plain ws://"
            );
        }
        let origins = Arc::new(ds.config.web.allowed_origins.clone());
        (Some(tcp), Some(token), origins)
    } else {
        (None, None, Arc::new(Vec::new()))
    };

    let ws_handshake_sem = Arc::new(Semaphore::new(MAX_IN_FLIGHT_WS_HANDSHAKES));

    loop {
        #[cfg(unix)]
        {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result?;
                    // Verify peer UID matches our UID (SO_PEERCRED / getpeereid)
                    match stream.peer_cred() {
                        Ok(cred) => {
                            let my_uid = unsafe { libc::getuid() };
                            if cred.uid() != my_uid {
                                log::warn!(
                                    "rejected Unix socket connection from uid {} (expected {})",
                                    cred.uid(),
                                    my_uid,
                                );
                                continue;
                            }
                        }
                        Err(e) => {
                            log::warn!("failed to get peer credentials, rejecting: {e}");
                            continue;
                        }
                    }
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
                }
                result = tcp_accept(&tcp_listener) => {
                    let (stream, addr) = result?;
                    stream.set_nodelay(true).ok();
                    log::info!("TCP client connected from {addr}");
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
                }
                result = tcp_accept(&ws_listener) => {
                    let (stream, addr) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            // EMFILE/ENFILE/ECONNABORTED and similar
                            // transient errors must not kill the entire
                            // daemon — log and keep accepting.
                            log::warn!("ws accept failed: {e}");
                            continue;
                        }
                    };
                    stream.set_nodelay(true).ok();
                    // ws_listener is only Some when ws_token is also Some
                    // (set together in prepare phase); the .as_ref()/.clone()
                    // here is cheap (Arc refcount bump) so the unwrap is
                    // load-bearing only for the type system's benefit.
                    let token = ws_token
                        .as_ref()
                        .expect("ws_listener Some implies ws_token Some")
                        .clone();
                    spawn_ws_client(
                        stream,
                        addr,
                        state.clone(),
                        shutdown.clone(),
                        input_notify.clone(),
                        token,
                        ws_allowed_origins.clone(),
                        ws_handshake_sem.clone(),
                    );
                }
                _ = shutdown.notified() => {
                    log::info!("accept loop shutting down");
                    connection::graceful_shutdown(&state).await;
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
                    stream.set_nodelay(true).ok();
                    log::info!("TCP client connected from {addr}");
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
                    continue;
                }
                result = tcp_accept(&ws_listener) => {
                    let (stream, addr) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            log::warn!("ws accept failed: {e}");
                            continue;
                        }
                    };
                    stream.set_nodelay(true).ok();
                    // ws_listener is only Some when ws_token is also Some
                    // (set together in prepare phase); the .as_ref()/.clone()
                    // here is cheap (Arc refcount bump) so the unwrap is
                    // load-bearing only for the type system's benefit.
                    let token = ws_token
                        .as_ref()
                        .expect("ws_listener Some implies ws_token Some")
                        .clone();
                    spawn_ws_client(
                        stream,
                        addr,
                        state.clone(),
                        shutdown.clone(),
                        input_notify.clone(),
                        token,
                        ws_allowed_origins.clone(),
                        ws_handshake_sem.clone(),
                    );
                    continue;
                }
                _ = shutdown.notified() => {
                    log::info!("accept loop shutting down");
                    connection::graceful_shutdown(&state).await;
                    break;
                }
            }
            let connected = pipe_server;
            pipe_server = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&pipe_name)?;
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

    // Signal the tray thread that the server has exited.
    server_exited.store(true, Ordering::Relaxed);
    Ok(())
}

/// Legacy entry point: prepare + run in one call (for headless/daemonize mode).
pub async fn run_daemon() -> Result<()> {
    let ds = prepare_daemon().await?;
    run_daemon_loop(ds).await
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

/// Cap on concurrent in-flight WS handshakes. A handshake is bounded by
/// `ws::HANDSHAKE_TIMEOUT` (10s); 64 concurrent slots give legitimate
/// burst load plenty of room while putting a hard ceiling on tasks an
/// attacker can pin by completing the TCP handshake and stalling the
/// HTTP upgrade. Excess attempts are rejected immediately rather than
/// queued so an attacker can't hold attempts in line.
const MAX_IN_FLIGHT_WS_HANDSHAKES: usize = 64;

/// Perform the WS handshake off the accept-loop thread and, on success,
/// hand the wrapped stream to `connection::handle_client`. Errors from
/// the handshake (bad token, malformed upgrade, etc.) are logged and
/// the connection is dropped — the accept loop must stay responsive
/// for other clients regardless of any single peer's misbehaviour.
#[allow(clippy::too_many_arguments)]
fn spawn_ws_client(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
    state: Arc<Mutex<Server>>,
    shutdown: Arc<Notify>,
    input_notify: Arc<Notify>,
    token: ws::SharedToken,
    allowed_origins: Arc<Vec<String>>,
    handshake_sem: Arc<Semaphore>,
) {
    // `try_acquire_owned` is non-blocking: when the cap is reached we
    // reject the new attempt immediately and free the TCP stream rather
    // than queueing it, so an attacker can't keep a pipeline of stalled
    // upgrades in line.
    let permit = match handshake_sem.try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            log::warn!(
                "ws handshake from {addr} rejected: {MAX_IN_FLIGHT_WS_HANDSHAKES} \
                 concurrent handshakes already in flight"
            );
            return;
        }
    };

    tokio::spawn(async move {
        log::debug!("ws TCP accept from {addr}; starting handshake");
        let ws_stream = match ws::accept_ws(stream, token, allowed_origins).await {
            Ok(ws) => ws,
            Err(e) => {
                log::warn!("ws handshake from {addr} failed: {e}");
                return;
            }
        };
        // Handshake done — release the slot before the long-lived client
        // session begins; the session itself does not count against the
        // handshake budget.
        drop(permit);
        log::info!("ws upgrade from {addr} accepted (handshake complete)");
        let (reader, writer) = tokio::io::split(ws_stream);
        connection::handle_client(reader, writer, state, shutdown, input_notify).await;
    });
}
