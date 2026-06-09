//! Background tokio task that refreshes the shared usage snapshot.
//!
//! Polls Claude and Codex independently. Each provider has its own retry
//! backoff so a 429 from one doesn't starve the other. Refresh interval is
//! deliberately conservative — Claude's `/v1/messages` ping costs ~1 token
//! per call but rate-limits if you hammer it, and Codex's `wham/usage`
//! similarly throttles.

use std::time::Duration;

use chrono::Utc;
use tokio::time::sleep;

use super::snapshot::{ProviderState, SharedSnapshot};
use super::{claude, codex, tokens};

/// Default cadence — chosen to stay well under both providers' soft rate
/// limits while keeping the bar within ~one minute of fresh.
pub const DEFAULT_REFRESH: Duration = Duration::from_secs(60);
/// Backoff after a transport / parse failure. Doubles up to 10 minutes.
const ERROR_BACKOFF_INITIAL: Duration = Duration::from_secs(60);
const ERROR_BACKOFF_MAX: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct PollerConfig {
    pub claude_enabled: bool,
    pub codex_enabled: bool,
    pub refresh: Duration,
}

impl Default for PollerConfig {
    fn default() -> Self {
        Self {
            claude_enabled: true,
            codex_enabled: true,
            refresh: DEFAULT_REFRESH,
        }
    }
}

/// Spawn the usage poller on a dedicated OS thread that owns its own
/// current-thread tokio runtime. Returns immediately; the poller runs
/// for the lifetime of the process. The thread is detached — no join
/// handle is kept since the poller is purely additive (read-only HTTP
/// + a shared mutex write) and abandoning it on shutdown is fine.
///
/// This pattern (rather than relying on a globally accessible runtime)
/// matches `crates/loom/src/connection.rs`, where the IO thread builds
/// its own runtime — keeps the usage probes independent of the
/// connection lifecycle so they keep working after disconnect.
pub fn spawn(config: PollerConfig, snapshot: SharedSnapshot) {
    let _ = std::thread::Builder::new()
        .name("loom-usage-poller".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("[usage] could not build tokio runtime: {e}");
                    return;
                }
            };
            rt.block_on(async move {
                let mut handles = Vec::new();
                if config.claude_enabled {
                    handles.push(tokio::spawn(provider_loop(
                        "claude",
                        config.refresh,
                        snapshot.clone(),
                        poll_claude,
                    )));
                }
                if config.codex_enabled {
                    handles.push(tokio::spawn(provider_loop(
                        "codex",
                        config.refresh,
                        snapshot.clone(),
                        poll_codex,
                    )));
                }
                // Park the runtime forever — provider_loop never returns.
                for h in handles {
                    let _ = h.await;
                }
            });
        });
}

async fn provider_loop<F, Fut>(
    name: &'static str,
    refresh: Duration,
    snapshot: SharedSnapshot,
    poll: F,
) where
    F: Fn(reqwest::Client, SharedSnapshot) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("loom-usage-probe")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::error!("[usage:{name}] failed to build HTTP client: {e}");
            return;
        }
    };

    let mut backoff = ERROR_BACKOFF_INITIAL;
    loop {
        match poll(http.clone(), snapshot.clone()).await {
            Ok(()) => {
                backoff = ERROR_BACKOFF_INITIAL;
                sleep(refresh).await;
            }
            Err(e) => {
                log::warn!("[usage:{name}] poll failed: {e:#}");
                sleep(backoff).await;
                backoff = (backoff * 2).min(ERROR_BACKOFF_MAX);
            }
        }
    }
}

async fn poll_claude(http: reqwest::Client, snapshot: SharedSnapshot) -> anyhow::Result<()> {
    let tokens = match tokens::load_claude_tokens() {
        Ok(t) => t,
        Err(e) => {
            record_error(&snapshot, ProviderKind::Claude, format!("{e:#}"));
            return Err(e);
        }
    };
    match claude::fetch(&http, &tokens).await {
        Ok(usage) => {
            snapshot.with_mut(|s| {
                s.claude = ProviderState {
                    data: Some(usage),
                    last_ok_unix: Some(Utc::now().timestamp()),
                    last_error: None,
                };
                s.last_poll_at = Some(std::time::Instant::now());
            });
            Ok(())
        }
        Err(e) => {
            record_error(&snapshot, ProviderKind::Claude, format!("{e:#}"));
            Err(e)
        }
    }
}

async fn poll_codex(http: reqwest::Client, snapshot: SharedSnapshot) -> anyhow::Result<()> {
    let tokens = match tokens::load_codex_tokens() {
        Ok(t) => t,
        Err(e) => {
            record_error(&snapshot, ProviderKind::Codex, format!("{e:#}"));
            return Err(e);
        }
    };
    match codex::fetch(&http, &tokens).await {
        Ok(usage) => {
            snapshot.with_mut(|s| {
                s.codex = ProviderState {
                    data: Some(usage),
                    last_ok_unix: Some(Utc::now().timestamp()),
                    last_error: None,
                };
                s.last_poll_at = Some(std::time::Instant::now());
            });
            Ok(())
        }
        Err(e) => {
            record_error(&snapshot, ProviderKind::Codex, format!("{e:#}"));
            Err(e)
        }
    }
}

#[derive(Copy, Clone)]
enum ProviderKind {
    Claude,
    Codex,
}

fn record_error(snapshot: &SharedSnapshot, kind: ProviderKind, msg: String) {
    snapshot.with_mut(|s| match kind {
        ProviderKind::Claude => s.claude.last_error = Some(msg),
        ProviderKind::Codex => s.codex.last_error = Some(msg),
    });
}
