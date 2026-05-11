//! WebSocket transport adapter for browser clients.
//!
//! Wraps a `WebSocketStream<TcpStream>` so the existing `handle_client`
//! reader/writer loop can drive it without knowing about WS framing. Each
//! inbound `Binary` message contributes its payload to a read buffer that
//! `AsyncRead` drains byte by byte; each `poll_flush` packages the
//! accumulated write bytes into a single `Binary` message. The mapping is
//! stream-oriented (not message-aligned), which lets the wire codec frame
//! `[u8 tag][u32 LE len][payload]` chunks straddle WS message boundaries
//! exactly the way TCP/Unix transports already allow.

use anyhow::{Result, anyhow};
use futures_util::{Sink, Stream, ready};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{WebSocketStream, accept_hdr_async};

/// Cap on a single inbound WS message. Sized to match the protocol's
/// largest legal frame (`MAX_DATA_FRAME_LEN` in ciri-protocol) so a
/// malicious or buggy peer cannot exhaust memory by claiming a single
/// huge binary message. Kept as a local constant because the codec
/// constant is currently `pub(super)` — bump both together if the
/// codec ever lifts its cap.
const MAX_WS_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Cap on the outbound flush buffer. The codec batches multiple frames
/// per flush via `BufWriter`, so this needs to be at least as large as
/// the worst-case batch — keep it aligned with the inbound cap.
const MAX_WS_FLUSH_BYTES: usize = MAX_WS_MESSAGE_BYTES;

/// Adapter exposing a binary WebSocket as `AsyncRead + AsyncWrite`.
pub struct WsStream {
    ws: WebSocketStream<TcpStream>,
    /// Bytes pulled from the most recent inbound Binary message that
    /// haven't been delivered to the reader yet.
    read_buf: Vec<u8>,
    read_pos: usize,
    /// Bytes accepted by `poll_write` but not yet packaged into a WS
    /// message. Drained by `poll_flush`.
    write_buf: Vec<u8>,
}

impl WsStream {
    fn new(ws: WebSocketStream<TcpStream>) -> Self {
        Self {
            ws,
            read_buf: Vec::new(),
            read_pos: 0,
            write_buf: Vec::new(),
        }
    }
}

/// Perform the WebSocket handshake and validate the `?token=…` query
/// parameter against `expected_token` in constant time. Returns the
/// established adapter on success.
///
/// The handshake responds with `401 Unauthorized` when the token is
/// missing or wrong, so the browser sees a deterministic failure rather
/// than a generic protocol error.
// `ErrorResponse` is `http::Response<Option<String>>` — clippy flags the
// `Result<Response, ErrorResponse>` variant size, but the type is dictated
// by tungstenite's callback contract so we can't shrink it.
#[allow(clippy::result_large_err)]
pub async fn accept_ws(tcp: TcpStream, expected_token: &str) -> Result<WsStream> {
    let expected = expected_token.as_bytes().to_vec();

    let ws = accept_hdr_async(tcp, move |req: &Request, response: Response| {
        let token_ok = req
            .uri()
            .query()
            .and_then(extract_token)
            .map(|t| constant_time_eq(t.as_bytes(), &expected))
            .unwrap_or(false);
        if !token_ok {
            let err: ErrorResponse = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Some("unauthorized".to_string()))
                .expect("static error response builds");
            return Err(err);
        }
        Ok(response)
    })
    .await
    .map_err(|e| anyhow!("ws handshake failed: {e}"))?;

    Ok(WsStream::new(ws))
}

/// Find a `token=` pair in a URL query string without allocating. Only
/// the first occurrence is returned; later duplicates are ignored, which
/// is consistent with how browsers and servers usually treat them.
fn extract_token(query: &str) -> Option<&str> {
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            return Some(value);
        }
    }
    None
}

/// Length-aware constant-time byte compare. Length mismatches short
/// circuit (length is not a secret), but for equal lengths every byte
/// is examined so an attacker cannot time the comparison.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

impl AsyncRead for WsStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if self.read_pos < self.read_buf.len() {
                let remaining = &self.read_buf[self.read_pos..];
                let n = remaining.len().min(buf.remaining());
                buf.put_slice(&remaining[..n]);
                self.read_pos += n;
                return Poll::Ready(Ok(()));
            }
            // No buffered bytes — drop the old buffer so the next Binary
            // message can reuse the allocation without holding stale data.
            self.read_buf.clear();
            self.read_pos = 0;

            match Pin::new(&mut self.ws).poll_next(cx) {
                Poll::Ready(Some(Ok(msg))) => match msg {
                    Message::Binary(data) => {
                        if data.len() > MAX_WS_MESSAGE_BYTES {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "ws binary message {} bytes exceeds limit {MAX_WS_MESSAGE_BYTES}",
                                    data.len(),
                                ),
                            )));
                        }
                        self.read_buf = data;
                    }
                    Message::Close(_) => return Poll::Ready(Ok(())),
                    Message::Text(_) => {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "text frame on binary ws channel",
                        )));
                    }
                    // tungstenite responds to pings automatically; pongs
                    // we just discard. Raw frames shouldn't reach us in
                    // server mode, but ignore them defensively.
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                },
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Err(io::Error::other(e)));
                }
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl AsyncWrite for WsStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.write_buf.len().saturating_add(buf.len()) > MAX_WS_FLUSH_BYTES {
            return Poll::Ready(Err(io::Error::other(format!(
                "ws flush buffer would exceed limit {MAX_WS_FLUSH_BYTES}; \
                 pending={} new={}",
                self.write_buf.len(),
                buf.len(),
            ))));
        }
        self.write_buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.write_buf.is_empty() {
            match Pin::new(&mut self.ws).poll_ready(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(io::Error::other(e)));
                }
                Poll::Pending => return Poll::Pending,
            }
            let data = std::mem::take(&mut self.write_buf);
            if let Err(e) = Pin::new(&mut self.ws).start_send(Message::Binary(data)) {
                return Poll::Ready(Err(io::Error::other(e)));
            }
        }
        Pin::new(&mut self.ws)
            .poll_flush(cx)
            .map_err(io::Error::other)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(&mut self.ws)
            .poll_close(cx)
            .map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_extraction_finds_first_pair() {
        assert_eq!(extract_token("token=abc"), Some("abc"));
        assert_eq!(extract_token("foo=1&token=abc"), Some("abc"));
        assert_eq!(extract_token("token=abc&token=second"), Some("abc"));
        assert_eq!(extract_token("foo=bar"), None);
        assert_eq!(extract_token(""), None);
    }

    #[test]
    fn token_extraction_preserves_empty_value() {
        // An empty `token=` is returned as-is; the constant_time_eq
        // step is responsible for rejecting it against a real token.
        assert_eq!(extract_token("token="), Some(""));
    }

    #[test]
    fn constant_time_eq_matches_only_exact_input() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"hellp"));
        assert!(!constant_time_eq(b"hello", b"helloworld"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
