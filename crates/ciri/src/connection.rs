use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use crossbeam_channel::{Receiver, Sender};
use std::io;

/// Messages from server to client (received on the winit thread).
pub enum ServerEvent {
    Control(ServerMessage),
    CellDelta(CellDeltaBorrowed),
    FullPaneSync(FullPaneSync),
    Disconnected,
}

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
            log::warn!("server version {peer} differs from client {local} (minor mismatch, may be unstable)");
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
            if writer.flush().await.is_err() { break; }
        }
    });

    // Reader loop
    loop {
        match codec::read_frame(&mut reader).await {
            Ok(codec::Frame::ServerMsg(msg)) => {
                if event_tx.send(ServerEvent::Control(msg)).is_err() { break; }
            }
            Ok(codec::Frame::CellDelta(delta)) => {
                if event_tx.send(ServerEvent::CellDelta(delta)).is_err() { break; }
            }
            Ok(codec::Frame::FullPaneSync(sync)) => {
                if event_tx.send(ServerEvent::FullPaneSync(sync)).is_err() { break; }
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
    // Validate session name to prevent path traversal
    if session_name.is_empty()
        || session_name.contains('/')
        || session_name.contains('\\')
        || session_name.contains("..")
        || session_name.contains('\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid session name: {session_name:?}"),
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
                    { let sock_path = transport::server_socket_path();
                      connect_result = tokio::net::UnixStream::connect(&sock_path).await; }
                    #[cfg(windows)]
                    { connect_result = tokio::net::windows::named_pipe::ClientOptions::new()
                        .open(&transport::server_pipe_name()); }
                    if connect_result.is_ok() { break; }
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

/// Connect to a remote ciri-server via SSH stdio proxy tunnel.
pub fn connect_remote(
    host: &str,
    remote_port: u16,
    ssh_port: u16,
    viewport: codec::ClientHello,
) -> io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
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
                // Spawn SSH process with -W for stdio proxy
                let mut child = match tokio::process::Command::new("ssh")
                    .args(["-p", &ssh_port.to_string()])
                    .args(["-W", &format!("localhost:{remote_port}")])
                    .arg(&host)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                {
                    Ok(child) => child,
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
