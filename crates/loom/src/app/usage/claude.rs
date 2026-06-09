//! Anthropic OAuth usage probe.
//!
//! `/api/oauth/usage` rate-limits aggressively (claude-code#31637), so we
//! follow the claude-monitor approach: send a 1-token Haiku ping to
//! `POST /v1/messages` and read `anthropic-ratelimit-unified-*` headers from
//! the response. Both 200 and 429 carry the headers, so we don't have to
//! actually succeed at inference — the request itself is the probe.

use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};

use super::snapshot::{ClaudeUsage, UsageWindow};
use super::tokens::ClaudeTokens;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
/// Mirrors what claude-monitor sends. `User-Agent` impersonates the Claude
/// Code CLI so the OAuth token isn't rejected as an unknown client.
const CLAUDE_CODE_USER_AGENT: &str = "claude-cli/2.1.0 (external, cli)";
const PING_BODY: &str = r#"{"model":"claude-haiku-4-5-20251001","max_tokens":1,"messages":[{"role":"user","content":"x"}]}"#;

pub async fn fetch(
    http: &reqwest::Client,
    tokens: &ClaudeTokens,
) -> Result<ClaudeUsage> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", tokens.access_token))
            .context("invalid Claude access token bytes")?,
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    headers.insert(
        "anthropic-beta",
        HeaderValue::from_static("oauth-2025-04-20"),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(CLAUDE_CODE_USER_AGENT));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    let resp = http
        .post(ENDPOINT)
        .headers(headers)
        .body(PING_BODY)
        .send()
        .await
        .context("Claude messages ping failed")?;

    let status = resp.status();
    // 401 means token is bad — surface so the poller can stop spamming retries.
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow!(
            "Claude OAuth token rejected (401) — try `claude /login` to refresh"
        ));
    }
    // 200 OK and 429 both carry the rate-limit headers; anything else means
    // the probe didn't get far enough to give us a usable signal.
    if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Claude messages ping returned {status}: {body}"
        ));
    }

    // Diagnostic: dump every anthropic-* response header at debug
    // level so users staring at "claude 0%/0%" can tell whether the
    // values are genuinely zero or whether Anthropic stopped emitting
    // these headers entirely. Cheap (just a string format).
    if log::log_enabled!(log::Level::Debug) {
        let mut anth: Vec<String> = resp
            .headers()
            .iter()
            .filter(|(k, _)| k.as_str().starts_with("anthropic-"))
            .map(|(k, v)| {
                format!(
                    "{}: {}",
                    k.as_str(),
                    v.to_str().unwrap_or("<non-utf8>"),
                )
            })
            .collect();
        anth.sort();
        log::debug!(
            "[usage:claude] status={status} anthropic-headers=[{}]",
            anth.join("; ")
        );
    }

    // Anthropic emits utilization headers as a 0..1 *fraction*
    // (e.g. `0.6` = 60%). We pass it through as-is — the Lua
    // `format-usage` plugin owns presentation, including unit
    // conversion. Codex's wham/usage returns 0..100 percent, so
    // codex.rs normalizes that side too — every consumer (Rust /
    // Lua) sees fractions consistently.
    let header_frac = |k: &str| -> Option<f64> {
        read_header(resp.headers(), k).and_then(|v| v.parse().ok())
    };
    let header_epoch =
        |k: &str| -> Option<i64> { read_header(resp.headers(), k).and_then(|v| v.parse().ok()) };
    let header_str = |k: &str| read_header(resp.headers(), k).map(str::to_owned);

    Ok(ClaudeUsage {
        session: UsageWindow {
            utilization: header_frac("anthropic-ratelimit-unified-5h-utilization"),
            resets_at_unix: header_epoch("anthropic-ratelimit-unified-5h-reset"),
            status: header_str("anthropic-ratelimit-unified-5h-status"),
        },
        weekly: UsageWindow {
            utilization: header_frac("anthropic-ratelimit-unified-7d-utilization"),
            resets_at_unix: header_epoch("anthropic-ratelimit-unified-7d-reset"),
            status: header_str("anthropic-ratelimit-unified-7d-status"),
        },
        organization_id: header_str("anthropic-organization-id"),
    })
}

fn read_header<'h>(headers: &'h HeaderMap, name: &str) -> Option<&'h str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}
