//! `ciritty web` — turnkey browser access.
//!
//! Resolves (or mints) an auth token and enables `[web]`, persisting them
//! (plus an explicit `--bind`/`--port`/`--static-dir`) to the user's config
//! so the desktop daemon and future runs share one self-consistent gateway
//! configuration, prints the access URL, then runs `ciritty-server` in the
//! foreground with the chosen web settings. The server self-hosts the SPA
//! + the authenticated `/ws` upgrade on one port (see `ciri-server`'s
//! `daemon::web`).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use ciri_config::config::CiriConfig;
use ciri_config::{MIN_WEB_TOKEN_BYTES, web_token_is_usable};
use ciri_config::writer::EditableConfig;

/// Entry point for `CliCommand::Web`.
pub fn run_web(
    port: Option<u16>,
    bind: Option<String>,
    token: Option<String>,
    static_dir: Option<String>,
    open: bool,
) -> Result<()> {
    // Lenient load: tolerate an already schema-invalid `[web]` (e.g. a
    // stale `enabled = true` with an unusable token) so this command can
    // REPAIR it — `CiriConfig::load()` would reject it up front, leaving no
    // recovery but hand-editing. The effective settings are re-validated
    // below before anything is persisted.
    let mut config = CiriConfig::load_lenient().context("loading config")?;

    // Token precedence: explicit `--token` (warned — it lingers in this
    // process's argv), else the `CIRITTY_WEB_TOKEN` env var (readable only
    // by the owner via /proc/<pid>/environ, unlike world-readable argv),
    // else a usable configured token, else a freshly minted one.
    let token = if let Some(t) = token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        log::warn!(
            "--token stays visible to other users in process listings (ps, \
             /proc/<pid>/cmdline) for as long as `ciritty web` runs; prefer the \
             CIRITTY_WEB_TOKEN environment variable, or omit --token to auto-generate one"
        );
        t
    } else if let Some(t) = std::env::var("CIRITTY_WEB_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        t
    } else if web_token_is_usable(&config.web.token) {
        config.web.token.trim().to_string()
    } else {
        generate_token()?
    };
    // Validate the resolved token (covers an explicit `--token` too) BEFORE
    // persisting it: a too-short or placeholder token would otherwise be
    // saved with `enabled = true`, and the server — which re-validates the
    // config on load — would then refuse to start on EVERY launch (desktop
    // included) until the config is hand-edited.
    if !web_token_is_usable(&token) {
        bail!(
            "token is unusable (under {MIN_WEB_TOKEN_BYTES} bytes, or a known \
             placeholder). Pass a real --token (e.g. `openssl rand -hex 16`), or \
             omit it to auto-generate one.",
        );
    }

    // Whether a setting came from an explicit flag. An explicit value is
    // persisted (so the saved config stays self-consistent with
    // `enabled = true`, AND so it still applies when the user restarts an
    // already-running server, which never sees the transient `--web-*`
    // flags); an inherited one leaves the file's value untouched.
    let port_overridden = port.is_some();
    let bind_overridden = bind.is_some();

    // Resolve the effective listen address up front so a misconfiguration
    // fails fast — before anything is persisted.
    let port = port.unwrap_or(config.web.port);
    let bind = bind.unwrap_or_else(|| {
        if config.web.bind.is_empty() {
            "127.0.0.1".to_string()
        } else {
            config.web.bind.clone()
        }
    });

    // Resolve an explicit --static-dir to an ABSOLUTE path before it's used
    // anywhere (validation, persistence, the child flag). A later
    // desktop/tray launch resolves a relative web.static_dir against its OWN
    // cwd, so persisting a non-absolute value would serve 503 after a
    // restart from a different directory.
    //   • `~/…` / `~\…`: expand the home shortcut here — the shell may not
    //     (PowerShell, quoted args), and once absolutized the buried `~`
    //     would never be expanded by the daemon's resolver.
    //   • anything else: make it absolute against THIS command's cwd
    //     (intuitive for an interactive flag). `absolute` is lexical, so it
    //     also works when the bundle isn't built there yet.
    let static_dir = static_dir.map(|d| {
        let resolved = if d.starts_with("~/") || d.starts_with("~\\") {
            ciri_config::config::expand_config_path(&d)
        } else {
            std::path::absolute(&d).unwrap_or_else(|_| PathBuf::from(&d))
        };
        resolved.to_string_lossy().into_owned()
    });
    // Treat any IP that parses as loopback as loopback — covers 127.0.0.0/8,
    // ::1, and alternate spellings like 0:0:0:0:0:0:0:1, matching the
    // server/schema's own `is_loopback()` preflight. A non-IP spelling
    // (e.g. "localhost") is left for the schema validation below to reject
    // with a precise "must be an IP" message rather than the origins one.
    let loopback = bind
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(true);

    // A non-loopback bind with no Origin allowlist is a CSRF magnet, and
    // the server refuses to start in that case. Fail HERE — before
    // persisting `enabled = true` / printing the token — with an
    // actionable message, rather than a cryptic "ciritty-server exited"
    // after the fact.
    if !loopback && config.web.allowed_origins.is_empty() {
        bail!(
            "bind {bind} is not loopback, but web.allowed_origins is empty. Add the \
             browser origin(s) you'll serve (e.g. \
             allowed_origins = [\"https://terminal.example.com\"]) to your config, \
             then rerun.",
        );
    }

    // `ciritty web` serves the SPA over http(s), so the browser always sends
    // a real scheme://host[:port] Origin — never the special "null". If the
    // allowlist contains ONLY "null" (the Origin of file:// / sandboxed
    // clients), every login from the self-hosted page would 401. Fail fast
    // rather than print a URL that can't authenticate.
    if !config.web.allowed_origins.is_empty()
        && config.web.allowed_origins.iter().all(|o| o == "null")
    {
        bail!(
            "web.allowed_origins is only [\"null\"], but the self-hosted web UI is \
             served over http(s) and sends a real Origin — every login would be \
             rejected. Add the origin you'll open (e.g. \"http://127.0.0.1:{port}\") \
             to allowed_origins, then rerun.",
        );
    }

    // Validate BOTH the config we'll PERSIST and the config the spawned
    // server will RUN, BEFORE writing anything. The server re-validates the
    // whole file on load (and would exit on a bad value like `--port 0` or
    // a non-IP `--bind localhost`), so an `enabled = true` left next to a
    // now-inconsistent bind/port/origins would make the child — AND every
    // later desktop/server launch — refuse to start until the file is
    // hand-edited.
    //
    // 1) Persisted form: the stored bind/port, plus enabled/token and any
    //    explicit --bind/--port (only those land on disk, below). This is
    //    exactly what a future `CiriConfig::load()` will see.
    config.web.enabled = true;
    config.web.token = token.clone();
    if bind_overridden {
        config.web.bind = bind.clone();
    }
    if port_overridden {
        config.web.port = port;
    }
    // static_dir is identical in both forms (no resolution step), so set it
    // once here; the schema only NUL-checks it, but persisting an explicit
    // value is what makes it survive a restart of a running server.
    if let Some(dir) = &static_dir {
        config.web.static_dir = dir.clone();
    }
    config.validate_schema().context(
        "enabling web with these settings would write a config that fails to load — \
         fix [web] (bind / allowed_origins / token) in your config and rerun",
    )?;
    // 2) Effective (runtime) form: the resolved bind/port the child runs
    //    with. Differs from the persisted form only when --bind/--port were
    //    omitted (e.g. an empty stored bind resolving to loopback).
    config.web.bind = bind.clone();
    config.web.port = port;
    config
        .validate_schema()
        .context("resolved web settings are invalid")?;

    // The schema can't express the cross-section invariant that
    // remote.port != web.port (derive-Validate sees each table in
    // isolation), so the daemon enforces it at runtime and exits on a
    // clash. Re-check here, before persisting, so a colliding `--port` (or
    // stored web.port) isn't written with enabled = true only to brick
    // every later desktop/server launch.
    if config.remote.enabled && config.remote.port == port {
        bail!(
            "remote.port and web.port are both {port} — pick distinct ports \
             (defaults are 7890/7891), e.g. `ciritty web --port 7891`.",
        );
    }

    // Decide up front whether we'll spawn a server or defer to a running one.
    let running = crate::connection::server_is_running();

    // Spawn path only: preflight-bind the chosen address so we never persist
    // `enabled = true` + a port a FOREIGN process holds — the child would
    // exit on the bind error, and every later desktop/server launch would
    // then fail the same way until the config is hand-edited. Skipped when a
    // daemon is already running (it may legitimately hold the port, and we
    // won't spawn anyway). A tiny TOCTOU window remains before the child's
    // real bind, but a persistently occupied port — the actual failure mode
    // — is caught here, before anything is written.
    if !running
        && let Err(e) = preflight_bind(&bind, port)
    {
        bail!(
            "can't bind {bind}:{port} — {e}. Another process (or a stale ciritty) may \
             be using it; pick a free port with `ciritty web --port N`.",
        );
    }

    // Persist (comment-preserving) so the desktop app and later `ciritty
    // web` runs reuse the same gateway. enabled + token always; bind/port/
    // static_dir ONLY when explicitly given — a one-off run must not
    // silently rewrite stored settings, but an override we DID act on has to
    // be saved (both to keep the file self-consistent, and so it still
    // applies when a running server is restarted to pick up the changes).
    let mut editable = EditableConfig::load().context("opening config for write")?;
    editable.set_web_enabled(true);
    editable.set_web_token(&token);
    if bind_overridden {
        editable.set_web_bind(&bind);
    }
    if port_overridden {
        editable.set_web_port(port);
    }
    if let Some(dir) = &static_dir {
        editable.set_web_static_dir(dir);
    }
    editable.save().context("saving config")?;

    // Build the URL the user should open. When an Origin allowlist is set,
    // `/ws` authenticates only requests whose browser Origin EXACTLY matches
    // an entry — so a bind-derived URL (e.g. 127.0.0.1) would 401 even with
    // the right token under configs like ["http://localhost:7891"] or a
    // proxied ["https://terminal.example.com"]. Prefer the first allowlisted
    // origin (itself a scheme://host[:port] base); fall back to the
    // bind-derived URL only when the policy is open or lists just the
    // special "null" origin. The bind-derived form maps wildcard binds to
    // loopback and brackets IPv6 literals so the URL stays parseable.
    let url = match config.web.allowed_origins.iter().find(|o| o.as_str() != "null") {
        Some(origin) => format!("{}/", origin.trim_end_matches('/')),
        None => {
            let display_host = match bind.as_str() {
                "0.0.0.0" => "127.0.0.1".to_string(),
                "::" => "[::1]".to_string(),
                h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
                h => h.to_string(),
            };
            format!("http://{display_host}:{port}/")
        }
    };

    // If a daemon is already running, do NOT spawn a second one (on Unix a
    // fresh socket would be renamed over the live one, splitting clients
    // across two daemons; on Windows the child just exits on the existing
    // pipe). The new [web] config is saved above, but the RUNNING server
    // keeps its CURRENT settings until restarted — so frame the saved
    // URL/token as pending, not already live.
    if running {
        println!();
        println!("  Web settings saved to your config.");
        println!();
        println!("  A ciritty server is already running with its current settings:");
        println!("    • If they already match what you just saved (e.g. a prior");
        println!("      `ciritty web` with the same options), open:");
        println!("        {url}");
        println!("      and log in with token:  {token}");
        println!("    • If you changed --port/--bind/--token/--static-dir, the running");
        println!("      server still uses the OLD ones — restart to apply:");
        println!("      `ciritty kill-server` (sessions are saved), then rerun `ciritty web`.");
        println!();
        return Ok(());
    }

    println!();
    println!("  ciritty web");
    println!();
    println!("    URL:    {url}");
    println!("    Token:  {token}");
    println!();
    println!("  Starting the server… open the URL and paste the token to log in.");
    println!("  The token is saved to your config; rerun with `ciritty web`.");
    if !loopback {
        println!();
        println!("  NOTE: {bind} is reachable beyond loopback — the gateway speaks");
        println!("        plain http/ws, so terminate TLS upstream (reverse proxy or");
        println!("        tunnel) before exposing it to an untrusted network.");
    }
    println!();

    // Spawn the server (non-blocking) so the browser is opened only once
    // it's actually listening — otherwise `--open` races a port that isn't
    // up yet and lands on a connection-refused page. Inherited stdio
    // streams the server's logs and lets Ctrl-C stop both. The token lives
    // in the persisted config, so it is deliberately NOT passed on the
    // command line (which would leak it into the process table).
    let server = server_exe_path();
    let mut cmd = Command::new(&server);
    cmd.arg("--headless")
        .arg("--web")
        .arg("--web-port")
        .arg(port.to_string())
        .arg("--web-bind")
        .arg(&bind);
    if let Some(dir) = &static_dir {
        cmd.arg("--web-static-dir").arg(dir);
    }
    // The server reads the token from the persisted config, never the
    // environment. Scrub CIRITTY_WEB_TOKEN from the child so a token supplied
    // that way doesn't outlive this command in the long-running server's
    // /proc/<pid>/environ or a core dump.
    cmd.env_remove("CIRITTY_WEB_TOKEN");
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning {}", server.display()))?;

    if open {
        if wait_for_listener(&bind, port) {
            if let Err(e) = open_in_browser(&url) {
                log::warn!("could not open browser: {e}");
            }
        } else {
            log::warn!("gateway didn't come up in time — not opening the browser");
        }
    }

    // Block in the foreground until the server exits.
    let status = child
        .wait()
        .with_context(|| format!("waiting on {}", server.display()))?;
    if !status.success() {
        bail!("ciritty-server exited with {status}");
    }
    Ok(())
}

/// Try to bind the chosen web address, then release it — a fast check that
/// the port is actually free before we persist `enabled = true` and spawn
/// the server. Binds the same address the daemon will: a wildcard
/// (`0.0.0.0`/`::`) or IP literal as given (an unparseable host, already
/// rejected by validation, falls back to loopback).
fn preflight_bind(bind: &str, port: u16) -> std::io::Result<()> {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};

    let ip: IpAddr = bind.parse().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let listener = TcpListener::bind(SocketAddr::from((ip, port)))?;
    drop(listener);
    Ok(())
}

/// Poll the gateway's listen address until it accepts a connection, so
/// `--open` doesn't race a port that isn't up yet. Returns false if it
/// never came up within the budget (~5s). Probes the bind's own address
/// family — a wildcard (`0.0.0.0`/`::`) or unparseable host maps to the
/// matching loopback, an IPv6 literal like `::1` is probed over IPv6 — so
/// the readiness check doesn't connect to the wrong family and time out.
fn wait_for_listener(bind: &str, port: u16) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    let ip: IpAddr = match bind {
        "0.0.0.0" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        "::" => IpAddr::V6(Ipv6Addr::LOCALHOST),
        b => b.parse().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    };
    let addr = SocketAddr::from((ip, port));
    for _ in 0..50 {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Mint a 32-hex-char (128-bit) token from the OS CSPRNG.
fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("RNG failure generating token: {e}"))?;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).expect("nibble < 16"));
        s.push(char::from_digit((b & 0x0f) as u32, 16).expect("nibble < 16"));
    }
    Ok(s)
}

/// Locate the `ciritty-server` binary next to this executable, falling
/// back to a bare name on `PATH`. Mirrors `connection::spawn_server`.
fn server_exe_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let bin = if cfg!(windows) {
        "ciritty-server.exe"
    } else {
        "ciritty-server"
    };
    exe.parent()
        .map(|p| p.join(bin))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(bin))
}

/// Best-effort "open this URL in the default browser".
fn open_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // `start` is a cmd builtin; the empty "" is the window title arg
        // so a URL with spaces isn't mistaken for the title.
        Command::new("cmd").args(["/C", "start", "", url]).status()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).status()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).status()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_bind_rejects_an_occupied_port() {
        use std::net::TcpListener;
        // Hold an ephemeral port, then assert the preflight reports it as
        // unbindable — the check that stops `ciritty web` from persisting an
        // enabled config on a port a foreign process already owns.
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = occupied.local_addr().unwrap().port();
        assert!(
            preflight_bind("127.0.0.1", port).is_err(),
            "an in-use port must preflight as unbindable",
        );
        // Once released, the same port binds cleanly again.
        drop(occupied);
        assert!(
            preflight_bind("127.0.0.1", port).is_ok(),
            "a freed port must preflight-bind",
        );
    }

    #[test]
    fn preflight_bind_resolves_wildcard_and_loopback() {
        // 0.0.0.0 / a loopback literal both parse and bind on an ephemeral
        // port (0 → OS-assigned), proving the address resolution path.
        assert!(preflight_bind("0.0.0.0", 0).is_ok());
        assert!(preflight_bind("127.0.0.1", 0).is_ok());
    }
}
