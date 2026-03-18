use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use crossbeam_channel::{Receiver, Sender};
use std::io;

/// Messages from server to client (received on the winit thread).
pub enum ServerEvent {
    Control(ServerMessage),
    CellDelta(CellDelta),
    FullPaneSync(FullPaneSync),
    Disconnected,
}

/// Connect to a running server session, or spawn a new one.
/// Returns channels for bidirectional communication.
pub fn connect_or_spawn(
    session_name: &str,
) -> io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
    let sock_path = transport::socket_path(session_name);

    // Try to connect to existing server
    if !sock_path.exists() {
        // Spawn server
        spawn_server(session_name)?;
        // Wait a bit for it to start
        for _ in 0..50 {
            if sock_path.exists() { break; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    let (msg_tx, msg_rx) = crossbeam_channel::bounded::<ClientMessage>(256);
    let (event_tx, event_rx) = crossbeam_channel::bounded::<ServerEvent>(256);

    let session = session_name.to_string();
    std::thread::Builder::new()
        .name("server-io".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async move {
                let sock_path = transport::socket_path(&session);
                let stream = match tokio::net::UnixStream::connect(&sock_path).await {
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
            });
        })?;

    Ok((msg_tx, event_rx))
}

fn spawn_server(session_name: &str) -> io::Result<()> {
    use std::process::Command;
    // Try to find ciri-server binary next to the current executable
    let exe = std::env::current_exe().unwrap_or_default();
    let server_exe = exe.parent()
        .map(|p| p.join("ciri-server"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("ciri-server"));

    // Ensure socket directory exists
    let sock_path = transport::socket_path(session_name);
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    log::info!("spawning server: {} {}", server_exe.display(), session_name);

    // Fork server as daemon (setsid for session independence)
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new(&server_exe);
        cmd.arg(session_name)
            .stdin(std::process::Stdio::null())
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
            .arg(session_name)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }

    Ok(())
}
