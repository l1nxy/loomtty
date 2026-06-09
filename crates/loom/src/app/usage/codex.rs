//! Codex / ChatGPT-backed usage probe.
//!
//! `GET https://chatgpt.com/backend-api/wham/usage` returns the same payload
//! Codex's TUI uses for `/status`. The shape here mirrors the upstream
//! `RateLimitStatusPayload` from `openai/codex` (codex-backend-openapi-models).

use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;

use super::snapshot::{CodexUsage, UsageWindow};
use super::tokens::CodexTokenSet;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_USER_AGENT: &str = "codex-cli";

#[derive(Debug, Deserialize)]
struct Payload {
    plan_type: Option<String>,
    rate_limit: Option<RateLimit>,
    credits: Option<Credits>,
}

#[derive(Debug, Deserialize)]
struct RateLimit {
    primary_window: Option<Window>,
    secondary_window: Option<Window>,
}

#[derive(Debug, Deserialize)]
struct Window {
    used_percent: Option<f64>,
    reset_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Credits {
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    balance: Option<String>,
}

pub async fn fetch(
    http: &reqwest::Client,
    tokens: &CodexTokenSet,
) -> Result<CodexUsage> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", tokens.access_token))
            .context("invalid Codex access token bytes")?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(CODEX_USER_AGENT));
    if let Some(acc) = tokens.chatgpt_account_id.as_deref() {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(b"ChatGPT-Account-Id"),
            HeaderValue::from_str(acc),
        ) {
            headers.insert(name, value);
        }
    }

    let resp = http
        .get(ENDPOINT)
        .headers(headers)
        .send()
        .await
        .context("Codex wham/usage request failed")?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow!(
            "Codex OAuth token rejected (401) — try `codex login` to refresh"
        ));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Codex wham/usage returned {status}: {body}"));
    }

    let payload: Payload = resp
        .json()
        .await
        .context("decode Codex wham/usage response")?;

    let (primary, secondary) = match payload.rate_limit {
        Some(r) => (
            r.primary_window.map(window_to_usage).unwrap_or_default(),
            r.secondary_window.map(window_to_usage).unwrap_or_default(),
        ),
        None => (UsageWindow::default(), UsageWindow::default()),
    };

    let credits = payload.credits.unwrap_or(Credits {
        has_credits: None,
        unlimited: None,
        balance: None,
    });

    Ok(CodexUsage {
        primary,
        secondary,
        plan: payload.plan_type,
        balance: credits.balance,
        has_credits: credits.has_credits.unwrap_or(false),
        unlimited: credits.unlimited.unwrap_or(false),
    })
}

fn window_to_usage(w: Window) -> UsageWindow {
    // Codex's `wham/usage` reports `used_percent` on a 0..100 scale
    // (i32 in their OpenAPI schema). We keep `UsageWindow.utilization`
    // as a 0..1 fraction across providers so the Lua `format-usage`
    // handler doesn't have to special-case which provider produced
    // the value — divide by 100 here once at the boundary.
    UsageWindow {
        utilization: w.used_percent.map(|p| p / 100.0),
        resets_at_unix: w.reset_at,
        status: None,
    }
}
