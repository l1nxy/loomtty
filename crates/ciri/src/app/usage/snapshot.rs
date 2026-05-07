//! Shared usage snapshot polled in the background and read by the status bar.

use std::sync::{Arc, Mutex};
use std::time::Instant;

/// One rolling rate-limit window (e.g. Claude 5h / 7d, Codex primary / secondary).
#[derive(Debug, Clone, Default)]
pub struct UsageWindow {
    /// Utilization as a 0..=1 fraction (so `0.6` = 60% used). `None`
    /// if the provider didn't report it. Both Anthropic and Codex
    /// fetchers normalize to fractions at the boundary so plugins
    /// see one consistent unit. Display-side scaling
    /// (`* 100` for `%d%%`) lives in the Lua `format-usage` handler.
    pub utilization: Option<f64>,
    /// Unix epoch seconds when this window resets. `None` if unknown.
    pub resets_at_unix: Option<i64>,
    /// Provider-specific status string, e.g. `"allow"` / `"exceeded"` for Anthropic.
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ClaudeUsage {
    pub session: UsageWindow,
    pub weekly: UsageWindow,
    /// `anthropic-organization-id` header — opaque opaque id for the active org.
    pub organization_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CodexUsage {
    pub primary: UsageWindow,
    pub secondary: UsageWindow,
    /// Plan tier reported by `wham/usage` (`free`, `plus`, `pro`, …).
    pub plan: Option<String>,
    /// Account-level credits balance, `None` if not reported.
    pub balance: Option<String>,
    pub has_credits: bool,
    pub unlimited: bool,
}

/// One provider's last successful state plus optional error from the most recent poll.
#[derive(Debug, Clone, Default)]
pub struct ProviderState<T: Default + Clone> {
    pub data: Option<T>,
    pub last_ok_unix: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageSnapshot {
    pub claude: ProviderState<ClaudeUsage>,
    pub codex: ProviderState<CodexUsage>,
    /// Wall-clock instant of the most recent poll completion — used by the
    /// status bar to mark stale data.
    pub last_poll_at: Option<Instant>,
}

/// Cheaply clonable handle to the snapshot. UI thread reads via `read()`,
/// poller writes via `with_mut`.
#[derive(Debug, Clone, Default)]
pub struct SharedSnapshot(Arc<Mutex<UsageSnapshot>>);

impl SharedSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&self) -> UsageSnapshot {
        self.0
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub(super) fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut UsageSnapshot) -> R,
    {
        let mut guard = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut guard)
    }
}
