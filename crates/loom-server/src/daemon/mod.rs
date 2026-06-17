pub(crate) mod client;
mod connection;
mod damage;
pub(crate) mod server;
pub(crate) mod session;
mod tick;
mod web;

use anyhow::{Context, Result};
use loom_protocol::transport;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::sync::{Mutex, Notify};

use server::Server;

fn parse_cursor_shape_config(name: &str) -> u8 {
    use loom_protocol::message::{
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

/// Shared daemon state returned by `prepare_daemon_with`.
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
    config: loom_config::config::LoomConfig,
    /// `true` when web was requested explicitly via the server's `--web*`
    /// flags (i.e. `loomtty web`), vs. merely `[web] enabled` in the loaded
    /// config. A web bind failure is fatal for the explicit case (the command
    /// promised a listener) but best-effort for the desktop-config case.
    web_required: bool,
}

/// Initialize the server, bind the socket, start tick loop and signal
/// handlers. Returns shared state for `run_daemon_loop` or the tray.
///
/// `config_override` injects an already-built config instead of loading
/// from disk — used by the `loomtty web` command / `--web*` server flags
/// to override `[web]` settings in memory. `None` loads from disk.
pub async fn prepare_daemon_with(
    config_override: Option<loom_config::config::LoomConfig>,
) -> Result<DaemonState> {
    // An injected override means the server was launched with `--web*` (i.e.
    // `loomtty web`) — web was asked for explicitly, so a bind failure must be
    // fatal rather than best-effort. A `None` override (desktop config load)
    // keeps web best-effort.
    let web_required = config_override.is_some();
    let config = match config_override {
        Some(c) => c,
        None => loom_config::config::LoomConfig::load()?,
    };
    let shell = config.terminal.shell.clone();

    // Apply user's preferred cursor shape as alacritty's default. TUI apps
    // (vim/neovim/etc.) that send DECSCUSR will still override this per-mode;
    // it only fills in when no app override is active.
    loom_term::pane::set_default_cursor_shape(parse_cursor_shape_config(
        &config.terminal.cursor_shape,
    ));

    match crate::shell_integration::ensure_integration_dir() {
        Ok(dir) => {
            unsafe { std::env::set_var("LOOM_SHELL_INTEGRATION_DIR", &dir) };
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
    log::info!("loomtty-server listening on {}", sock_path.display());
    #[cfg(windows)]
    log::info!("loomtty-server listening on {}", pipe_name);

    let theme = &config.theme;
    let parse = loom_term::pane::TerminalColors::parse_hex;
    let terminal_colors = loom_term::pane::TerminalColors {
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
    // Resolve the new-pane sizing policy (explicit `default_column_width`
    // override, or the Fixed/Dynamic half-vs-full choice). Dynamic mode is
    // applied per-pane against the live viewport inside the session.
    server.column_sizing = config.layout.column_sizing();
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
        web_required,
    })
}

/// Run the accept loop. Call after `prepare_daemon_with`.
pub async fn run_daemon_loop(mut ds: DaemonState) -> Result<()> {
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

    // Port collision check (runtime gate; schema-level can't see across
    // RemoteConfig and WebConfig with derive-Validate).
    if ds.config.remote.enabled
        && ds.config.web.enabled
        && ds.config.remote.port == ds.config.web.port
    {
        anyhow::bail!(
            "remote.port and web.port both set to {} — pick distinct ports \
             (defaults are 7890/7891)",
            ds.config.remote.port,
        );
    }

    let tcp_listener = if ds.config.remote.enabled {
        let addr = format!("127.0.0.1:{}", ds.config.remote.port);
        let tcp = tokio::net::TcpListener::bind(&addr).await?;
        log::warn!(
            "loomtty-server TCP listener on {addr} (remote enabled) — \
             WARNING: no authentication, intended for SSH tunnel use only"
        );
        Some(tcp)
    } else {
        None
    };

    // Web gateway: if `[web] enabled`, bind + serve the SPA and authenticated
    // `/ws` upgrade on the web port. The gateway's shutdown handle is stored on
    // `Server` so the settings-panel toggle can stop/restart it at runtime and
    // `graceful_shutdown` stops it on daemon exit. A bind failure here is
    // LOGGED and the daemon continues WITHOUT web rather than refusing to
    // start — a taken/privileged web port must not take the terminal sessions
    // down with it. The runtime toggle (or a restart) can start it later.
    if ds.config.web.enabled {
        let start = start_web_gateway(
            &ds.config.web.token,
            &ds.config.web.bind,
            ds.config.web.port,
            &ds.config.web.allowed_origins,
            &ds.config.web.static_dir,
            state.clone(),
            shutdown.clone(),
            input_notify.clone(),
        )
        .await;
        // The loaded config still holds a plaintext copy of the secret; on
        // success the zeroized SharedToken inside the gateway now owns the live
        // copy. Wipe the source regardless (even on the fatal path below) so a
        // core dump can't recover it.
        use zeroize::Zeroize;
        ds.config.web.token.zeroize();
        match start {
            Ok((addr, gateway_shutdown, task)) => {
                let mut s = state.lock().await;
                s.web_gateway_shutdown = Some(gateway_shutdown);
                s.web_gateway_task = Some(task);
                s.web_gateway_addr = Some(addr);
            }
            // Explicit `loomtty web` launch: the command promised a listener
            // (and the CLI's `--open` waits for it), so fail loudly instead of
            // running headless with no web.
            Err(e) if ds.web_required => {
                return Err(e.context("web gateway requested via --web failed to start"));
            }
            // Desktop config (`[web] enabled`): best-effort. A taken/privileged
            // web port must not take the terminal sessions down with it.
            Err(e) => {
                log::error!(
                    "[web] enabled but the gateway failed to start: {e:#} — continuing \
                     without web; use the settings toggle or restart to retry"
                );
            }
        }
    }

    // Run the accept loop inside a captured future so the web gateway is
    // stopped (via `graceful_shutdown`) and `server_exited` is set on EVERY
    // exit path — not only the clean `shutdown.notified()` break. A listener
    // or named-pipe `?` error used to return straight out of this function,
    // leaking the spawned web gateway task; the drop-guard that once covered
    // those paths went away when `start_web_gateway` was extracted.
    let accept_result: Result<()> = async {
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
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify, None));
                }
                result = tcp_accept(&tcp_listener) => {
                    let (stream, addr) = result?;
                    stream.set_nodelay(true).ok();
                    log::info!("TCP client connected from {addr}");
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify, None));
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
                        // `connect()` is one-shot; once it fails, the
                        // existing pipe_server handle is unusable. Build
                        // a fresh one so the next loop iteration has a
                        // working listener (without this, the daemon
                        // silently stops accepting pipe clients).
                        pipe_server = ServerOptions::new()
                            .reject_remote_clients(true)
                            .create(&pipe_name)?;
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
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify, None));
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
                None,
            ));
        }
    }
        Ok(())
    }
    .await;

    // Stop the web gateway (if still running) and mark the server shut down.
    // Idempotent, and runs whether the loop ended via a clean shutdown or an
    // accept error, so the spawned gateway task never outlives the daemon.
    connection::graceful_shutdown(&state).await;

    // Signal the tray thread that the server has exited.
    server_exited.store(true, Ordering::Relaxed);
    accept_result
}

/// Resolve a `[web] bind` string + port to the socket address the gateway
/// listens on: an empty bind means loopback, otherwise the string must parse
/// as an `IpAddr` (`None` if it doesn't). Mirrors the binding logic in
/// [`start_web_gateway`] so callers (the runtime toggle in `connection.rs`)
/// can compare a requested endpoint against a running gateway's address.
pub(crate) fn resolve_web_bind_addr(bind: &str, port: u16) -> Option<std::net::SocketAddr> {
    let ip = if bind.is_empty() {
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    } else {
        bind.parse().ok()?
    };
    Some(std::net::SocketAddr::new(ip, port))
}

/// Bind and spawn the web gateway, returning its bound address, a `watch`
/// cancel latch that stops it when set to `true` (or dropped), and the accept
/// loop's join handle (await it after cancelling to know the listener — and its
/// port — has been released, which a same-port rebind depends on). The same
/// latch is handed to every WebSocket session, so cancelling it also tears down
/// already-authenticated browsers, not just the accept loop. Shared by daemon
/// startup (when `[web] enabled`) and the runtime settings-panel toggle
/// (handled in `connection.rs`). Validates the bind and the non-loopback origin
/// policy — the same checks the startup path used to do inline.
pub(crate) async fn start_web_gateway(
    token: &str,
    bind: &str,
    port: u16,
    allowed_origins: &[String],
    static_dir: &str,
    state: Arc<Mutex<Server>>,
    shutdown: Arc<Notify>,
    input_notify: Arc<Notify>,
) -> Result<(
    std::net::SocketAddr,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
)> {
    // Token: trimmed, zeroized-on-drop, refcounted; enforces the 16-byte floor.
    let token = web::prepare_web_token(token)?;
    // Reject port 0 — it would ask the OS for an arbitrary ephemeral port the
    // client can't predict. The config schema rejects it; enforce the same here
    // so a hand-written / defaulted `SetWebEnabled { port: 0 }` over the socket
    // can't install a gateway on a random port and report success.
    if port == 0 {
        anyhow::bail!("[web] port must not be 0 (it would bind an arbitrary ephemeral port)");
    }
    // `bind` is empty (=> loopback) or a parseable IpAddr.
    let addr = resolve_web_bind_addr(bind, port)
        .with_context(|| format!("[web] bind {bind:?} failed to parse"))?;
    let parsed_bind = addr.ip();
    // An empty allowlist is only safe on loopback; on a non-loopback bind an
    // unscoped allowlist invites CSRF from any page in the browser — refuse.
    if !parsed_bind.is_loopback() && allowed_origins.is_empty() {
        anyhow::bail!(
            "[web] bind={parsed_bind} is not loopback but allowed_origins is empty. \
             Add the browser origin(s) you intend to serve before exposing the gateway."
        );
    }
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding web gateway to {addr}"))?;
    log::info!("loomtty-server web listener (HTTP + WS) on {addr}");
    if !parsed_bind.is_loopback() {
        log::warn!(
            "web bind={parsed_bind} is not loopback — terminate TLS upstream; \
             the gateway speaks plain http:// + ws://"
        );
        if allowed_origins.iter().any(|o| o == "null") {
            log::warn!(
                "web allowed_origins contains \"null\" on a non-loopback bind — this \
                 admits any sandboxed/file:// page in the user's browser; remove it \
                 unless you intentionally accept that risk"
            );
        }
    }
    // Per-gateway cancel latch: set to `true` (or dropped) by a runtime
    // "disable" / token rotation, or by `graceful_shutdown` on daemon exit.
    // `serve_web` stops accepting when it flips, and every WebSocket session
    // selects on it too so live browsers are torn down, not just new accepts.
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let origins = Arc::new(allowed_origins.to_vec());
    let static_dir = web::resolve_static_dir(static_dir);
    let web_state = web::WebState::new(
        token,
        origins,
        state,
        shutdown,
        input_notify,
        cancel_rx.clone(),
    );
    let app = web::build_router(web_state, static_dir);
    let task = tokio::spawn(web::serve_web(listener, app, cancel_rx));
    Ok((addr, cancel_tx, task))
}

/// Prepare + run in one call (headless / daemonize mode).
/// `config_override` injects an already-built config (used by `loomtty
/// web` / `--web*` server flags to override `[web]` settings without
/// rewriting the user's TOML); `None` loads from disk.
pub async fn run_daemon_with(
    config_override: Option<loom_config::config::LoomConfig>,
) -> Result<()> {
    let ds = prepare_daemon_with(config_override).await?;
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
