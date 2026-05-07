//! Locate OAuth tokens for Claude Code and Codex CLI.
//!
//! Claude Code stores OAuth credentials under
//! - macOS: Keychain entry `service = "Claude Code-credentials"`,
//! - Linux/Windows: `~/.claude/.credentials.json` (mode 0600 on Unix).
//!
//! Codex CLI writes `~/.codex/auth.json` on every platform.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde::Deserialize;

/// What Claude Code's `~/.claude/.credentials.json` (and Keychain blob) looks like.
#[derive(Debug, Deserialize)]
struct ClaudeCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOauth>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
}

/// Just the bits of `~/.codex/auth.json` we need: an access token plus the
/// `chatgpt_account_id` claim from the id_token JWT (used as the
/// `ChatGPT-Account-Id` header).
#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    tokens: Option<CodexTokens>,
}

#[derive(Debug, Deserialize)]
struct CodexTokens {
    access_token: String,
    id_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeTokens {
    pub access_token: String,
}

#[derive(Debug, Clone)]
pub struct CodexTokenSet {
    pub access_token: String,
    pub chatgpt_account_id: Option<String>,
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn load_claude_tokens() -> Result<ClaudeTokens> {
    if cfg!(target_os = "macos") {
        match read_claude_keychain() {
            Ok(json) => return parse_claude_json(&json),
            Err(keychain_err) => {
                // Fall back to the file path — some users export the keychain
                // entry to `~/.claude/.credentials.json` so SSH/sudo sessions
                // that can't reach the Keychain still work.
                if let Ok(tokens) = load_claude_tokens_from_file() {
                    return Ok(tokens);
                }
                return Err(keychain_err.context("Claude Code Keychain entry not found"));
            }
        }
    }
    load_claude_tokens_from_file()
}

fn load_claude_tokens_from_file() -> Result<ClaudeTokens> {
    let path = home()
        .ok_or_else(|| anyhow!("no home directory"))?
        .join(".claude")
        .join(".credentials.json");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    parse_claude_json(&raw)
}

fn parse_claude_json(raw: &str) -> Result<ClaudeTokens> {
    let parsed: ClaudeCredentialsFile = serde_json::from_str(raw)
        .context("parse Claude Code credentials JSON")?;
    let oauth = parsed
        .claude_ai_oauth
        .ok_or_else(|| anyhow!("no claudeAiOauth section in Claude credentials"))?;
    Ok(ClaudeTokens {
        access_token: oauth.access_token,
    })
}

#[cfg(target_os = "macos")]
fn read_claude_keychain() -> Result<String> {
    use security_framework::passwords::get_generic_password;
    // Claude Code stores the JSON blob under service = "Claude Code-credentials"
    // with the macOS user as the account.
    let user = std::env::var("USER").unwrap_or_default();
    let raw = get_generic_password("Claude Code-credentials", &user)
        .context("read Claude Code Keychain entry")?;
    String::from_utf8(raw).context("Keychain entry was not valid UTF-8")
}

#[cfg(not(target_os = "macos"))]
fn read_claude_keychain() -> Result<String> {
    Err(anyhow!("Keychain only supported on macOS"))
}

pub fn load_codex_tokens() -> Result<CodexTokenSet> {
    let path = home()
        .ok_or_else(|| anyhow!("no home directory"))?
        .join(".codex")
        .join("auth.json");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    let parsed: CodexAuthFile =
        serde_json::from_str(&raw).context("parse ~/.codex/auth.json")?;
    let tokens = parsed
        .tokens
        .ok_or_else(|| anyhow!("no tokens section in ~/.codex/auth.json (api-key mode?)"))?;
    let chatgpt_account_id = tokens
        .id_token
        .as_deref()
        .and_then(extract_chatgpt_account_id);
    Ok(CodexTokenSet {
        access_token: tokens.access_token,
        chatgpt_account_id,
    })
}

/// Decode the `https://api.openai.com/auth.chatgpt_account_id` claim out of
/// Codex's id_token JWT. Returns `None` for any decode failure — the upstream
/// fetcher will simply skip the optional `ChatGPT-Account-Id` header.
fn extract_chatgpt_account_id(jwt: &str) -> Option<String> {
    let payload_b64 = jwt.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    v.get("https://api.openai.com/auth")?
        .get("chatgpt_account_id")?
        .as_str()
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_credentials_blob() {
        let raw = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-foo","refreshToken":"r"}}"#;
        let tokens = parse_claude_json(raw).unwrap();
        assert_eq!(tokens.access_token, "sk-ant-oat01-foo");
    }

    #[test]
    fn rejects_claude_credentials_without_oauth_section() {
        let raw = r#"{"other":{}}"#;
        assert!(parse_claude_json(raw).is_err());
    }

    #[test]
    fn extract_account_id_handles_missing_claim_gracefully() {
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"sub":"u1"}"#);
        let jwt = format!("h.{payload}.s");
        assert_eq!(extract_chatgpt_account_id(&jwt), None);
    }

    #[test]
    fn extract_account_id_decodes_claim() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acc-42"}}"#,
        );
        let jwt = format!("h.{payload}.s");
        assert_eq!(
            extract_chatgpt_account_id(&jwt).as_deref(),
            Some("acc-42")
        );
    }
}
