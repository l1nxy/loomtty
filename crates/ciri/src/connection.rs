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
                let port = transport::server_port();
                std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok()
            }
        };
        if !server_ready() {
            spawn_server(session_name)?;
        }
    }

    let (msg_tx, msg_rx) = crossbeam_channel::bounded::<ClientMessage>(256);
    let (event_tx, event_rx) = crossbeam_channel::bounded::<ServerEvent>(256);

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
                    { connect_result = tokio::net::TcpStream::connect(
                        format!("127.0.0.1:{}", transport::server_port())
                      ).await; }
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

                let (reader, writer) = stream.into_split();
                let mut reader = tokio::io::BufReader::new(reader);
                let mut writer = tokio::io::BufWriter::new(writer);

                // Send ClientHello (version + viewport), read ServerHello
                if let Err(e) = codec::write_client_hello(&mut writer, &viewport).await {
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
                        let msg = match tokio::task::block_in_place(|| writer_msg_rx.recv()) {
                            Ok(m) => m,
                            Err(_) => break,
                        };
                        if let Err(e) = codec::encode_client_msg(&mut writer, &msg).await {
                            log::warn!("write error: {e}");
                            break;
                        }
                        while let Ok(extra) = writer_msg_rx.try_recv() {
                            if let Err(e) = codec::encode_client_msg(&mut writer, &extra).await {
                                log::warn!("write error: {e}");
                                return;
                            }
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

    #[cfg(not(unix))]
    {
        Command::new(&server_exe)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;
    }

    Ok(())
}
