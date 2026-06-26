//! Browser web gateway: serves the SPA over plain HTTP and upgrades
//! `/ws` to the binary loom-protocol, all on the single `[web]` port.
//!
//! Routing is owned by an `axum` `Router`:
//!   - `GET /ws` → authenticated WebSocket upgrade. After the upgrade the
//!     socket is adapted to `AsyncRead + AsyncWrite` (`AxumWsStream`) and
//!     handed to the same `connection::handle_client` loop every other
//!     transport (Unix socket, TCP, named pipe) uses.
//!   - everything else → static files from `web.static_dir` via
//!     `tower_http`'s `ServeDir`, with an `index.html` SPA fallback.
//!
//! Authentication (token + Origin allowlist) gates **only** `/ws`; the
//! static shell is public by design — it carries no secrets, and the
//! terminal data plane is unreachable without the token. See
//! `WebConfig::static_dir` for the operator-facing contract.

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::{Result, anyhow};
use axum::Router;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{ConnectInfo, RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use futures_util::{Sink, Stream, ready};
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use percent_encoding::percent_decode_str;
use subtle::ConstantTimeEq;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore};
use tower::ServiceExt as _;
use tower_http::services::ServeDir;
use zeroize::Zeroizing;

use loom_config::schema::MIN_WEB_TOKEN_BYTES;
use loom_protocol::codec::MAX_DATA_FRAME_LEN;

use super::connection;
use super::server::Server;

/// Shared, zeroized-on-drop bearer token. The Arc lets every request
/// handler hold a refcount instead of cloning the secret bytes; the
/// inner `Zeroizing<String>` wipes the heap buffer when the last
/// refcount goes away.
pub(crate) type SharedToken = Arc<Zeroizing<String>>;

/// Cap on a single inbound WS message. A wire frame is
/// `[u8 tag][u32 LE len][payload]`, so a full-payload frame is
/// `MAX_DATA_FRAME_LEN + 5` bytes; the cap includes that 5-byte header so
/// a client that packs one maximal frame into a single Binary message is
/// not wrongly rejected, while still bounding per-message memory.
const MAX_WS_MESSAGE_BYTES: usize = MAX_DATA_FRAME_LEN as usize + 5;

/// Cap on the outbound pending-write buffer. The codec frames as
/// `[u8 tag][u32 LE len][payload]`, so one legitimate frame can reach
/// `MAX_DATA_FRAME_LEN + 5`; the extra slack covers the header.
const MAX_WS_WRITE_BYTES: usize = MAX_DATA_FRAME_LEN as usize + 64;

/// Cap on concurrent **authenticated** web connections. The permit is
/// taken only after the token check passes, so an unauthenticated flood
/// can never consume slots; this bounds resource use by a buggy or
/// abusive authenticated client. 64 simultaneous browser tabs is far
/// beyond any single-user deployment. The pre-auth flood is bounded
/// separately by [`MAX_WEB_CONNECTIONS`] at the accept layer.
const MAX_CONCURRENT_WEB_CONNECTIONS: usize = 64;

/// Cap on concurrent **accepted** sockets (authenticated or not), enforced
/// at the accept layer by [`CappedListener`] before any HTTP/WS handshake
/// is parsed. This is the pre-auth backstop: a peer that opens many
/// connections can pin at most this many fds/tasks, and excess connections
/// are closed immediately rather than queued. Set well above
/// [`MAX_CONCURRENT_WEB_CONNECTIONS`] so a page's short-lived parallel
/// asset fetches never starve real WS sessions, while still bounding a
/// flood. Paired with [`WEB_HANDSHAKE_TIMEOUT`] so a peer can neither hold
/// a slot indefinitely nor fill every slot with stalled handshakes.
/// (TLS/connection limits at a fronting reverse proxy remain the
/// recommendation for untrusted exposure.)
const MAX_WEB_CONNECTIONS: usize = 256;

/// Time budget for reading a request's headers, applied per connection by
/// [`serve_web`] via hyper's `header_read_timeout`. It bounds a stalled or
/// slow-drip handshake AND an idle HTTP/1 keep-alive socket sitting between
/// requests, so neither can hold an accept permit forever. It does NOT
/// affect an upgraded WebSocket — once upgraded the connection is no longer
/// reading request headers, so a long-idle terminal session is untouched.
/// Mirrors the 10 s handshake timeout the pre-axum WS path enforced.
const WEB_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Validate the configured web token before the gateway accepts the
/// first connection. Returns the trimmed, zeroized-on-drop, refcounted
/// token. Mirrors the schema-level floor in `WebConfig::validate`; this
/// is the runtime backstop and the only path that allocates the
/// zeroizing container.
pub(crate) fn prepare_web_token(raw: &str) -> Result<SharedToken> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!(
            "[web] enabled but token is empty — refusing to start. \
             Set a non-trivial value for web.token in your config."
        ));
    }
    if trimmed.len() < MIN_WEB_TOKEN_BYTES {
        return Err(anyhow!(
            "[web] token is {} bytes — refusing to start. \
             Minimum is {MIN_WEB_TOKEN_BYTES} bytes (generate one with \
             `openssl rand -hex 16` or `pwgen -s 32 1`).",
            trimmed.len(),
        ));
    }
    Ok(Arc::new(Zeroizing::new(trimmed.to_string())))
}

/// Per-server state shared into every axum handler. All fields are `Arc`
/// so `Clone` (required by axum `State`) is a refcount bump.
#[derive(Clone)]
pub(crate) struct WebState {
    /// `None` = `auth = "none"` (demo mode): `/ws` skips the token check.
    /// The daemon only constructs that state on a loopback bind.
    token: Option<SharedToken>,
    allowed_origins: Arc<Vec<String>>,
    conn_sem: Arc<Semaphore>,
    server: Arc<Mutex<Server>>,
    shutdown: Arc<Notify>,
    input_notify: Arc<Notify>,
}

impl WebState {
    pub(crate) fn new(
        token: Option<SharedToken>,
        allowed_origins: Arc<Vec<String>>,
        server: Arc<Mutex<Server>>,
        shutdown: Arc<Notify>,
        input_notify: Arc<Notify>,
    ) -> Self {
        Self {
            token,
            allowed_origins,
            conn_sem: Arc::new(Semaphore::new(MAX_CONCURRENT_WEB_CONNECTIONS)),
            server,
            shutdown,
            input_notify,
        }
    }
}

/// Resolve the directory the SPA is served from. Empty config →
/// `<server-exe-dir>/web`. A configured value is resolved via the config
/// path resolver: `~/…` expands to home, an absolute path is used as-is,
/// and a RELATIVE path is joined to the config directory (NOT the process
/// cwd) — so a tray/daemon launch from an arbitrary directory resolves the
/// same config to the same bundle instead of serving 503. Logs the
/// outcome; returns the canonical path on success, or `None` (caller serves
/// a 503 for HTTP GETs while the WS gateway keeps working) when the
/// directory is absent or unreadable.
pub(crate) fn resolve_static_dir(configured: &str) -> Option<PathBuf> {
    let trimmed = configured.trim();
    let candidate = if trimmed.is_empty() {
        match std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join("web")))
        {
            Some(p) => p,
            None => {
                log::warn!("[web] could not resolve the server exe dir for the default static_dir");
                return None;
            }
        }
    } else {
        loom_config::config::expand_config_path(trimmed)
    };
    match std::fs::canonicalize(&candidate) {
        Ok(c) if c.is_dir() => {
            log::info!("loomtty-server serving web UI from {}", c.display());
            Some(c)
        }
        _ => {
            log::warn!(
                "[web] static_dir {} is missing or not a directory — serving the WS \
                 gateway only; HTTP GETs will return 503 until a built web bundle is \
                 placed there (see `web.static_dir`)",
                candidate.display(),
            );
            None
        }
    }
}

/// Build the axum app: the authenticated `/ws` route plus static serving.
///
/// Layout: `/ws` → upgrade; `/assets/*` → hashed bundle output, cached a
/// year immutable; everything else → the rest of the built site
/// (`index.html` at `/` via the directory index, plus `sw.js`,
/// `manifest.webmanifest`, icons), `no-cache` so a redeploy is picked up
/// immediately. The client routes by `?session=` query, not URL path, so
/// no SPA path-fallback is needed — unknown paths correctly 404.
pub(crate) fn build_router(state: WebState, static_dir: Option<PathBuf>) -> Router {
    let mut app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/auth-mode", get(auth_mode_handler));

    if let Some(dir) = static_dir {
        // One ServeDir serves the whole built site: `/` → index.html (via
        // the directory index), `/assets/*`, `sw.js`, manifest, icons.
        // `set_static_cache_headers` stamps Cache-Control by path, on
        // SUCCESSFUL responses only — see its doc.
        app = app
            .fallback_service(ServeDir::new(&dir))
            .layer(middleware::from_fn(set_static_cache_headers));
    } else {
        app = app.fallback(spa_unavailable);
    }

    app.with_state(state)
}

/// Stamp `Cache-Control` on static responses: hashed `/assets/*` get a
/// year of immutable caching; the rest of the shell (`index.html`,
/// `sw.js`, `manifest`, icons) gets `no-cache` so a redeploy is seen
/// immediately. Applied to **2xx only**, so a 404 (e.g. a renamed chunk
/// mid-deploy) is never pinned in a cache for a year, and the `/ws`
/// 101/401/503 responses are left untouched.
async fn set_static_cache_headers(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    // API handlers stamp their own Cache-Control (e.g. `/api/auth-mode`
    // sets `no-store`); don't overwrite it with the static-shell policy.
    let api = path.starts_with("/api/");
    let immutable = path.starts_with("/assets/");
    let mut res = next.run(req).await;
    if !api && res.status().is_success() {
        let value = if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    }
    res
}

/// 503 fallback used when no built web bundle is installed.
async fn spa_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "loomtty web UI is not installed on this server (set web.static_dir or place a \
         built bundle next to the server binary)\n",
    )
        .into_response()
}

/// Authenticated WebSocket upgrade. Validates Origin + token **before**
/// upgrading (a failure returns `401` with no upgrade), bounds concurrent
/// authenticated sessions, then adapts the socket to a byte stream and
/// runs the shared client loop. The total accepted-socket count
/// (handshaking peers included) is bounded earlier, at the accept layer,
/// by [`CappedListener`].
async fn ws_handler(
    State(st): State<WebState>,
    ConnectInfo(WebPeer(addr)): ConnectInfo<WebPeer>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers, &st.allowed_origins, st.token.is_none()) {
        log::warn!("ws upgrade from {addr} rejected: Origin not allowed");
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let authed = match st.token.as_ref() {
        // Demo mode (`auth = "none"`, loopback-only): no token to check. The
        // same-origin gate enforced above is the only thing standing between
        // this terminal and any other page in the user's local browser.
        None => true,
        Some(expected) => extract_token(&headers, query.as_deref())
            .as_deref()
            .map(|t| bool::from(t.as_bytes().ct_eq(expected.as_bytes())))
            .unwrap_or(false),
    };
    if !authed {
        log::warn!("ws upgrade from {addr} rejected: bad or missing token");
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let permit = match st.conn_sem.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            log::warn!(
                "ws upgrade from {addr} rejected: {MAX_CONCURRENT_WEB_CONNECTIONS} \
                 concurrent web connections already active"
            );
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    log::info!("ws upgrade from {addr} accepted");
    let WebState {
        server,
        shutdown,
        input_notify,
        ..
    } = st;
    ws.max_message_size(MAX_WS_MESSAGE_BYTES)
        .max_frame_size(MAX_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            let (reader, writer) = tokio::io::split(AxumWsStream::new(socket));
            connection::handle_client(reader, writer, server, shutdown, input_notify).await;
            // Hold the permit for the whole session so the cap bounds
            // live connections, then release on disconnect.
            drop(permit);
        })
}

/// Advertise the gateway's auth mode so the SPA can skip the login card
/// on a demo (`auth = "none"`) gateway. Deliberately public (like the
/// static shell): it reveals only whether a token is required, never the
/// token. `no-store` so a mode flip after restart isn't masked by a cache.
async fn auth_mode_handler(State(st): State<WebState>) -> Response {
    let mode = if st.token.is_none() { "none" } else { "token" };
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        format!("{{\"auth\":\"{mode}\"}}"),
    )
        .into_response()
}

/// Returns true if the request Origin satisfies the policy.
///
/// - A non-empty allowlist always requires an exact Origin match; a missing
///   or unparseable Origin fails.
/// - An empty allowlist is permissive **only when token auth is enabled**
///   (`tokenless == false`): there the token is the gate, and the daemon
///   already refuses an empty allowlist off a loopback bind.
/// - When token auth is disabled (`auth = "none"` demo mode, `tokenless ==
///   true`) an empty allowlist instead enforces **same-origin**: the upgrade's
///   Origin must match its own Host. Loopback binding only hides the port from
///   the network — any other page in the user's local browser can still open
///   `ws://127.0.0.1:<port>/ws`, so without this a tokenless gateway would be
///   drivable cross-origin by an untrusted local page.
fn origin_allowed(headers: &HeaderMap, allowed: &[String], tokenless: bool) -> bool {
    if !allowed.is_empty() {
        let Some(value) = headers.get(header::ORIGIN) else {
            return false;
        };
        let Ok(origin) = value.to_str() else {
            return false;
        };
        return allowed.iter().any(|o| o == origin);
    }
    // Empty allowlist: permissive under token auth (the token gates), but
    // same-origin-only when tokenless (nothing else gates the data plane).
    !tokenless || same_origin(headers)
}

/// True when the request's `Origin` authority equals its `Host` header — i.e.
/// the page driving the upgrade was served by this same gateway. Gates the
/// tokenless demo mode: the loomtty SPA is served from the gateway, so its
/// Origin matches Host, while any other page in the user's browser carries a
/// foreign Origin. Fails closed on a missing / `null` / unparseable Origin or
/// a missing Host.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    // `Origin: scheme://host[:port]`; compare its authority to `Host`. A
    // schemeless / `null` Origin has no `://` and is rejected.
    match origin.split_once("://") {
        Some((_scheme, authority)) => authority == host,
        None => false,
    }
}

/// Pull the bearer token from `Authorization: Bearer <token>` (preferred;
/// stays out of logs/history) or fall back to `?token=…` (the only option
/// for browsers, which can't set headers on a WS upgrade). Header wins
/// when both are present.
fn extract_token(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    let header_token = extract_bearer(headers);
    let query_token = query.and_then(extract_token_query);
    if header_token.is_some() && query_token.is_some() {
        log::debug!("ws auth: both Authorization header and ?token= present — header wins");
    }
    header_token.or(query_token)
}

/// Parse `Authorization: Bearer <token>`. Scheme compare is
/// case-insensitive (RFC 7235 §2.1). A present-but-non-UTF-8 header is
/// dropped (logged at debug, never the bytes); a token with embedded
/// whitespace is rejected as almost-certainly malformed.
fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?;
    let Ok(value) = raw.to_str() else {
        log::debug!("ws auth: Authorization header is not valid UTF-8 — falling back to query");
        return None;
    };
    let (scheme, rest) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let token = rest.trim();
    if token.is_empty() || token.contains(char::is_whitespace) {
        return None;
    }
    Some(token.to_string())
}

/// Find a `token=` pair in a URL query and percent-decode the value.
/// Accepts both `&` (RFC 3986) and `;` (legacy) separators. Invalid
/// UTF-8 short-circuits to `None` so the caller emits a 401 rather than
/// feeding replacement characters into the constant-time compare.
fn extract_token_query(query: &str) -> Option<String> {
    for pair in query.split(['&', ';']) {
        if let Some(value) = pair.strip_prefix("token=") {
            return percent_decode_str(value)
                .decode_utf8()
                .ok()
                .map(|s| s.into_owned());
        }
    }
    None
}

/// Adapter exposing an axum `WebSocket` as `AsyncRead + AsyncWrite`.
///
/// Each inbound `Binary` message contributes its payload to a read buffer
/// drained byte by byte; each `poll_flush` packages the accumulated write
/// bytes into a single `Binary` message. The mapping is stream-oriented
/// (not message-aligned) so the wire codec's framed chunks may straddle
/// WS message boundaries exactly as TCP allows. Mirrors the byte-stream
/// adapter every other transport relies on.
///
/// Generic over the inner WS so unit tests can exercise the read/write
/// plumbing against an in-memory `Message` stream; production uses
/// `AxumWsStream<WebSocket>`.
struct AxumWsStream<S> {
    ws: S,
    /// Bytes from the most recent inbound Binary message not yet
    /// delivered to the reader.
    read_buf: Bytes,
    read_pos: usize,
    /// Bytes accepted by `poll_write` but not yet packaged into a WS
    /// message. Drained by `poll_flush`.
    write_buf: Vec<u8>,
}

impl<S> AxumWsStream<S> {
    fn new(ws: S) -> Self {
        Self {
            ws,
            read_buf: Bytes::new(),
            read_pos: 0,
            write_buf: Vec::new(),
        }
    }
}

impl<S> AsyncRead for AxumWsStream<S>
where
    S: Stream<Item = Result<Message, axum::Error>> + Unpin,
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
            self.read_buf = Bytes::new();
            self.read_pos = 0;

            match Pin::new(&mut self.ws).poll_next(cx) {
                Poll::Ready(Some(Ok(msg))) => match msg {
                    Message::Binary(data) => {
                        if data.is_empty() {
                            // Zero-length frames carry no bytes; re-poll
                            // rather than storing an empty buffer.
                            continue;
                        }
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
                    // tungstenite (under axum) answers pings automatically;
                    // pongs we discard.
                    Message::Ping(_) | Message::Pong(_) => continue,
                },
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(io::Error::other(e))),
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S> AsyncWrite for AxumWsStream<S>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.len() > MAX_WS_WRITE_BYTES {
            return Poll::Ready(Err(io::Error::other(format!(
                "single ws write {} bytes exceeds limit {MAX_WS_WRITE_BYTES}",
                buf.len(),
            ))));
        }
        if self.write_buf.len().saturating_add(buf.len()) > MAX_WS_WRITE_BYTES {
            ready!(self.as_mut().poll_flush(cx))?;
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
            if let Err(e) = Pin::new(&mut self.ws).start_send(Message::Binary(Bytes::from(data))) {
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

/// Serve the web gateway on `listener` until `shutdown` fires.
///
/// Connections are driven by hyper's HTTP/1 server directly (not
/// `axum::serve`) so [`WEB_HANDSHAKE_TIMEOUT`] can bound header reads — the
/// pre-auth slow-drip / idle-keep-alive backstop. [`CappedListener`] bounds
/// the total accepted-socket count; the peer address is injected as a
/// `ConnectInfo<WebPeer>` request extension (axum's connect-info
/// make-service isn't in this hand-rolled path) so `ws_handler` can log it.
pub(crate) async fn serve_web(listener: TcpListener, app: Router, shutdown: Arc<Notify>) {
    let mut capped = CappedListener::new(listener, MAX_WEB_CONNECTIONS);
    loop {
        let (io, addr) = tokio::select! {
            biased;
            // Stop accepting on shutdown. In-flight connections are left to
            // drain: WS sessions exit via the daemon's master `shutdown`,
            // and the runtime's shutdown timeout reaps anything still idle.
            _ = shutdown.notified() => {
                log::debug!("web: shutdown signalled; halting accept loop");
                break;
            }
            accepted = capped.accept() => accepted,
        };
        let app = app.clone();
        tokio::spawn(serve_conn(io, addr, app));
    }
}

/// Drive one accepted connection: inject the peer's `ConnectInfo`, adapt
/// the request body, and serve it over HTTP/1 with a header-read timeout.
/// The accept permit travels inside `io` and is released when the
/// connection — or, after an upgrade, the WebSocket — finally closes.
async fn serve_conn(io: PermittedStream, addr: SocketAddr, app: Router) {
    serve_conn_with(io, addr, app, WEB_HANDSHAKE_TIMEOUT).await
}

/// `serve_conn` with the header-read timeout injected, so tests can drive a
/// stalled connection without waiting the full production budget.
async fn serve_conn_with(
    io: PermittedStream,
    addr: SocketAddr,
    app: Router,
    header_timeout: Duration,
) {
    let service = app
        .layer(axum::Extension(ConnectInfo(WebPeer(addr))))
        .map_request(|req: axum::http::Request<hyper::body::Incoming>| {
            req.map(axum::body::Body::new)
        });
    let conn = http1::Builder::new()
        // `header_read_timeout` needs a timer wired explicitly (unlike
        // `axum::serve`, which sets one up internally).
        .timer(TokioTimer::new())
        .header_read_timeout(header_timeout)
        .serve_connection(TokioIo::new(io), TowerToHyperService::new(service))
        .with_upgrades();
    if let Err(e) = conn.await {
        log::debug!("web: connection from {addr} ended: {e}");
    }
}

/// Peer address for an accepted web socket, injected as a
/// `ConnectInfo<WebPeer>` request extension so `ws_handler` can log which
/// peer a rejected/accepted upgrade came from. (A bespoke newtype because
/// axum's stock `ConnectInfo<SocketAddr>` is wired only for its own
/// connect-info make-service, which this hand-rolled serve path bypasses.)
#[derive(Clone, Copy, Debug)]
struct WebPeer(SocketAddr);

/// A `TcpListener` that bounds the number of concurrently open sockets.
///
/// Each accepted connection takes a permit that lives for the connection's
/// whole lifetime (embedded in the returned [`PermittedStream`], released
/// on drop). When all [`MAX_WEB_CONNECTIONS`] permits are out a new
/// connection is accepted and then immediately closed — load-shed, not
/// queued — so a peer that floods connections can pin at most that many
/// fds/tasks. This is the pre-auth backstop the post-auth [`WebState`]
/// semaphore cannot give (it is reached only after a successful token
/// check); paired with [`WEB_HANDSHAKE_TIMEOUT`], a slow peer can't hold a
/// slot indefinitely either.
struct CappedListener {
    inner: TcpListener,
    sem: Arc<Semaphore>,
    max: usize,
}

impl CappedListener {
    fn new(inner: TcpListener, max: usize) -> Self {
        Self {
            inner,
            sem: Arc::new(Semaphore::new(max)),
            max,
        }
    }

    /// Accept the next connection that fits under the cap, load-shedding
    /// (accept-then-close) any arriving while the cap is full. Never returns
    /// on a transient accept error — it logs, pauses briefly, and retries.
    async fn accept(&mut self) -> (PermittedStream, SocketAddr) {
        loop {
            let (stream, addr) = match self.inner.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    // Per-connection accept errors (ECONNABORTED, …) are
                    // transient; the short pause also avoids busy-spinning
                    // on a persistent condition such as EMFILE.
                    log::debug!("web: accept error: {e}");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
            };
            match self.sem.clone().try_acquire_owned() {
                Ok(permit) => {
                    // Terminal interactivity needs small input/render frames
                    // on the wire immediately; disable Nagle to avoid the
                    // delayed-ACK coalescing latency (matches the pre-axum WS
                    // accept path). Best-effort: a failure here is harmless.
                    if let Err(e) = stream.set_nodelay(true) {
                        log::debug!("web: set_nodelay failed for {addr}: {e}");
                    }
                    return (
                        PermittedStream {
                            inner: stream,
                            _permit: permit,
                        },
                        addr,
                    );
                }
                Err(_) => {
                    log::warn!(
                        "web: refusing connection from {addr} — {} sockets already open \
                         (accept cap reached)",
                        self.max,
                    );
                    // Drop the stream → FIN, freeing the fd at once; keep
                    // looping for the next (possibly legitimate) peer.
                    drop(stream);
                }
            }
        }
    }
}

/// A `TcpStream` that holds an accept-layer permit for its lifetime. The
/// permit returns to [`CappedListener`]'s semaphore when the connection
/// closes and this is dropped. All I/O delegates straight to the inner
/// stream.
struct PermittedStream {
    inner: TcpStream,
    _permit: OwnedSemaphorePermit,
}

impl AsyncRead for PermittedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for PermittedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn extract_token_query_finds_first_pair() {
        assert_eq!(extract_token_query("token=abc").as_deref(), Some("abc"));
        assert_eq!(
            extract_token_query("foo=1&token=abc").as_deref(),
            Some("abc")
        );
        assert_eq!(
            extract_token_query("token=abc&token=second").as_deref(),
            Some("abc")
        );
        assert_eq!(extract_token_query("foo=bar").as_deref(), None);
        assert_eq!(extract_token_query("").as_deref(), None);
        assert_eq!(extract_token_query("token=").as_deref(), Some(""));
    }

    #[test]
    fn extract_token_query_percent_decodes() {
        assert_eq!(
            extract_token_query("token=abc%2Bdef").as_deref(),
            Some("abc+def")
        );
        assert_eq!(
            extract_token_query("token=ab%3Dcd").as_deref(),
            Some("ab=cd")
        );
        assert_eq!(
            extract_token_query("token=%E4%B8%AD").as_deref(),
            Some("中")
        );
    }

    #[test]
    fn extract_token_query_stops_at_unencoded_separator() {
        assert_eq!(extract_token_query("token=abc&def").as_deref(), Some("abc"));
        assert_eq!(
            extract_token_query("token=abc%26def").as_deref(),
            Some("abc&def")
        );
        assert_eq!(extract_token_query("token=abc;def").as_deref(), Some("abc"));
        assert_eq!(
            extract_token_query("foo=1;token=abc").as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn extract_token_query_rejects_invalid_utf8() {
        assert_eq!(extract_token_query("token=%FF").as_deref(), None);
    }

    #[test]
    fn extract_bearer_matches_scheme_case_insensitively() {
        for scheme in &["Bearer", "bearer", "BEARER", "BeArEr"] {
            let h = headers(&[("authorization", &format!("{scheme} abc123"))]);
            assert_eq!(
                extract_bearer(&h).as_deref(),
                Some("abc123"),
                "scheme {scheme}"
            );
        }
    }

    #[test]
    fn extract_bearer_rejects_embedded_whitespace() {
        let h = headers(&[("authorization", "Bearer tok en")]);
        assert_eq!(extract_bearer(&h), None);
    }

    #[test]
    fn extract_bearer_ignores_empty_credentials() {
        assert_eq!(
            extract_bearer(&headers(&[("authorization", "Bearer   ")])),
            None
        );
        // No space → split_once fails.
        assert_eq!(
            extract_bearer(&headers(&[("authorization", "Bearer")])),
            None
        );
    }

    #[test]
    fn extract_bearer_rejects_wrong_scheme() {
        let h = headers(&[("authorization", "Basic dXNlcjpwYXNz")]);
        assert_eq!(extract_bearer(&h), None);
    }

    #[test]
    fn extract_token_header_wins_over_query() {
        let h = headers(&[("authorization", "Bearer correct")]);
        assert_eq!(
            extract_token(&h, Some("token=wrong")).as_deref(),
            Some("correct")
        );
        // A wrong header still wins (and will fail the compare) so an
        // attacker who can inject only the header can't silently downgrade
        // a query-authenticated client.
        let h = headers(&[("authorization", "Bearer wrong")]);
        assert_eq!(
            extract_token(&h, Some("token=correct")).as_deref(),
            Some("wrong")
        );
    }

    #[test]
    fn extract_token_falls_back_to_query() {
        let h = HeaderMap::new();
        assert_eq!(
            extract_token(&h, Some("token=via-query")).as_deref(),
            Some("via-query")
        );
        assert_eq!(extract_token(&h, None), None);
    }

    #[test]
    fn origin_allowed_empty_allowlist_is_permissive_under_token_auth() {
        // Token mode (`tokenless = false`): the token is the gate, so an empty
        // allowlist accepts any/no Origin (daemon enforces loopback-only here).
        assert!(origin_allowed(&HeaderMap::new(), &[], false));
        assert!(origin_allowed(
            &headers(&[("origin", "http://anywhere.example")]),
            &[],
            false
        ));
        assert!(origin_allowed(&headers(&[("origin", "null")]), &[], false));
    }

    #[test]
    fn origin_allowed_tokenless_empty_allowlist_requires_same_origin() {
        // Demo mode (`tokenless = true`): an empty allowlist falls back to a
        // same-origin check, so only the gateway's own SPA can drive `/ws`.
        let same = headers(&[
            ("host", "127.0.0.1:7681"),
            ("origin", "http://127.0.0.1:7681"),
        ]);
        assert!(origin_allowed(&same, &[], true), "same-origin must pass");

        // A foreign page in the same browser (cross-origin) is rejected.
        let cross = headers(&[
            ("host", "127.0.0.1:7681"),
            ("origin", "http://evil.example"),
        ]);
        assert!(!origin_allowed(&cross, &[], true), "cross-origin must fail");

        // Missing Origin, `null` Origin, and missing Host all fail closed.
        assert!(!origin_allowed(
            &headers(&[("host", "127.0.0.1:7681")]),
            &[],
            true
        ));
        assert!(!origin_allowed(
            &headers(&[("host", "127.0.0.1:7681"), ("origin", "null")]),
            &[],
            true
        ));
        assert!(!origin_allowed(
            &headers(&[("origin", "http://127.0.0.1:7681")]),
            &[],
            true
        ));
    }

    #[test]
    fn origin_allowed_strict_allowlist_requires_match() {
        let allowed = vec!["https://terminal.example.com".to_string()];
        // A non-empty allowlist requires an exact match regardless of mode.
        for tokenless in [false, true] {
            assert!(origin_allowed(
                &headers(&[("origin", "https://terminal.example.com")]),
                &allowed,
                tokenless
            ));
            assert!(!origin_allowed(
                &headers(&[("origin", "https://attacker.example.com")]),
                &allowed,
                tokenless
            ));
            // Missing Origin under strict policy must fail.
            assert!(!origin_allowed(&HeaderMap::new(), &allowed, tokenless));
        }
    }

    #[test]
    fn prepare_web_token_rejects_empty_and_short() {
        for raw in ["", "   ", "\n", "\t\t"] {
            assert!(prepare_web_token(raw).is_err(), "{raw:?} must error");
        }
        assert!(prepare_web_token(&"a".repeat(MIN_WEB_TOKEN_BYTES - 1)).is_err());
    }

    #[test]
    fn prepare_web_token_trims_then_checks_and_accepts() {
        // Trailing newline from `openssl rand -hex 16 > token.txt` is the
        // classic auth-mismatch source; trim before the length check.
        let token = prepare_web_token("  deadbeefcafebabe1234  \n").unwrap();
        assert_eq!(token.as_str(), "deadbeefcafebabe1234");
        assert_eq!(
            prepare_web_token(&"a".repeat(MIN_WEB_TOKEN_BYTES))
                .unwrap()
                .len(),
            MIN_WEB_TOKEN_BYTES
        );
    }

    #[test]
    fn subtle_ct_eq_matches_only_exact_input() {
        let eq = |a: &[u8], b: &[u8]| bool::from(a.ct_eq(b));
        assert!(eq(b"hello", b"hello"));
        assert!(!eq(b"hello", b"hellp"));
        assert!(!eq(b"hello", b"helloworld"));
        assert!(eq(b"", b""));
    }
}

/// In-process router tests: drive the real axum app via `oneshot` (no
/// bound port, no daemon) to verify static serving, caching headers, the
/// SPA fallback, and that `/ws` enforces auth before upgrading.
#[cfg(test)]
mod router_tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{HeaderMap, Request, StatusCode};
    use std::net::SocketAddr;
    use std::path::Path;
    use tower::ServiceExt; // for `oneshot`

    use crate::daemon::server::Server;
    use loom_term::pane::TerminalColors;

    fn test_state() -> WebState {
        let server = std::sync::Arc::new(tokio::sync::Mutex::new(Server::new(
            "",
            8.0,
            TerminalColors::default(),
        )));
        WebState::new(
            Some(prepare_web_token("0123456789abcdef0123456789abcdef").unwrap()),
            std::sync::Arc::new(Vec::new()),
            server,
            std::sync::Arc::new(tokio::sync::Notify::new()),
            std::sync::Arc::new(tokio::sync::Notify::new()),
        )
    }

    fn test_state_noauth() -> WebState {
        let server = std::sync::Arc::new(tokio::sync::Mutex::new(Server::new(
            "",
            8.0,
            TerminalColors::default(),
        )));
        WebState::new(
            None,
            std::sync::Arc::new(Vec::new()),
            server,
            std::sync::Arc::new(tokio::sync::Notify::new()),
            std::sync::Arc::new(tokio::sync::Notify::new()),
        )
    }

    fn write_site(dir: &Path) {
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><title>loomtty</title>",
        )
        .unwrap();
        std::fs::write(dir.join("assets/app-abc123.js"), "console.log(1)").unwrap();
        std::fs::write(dir.join("sw.js"), "/* sw */").unwrap();
    }

    fn site_router() -> (tempfile::TempDir, Router) {
        site_router_with(test_state())
    }

    fn site_router_with(state: WebState) -> (tempfile::TempDir, Router) {
        let tmp = tempfile::tempdir().unwrap();
        write_site(tmp.path());
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let router = build_router(state, Some(dir));
        (tmp, router)
    }

    async fn get(router: &Router, uri: &str) -> (StatusCode, HeaderMap, String) {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let res = router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    fn cache_control(headers: &HeaderMap) -> String {
        headers
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }

    #[tokio::test]
    async fn serves_index_at_root_with_no_cache() {
        let (_tmp, router) = site_router();
        let (status, headers, body) = get(&router, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("loomtty"), "body was: {body}");
        assert_eq!(cache_control(&headers), "no-cache");
    }

    #[tokio::test]
    async fn serves_hashed_asset_immutable() {
        let (_tmp, router) = site_router();
        let (status, headers, _) = get(&router, "/assets/app-abc123.js").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            cache_control(&headers).contains("immutable"),
            "cache-control was: {}",
            cache_control(&headers),
        );
    }

    #[tokio::test]
    async fn unknown_deep_path_is_404() {
        // The client routes by `?session=` query, not URL path, so there
        // are no deep client routes to fall back for — an unknown path is
        // a genuine miss, not a SPA route.
        let (_tmp, router) = site_router();
        let (status, _, _) = get(&router, "/sessions/42").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn missing_asset_is_404_and_not_immutably_cached() {
        let (_tmp, router) = site_router();
        let (status, headers, _) = get(&router, "/assets/missing-deadbeef.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // A 404 for a renamed/missing chunk mid-deploy must NOT be pinned
        // in a cache for a year — the immutable header is 2xx-only.
        assert!(
            !cache_control(&headers).contains("immutable"),
            "404 cache-control was: {}",
            cache_control(&headers),
        );
    }

    #[tokio::test]
    async fn missing_static_dir_returns_503() {
        let router = build_router(test_state(), None);
        let (status, _, _) = get(&router, "/").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn ws_auth_over_real_socket() {
        // The `WebSocketUpgrade` extractor needs hyper's `OnUpgrade`
        // extension, which only a real connection provides — so drive the
        // auth gate over an actual bound socket rather than `oneshot`.
        let (_tmp, router) = site_router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Serve through the production path (serve_web → CappedListener +
        // hyper http1 + header-read timeout + injected ConnectInfo).
        let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
        let server = tokio::spawn(serve_web(listener, router, shutdown));

        async fn status_line(addr: SocketAddr, path: &str) -> String {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
            let req = format!(
                "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: Upgrade\r\n\
                 Upgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                 Sec-WebSocket-Version: 13\r\n\r\n",
            );
            s.write_all(req.as_bytes()).await.unwrap();
            let mut buf = [0u8; 64];
            let n = s.read(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf[..n])
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        }

        // No token → 401; correct token → 101 Switching Protocols.
        assert!(
            status_line(addr, "/ws").await.contains("401"),
            "missing token must be rejected",
        );
        assert!(
            status_line(addr, "/ws?token=0123456789abcdef0123456789abcdef")
                .await
                .contains("101"),
            "correct token must upgrade",
        );

        server.abort();
    }

    #[tokio::test]
    async fn ws_demo_mode_enforces_same_origin() {
        // Demo mode (`auth = "none"`): a tokenless upgrade succeeds ONLY when
        // the page is same-origin with the gateway (the SPA we served). A
        // foreign or missing Origin — any other page in the user's local
        // browser reaching loopback — must still be rejected.
        let (_tmp, router) = site_router_with(test_state_noauth());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
        let server = tokio::spawn(serve_web(listener, router, shutdown));

        // `origin` of `None` omits the header entirely.
        async fn status_line(addr: SocketAddr, origin: Option<&str>) -> String {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
            let origin_line = origin
                .map(|o| format!("Origin: {o}\r\n"))
                .unwrap_or_default();
            let req = format!(
                "GET /ws HTTP/1.1\r\nHost: {addr}\r\n{origin_line}Connection: Upgrade\r\n\
                 Upgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                 Sec-WebSocket-Version: 13\r\n\r\n",
            );
            s.write_all(req.as_bytes()).await.unwrap();
            let mut buf = [0u8; 64];
            let n = s.read(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf[..n])
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        }

        // Same-origin (Origin authority == Host) → 101 Switching Protocols.
        let same = status_line(addr, Some(&format!("http://{addr}"))).await;
        assert!(
            same.contains("101"),
            "same-origin tokenless upgrade must succeed, got: {same}"
        );
        // Foreign Origin → 401, even though no token is required.
        let cross = status_line(addr, Some("http://evil.example")).await;
        assert!(
            cross.contains("401"),
            "cross-origin upgrade must be rejected, got: {cross}"
        );
        // Missing Origin → 401 (fails closed).
        let none = status_line(addr, None).await;
        assert!(
            none.contains("401"),
            "originless upgrade must be rejected, got: {none}"
        );

        server.abort();
    }

    #[tokio::test]
    async fn auth_mode_endpoint_reports_mode() {
        let (_tmp, router) = site_router();
        let (status, headers, body) = get(&router, "/api/auth-mode").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "{\"auth\":\"token\"}");
        assert_eq!(cache_control(&headers), "no-store");

        let (_tmp2, router) = site_router_with(test_state_noauth());
        let (status, _, body) = get(&router, "/api/auth-mode").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "{\"auth\":\"none\"}");
    }

    #[tokio::test]
    async fn directory_traversal_does_not_escape_root() {
        let (_tmp, router) = site_router();
        // ServeDir rejects `..` traversal; the encoded form must not reach
        // a file outside the served root either.
        for uri in [
            "/../Cargo.toml",
            "/..%2f..%2fCargo.toml",
            "/%2e%2e/Cargo.toml",
        ] {
            let (status, _, body) = get(&router, uri).await;
            assert_ne!(status, StatusCode::OK, "{uri} unexpectedly served 200");
            assert!(!body.contains("[package]"), "{uri} leaked a manifest");
        }
    }
}

/// Byte-plumbing tests for `AxumWsStream` against an in-memory `Message`
/// stream — no real WebSocket needed. Covers the same read/write
/// invariants the previous tungstenite adapter was tested for.
#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::collections::VecDeque;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// In-memory stand-in for axum's `WebSocket`: yields queued inbound
    /// messages and records what the writer sends.
    struct MockWs {
        inbound: VecDeque<Result<Message, axum::Error>>,
        sent: Vec<Message>,
    }

    impl MockWs {
        fn new(inbound: Vec<Message>) -> Self {
            Self {
                inbound: inbound.into_iter().map(Ok).collect(),
                sent: Vec::new(),
            }
        }
    }

    impl Stream for MockWs {
        type Item = Result<Message, axum::Error>;
        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.get_mut().inbound.pop_front())
        }
    }

    impl Sink<Message> for MockWs {
        type Error = axum::Error;
        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), axum::Error>> {
            Poll::Ready(Ok(()))
        }
        fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), axum::Error> {
            self.get_mut().sent.push(item);
            Ok(())
        }
        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), axum::Error>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), axum::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn poll_read_drains_binary_across_short_reads() {
        let payload: Vec<u8> = (0..100u8).collect();
        let mut s = AxumWsStream::new(MockWs::new(vec![Message::Binary(Bytes::from(
            payload.clone(),
        ))]));
        let mut out = Vec::new();
        let mut chunk = [0u8; 32];
        loop {
            let n = s.read(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(out, payload);
    }

    #[tokio::test]
    async fn poll_read_reassembles_consecutive_binary_messages() {
        let mut s = AxumWsStream::new(MockWs::new(vec![
            Message::Binary(Bytes::from_static(&[1, 2, 3])),
            Message::Binary(Bytes::from_static(&[4, 5])),
            Message::Binary(Bytes::from_static(&[6, 7, 8, 9])),
        ]));
        let mut out = Vec::new();
        let mut chunk = [0u8; 4];
        loop {
            let n = s.read(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[tokio::test]
    async fn poll_write_packages_buffered_bytes_into_single_binary() {
        let mut s = AxumWsStream::new(MockWs::new(vec![]));
        s.write_all(&[1, 2, 3]).await.unwrap();
        s.write_all(&[4, 5, 6]).await.unwrap();
        s.flush().await.unwrap();
        assert_eq!(s.ws.sent.len(), 1);
        match &s.ws.sent[0] {
            Message::Binary(b) => assert_eq!(b.as_ref(), &[1, 2, 3, 4, 5, 6]),
            other => panic!("expected Binary, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn close_message_surfaces_as_eof() {
        let mut s = AxumWsStream::new(MockWs::new(vec![Message::Close(None)]));
        let mut chunk = [0u8; 8];
        assert_eq!(s.read(&mut chunk).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn text_message_is_invalid_on_binary_channel() {
        let mut s = AxumWsStream::new(MockWs::new(vec![Message::Text("hi".into())]));
        let mut chunk = [0u8; 8];
        let err = s.read(&mut chunk).await.expect_err("text must error");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn ping_frames_are_skipped() {
        let mut s = AxumWsStream::new(MockWs::new(vec![
            Message::Ping(Bytes::from_static(&[0xAA])),
            Message::Binary(Bytes::from_static(&[7, 7, 7])),
        ]));
        let mut chunk = [0u8; 8];
        let n = s.read(&mut chunk).await.unwrap();
        assert_eq!(&chunk[..n], &[7, 7, 7]);
    }
}

/// Accept-layer connection cap: a flood/slow-drip can pin at most
/// `MAX_WEB_CONNECTIONS` sockets, and excess connections are closed
/// immediately rather than queued — the pre-auth backstop.
#[cfg(test)]
mod listener_tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn capped_listener_sheds_beyond_limit_and_recovers() {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let mut capped = CappedListener::new(tcp, 1); // cap = 1 socket

        // Drive accepts in a task, forwarding each PERMITTED stream out so
        // the test can hold it (and thus its permit) alive.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PermittedStream>();
        let pump = tokio::spawn(async move {
            loop {
                let (io, _peer) = capped.accept().await;
                if tx.send(io).is_err() {
                    break;
                }
            }
        });

        // First peer: takes the only permit; its stream is handed out.
        let _c1 = TcpStream::connect(addr).await.unwrap();
        let held = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("first connection should be accepted")
            .expect("pump channel open");

        // Second peer, while the permit is held: the listener accepts the
        // TCP connection then closes it (load-shed) and never yields it.
        // The close surfaces as a clean EOF (FIN → read returns 0) on most
        // platforms, or a reset error (RST) on others; either proves the
        // server dropped it. A timeout means it was wrongly left open.
        let mut c2 = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1];
        let closed = match tokio::time::timeout(Duration::from_secs(5), c2.read(&mut buf)).await {
            Ok(Ok(0)) => true,  // FIN: clean EOF
            Ok(Ok(_)) => false, // server sent data — it did NOT shed
            Ok(Err(_)) => true, // RST/abort — also a server-side close
            Err(_) => false,    // timed out — connection left open
        };
        assert!(closed, "a shed connection must be closed by the server");
        assert!(
            rx.try_recv().is_err(),
            "a shed connection must not reach the app"
        );

        // Releasing the held permit admits the next connection.
        drop(held);
        let _c3 = TcpStream::connect(addr).await.unwrap();
        let _held3 = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a freed permit should admit the next connection")
            .expect("pump channel open");

        pump.abort();
    }

    /// A peer that connects but never sends a request line/headers must be
    /// closed once the header-read timeout elapses, so it can't hold its
    /// accept permit forever. Uses a short timeout in place of the
    /// production [`WEB_HANDSHAKE_TIMEOUT`].
    #[tokio::test]
    async fn silent_connection_is_reaped_by_header_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(addr).await.unwrap();
        let (server_stream, peer) = listener.accept().await.unwrap();

        // Wrap the accepted stream like the production path does.
        let sem = Arc::new(Semaphore::new(1));
        let permit = sem.try_acquire_owned().unwrap();
        let io = PermittedStream {
            inner: server_stream,
            _permit: permit,
        };
        // Serve with a short header-read timeout; the router is irrelevant
        // because the client never sends a request to route.
        tokio::spawn(serve_conn_with(
            io,
            peer,
            Router::new(),
            Duration::from_millis(300),
        ));

        // The client stays silent; its socket must be closed shortly after
        // the timeout (clean EOF, or a reset on some platforms).
        let mut buf = [0u8; 1];
        let closed = match tokio::time::timeout(Duration::from_secs(5), client.read(&mut buf)).await
        {
            Ok(Ok(0)) => true,  // EOF
            Ok(Ok(_)) => false, // server sent data — not reaped
            Ok(Err(_)) => true, // reset — also closed
            Err(_) => false,    // still open after 5s — timeout never fired
        };
        assert!(
            closed,
            "a silent pre-auth connection must be closed after the header-read timeout",
        );
    }
}
