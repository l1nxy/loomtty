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

    // Web gateway: serve the SPA + the authenticated `/ws` upgrade on the
    // `[web]` port via an axum app on its own task. Bound here (not in the
    // accept loop) so a bind failure still propagates synchronously. Its
    // graceful shutdown rides a dedicated Notify (not the master
    // `shutdown`, whose `notify_one` would otherwise be stolen from the
    // accept loop), triggered when this function returns by ANY path —
    // normal shutdown, an early `?` from the accept loop, or an unwind —
    // via the drop guard below (so the serve task can never be left
    // waiting on `serve_shutdown.notified()`).
    let web_shutdown = Arc::new(Notify::new());
    let _web_shutdown_guard = NotifyOnDrop(web_shutdown.clone());
    if ds.config.web.enabled {
        // Token: trimmed, zeroized-on-drop, refcounted. The helper enforces
        // the 16-byte floor and strips a trailing newline so a stray one in
        // the config does not silently break auth.
        let token = web::prepare_web_token(&ds.config.web.token)?;
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
        // An empty allowlist is only safe on loopback (only same-host pages
        // can reach the listener); on a non-loopback bind an unscoped
        // allowlist invites CSRF from any page in the browser — refuse.
        if !parsed_bind.is_loopback() && ds.config.web.allowed_origins.is_empty() {
            anyhow::bail!(
                "[web] bind={parsed_bind} is not loopback but allowed_origins is empty. \
                 Add the browser origin(s) you intend to serve (e.g. \
                 allowed_origins = [\"https://terminal.example.com\"]) before exposing \
                 the gateway."
            );
        }
        // Construct the listener from the canonical IpAddr so the logged
        // address and the loopback check agree on one normalised form
        // (matters for IPv6: `::0001` and `::1` parse equal but compare as
        // different strings).
        let addr = std::net::SocketAddr::new(parsed_bind, ds.config.web.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        log::info!("loomtty-server web listener (HTTP + WS) on {addr}");
        if !parsed_bind.is_loopback() {
            log::warn!(
                "web bind={parsed_bind} is not loopback — terminate TLS upstream \
                 and ensure the token is rotated; the gateway speaks plain http:// + ws://"
            );
            // `"null"` is the Origin every sandboxed iframe, `data:` URI,
            // `file://` page, and some redirected cross-origin request
            // shares — on a public bind, listing it is close to "no Origin
            // check at all" for anyone who can lure a victim into a local
            // file.
            if ds.config.web.allowed_origins.iter().any(|o| o == "null") {
                log::warn!(
                    "web allowed_origins contains \"null\" on a non-loopback bind — \
                     this admits any sandboxed/file:// page in the user's browser; \
                     remove it unless you intentionally accept that risk"
                );
            }
        }
        let origins = Arc::new(ds.config.web.allowed_origins.clone());
        let static_dir = web::resolve_static_dir(&ds.config.web.static_dir);
        // The loaded config still holds a plaintext copy of the secret; the
        // zeroized SharedToken now owns the live copy. Wipe the source so a
        // core dump or post-startup memory read can't recover it from
        // `ds.config` (best-effort: the allocator may have reused the
        // buffer already, but it's free defense).
        use zeroize::Zeroize;
        ds.config.web.token.zeroize();
        let web_state = web::WebState::new(
            token,
            origins,
            state.clone(),
            shutdown.clone(),
            input_notify.clone(),
        );
        let app = web::build_router(web_state, static_dir);
        let serve_shutdown = web_shutdown.clone();
        // Serve the SPA + `/ws` over hyper directly (with a header-read
        // timeout) rather than `axum::serve`, so stalled or idle pre-auth
        // sockets can't pin accept permits. `serve_web` stops accepting when
        // `serve_shutdown` fires (driven by the drop guard above on every
        // exit path). See `daemon::web::serve_web`.
        tokio::spawn(web::serve_web(listener, app, serve_shutdown));
    }

    loop {
        #[cfg(unix)]
        {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            // Transient errors (EMFILE/ENFILE/ECONNABORTED)
                            // must not tear down the accept loop — log and
                            // keep accepting.
                            log::warn!("unix accept failed: {e}");
                            continue;
                        }
                    };
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
                    let (stream, addr) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            // Transient accept errors must not tear down the
                            // loop — log and keep accepting.
                            log::warn!("tcp accept failed: {e}");
                            continue;
                        }
                    };
                    stream.set_nodelay(true).ok();
                    log::info!("TCP client connected from {addr}");
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
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
                    let (stream, addr) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            // Transient accept errors must not tear down the
                            // loop — log and keep accepting.
                            log::warn!("tcp accept failed: {e}");
                            continue;
                        }
                    };
                    stream.set_nodelay(true).ok();
                    log::info!("TCP client connected from {addr}");
                    let state = state.clone();
                    let client_shutdown = shutdown.clone();
                    let client_input_notify = input_notify.clone();
                    let (reader, writer) = stream.into_split();
                    tokio::spawn(connection::handle_client(reader, writer, state, client_shutdown, client_input_notify));
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

    // The web server task is signalled by `_web_shutdown_guard` on the way
    // out of this function (it fires on drop), so no explicit call here.

    // Signal the tray thread that the server has exited.
    server_exited.store(true, Ordering::Relaxed);
    Ok(())
}

/// Fires a `Notify` exactly once on drop. Used to drive the web server's
/// graceful shutdown on EVERY exit path of the accept loop — the normal
/// `break`, an early `?` (e.g. a fatal listener error), or an unwind —
/// not just the happy path. `notify_one` (not `notify_waiters`) so the
/// signal is stored if the serve task hasn't yet reached `.notified()`.
struct NotifyOnDrop(Arc<Notify>);

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
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

