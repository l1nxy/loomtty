use anyhow::Result;
use ciri_protocol::message::*;

/// Send a control command to the server and print the response.
pub fn run_control_command(msg: ClientMessage) -> Result<()> {
    use ciri_protocol::transport;
    use std::io::{Read, Write};

    // Connect to server (single attempt — no separate probe to avoid consuming
    // the only Windows named pipe instance and causing ERROR_PIPE_BUSY).
    #[cfg(unix)]
    let stream_result = {
        let p = transport::server_socket_path();
        if !p.exists() {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "socket not found"))
        } else {
            std::os::unix::net::UnixStream::connect(&p)
        }
    };
    #[cfg(windows)]
    let stream_result = {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(transport::server_pipe_name())
    };

    let mut stream = match stream_result {
        Ok(s) => s,
        Err(_) => {
            // Server is not running
            if matches!(msg, ClientMessage::ListSessions) {
                let dir = transport::state_dir();
                let saved = ciri_session::restore::list_sessions(&dir).unwrap_or_default();
                if saved.is_empty() {
                    println!("no sessions");
                } else {
                    println!("{:<20} {}", "NAME", "STATUS");
                    for name in saved {
                        println!("{:<20} saved", name);
                    }
                }
                return Ok(());
            }
            eprintln!("server is not running");
            std::process::exit(1);
        }
    };

    // Set timeouts so we don't hang if the server is unresponsive
    #[cfg(unix)]
    {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
    }

    // Send a minimal ClientHello
    let magic = b"CIRI";
    let version = {
        let v = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = v.split('.').collect();
        let major: u8 = parts[0].parse().unwrap();
        let minor: u8 = parts[1].parse().unwrap();
        let patch: u16 = parts[2].parse().unwrap();
        (major as u32) << 24 | (minor as u32) << 16 | patch as u32
    };
    let session_name = "__control__";
    let name_bytes = session_name.as_bytes();
    let mut hello = Vec::with_capacity(27 + name_bytes.len());
    hello.extend_from_slice(magic);
    hello.extend_from_slice(&version.to_le_bytes());
    hello.push(ciri_protocol::codec::WIRE_PROTOCOL_VERSION);
    hello.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    hello.extend_from_slice(name_bytes);
    hello.extend_from_slice(&1024u32.to_le_bytes()); // width
    hello.extend_from_slice(&768u32.to_le_bytes());  // height
    hello.extend_from_slice(&8.0f32.to_bits().to_le_bytes()); // cell_w
    hello.extend_from_slice(&16.0f32.to_bits().to_le_bytes()); // cell_h
    stream.write_all(&hello)?;
    stream.flush()?;

    // Read ServerHello (8 bytes)
    let mut server_hello = [0u8; 8];
    stream.read_exact(&mut server_hello)?;

    // Send the control message
    let payload = rmp_serde::to_vec(&msg)
        .map_err(|e| anyhow::anyhow!("failed to serialize: {e}"))?;
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(0x01); // TAG_CLIENT_MSG
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    stream.write_all(&frame)?;
    stream.flush()?;

    // Read response frames until we get a ServerMessage
    loop {
        let mut header = [0u8; 5];
        if stream.read_exact(&mut header).is_err() {
            break;
        }
        let tag = header[0];
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload)?;

        if tag == 0x10 {
            // ServerMessage
            if let Ok(server_msg) = rmp_serde::from_slice::<ServerMessage>(&payload) {
                match server_msg {
                    ServerMessage::SessionList { sessions } => {
                        if sessions.is_empty() {
                            println!("no sessions");
                        } else {
                            println!("{:<20} {:<10} {:<6} {}", "NAME", "STATUS", "PANES", "CLIENTS");
                            for s in sessions {
                                let status = if s.running { "running" } else { "saved" };
                                println!("{:<20} {:<10} {:<6} {}", s.name, status, s.pane_count, s.client_count);
                            }
                        }
                        return Ok(());
                    }
                    ServerMessage::SessionKilled { session_name } => {
                        println!("killed session '{session_name}'");
                        return Ok(());
                    }
                    ServerMessage::ServerShutdown => {
                        println!("server shutting down");
                        return Ok(());
                    }
                    ServerMessage::Error { message } => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                    _ => {
                        // Skip other messages (StateSync etc from initial connect)
                        continue;
                    }
                }
            }
        }
        // Skip non-ServerMessage frames (FullPaneSync etc)
    }

    Ok(())
}

/// Check if a session with the given name is currently running on the server.
/// Returns false if the server is unreachable or the session is not found.
pub fn session_exists_on_server(name: &str) -> bool {
    use ciri_protocol::transport;
    use std::io::{Read, Write};

    // Try to connect to the server
    #[cfg(unix)]
    let stream_result = {
        let p = transport::server_socket_path();
        if !p.exists() { return false; }
        std::os::unix::net::UnixStream::connect(&p)
    };
    #[cfg(windows)]
    let stream_result = {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(transport::server_pipe_name())
    };

    let mut stream = match stream_result {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Set a short timeout so we don't hang
    #[cfg(unix)]
    {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    }

    // Send ClientHello for __control__ session
    let magic = b"CIRI";
    let version = {
        let v = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = v.split('.').collect();
        let major: u8 = parts[0].parse().unwrap();
        let minor: u8 = parts[1].parse().unwrap();
        let patch: u16 = parts[2].parse().unwrap();
        (major as u32) << 24 | (minor as u32) << 16 | patch as u32
    };
    let session_name_bytes = b"__control__";
    let mut hello = Vec::with_capacity(27 + session_name_bytes.len());
    hello.extend_from_slice(magic);
    hello.extend_from_slice(&version.to_le_bytes());
    hello.push(ciri_protocol::codec::WIRE_PROTOCOL_VERSION);
    hello.extend_from_slice(&(session_name_bytes.len() as u16).to_le_bytes());
    hello.extend_from_slice(session_name_bytes);
    hello.extend_from_slice(&1024u32.to_le_bytes());
    hello.extend_from_slice(&768u32.to_le_bytes());
    hello.extend_from_slice(&8.0f32.to_bits().to_le_bytes());
    hello.extend_from_slice(&16.0f32.to_bits().to_le_bytes());
    if stream.write_all(&hello).is_err() || stream.flush().is_err() {
        return false;
    }

    // Read ServerHello
    let mut server_hello = [0u8; 8];
    if stream.read_exact(&mut server_hello).is_err() {
        return false;
    }

    // Send ListSessions
    let msg = ClientMessage::ListSessions;
    let payload = match rmp_serde::to_vec(&msg) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(0x01);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    if stream.write_all(&frame).is_err() || stream.flush().is_err() {
        return false;
    }

    // Read response frames looking for SessionList
    for _ in 0..20 {
        let mut header = [0u8; 5];
        if stream.read_exact(&mut header).is_err() {
            break;
        }
        let tag = header[0];
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        let mut payload = vec![0u8; len];
        if stream.read_exact(&mut payload).is_err() {
            break;
        }
        if tag == 0x10 {
            if let Ok(ServerMessage::SessionList { sessions }) = rmp_serde::from_slice(&payload) {
                return sessions.iter().any(|s| s.running && s.name == name);
            }
        }
    }
    false
}
