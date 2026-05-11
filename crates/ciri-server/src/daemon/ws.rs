//! WebSocket transport adapter for browser clients.
//!
//! Wraps a `WebSocketStream` so the existing `handle_client` reader/writer
//! loop can drive it without knowing about WS framing. Each inbound
//! `Binary` message contributes its payload to a read buffer that
//! `AsyncRead` drains byte by byte; each `poll_flush` packages the
//! accumulated write bytes into a single `Binary` message. The mapping is
//! stream-oriented (not message-aligned), which lets the wire codec frame
//! `[u8 tag][u32 LE len][payload]` chunks straddle WS message boundaries
//! exactly the way TCP/Unix transports already allow.

use anyhow::{Result, anyhow};
use futures_util::{Sink, Stream, ready};
use percent_encoding::percent_decode_str;
use std::borrow::Cow;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{WebSocketStream, accept_hdr_async};

use ciri_protocol::codec::MAX_DATA_FRAME_LEN;

/// Cap on a single inbound WS message and on the outbound pending-write
/// buffer. Both are sourced directly from the protocol cap so a malicious
/// or buggy peer cannot exhaust memory by claiming a single huge binary
/// message.
///
/// Callers wrap `WsStream` in a `BufWriter` in `handle_client`, which
/// flushes after every batched frame group — the outbound cap exists
/// only as a tripwire against an unbuffered writer or a buggy caller
/// that accumulates many MiB between flushes.
const MAX_WS_MESSAGE_BYTES: usize = MAX_DATA_FRAME_LEN as usize;
const MAX_WS_WRITE_BYTES: usize = MAX_DATA_FRAME_LEN as usize;

/// Adapter exposing a binary WebSocket as `AsyncRead + AsyncWrite`.
///
/// Generic over the underlying byte stream so unit tests can exercise
/// the read/write impls against an in-memory duplex pair instead of a
/// real TCP socket. Production code uses `WsStream<TcpStream>`.
pub struct WsStream<S = TcpStream> {
    ws: WebSocketStream<S>,
    /// Bytes pulled from the most recent inbound Binary message that
    /// haven't been delivered to the reader yet.
    read_buf: Vec<u8>,
    read_pos: usize,
    /// Bytes accepted by `poll_write` but not yet packaged into a WS
    /// message. Drained by `poll_flush`.
    write_buf: Vec<u8>,
}

impl<S> WsStream<S> {
    fn new(ws: WebSocketStream<S>) -> Self {
        Self {
            ws,
            read_buf: Vec::new(),
            read_pos: 0,
            write_buf: Vec::new(),
        }
    }
}

/// Perform the WebSocket handshake and validate the bearer token.
///
/// The token may be supplied as `Authorization: Bearer <token>` (preferred
/// for non-browser clients — does not leak into request logs, browser
/// history, or `Referer`) or as `?token=…` on the upgrade URL (the only
/// option for browsers, which cannot set custom headers on a `WebSocket`
/// upgrade).
///
/// The byte-wise compare is constant-time **for inputs of the same byte
/// length**; the fast path on length mismatch leaks the secret's length
/// through timing. Generate tokens at a fixed length (e.g. 32 hex chars)
/// to neutralise this side channel.
///
/// On failure the response is `401 Unauthorized` so the client sees a
/// deterministic error instead of a generic protocol close.
// `ErrorResponse` is `http::Response<Option<String>>` — clippy flags the
// `Result<Response, ErrorResponse>` variant size, but the type is dictated
// by tungstenite's callback contract so we can't shrink it.
#[allow(clippy::result_large_err)]
pub async fn accept_ws(tcp: TcpStream, expected_token: &str) -> Result<WsStream<TcpStream>> {
    if expected_token.is_empty() {
        // Defensive: the daemon already rejects empty tokens at startup,
        // but if someone ever bypasses that, refuse to authenticate any
        // request rather than accepting all of them.
        return Err(anyhow!("refusing ws handshake: empty expected token"));
    }
    let expected = expected_token.as_bytes().to_vec();

    let ws = accept_hdr_async(tcp, move |req: &Request, response: Response| {
        let token_ok = extract_bearer(req)
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

/// Pull the bearer token from `Authorization: Bearer <token>` if present,
/// otherwise fall back to `?token=…` on the upgrade URL. The scheme name
/// compare is case-insensitive per RFC 7235 §2.1 (`"Bearer"`, `"bearer"`,
/// `"BEARER"`, etc. all match).
fn extract_bearer(req: &Request) -> Option<Cow<'_, str>> {
    if let Some(value) = req.headers().get("authorization")
        && let Ok(s) = value.to_str()
        && let Some((scheme, rest)) = s.split_once(' ')
        && scheme.eq_ignore_ascii_case("Bearer")
    {
        let trimmed = rest.trim();
        // Tokens are required to be a single non-empty whitespace-free
        // run. A whitespace-containing value almost certainly indicates
        // operator error (concatenated headers, malformed input) — reject
        // it rather than forwarding garbage to the constant-time compare.
        if !trimmed.is_empty() && !trimmed.contains(char::is_whitespace) {
            return Some(Cow::Borrowed(trimmed));
        }
    }
    let query = req.uri().query()?;
    extract_token_query(query)
}

/// Find a `token=` pair in a URL query string and percent-decode the
/// value. Only the first occurrence is returned; later duplicates are
/// ignored, matching how browsers and servers usually treat them.
fn extract_token_query(query: &str) -> Option<Cow<'_, str>> {
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            // Decode `%xx` escapes so a token containing `+`, `=`, `&`,
            // `%`, or non-ASCII survives the URL round-trip. Invalid
            // UTF-8 short-circuits to `None` so the caller falls through
            // to a 401 rather than feeding U+FFFD replacement characters
            // into the constant-time compare.
            return percent_decode_str(value).decode_utf8().ok();
        }
    }
    None
}

/// Length-aware constant-time byte compare. Length mismatches fast-exit
/// (this leaks the secret's length); for equal lengths every byte is
/// examined regardless of where they first differ.
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

impl<S> AsyncRead for WsStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
            // Drained — drop the previous payload before fetching the
            // next one. The new Vec comes from tungstenite, so the old
            // allocation is released here rather than reused.
            self.read_buf = Vec::new();
            self.read_pos = 0;

            match Pin::new(&mut self.ws).poll_next(cx) {
                Poll::Ready(Some(Ok(msg))) => match msg {
                    Message::Binary(data) => {
                        if data.len() > MAX_WS_MESSAGE_BYTES {
                            return Poll::Ready(Err(io::Error::other(format!(
                                "ws binary message {} bytes exceeds limit {MAX_WS_MESSAGE_BYTES}",
                                data.len(),
                            ))));
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
                    // we discard. Raw frames shouldn't reach us in server
                    // mode, but ignore them defensively.
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

impl<S> AsyncWrite for WsStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.write_buf.len().saturating_add(buf.len()) > MAX_WS_WRITE_BYTES {
            return Poll::Ready(Err(io::Error::other(format!(
                "ws write would exceed pending-write limit {MAX_WS_WRITE_BYTES}; \
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
                Poll::Ready(Err(e)) => return Poll::Ready(Err(io::Error::other(e))),
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

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(&mut self.get_mut().ws)
            .poll_close(cx)
            .map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::protocol::Role;

    #[test]
    fn extract_token_query_finds_first_pair() {
        assert_eq!(extract_token_query("token=abc").as_deref(), Some("abc"));
        assert_eq!(
            extract_token_query("foo=1&token=abc").as_deref(),
            Some("abc"),
        );
        assert_eq!(
            extract_token_query("token=abc&token=second").as_deref(),
            Some("abc"),
        );
        assert_eq!(extract_token_query("foo=bar").as_deref(), None);
        assert_eq!(extract_token_query("").as_deref(), None);
        // Empty value is returned verbatim; the constant-time compare is
        // responsible for rejecting it against the configured token.
        assert_eq!(extract_token_query("token=").as_deref(), Some(""));
    }

    #[test]
    fn extract_token_query_percent_decodes() {
        // `+`, `=`, `&`, `%`, and non-ASCII all percent-encode in URLs;
        // the decoder must round-trip them or browsers that encode the
        // token field will silently fail to authenticate.
        assert_eq!(
            extract_token_query("token=abc%2Bdef").as_deref(),
            Some("abc+def"),
        );
        assert_eq!(
            extract_token_query("token=ab%3Dcd").as_deref(),
            Some("ab=cd"),
        );
        assert_eq!(
            extract_token_query("token=%E4%B8%AD").as_deref(),
            Some("中"),
        );
    }

    #[test]
    fn extract_token_query_stops_at_unencoded_separator() {
        // `&` is the query-pair separator. An unencoded `&` inside a
        // token splits the pair before percent-decoding even runs; the
        // returned value is truncated. Operators must percent-encode `&`
        // in tokens (`%26`) — this assertion documents the contract.
        assert_eq!(
            extract_token_query("token=abc&def").as_deref(),
            Some("abc"),
        );
        assert_eq!(
            extract_token_query("token=abc%26def").as_deref(),
            Some("abc&def"),
        );
    }

    fn req_with(headers: &[(&str, &str)], query: Option<&str>) -> Request {
        let uri = match query {
            Some(q) => format!("/?{q}"),
            None => "/".to_string(),
        };
        let mut builder = tokio_tungstenite::tungstenite::http::Request::builder().uri(&uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap()
    }

    #[test]
    fn extract_bearer_matches_scheme_case_insensitively() {
        for scheme in &["Bearer", "bearer", "BEARER", "BeArEr"] {
            let req = req_with(&[("authorization", &format!("{scheme} abc123"))], None);
            assert_eq!(
                extract_bearer(&req).as_deref(),
                Some("abc123"),
                "scheme {scheme} should be accepted",
            );
        }
    }

    #[test]
    fn extract_bearer_rejects_embedded_whitespace_and_falls_back() {
        // "tok en" must not be forwarded — tokens never contain spaces.
        // With no query fallback the result is None; with a query
        // fallback it picks up the query value instead.
        let req = req_with(&[("authorization", "Bearer tok en")], None);
        assert_eq!(extract_bearer(&req).as_deref(), None);

        let req = req_with(&[("authorization", "Bearer tok en")], Some("token=fallback"));
        assert_eq!(extract_bearer(&req).as_deref(), Some("fallback"));
    }

    #[test]
    fn extract_bearer_ignores_empty_credentials() {
        let req = req_with(&[("authorization", "Bearer   ")], Some("token=q"));
        assert_eq!(extract_bearer(&req).as_deref(), Some("q"));

        let req = req_with(&[("authorization", "Bearer")], Some("token=q"));
        // No space → split_once fails → falls back to query.
        assert_eq!(extract_bearer(&req).as_deref(), Some("q"));
    }

    #[test]
    fn extract_bearer_falls_back_to_query_when_no_header() {
        let req = req_with(&[], Some("token=via-query"));
        assert_eq!(extract_bearer(&req).as_deref(), Some("via-query"));

        let req = req_with(&[], None);
        assert_eq!(extract_bearer(&req).as_deref(), None);
    }

    #[test]
    fn extract_bearer_rejects_wrong_scheme() {
        let req = req_with(
            &[("authorization", "Basic dXNlcjpwYXNz")],
            Some("token=q"),
        );
        assert_eq!(extract_bearer(&req).as_deref(), Some("q"));
    }

    #[test]
    fn extract_token_query_rejects_invalid_utf8() {
        // %FF is not a valid UTF-8 start byte. Lossy decoding would
        // return "\u{FFFD}" and accept it; we want None instead so the
        // caller emits a 401 rather than letting U+FFFD reach the
        // constant-time compare.
        assert_eq!(extract_token_query("token=%FF").as_deref(), None);
    }

    #[test]
    fn constant_time_eq_matches_only_exact_input() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"hellp"));
        assert!(!constant_time_eq(b"hello", b"helloworld"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    /// Build a back-to-back WS server/client pair over an in-memory
    /// duplex stream so the `AsyncRead` / `AsyncWrite` impls can be
    /// exercised without binding a real TCP port.
    async fn paired_ws() -> (
        WsStream<tokio::io::DuplexStream>,
        WebSocketStream<tokio::io::DuplexStream>,
    ) {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let server = WebSocketStream::from_raw_socket(a, Role::Server, None).await;
        let client = WebSocketStream::from_raw_socket(b, Role::Client, None).await;
        (WsStream::new(server), client)
    }

    #[tokio::test]
    async fn poll_read_drains_single_binary_message_across_short_reads() {
        let (mut server, mut client) = paired_ws().await;
        // Client sends 100 bytes in a single Binary message.
        let payload: Vec<u8> = (0..100u8).collect();
        client.send(Message::Binary(payload.clone())).await.unwrap();
        client.flush().await.unwrap();

        // Reader pulls 32 bytes at a time across multiple poll_reads.
        let mut out = Vec::new();
        let mut chunk = [0u8; 32];
        while out.len() < 100 {
            let n = server.read(&mut chunk).await.unwrap();
            assert!(n > 0, "premature EOF at {} bytes", out.len());
            out.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(out, payload);
    }

    #[tokio::test]
    async fn poll_write_packages_buffered_bytes_into_single_binary() {
        let (mut server, mut client) = paired_ws().await;
        server.write_all(&[1, 2, 3]).await.unwrap();
        server.write_all(&[4, 5, 6]).await.unwrap();
        server.flush().await.unwrap();
        let msg = client.next().await.unwrap().unwrap();
        match msg {
            Message::Binary(b) => assert_eq!(b, vec![1, 2, 3, 4, 5, 6]),
            other => panic!("expected Binary, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn poll_read_treats_close_as_eof() {
        let (mut server, mut client) = paired_ws().await;
        client.close(None).await.unwrap();

        let mut chunk = [0u8; 16];
        let n = server.read(&mut chunk).await.unwrap();
        assert_eq!(n, 0, "close frame should surface as EOF");
    }

    #[tokio::test]
    async fn poll_read_rejects_text_frame() {
        let (mut server, mut client) = paired_ws().await;
        client.send(Message::Text("hi".into())).await.unwrap();
        client.flush().await.unwrap();

        let mut chunk = [0u8; 16];
        let err = server.read(&mut chunk).await.expect_err("text must error");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
