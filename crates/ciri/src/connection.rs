use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use crossbeam_channel::{Receiver, Sender};
use std::io;

pub use ciri_app::app::{RemoteProbeResult, RemoteQueryResult, ServerEvent};

/// Run the protocol IO loop over any AsyncRead + AsyncWrite pair.
/// Performs handshake, spawns writer task, runs reader loop.
async fn run_protocol_io<R, W>(
    reader: R,
    writer: W,
    viewport: &codec::ClientHello,
    msg_rx: crossbeam_channel::Receiver<ClientMessage>,
    event_tx: crossbeam_channel::Sender<ServerEvent>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = tokio::io::BufWriter::new(writer);

    // Send ClientHello (version + viewport), read ServerHello
    if let Err(e) = codec::write_client_hello(&mut writer, viewport).await {
        log::error!("hello write failed: {e}");
        let _ = event_tx.send(ServerEvent::Disconnected);
        return;
    }
    match codec::read_server_hello(&mut reader).await {
        Ok(codec::VersionCompat::Exact(v)) => {
            log::info!("server handshake ok (v{v})");
        }
        Ok(codec::VersionCompat::PatchMismatch { peer, local }) => {
            log::warn!("server version {peer} differs from client {local} (patch mismatch)");
        }
        Ok(codec::VersionCompat::MinorMismatch { peer, local }) => {
            log::warn!(
                "server version {peer} differs from client {local} (minor mismatch, may be unstable)"
            );
        }
        Err(e) => {
            log::error!("server rejected connection: {e}");
            let _ = event_tx.send(ServerEvent::Disconnected);
            return;
        }
    }

    // Spawn writer task
    let writer_msg_rx = msg_rx;
    let write_handle = tokio::spawn(async move {
        while let Ok(msg) = tokio::task::block_in_place(|| writer_msg_rx.recv()) {
            if let Err(e) = codec::encode_client_msg(&mut writer, &msg).await {
                log::warn!("write error: {e}");
                break;
            }
            use tokio::io::AsyncWriteExt;
            if writer.flush().await.is_err() {
                break;
            }
        }
    });

    // Reader loop
    loop {
        match codec::read_frame(&mut reader).await {
            Ok(codec::Frame::ServerMsg(msg)) => {
                if event_tx.send(ServerEvent::Control(msg)).is_err() {
                    break;
                }
            }
            Ok(codec::Frame::CellDelta(delta)) => {
                if event_tx.send(ServerEvent::CellDelta(delta)).is_err() {
                    break;
                }
            }
            Ok(codec::Frame::FullPaneSync(sync)) => {
                if event_tx.send(ServerEvent::FullPaneSync(sync)).is_err() {
                    break;
                }
            }
            Ok(codec::Frame::ClientMsg(_)) => {
                // Shouldn't receive client messages from server
            }
            Err(e) => {
                if e.kind() != io::ErrorKind::UnexpectedEof {
                    log::warn!("server read error: {e}");
                }
                let _ = event_tx.send(ServerEvent::Disconnected);
                break;
            }
        }
    }

    write_handle.abort();
}

/// Connect to a running server session, or spawn a new one.
/// Returns channels for bidirectional communication.
pub fn connect_or_spawn(
    session_name: &str,
    viewport: codec::ClientHello,
) -> io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
    // Validate session name using the same function as the server to prevent
    // divergent validation rules (client allows what server rejects or vice versa).
    if let Err(reason) = ciri_session::names::validate_name(session_name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid session name: {reason}"),
        ));
    }
    let _sock_path = transport::server_socket_path();

    // Spawn server if not running (non-blocking — IO thread handles retry)
    {
        let server_ready = || -> bool {
            #[cfg(unix)]
            {
                if !_sock_path.exists() {
                    return false;
                }
                // Probe the socket to distinguish live server from stale socket.
                // A stale socket (left by a crashed server) returns ConnectionRefused.
                match std::os::unix::net::UnixStream::connect(&_sock_path) {
                    Ok(_) => true,
                    Err(_) => {
                        // Socket file exists but nobody is listening — stale.
                        log::info!("removing stale server socket: {}", _sock_path.display());
                        let _ = std::fs::remove_file(&_sock_path);
                        false
                    }
                }
            }
            #[cfg(windows)]
            {
                // On Windows, opening a named pipe consumes the server's only pipe
                // instance. Use WaitNamedPipeW to probe without connecting, avoiding
                // ERROR_PIPE_BUSY (os error 231) on the real connection attempt.
                use std::os::windows::ffi::OsStrExt;
                unsafe extern "system" {
                    fn WaitNamedPipeW(name: *const u16, timeout: u32) -> i32;
                }
                let pipe_name = transport::server_pipe_name();
                let wide: Vec<u16> = std::ffi::OsStr::new(&pipe_name)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                // Timeout 0 = don't wait, just check if pipe exists
                unsafe { WaitNamedPipeW(wide.as_ptr(), 0) != 0 }
            }
        };
        if !server_ready() {
            spawn_server(session_name)?;
        }
    }

    let (msg_tx, msg_rx) = crossbeam_channel::bounded::<ClientMessage>(256);
    // Unbounded: reader must never block on send, otherwise it can't detect
    // socket EOF and the TUI freezes.  Memory is bounded in practice because
    // the main thread drains ~12 000 events/sec (budget=200 × 60 fps).
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<ServerEvent>();

    let _session = session_name.to_string();
    std::thread::Builder::new()
        .name("server-io".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async move {
                // Retry connecting with backoff (server may still be starting)
                let mut connect_result = Err(io::Error::new(io::ErrorKind::ConnectionRefused, ""));
                for attempt in 0..50 {
                    #[cfg(unix)]
                    {
                        let sock_path = transport::server_socket_path();
                        connect_result = tokio::net::UnixStream::connect(&sock_path).await;
                    }
                    #[cfg(windows)]
                    {
                        connect_result = tokio::net::windows::named_pipe::ClientOptions::new()
                            .open(&transport::server_pipe_name());
                    }
                    if connect_result.is_ok() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    if attempt == 0 {
                        log::debug!("waiting for server...");
                    }
                }
                let connect_result = connect_result;

                let stream = match connect_result {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to connect to server: {e}");
                        let _ = event_tx.send(ServerEvent::Disconnected);
                        return;
                    }
                };

                #[cfg(unix)]
                let (reader, writer) = stream.into_split();
                #[cfg(windows)]
                let (reader, writer) = tokio::io::split(stream);
                run_protocol_io(reader, writer, &viewport, msg_rx, event_tx).await;
            });
        })?;

    Ok((msg_tx, event_rx))
}

/// Validate an SSH hostname/address using an allowlist approach modeled on
/// OpenSSH's `valid_domain()` from `misc.c`.  An allowlist is safer than a
/// blocklist because it rejects unknown-dangerous characters by default.
///
/// Allowed forms:
///   - Hostnames: `[a-zA-Z0-9][a-zA-Z0-9._-]*` (no consecutive dots)
///   - user@host: `@` permitted for SSH user syntax
///   - IPv6 literals: `[::1]` — brackets, colons, hex digits
///   - IPv4 addresses: digits and dots
fn validate_ssh_host(host: &str) -> io::Result<()> {
    if host.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SSH host cannot be empty",
        ));
    }
    // Reject hosts starting with '-' (could be interpreted as SSH flags)
    if host.starts_with('-') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SSH host cannot start with '-'",
        ));
    }
    // First char must be alphanumeric, '_', or '[' (IPv6 literal).
    let first = host.chars().next().unwrap();
    if !first.is_ascii_alphanumeric() && first != '_' && first != '[' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("SSH host starts with invalid character: {first:?}"),
        ));
    }
    // Allowlist: only characters that are safe in hostnames, IPv4/IPv6, and
    // user@host syntax.  This matches OpenSSH's valid_domain() plus extensions
    // for user@host and IPv6 brackets.
    for ch in host.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '@' | ':' | '[' | ']' | '%')
        {
            continue;
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("SSH host contains disallowed character: {ch:?}"),
        ));
    }
    // Reject consecutive dots (invalid hostname, potential path traversal).
    if host.contains("..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SSH host contains consecutive dots",
        ));
    }
    Ok(())
}

/// Connect to a remote ciri-server via SSH stdio proxy tunnel.
/// If the remote server is not running, attempts to start it via
/// `ssh ciri-server --daemonize` before connecting.
pub fn connect_remote(
    host: &str,
    remote_port: u16,
    ssh_port: u16,
    viewport: codec::ClientHello,
) -> io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
    validate_ssh_host(host)?;

    let (msg_tx, msg_rx) = crossbeam_channel::bounded::<ClientMessage>(256);
    let (event_tx, event_rx) = crossbeam_channel::bounded::<ServerEvent>(256);

    let host = host.to_string();

    std::thread::Builder::new()
        .name("remote-io".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async move {
                // Try to ensure the remote server is running before connecting.
                if let Err(e) = ensure_remote_server(&host, remote_port, ssh_port).await {
                    log::error!("failed to ensure remote server: {e}");
                    let _ = event_tx.send(ServerEvent::Disconnected);
                    return;
                }

                // Spawn SSH process with -W for stdio proxy
                let mut child = match spawn_ssh_tunnel(&host, remote_port, ssh_port) {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("failed to spawn ssh: {e}");
                        let _ = event_tx.send(ServerEvent::Disconnected);
                        return;
                    }
                };

                let stdin = child.stdin.take().expect("stdin");
                let stdout = child.stdout.take().expect("stdout");

                // Run protocol over SSH tunnel
                // stdout = data from remote server (reader)
                // stdin  = data to remote server (writer)
                run_protocol_io(stdout, stdin, &viewport, msg_rx, event_tx).await;

                // Clean up SSH process
                let _ = child.kill().await;
            });
        })?;

    Ok((msg_tx, event_rx))
}

fn spawn_ssh_tunnel(
    host: &str,
    remote_port: u16,
    ssh_port: u16,
) -> io::Result<tokio::process::Child> {
    tokio::process::Command::new("ssh")
        .args(["-p", &ssh_port.to_string()])
        .args(["-W", &format!("localhost:{remote_port}")])
        // Let the user's SSH config handle host key verification.
        // Do not override StrictHostKeyChecking — auto-accepting unknown
        // keys enables MITM attacks on first connection.
        .arg(host)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
}

/// Ensure the remote ciri-server is running. Probes first, starts if needed.
async fn ensure_remote_server(host: &str, remote_port: u16, ssh_port: u16) -> io::Result<()> {
    // Quick probe: try connecting to see if server is already up.
    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        probe_remote(host, remote_port, ssh_port),
    )
    .await;

    match probe {
        Ok(RemoteProbeResult::Sessions(_)) => {
            log::info!("remote server already running");
            return Ok(());
        }
        _ => {
            log::info!("remote server not reachable, attempting to start");
        }
    }

    // Start the server via SSH.
    let output = tokio::process::Command::new("ssh")
        .args(["-p", &ssh_port.to_string()])
        .args(["-o", "ConnectTimeout=10"])
        // Let the user's SSH config handle host key verification.
        // Do not override StrictHostKeyChecking — auto-accepting unknown
        // keys enables MITM attacks on first connection.
        .arg(host)
        .arg("ciri-server --daemonize")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if output.status.success() {
        log::info!("remote ciri-server started, verifying");
        // Wait for the server to bind its socket, then verify it's reachable.
        for attempt in 0..5 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let verify = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                probe_remote(host, remote_port, ssh_port),
            )
            .await;
            if matches!(verify, Ok(RemoteProbeResult::Sessions(_))) {
                log::info!("remote server verified on attempt {}", attempt + 1);
                return Ok(());
            }
        }
        return Err(io::Error::other(
            "remote ciri-server started but not reachable after 5 attempts",
        ));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(-1);

    // Exit code 127 = command not found on most shells.
    if code == 127 || stderr.contains("not found") || stderr.contains("No such file") {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "ciri-server not found on remote host. Install it with: cargo install ciri-server",
        ));
    }

    Err(io::Error::other(format!(
        "failed to start remote server (exit {code}): {stderr}"
    )))
}

/// Fire-and-forget: probe a remote host for ciri-server, query its sessions.
/// Sends the result through `result_tx`. Runs entirely in a background thread.
pub fn query_remote_sessions(
    host_name: &str,
    host: &str,
    remote_port: u16,
    ssh_port: u16,
    result_tx: crossbeam_channel::Sender<RemoteQueryResult>,
) {
    let host_name = host_name.to_string();
    let host = host.to_string();

    std::thread::Builder::new()
        .name("remote-query".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");

            let result = rt.block_on(async {
                // 5-second timeout for the entire probe
                match tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    probe_remote(&host, remote_port, ssh_port),
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => RemoteProbeResult::Error("Connection timed out".into()),
                }
            });

            let _ = result_tx.send(RemoteQueryResult {
                host_name,
                host,
                port: remote_port,
                ssh_port,
                result,
            });
        })
        .ok();
}

/// Internal: SSH tunnel → handshake → ListSessions → return result.
async fn probe_remote(host: &str, remote_port: u16, ssh_port: u16) -> RemoteProbeResult {
    // Spawn SSH process
    let mut child = match tokio::process::Command::new("ssh")
        .args(["-p", &ssh_port.to_string()])
        .args(["-W", &format!("localhost:{remote_port}")])
        .args(["-o", "ConnectTimeout=4"])
        // Let the user's SSH config handle host key verification.
        // Do not override StrictHostKeyChecking — auto-accepting unknown
        // keys enables MITM attacks on first connection.
        .arg(host)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return RemoteProbeResult::Error(format!("SSH spawn failed: {e}")),
    };

    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut writer = tokio::io::BufWriter::new(stdin);

    // Handshake with a dummy viewport
    let hello = codec::ClientHello {
        width: 800,
        height: 600,
        cell_width: 10.0,
        cell_height: 20.0,
        session_name: "__control__".to_string(),
    };
    if codec::write_client_hello(&mut writer, &hello)
        .await
        .is_err()
    {
        let _ = child.kill().await;
        return RemoteProbeResult::NoServer;
    }
    match codec::read_server_hello(&mut reader).await {
        Ok(_) => {}
        Err(_) => {
            let _ = child.kill().await;
            return RemoteProbeResult::NoServer;
        }
    }

    // Send ListSessions
    let msg = ClientMessage::ListSessions { all: true };
    if codec::encode_client_msg(&mut writer, &msg).await.is_err() {
        let _ = child.kill().await;
        return RemoteProbeResult::NoServer;
    }
    use tokio::io::AsyncWriteExt;
    let _ = writer.flush().await;

    // Read frames until we get a SessionList
    loop {
        match codec::read_frame(&mut reader).await {
            Ok(codec::Frame::ServerMsg(ServerMessage::SessionList { sessions })) => {
                let _ = child.kill().await;
                return RemoteProbeResult::Sessions(sessions);
            }
            Ok(_) => continue, // skip StateSync, FullPaneSync, etc.
            Err(_) => {
                let _ = child.kill().await;
                return RemoteProbeResult::NoServer;
            }
        }
    }
}

/// Synchronously probe a remote server and return session names (sorted by
/// last_attached, most recent first — as returned by the server).
/// Used at startup to pick a default session when the user didn't specify one.
pub fn probe_remote_sessions_blocking(host: &str, remote_port: u16, ssh_port: u16) -> Vec<String> {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };

    let result = rt.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(8),
            probe_remote(host, remote_port, ssh_port),
        )
        .await
    });

    match result {
        Ok(RemoteProbeResult::Sessions(sessions)) => {
            sessions.into_iter().map(|s| s.name).collect()
        }
        _ => Vec::new(),
    }
}

fn spawn_server(_session_name: &str) -> io::Result<()> {
    use std::process::Command;
    // Try to find ciri-server binary next to the current executable
    let exe = std::env::current_exe().unwrap_or_default();
    let server_bin = if cfg!(windows) {
        "ciri-server.exe"
    } else {
        "ciri-server"
    };
    let server_exe = exe
        .parent()
        .map(|p| p.join(server_bin))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(server_bin));

    // Ensure socket directory exists
    let sock_path = transport::server_socket_path();
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    log::info!("spawning server: {}", server_exe.display());

    // Fork server as daemon (setsid for session independence)
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new(&server_exe);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        cmd.spawn()?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        Command::new(&server_exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()?;
    }

    Ok(())
}
