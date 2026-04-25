use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use crossbeam_channel::{Receiver, Sender};
use std::io;
use std::sync::{Arc, Mutex};
use winit::event_loop::EventLoopProxy;

pub use ciri_app::app::{DisconnectReason, RemoteProbeResult, RemoteQueryResult, ServerEvent};

/// Max bytes of ssh stderr to retain for error classification. 8 KiB comfortably
/// covers the multi-line error messages openssh emits and caps memory if the
/// far end decides to spew.
const STDERR_TAIL_CAP: usize = 8 * 1024;

/// Grace window for receiving the first protocol byte. If the server hello
/// hasn't arrived within this window, the handshake is treated as timed out
/// so we can surface a crisp reason instead of hanging indefinitely.
const HANDSHAKE_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

/// Pick the best `DisconnectReason` for a just-ended ssh tunnel, in the
/// order: trust ssh's own stderr classification first, fall back to whatever
/// the protocol loop reported, and only invent a generic `Other` when ssh
/// exited non-zero without a recognizable message.
fn classify_disconnect(
    proto_result: Result<(), DisconnectReason>,
    stderr_trimmed: &str,
    exit_status: Option<std::process::ExitStatus>,
    elapsed: std::time::Duration,
) -> DisconnectReason {
    if !stderr_trimmed.is_empty()
        && let Some(classified) = classify_ssh_stderr(stderr_trimmed)
    {
        return classified;
    }
    let exit_nonzero = matches!(&exit_status, Some(s) if !s.success());
    let before_handshake = elapsed < HANDSHAKE_GRACE
        || matches!(
            proto_result,
            Err(DisconnectReason::HandshakeFailed(_)) | Err(DisconnectReason::Timeout)
        );
    match proto_result {
        Err(r) => r,
        Ok(()) if exit_nonzero && before_handshake => {
            if stderr_trimmed.is_empty() {
                DisconnectReason::Other("ssh exited".into())
            } else {
                DisconnectReason::Other(format!(
                    "ssh exited: {}",
                    stderr_trimmed.lines().last().unwrap_or(stderr_trimmed)
                ))
            }
        }
        Ok(()) => DisconnectReason::RemoteEof,
    }
}

/// Classify an ssh stderr tail into a `DisconnectReason`. Anchored to the
/// canonical OpenSSH error strings so we don't chase locale-translated text —
/// those are stable across versions.
fn classify_ssh_stderr(tail: &str) -> Option<DisconnectReason> {
    let lower = tail.to_ascii_lowercase();
    // Keep a short human summary rather than the whole ring buffer.
    let summary = tail
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(tail)
        .trim()
        .to_string();
    if lower.contains("could not resolve hostname") || lower.contains("name or service not known") {
        return Some(DisconnectReason::DnsFailure(summary));
    }
    if lower.contains("permission denied") {
        return Some(DisconnectReason::PermissionDenied(summary));
    }
    if lower.contains("host key verification failed")
        || lower.contains("remote host identification has changed")
    {
        return Some(DisconnectReason::HostKeyChanged(summary));
    }
    if lower.contains("connection refused") {
        return Some(DisconnectReason::ConnectionRefused(summary));
    }
    if lower.contains("connection timed out") || lower.contains("operation timed out") {
        return Some(DisconnectReason::Timeout);
    }
    None
}

/// Shared handle to a bounded ring buffer of the most recent ssh stderr bytes.
#[derive(Clone, Default)]
struct StderrTail(Arc<Mutex<Vec<u8>>>);

impl StderrTail {
    fn snapshot(&self) -> String {
        let guard = self.0.lock().unwrap();
        String::from_utf8_lossy(&guard).into_owned()
    }
}

/// Drain an `AsyncRead` into the shared tail buffer, keeping only the last
/// `STDERR_TAIL_CAP` bytes. Completes when the stream closes.
async fn drain_stderr<R>(mut reader: R, tail: StderrTail)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let mut guard = tail.0.lock().unwrap();
                guard.extend_from_slice(&buf[..n]);
                if guard.len() > STDERR_TAIL_CAP {
                    let drop = guard.len() - STDERR_TAIL_CAP;
                    guard.drain(..drop);
                }
            }
            Err(_) => break,
        }
    }
}

/// A spawned ssh stdio-tunnel with its stderr drain task already running.
/// Centralizes the "pipe + drain + kill + wait" dance so `connect_remote`
/// and `probe_remote` can't drift on the details (stderr must be read or
/// ssh blocks; kill must precede wait; the drain task must be awaited
/// before snapshotting the tail).
struct SshTunnel {
    child: tokio::process::Child,
    stdin: Option<tokio::process::ChildStdin>,
    stdout: Option<tokio::process::ChildStdout>,
    stderr_task: tokio::task::JoinHandle<()>,
    tail: StderrTail,
}

impl SshTunnel {
    /// Spawn `ssh -p PORT -W localhost:REMOTE [extra_opts…] -- HOST` with
    /// all three std streams piped. `extra_opts` is forwarded verbatim
    /// between the base forwarding flags and the `--` separator.
    async fn spawn(
        host: &str,
        remote_port: u16,
        ssh_port: u16,
        extra_opts: &[&str],
    ) -> io::Result<Self> {
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.args(["-p", &ssh_port.to_string()])
            .args(["-W", &format!("localhost:{remote_port}")])
            .args(extra_opts)
            .arg("--")
            .arg(host)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Harden against future early-returns / panics between spawn and
            // shutdown: without this the ssh child would linger until the
            // tokio runtime tears down.
            .kill_on_drop(true);
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let tail = StderrTail::default();
        let stderr_task = tokio::spawn(drain_stderr(stderr, tail.clone()));
        Ok(SshTunnel {
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr_task,
            tail,
        })
    }

    /// Consume the stdio pair so the caller can hand them to a protocol
    /// helper. Callable once.
    fn take_stdio(&mut self) -> (tokio::process::ChildStdin, tokio::process::ChildStdout) {
        (
            self.stdin.take().expect("stdio already taken"),
            self.stdout.take().expect("stdio already taken"),
        )
    }

    /// Kill ssh, wait for it to exit, await the stderr drain, and return the
    /// exit status alongside the trimmed tail. Invariant: after this returns,
    /// no background resources from this tunnel are still running.
    async fn shutdown(mut self) -> (Option<std::process::ExitStatus>, String) {
        let _ = self.child.kill().await;
        let exit_status = self.child.wait().await.ok();
        let _ = self.stderr_task.await;
        let snapshot = self.tail.snapshot();
        (exit_status, snapshot.trim().to_string())
    }
}

/// Run the protocol IO loop over any AsyncRead + AsyncWrite pair.
/// Performs handshake, spawns writer task, runs reader loop.
///
/// Returns `Ok(())` for clean mid-session drops (EOF) and
/// `Err(DisconnectReason)` for handshake / IO failures — the caller combines
/// this with spawn-time signals (stderr tail, exit status) to produce the
/// final reason sent on the event channel.
async fn run_protocol_io<R, W>(
    reader: R,
    writer: W,
    viewport: &codec::ClientHello,
    msg_rx: crossbeam_channel::Receiver<ClientMessage>,
    event_tx: crossbeam_channel::Sender<ServerEvent>,
    wake_proxy: Option<EventLoopProxy<()>>,
) -> Result<(), DisconnectReason>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = tokio::io::BufWriter::new(writer);

    // Send ClientHello (version + viewport), read ServerHello
    if let Err(e) = codec::write_client_hello(&mut writer, viewport).await {
        return Err(DisconnectReason::HandshakeFailed(format!(
            "hello write failed: {e}"
        )));
    }
    // Bound the hello read so DNS-failed ssh tunnels don't wedge the thread
    // for a minute before the stderr reaper concludes.
    let hello =
        match tokio::time::timeout(HANDSHAKE_GRACE, codec::read_server_hello(&mut reader)).await {
            Err(_) => return Err(DisconnectReason::Timeout),
            Ok(Err(e)) => {
                return Err(DisconnectReason::HandshakeFailed(format!(
                    "server rejected connection: {e}"
                )));
            }
            Ok(Ok(v)) => v,
        };
    match hello {
        codec::VersionCompat::Exact(v) => log::info!("server handshake ok (v{v})"),
        codec::VersionCompat::PatchMismatch { peer, local } => {
            log::warn!("server version {peer} differs from client {local} (patch mismatch)");
        }
        codec::VersionCompat::MinorMismatch { peer, local } => {
            log::warn!(
                "server version {peer} differs from client {local} (minor mismatch, may be unstable)"
            );
        }
    }

    // Spawn writer task
    let writer_msg_rx = msg_rx;
    let write_handle = tokio::spawn(async move {
        loop {
            // Use blocking recv in a spawned blocking task to avoid busy-waiting
            let msg = match tokio::task::block_in_place(|| writer_msg_rx.recv()) {
                Ok(m) => m,
                Err(_) => break, // sender dropped
            };
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

    // Reader loop — reuse a single buffer across frames to avoid per-frame allocation.
    let mut frame_buf = Vec::with_capacity(64 * 1024);
    let mut exit: Result<(), DisconnectReason> = Ok(());
    loop {
        match codec::read_frame_reuse(&mut reader, &mut frame_buf).await {
            Ok(codec::Frame::ServerMsg(msg)) => {
                if event_tx.send(ServerEvent::Control(msg)).is_err() {
                    break;
                }
                if let Some(ref proxy) = wake_proxy {
                    let _ = proxy.send_event(());
                }
            }
            Ok(codec::Frame::CellDelta(delta)) => {
                if event_tx.send(ServerEvent::CellDelta(delta)).is_err() {
                    break;
                }
                if let Some(ref proxy) = wake_proxy {
                    let _ = proxy.send_event(());
                }
            }
            Ok(codec::Frame::FullPaneSync(sync)) => {
                if event_tx.send(ServerEvent::FullPaneSync(sync)).is_err() {
                    break;
                }
                if let Some(ref proxy) = wake_proxy {
                    let _ = proxy.send_event(());
                }
            }
            Ok(codec::Frame::ClientMsg(_)) => {
                // Shouldn't receive client messages from server
            }
            Err(e) => {
                exit = Err(if e.kind() == io::ErrorKind::UnexpectedEof {
                    DisconnectReason::RemoteEof
                } else {
                    log::warn!("server read error: {e}");
                    DisconnectReason::Other(format!("server read error: {e}"))
                });
                break;
            }
        }
    }

    write_handle.abort();
    exit
}

/// Connect to a running server session, or spawn a new one.
/// Returns channels for bidirectional communication. `cancel` can be notified
/// to unwind the connection early (Esc while waiting for the first StateSync).
pub fn connect_or_spawn(
    session_name: &str,
    viewport: codec::ClientHello,
    wake_proxy: Option<EventLoopProxy<()>>,
    cancel: Arc<tokio::sync::Notify>,
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
                        let _ = event_tx.send(ServerEvent::Disconnected(DisconnectReason::Other(
                            format!("failed to connect to local server: {e}"),
                        )));
                        if let Some(ref proxy) = wake_proxy {
                            let _ = proxy.send_event(());
                        }
                        return;
                    }
                };

                #[cfg(unix)]
                let (reader, writer) = stream.into_split();
                #[cfg(windows)]
                let (reader, writer) = tokio::io::split(stream);
                let reason = tokio::select! {
                    r = run_protocol_io(
                        reader,
                        writer,
                        &viewport,
                        msg_rx,
                        event_tx.clone(),
                        wake_proxy.clone(),
                    ) => match r {
                        Ok(()) => DisconnectReason::RemoteEof,
                        Err(r) => r,
                    },
                    _ = cancel.notified() => DisconnectReason::Cancelled,
                };
                let _ = event_tx.send(ServerEvent::Disconnected(reason));
                if let Some(ref proxy) = wake_proxy {
                    let _ = proxy.send_event(());
                }
            });
        })?;

    Ok((msg_tx, event_rx))
}

/// Connect to a remote ciri-server via SSH stdio proxy tunnel. `cancel` can
/// be notified to kill the ssh child and surface `DisconnectReason::Cancelled`.
pub fn connect_remote(
    host: &str,
    remote_port: u16,
    ssh_port: u16,
    viewport: codec::ClientHello,
    wake_proxy: Option<EventLoopProxy<()>>,
    cancel: Arc<tokio::sync::Notify>,
) -> io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
    // Defence-in-depth: refuse to reach `ssh` with an unvalidated host even if
    // a future caller forgets to run its own check. `--` in the argv already
    // blocks option-injection, but keeping argv hygiene as a second layer.
    if let Err(e) = crate::remote_validate::validate_fields(host, remote_port, ssh_port) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid remote target: {e}"),
        ));
    }
    let (msg_tx, msg_rx) = crossbeam_channel::bounded::<ClientMessage>(256);
    // Unbounded: reader must never block on send, otherwise it can't detect
    // socket EOF and the TUI freezes.  Memory is bounded in practice because
    // the main thread drains ~12 000 events/sec (budget=200 × 60 fps).
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<ServerEvent>();

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
                let spawn_started = std::time::Instant::now();
                let mut tunnel = match SshTunnel::spawn(&host, remote_port, ssh_port, &[]).await {
                    Ok(t) => t,
                    Err(e) => {
                        log::error!("failed to spawn ssh: {e}");
                        let reason = if e.kind() == io::ErrorKind::NotFound {
                            DisconnectReason::SshNotFound
                        } else {
                            DisconnectReason::SshSpawnFailed(e.to_string())
                        };
                        let _ = event_tx.send(ServerEvent::Disconnected(reason));
                        if let Some(ref proxy) = wake_proxy {
                            let _ = proxy.send_event(());
                        }
                        return;
                    }
                };

                let (stdin, stdout) = tunnel.take_stdio();
                // Race the protocol loop against the cancel notify so Esc
                // during "Connecting" takes effect immediately instead of
                // waiting for an ssh timeout.
                let (proto_result, cancelled) = tokio::select! {
                    r = run_protocol_io(
                        stdout,
                        stdin,
                        &viewport,
                        msg_rx,
                        event_tx.clone(),
                        wake_proxy.clone(),
                    ) => (r, false),
                    _ = cancel.notified() => (Ok(()), true),
                };
                let (exit_status, stderr_trimmed) = tunnel.shutdown().await;

                let reason = if cancelled {
                    DisconnectReason::Cancelled
                } else {
                    classify_disconnect(
                        proto_result,
                        &stderr_trimmed,
                        exit_status,
                        spawn_started.elapsed(),
                    )
                };

                let _ = event_tx.send(ServerEvent::Disconnected(reason));
                if let Some(ref proxy) = wake_proxy {
                    let _ = proxy.send_event(());
                }
            });
        })?;

    Ok((msg_tx, event_rx))
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
                match tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    probe_remote(&host, remote_port, ssh_port),
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => RemoteProbeResult::Error("connection timed out".into()),
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

pub fn probe_remote_sessions_blocking(host: &str, remote_port: u16, ssh_port: u16) -> Vec<String> {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            log::warn!("failed to build probe runtime: {e}");
            return Vec::new();
        }
    };

    let result = rt.block_on(async {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            probe_remote(host, remote_port, ssh_port),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => RemoteProbeResult::Error("connection timed out".into()),
        }
    });

    match result {
        RemoteProbeResult::Sessions(sessions) => sessions.into_iter().map(|s| s.name).collect(),
        RemoteProbeResult::NoServer | RemoteProbeResult::Error(_) => Vec::new(),
    }
}

/// Internal: SSH tunnel → handshake → ListSessions → return result.
///
/// Three-way outcome:
///   - `Sessions(...)` — reached a ciri-server and listed its sessions.
///   - `NoServer` — ssh connected, but nothing answered the handshake
///     (ciri-server isn't running on the remote).
///   - `Error(msg)` — ssh itself failed (DNS, auth, refused, spawn). Stderr
///     tail drives the message, so the palette displays the real cause
///     instead of the misleading "(no ciritty-server)".
async fn probe_remote(host: &str, remote_port: u16, ssh_port: u16) -> RemoteProbeResult {
    if let Err(e) = crate::remote_validate::validate_fields(host, remote_port, ssh_port) {
        return RemoteProbeResult::Error(format!("invalid remote target: {e}"));
    }

    let mut tunnel = match SshTunnel::spawn(
        host,
        remote_port,
        ssh_port,
        &[
            "-o",
            "ConnectTimeout=4",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                return RemoteProbeResult::Error("ssh binary not found in PATH".into());
            }
            return RemoteProbeResult::Error(format!("ssh spawn failed: {e}"));
        }
    };

    let (stdin, stdout) = tunnel.take_stdio();
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut writer = tokio::io::BufWriter::new(stdin);

    let hello = codec::ClientHello {
        width: 800,
        height: 600,
        cell_width: 10.0,
        cell_height: 20.0,
        session_name: "__probe__".to_string(),
    };

    // Wrap the whole handshake+ListSessions sequence so every early return
    // funnels through one cleanup path. We don't care *which* stage tripped —
    // the cleanup + stderr classification handles the diagnosis.
    let outcome: Result<Vec<SessionInfo>, ()> = async {
        codec::write_client_hello(&mut writer, &hello)
            .await
            .map_err(|_| ())?;
        codec::read_server_hello(&mut reader)
            .await
            .map_err(|_| ())?;
        codec::encode_client_msg(&mut writer, &ClientMessage::ListSessions { all: true })
            .await
            .map_err(|_| ())?;
        use tokio::io::AsyncWriteExt;
        let _ = writer.flush().await;
        loop {
            match codec::read_frame(&mut reader).await {
                Ok(codec::Frame::ServerMsg(ServerMessage::SessionList { sessions })) => {
                    return Ok(sessions);
                }
                Ok(_) => continue,
                Err(_) => return Err(()),
            }
        }
    }
    .await;

    let (_exit, stderr_trimmed) = tunnel.shutdown().await;

    match outcome {
        Ok(sessions) => RemoteProbeResult::Sessions(sessions),
        Err(()) => {
            // If ssh printed something recognizable it was an ssh-layer
            // failure (DNS/auth/refused), not "no server listening".
            if !stderr_trimmed.is_empty() {
                if let Some(reason) = classify_ssh_stderr(&stderr_trimmed) {
                    return RemoteProbeResult::Error(reason.to_string());
                }
                return RemoteProbeResult::Error(
                    stderr_trimmed
                        .lines()
                        .last()
                        .unwrap_or(&stderr_trimmed)
                        .to_string(),
                );
            }
            // ssh exited quietly or is still happy — the remote just isn't
            // running a ciri-server.
            RemoteProbeResult::NoServer
        }
    }
}

fn spawn_server(_session_name: &str) -> io::Result<()> {
    use std::process::Command;
    // Try to find ciritty-server binary next to the current executable
    let exe = std::env::current_exe().unwrap_or_default();
    let server_bin = if cfg!(windows) {
        "ciritty-server.exe"
    } else {
        "ciritty-server"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_dns_failure() {
        let tail = "ssh: Could not resolve hostname bogus.example: Name or service not known\r\n";
        assert!(matches!(
            classify_ssh_stderr(tail),
            Some(DisconnectReason::DnsFailure(_))
        ));
    }

    #[test]
    fn classify_permission_denied() {
        let tail = "Permission denied (publickey,password).\r\n";
        assert!(matches!(
            classify_ssh_stderr(tail),
            Some(DisconnectReason::PermissionDenied(_))
        ));
    }

    #[test]
    fn classify_host_key_changed() {
        let tail = "@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\nHost key verification failed.\n";
        assert!(matches!(
            classify_ssh_stderr(tail),
            Some(DisconnectReason::HostKeyChanged(_))
        ));
    }

    #[test]
    fn classify_connection_refused() {
        let tail = "ssh: connect to host example.com port 22: Connection refused\r\n";
        assert!(matches!(
            classify_ssh_stderr(tail),
            Some(DisconnectReason::ConnectionRefused(_))
        ));
    }

    #[test]
    fn classify_timeout() {
        let tail = "ssh: connect to host example.com port 22: Operation timed out\r\n";
        assert!(matches!(
            classify_ssh_stderr(tail),
            Some(DisconnectReason::Timeout)
        ));
    }

    #[test]
    fn classify_unknown_returns_none() {
        assert!(classify_ssh_stderr("Debug: pure log noise").is_none());
        assert!(classify_ssh_stderr("").is_none());
    }

    #[test]
    fn permanent_flag_separates_retry_policy() {
        use DisconnectReason::*;
        assert!(DnsFailure("x".into()).is_permanent());
        assert!(PermissionDenied("x".into()).is_permanent());
        assert!(HostKeyChanged("x".into()).is_permanent());
        assert!(ConnectionRefused("x".into()).is_permanent());
        assert!(SshNotFound.is_permanent());
        assert!(InvalidTarget("x".into()).is_permanent());
        assert!(Cancelled.is_permanent());

        assert!(!Timeout.is_permanent());
        assert!(!RemoteEof.is_permanent());
        assert!(!Other("transient".into()).is_permanent());
    }

    #[test]
    fn classify_disconnect_prefers_stderr_match_over_proto_timeout() {
        // When ssh prints "Could not resolve hostname…", we want the reason
        // to be DnsFailure even though the protocol loop timed out waiting
        // for the server hello that never came.
        let r = classify_disconnect(
            Err(DisconnectReason::Timeout),
            "ssh: Could not resolve hostname foo: Name or service not known",
            Some(fake_exit(255)),
            std::time::Duration::from_secs(10),
        );
        assert!(matches!(r, DisconnectReason::DnsFailure(_)));
    }

    #[test]
    fn classify_disconnect_falls_through_to_proto_when_stderr_silent() {
        let r = classify_disconnect(
            Err(DisconnectReason::HandshakeFailed("bad magic".into())),
            "",
            Some(fake_exit(1)),
            std::time::Duration::from_secs(1),
        );
        assert!(matches!(r, DisconnectReason::HandshakeFailed(_)));
    }

    #[test]
    fn classify_disconnect_clean_exit_yields_remote_eof() {
        let r = classify_disconnect(
            Ok(()),
            "",
            Some(fake_exit(0)),
            std::time::Duration::from_secs(60),
        );
        assert!(matches!(r, DisconnectReason::RemoteEof));
    }

    /// Construct a fake `ExitStatus` with the given code for classification
    /// tests. Platform-specific because `ExitStatus` can only be built via
    /// `From<RawExitStatus>` extensions.
    fn fake_exit(code: i32) -> std::process::ExitStatus {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(code << 8)
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(code as u32)
        }
    }

    #[test]
    fn stderr_tail_is_bounded() {
        let tail = StderrTail::default();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            // Feed more than the cap and verify only the trailing window is kept.
            let mut huge = Vec::new();
            for _ in 0..16 {
                huge.extend_from_slice(&b"x".repeat(1024));
            }
            huge.extend_from_slice(b"FINAL");
            let reader = std::io::Cursor::new(huge);
            drain_stderr(tokio::io::BufReader::new(reader), tail.clone()).await;
        });
        let snapshot = tail.snapshot();
        assert!(snapshot.len() <= STDERR_TAIL_CAP);
        assert!(snapshot.ends_with("FINAL"));
    }
}
