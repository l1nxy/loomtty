use anyhow::Result;
use ciri_protocol::message::*;

/// Maximum frame payload size (16 MiB), matching the async codec limit.
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Build a serialized ClientHello for control connections.
fn build_control_hello() -> Vec<u8> {
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
    hello.extend_from_slice(&768u32.to_le_bytes()); // height
    hello.extend_from_slice(&8.0f32.to_bits().to_le_bytes()); // cell_w
    hello.extend_from_slice(&16.0f32.to_bits().to_le_bytes()); // cell_h
    hello
}

/// Send a control command to the server and print the response.
pub fn run_control_command(msg: ClientMessage, json: bool) -> Result<()> {
    use ciri_protocol::transport;
    use std::io::{Read, Write};

    // Connect to server (single attempt — no separate probe to avoid consuming
    // the only Windows named pipe instance and causing ERROR_PIPE_BUSY).
    #[cfg(unix)]
    let stream_result = {
        let p = transport::server_socket_path();
        if !p.exists() {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "socket not found",
            ))
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
            if matches!(msg, ClientMessage::ListSessions { .. }) {
                let dir = transport::state_dir();
                let saved = ciri_session::restore::list_sessions(&dir).unwrap_or_default();
                if saved.is_empty() {
                    println!("no sessions");
                } else {
                    println!("{:<20} STATUS", "NAME");
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
    let hello = build_control_hello();
    stream.write_all(&hello)?;
    stream.flush()?;

    // Read ServerHello (8 bytes)
    let mut server_hello = [0u8; 8];
    stream.read_exact(&mut server_hello)?;

    // Send the control message
    let payload =
        rmp_serde::to_vec(&msg).map_err(|e| anyhow::anyhow!("failed to serialize: {e}"))?;
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
        if len > MAX_FRAME_SIZE {
            return Err(anyhow::anyhow!(
                "frame too large ({len} bytes, max {MAX_FRAME_SIZE})"
            ));
        }
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
                            println!("{:<20} {:<10} {:<6} CLIENTS", "NAME", "STATUS", "PANES");
                            for s in sessions {
                                let status = if s.running { "running" } else { "saved" };
                                println!(
                                    "{:<20} {:<10} {:<6} {}",
                                    s.name, status, s.pane_count, s.client_count
                                );
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
                    ServerMessage::TemplateList { templates } => {
                        if templates.is_empty() {
                            println!("no templates");
                        } else {
                            println!(
                                "{:<20} {:<12} {:<6} DESCRIPTION",
                                "NAME", "WORKSPACES", "PANES"
                            );
                            for t in templates {
                                let desc = t.description.as_deref().unwrap_or("");
                                println!(
                                    "{:<20} {:<12} {:<6} {}",
                                    t.name, t.workspace_count, t.total_panes, desc
                                );
                            }
                        }
                        return Ok(());
                    }
                    ServerMessage::TemplateSaved { template_name } => {
                        println!("saved template '{template_name}'");
                        return Ok(());
                    }
                    ServerMessage::TemplateApplied { session_name } => {
                        println!("applied template to session '{session_name}'");
                        return Ok(());
                    }
                    ServerMessage::Error { message } => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                    ServerMessage::SessionInfoReply { info } => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&info).unwrap_or_default()
                            );
                        } else {
                            println!("Session: {}", info.name);
                            println!(
                                "  Status:           {}",
                                if info.running { "running" } else { "stopped" }
                            );
                            println!("  Panes:            {}", info.pane_count);
                            println!("  Clients:          {}", info.client_count);
                            println!("  Workspaces:       {}", info.workspace_count);
                            println!("  Active workspace: {}", info.active_workspace);
                        }
                        return Ok(());
                    }
                    ServerMessage::PaneListReply { panes } => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&panes).unwrap_or_default()
                            );
                        } else if panes.is_empty() {
                            println!("no panes");
                        } else {
                            println!(
                                "{:<8} {:<10} {:<30} {:<6} {:<4} {:<4} {:<4}",
                                "ID", "SIZE", "TITLE", "ACTIVE", "WS", "COL", "TILE"
                            );
                            for p in panes {
                                println!(
                                    "{:<8} {}x{:<7} {:<30} {:<6} {:<4} {:<4} {:<4}",
                                    p.pane_id,
                                    p.cols,
                                    p.rows,
                                    if p.title.len() > 30 {
                                        p.title[..27].to_string() + "..."
                                    } else {
                                        p.title.clone()
                                    },
                                    if p.is_active { "*" } else { "" },
                                    p.workspace_idx,
                                    p.column_idx,
                                    p.tile_idx,
                                );
                            }
                        }
                        return Ok(());
                    }
                    ServerMessage::CommandResult {
                        success,
                        message,
                        pane_id,
                    } => {
                        if json {
                            let obj = serde_json::json!({
                                "success": success,
                                "message": message,
                                "pane_id": pane_id,
                            });
                            println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
                        } else if success {
                            if let Some(id) = pane_id {
                                println!("{message} (pane {id})");
                            } else {
                                println!("{message}");
                            }
                        } else {
                            eprintln!("error: {message}");
                            std::process::exit(1);
                        }
                        return Ok(());
                    }
                    ServerMessage::LayoutReply {
                        layout,
                        session_name,
                    } => {
                        // Layout is always returned as JSON
                        let obj = serde_json::json!({
                            "session_name": session_name,
                            "layout": layout,
                        });
                        println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
                        return Ok(());
                    }
                    ServerMessage::PaneCapture {
                        session_name,
                        pane_id,
                        text,
                        truncated,
                    } => {
                        if json {
                            let obj = serde_json::json!({
                                "session_name": session_name,
                                "pane_id": pane_id,
                                "text": text,
                                "truncated": truncated,
                            });
                            let rendered =
                                serde_json::to_string_pretty(&obj).unwrap_or_default();
                            // Match the raw-text path's BrokenPipe handling:
                            // `println!` would panic on EPIPE, but a user
                            // doing `… --json | head` should see exit 0.
                            let stdout = std::io::stdout();
                            let mut lock = stdout.lock();
                            match lock
                                .write_all(rendered.as_bytes())
                                .and_then(|_| lock.write_all(b"\n"))
                                .and_then(|_| lock.flush())
                            {
                                Ok(()) => {}
                                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                                    return Ok(());
                                }
                                Err(e) => return Err(e.into()),
                            }
                        } else {
                            // Write raw text to stdout. `capture_text` emits
                            // a `\n` after every row except when
                            // `--join-wrapped` is set AND the final row
                            // soft-wrapped — that corner case legitimately
                            // ends without a newline. BrokenPipe
                            // (`... | head -c 100`) is a clean termination
                            // signal — exit 0 silently; other write errors
                            // (including WriteZero, which is a real failure
                            // and not just a closed pipe) propagate so
                            // callers see them.
                            let stdout = std::io::stdout();
                            let mut lock = stdout.lock();
                            // Emit the truncation warning to stderr
                            // *before* potentially returning early on a
                            // stdout BrokenPipe — otherwise the user
                            // doing `... | head` silently loses the
                            // signal that their capture was clipped.
                            // (`truncated` fires from either a pre-loop
                            // scrollback clamp or a mid-loop byte-budget
                            // break — wording is neutral on which.)
                            if truncated {
                                eprintln!(
                                    "ciritty: capture-pane response was truncated to fit \
                                     the control-frame budget (some rows omitted)"
                                );
                            }
                            match lock.write_all(text.as_bytes()).and_then(|_| lock.flush()) {
                                Ok(()) => {}
                                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                                    return Ok(());
                                }
                                Err(e) => return Err(e.into()),
                            }
                        }
                        return Ok(());
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
        if !p.exists() {
            return false;
        }
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
    let hello = build_control_hello();
    if stream.write_all(&hello).is_err() || stream.flush().is_err() {
        return false;
    }

    // Read ServerHello
    let mut server_hello = [0u8; 8];
    if stream.read_exact(&mut server_hello).is_err() {
        return false;
    }

    // Send ListSessions
    let msg = ClientMessage::ListSessions { all: true };
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
        if len > MAX_FRAME_SIZE {
            break;
        }
        let mut payload = vec![0u8; len];
        if stream.read_exact(&mut payload).is_err() {
            break;
        }
        if tag == 0x10
            && let Ok(ServerMessage::SessionList { sessions }) = rmp_serde::from_slice(&payload)
        {
            return sessions.iter().any(|s| s.running && s.name == name);
        }
    }
    false
}

/// Query the server for active (running) sessions, sorted by most recently used first.
/// Returns an empty Vec if the server is unreachable.
pub fn query_active_sessions() -> Vec<String> {
    use ciri_protocol::transport;
    use std::io::{Read, Write};

    #[cfg(unix)]
    let stream_result = {
        let p = transport::server_socket_path();
        if !p.exists() {
            return Vec::new();
        }
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
        Err(_) => return Vec::new(),
    };

    #[cfg(unix)]
    {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    }

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
        return Vec::new();
    }

    let mut server_hello = [0u8; 8];
    if stream.read_exact(&mut server_hello).is_err() {
        return Vec::new();
    }

    let msg = ClientMessage::ListSessions { all: false };
    let payload = match rmp_serde::to_vec(&msg) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(0x01);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    if stream.write_all(&frame).is_err() || stream.flush().is_err() {
        return Vec::new();
    }

    for _ in 0..20 {
        let mut header = [0u8; 5];
        if stream.read_exact(&mut header).is_err() {
            break;
        }
        let tag = header[0];
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if len > MAX_FRAME_SIZE {
            break;
        }
        let mut payload = vec![0u8; len];
        if stream.read_exact(&mut payload).is_err() {
            break;
        }
        if tag == 0x10
            && let Ok(ServerMessage::SessionList { sessions }) = rmp_serde::from_slice(&payload)
        {
            return sessions
                .into_iter()
                .filter(|s| s.running)
                .map(|s| s.name)
                .collect();
        }
    }
    Vec::new()
}
