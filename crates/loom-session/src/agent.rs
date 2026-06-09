//! AI Agent detection and resume command registry.
//!
//! To add a new agent, add one entry to the [`AGENTS`] array.

use serde::{Deserialize, Serialize};

/// Known AI agent types that can be auto-resumed.
///
/// The `Unknown` variant ensures forward compatibility: if a newer version
/// saves a session with a new agent kind, older versions can still load the
/// session without losing the entire tile. Unknown agents are simply not resumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    OpenCode,
    Droid,
    /// Fallback for agent kinds added in newer versions.
    #[serde(other)]
    Unknown,
}

/// Saved agent information for session persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedAgent {
    pub kind: AgentKind,
}

impl AgentKind {
    /// Kebab-case identifier matching the serde representation.
    /// Stable across releases — used as the wire token for the
    /// `PaneAgentChanged` protocol message and exposed to Lua plugins
    /// via `format-usage` ctx.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
            AgentKind::OpenCode => "open-code",
            AgentKind::Droid => "droid",
            AgentKind::Unknown => "unknown",
        }
    }
}

impl SavedAgent {
    /// Returns true if this agent should be serialized.
    /// Unknown agents (from newer versions) are dropped on re-save to avoid
    /// overwriting the original kind string (e.g. `"cursor"` → `"unknown"`).
    pub fn should_serialize(&self) -> bool {
        self.kind != AgentKind::Unknown
    }
}

/// Internal descriptor for each supported agent.
struct AgentDescriptor {
    kind: AgentKind,
    /// Executable name to match (without path or `.exe` suffix).
    exe_name: &'static str,
    /// Returns `true` if argv indicates a non-interactive invocation
    /// that should NOT be resumed.
    is_non_interactive: fn(&[String]) -> bool,
    /// Shell command to resume this agent's most recent session.
    resume_command: &'static str,
}

/// Agent registry. Add new agents here — one entry each.
const AGENTS: &[AgentDescriptor] = &[
    AgentDescriptor {
        kind: AgentKind::ClaudeCode,
        exe_name: "claude",
        is_non_interactive: |argv| {
            argv.iter().any(|a| {
                a == "--print"
                    || a == "--chrome-native-host"
                    || a == "--pipe"
                    || a == "-p"
                    || a == "mcp"
            })
        },
        resume_command: "claude --continue",
    },
    AgentDescriptor {
        kind: AgentKind::Codex,
        exe_name: "codex",
        is_non_interactive: |argv| argv.get(1).is_some_and(|a| a == "exec"),
        resume_command: "codex resume --last",
    },
    AgentDescriptor {
        kind: AgentKind::OpenCode,
        exe_name: "opencode",
        is_non_interactive: |argv| argv.get(1).is_some_and(|a| a == "run"),
        resume_command: "opencode --continue",
    },
    AgentDescriptor {
        kind: AgentKind::Droid,
        exe_name: "droid",
        is_non_interactive: |argv| argv.get(1).is_some_and(|a| a == "exec"),
        resume_command: "droid",
    },
];

/// Detect an agent from the process executable name and argument vector.
///
/// `exe_name` can be a full path or just a basename — path components and
/// `.exe` suffix are stripped before matching.
///
/// Returns `None` if no known interactive agent matches.
pub fn detect_agent(exe_name: &str, argv: &[String]) -> Option<SavedAgent> {
    let name = normalize_exe_name(exe_name);

    for desc in AGENTS {
        if name == desc.exe_name && !(desc.is_non_interactive)(argv) {
            return Some(SavedAgent { kind: desc.kind });
        }
    }
    None
}

/// Get the resume command for a given agent kind.
/// Returns `None` if the kind is not in the registry (e.g. added in a newer version).
pub fn resume_command(kind: AgentKind) -> Option<&'static str> {
    AGENTS
        .iter()
        .find(|d| d.kind == kind)
        .map(|d| d.resume_command)
}

/// Strip path components and `.exe` suffix from an executable name.
fn normalize_exe_name(exe: &str) -> &str {
    // Same logic as loom_procinfo::exe_basename, duplicated here to avoid
    // a dependency from loom-session → loom-procinfo.
    let name = exe.rsplit('/').next().unwrap_or(exe);
    let name = name.rsplit('\\').next().unwrap_or(name);
    name.strip_suffix(".exe").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    // ── Detection ────────────────────────────────────────────────────

    #[test]
    fn detect_claude_interactive() {
        let a = detect_agent("claude", &args(&["claude"]));
        assert_eq!(a.unwrap().kind, AgentKind::ClaudeCode);
    }

    #[test]
    fn detect_claude_with_flags() {
        // Interactive with extra flags
        let a = detect_agent(
            "claude",
            &args(&["claude", "--dangerously-skip-permissions"]),
        );
        assert_eq!(a.unwrap().kind, AgentKind::ClaudeCode);
    }

    #[test]
    fn reject_claude_print_mode() {
        assert!(detect_agent("claude", &args(&["claude", "--print", "hello"])).is_none());
    }

    #[test]
    fn reject_claude_native_host() {
        assert!(detect_agent("claude", &args(&["claude", "--chrome-native-host"])).is_none());
    }

    #[test]
    fn reject_claude_pipe() {
        assert!(detect_agent("claude", &args(&["claude", "--pipe"])).is_none());
    }

    #[test]
    fn reject_claude_mcp() {
        assert!(detect_agent("claude", &args(&["claude", "mcp"])).is_none());
    }

    #[test]
    fn detect_claude_full_path() {
        let a = detect_agent("/opt/claude-code/bin/claude", &args(&["claude"]));
        assert_eq!(a.unwrap().kind, AgentKind::ClaudeCode);
    }

    #[test]
    fn detect_claude_windows_exe() {
        let a = detect_agent("claude.exe", &args(&["claude"]));
        assert_eq!(a.unwrap().kind, AgentKind::ClaudeCode);
    }

    #[test]
    fn detect_claude_windows_full_path() {
        let a = detect_agent(
            r"C:\Users\me\AppData\Local\Programs\claude.exe",
            &args(&["claude"]),
        );
        assert_eq!(a.unwrap().kind, AgentKind::ClaudeCode);
    }

    // ── Codex ────────────────────────────────────────────────────────

    #[test]
    fn detect_codex_interactive() {
        let a = detect_agent("codex", &args(&["codex"]));
        assert_eq!(a.unwrap().kind, AgentKind::Codex);
    }

    #[test]
    fn reject_codex_exec() {
        assert!(detect_agent("codex", &args(&["codex", "exec", "fix bug"])).is_none());
    }

    // ── OpenCode ─────────────────────────────────────────────────────

    #[test]
    fn detect_opencode_interactive() {
        let a = detect_agent("opencode", &args(&["opencode"]));
        assert_eq!(a.unwrap().kind, AgentKind::OpenCode);
    }

    #[test]
    fn reject_opencode_run() {
        assert!(detect_agent("opencode", &args(&["opencode", "run", "prompt"])).is_none());
    }

    // ── Droid ────────────────────────────────────────────────────────

    #[test]
    fn detect_droid_interactive() {
        let a = detect_agent("droid", &args(&["droid"]));
        assert_eq!(a.unwrap().kind, AgentKind::Droid);
    }

    #[test]
    fn reject_droid_exec() {
        assert!(detect_agent("droid", &args(&["droid", "exec", "query"])).is_none());
    }

    // ── Unknown ──────────────────────────────────────────────────────

    #[test]
    fn unknown_program_returns_none() {
        assert!(detect_agent("vim", &args(&["vim", "file.rs"])).is_none());
        assert!(detect_agent("htop", &args(&["htop"])).is_none());
        assert!(detect_agent("", &args(&[])).is_none());
    }

    // ── Resume commands ──────────────────────────────────────────────

    #[test]
    fn resume_commands() {
        assert_eq!(
            resume_command(AgentKind::ClaudeCode),
            Some("claude --continue")
        );
        assert_eq!(
            resume_command(AgentKind::Codex),
            Some("codex resume --last")
        );
        assert_eq!(
            resume_command(AgentKind::OpenCode),
            Some("opencode --continue")
        );
        assert_eq!(resume_command(AgentKind::Droid), Some("droid"));
        assert_eq!(resume_command(AgentKind::Unknown), None);
    }

    // ── Serde ────────────────────────────────────────────────────────

    #[test]
    fn agent_kind_serde_roundtrip() {
        let agent = SavedAgent {
            kind: AgentKind::ClaudeCode,
        };
        let json = serde_json::to_string(&agent).unwrap();
        assert!(json.contains("claude-code"));

        let back: SavedAgent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, AgentKind::ClaudeCode);
    }

    #[test]
    fn all_kinds_serde() {
        for kind in [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::OpenCode,
            AgentKind::Droid,
        ] {
            let json = serde_json::to_string(&SavedAgent { kind }).unwrap();
            let back: SavedAgent = serde_json::from_str(&json).unwrap();
            assert_eq!(back.kind, kind);
        }
    }

    #[test]
    fn unknown_agent_kind_forward_compat() {
        // Simulate a session saved by a newer version with an unknown agent kind
        let json = r#"{"kind":"cursor"}"#;
        let agent: SavedAgent = serde_json::from_str(json).unwrap();
        assert_eq!(agent.kind, AgentKind::Unknown);
        assert_eq!(resume_command(AgentKind::Unknown), None);
        assert!(!agent.should_serialize());
    }

    #[test]
    fn unknown_agent_not_resaved_in_tile() {
        // Unknown agents should be dropped on re-save to preserve the original kind
        use crate::state::SavedTile;
        let tile = SavedTile {
            pane_id: 1,
            weight: 1.0,
            cwd: None,
            title: None,
            agent: Some(SavedAgent {
                kind: AgentKind::Unknown,
            }),
        };
        let json = serde_json::to_string(&tile).unwrap();
        // "agent" field should be omitted for Unknown kind
        assert!(
            !json.contains("agent"),
            "Unknown agent should not be serialized: {json}"
        );
    }

    #[test]
    fn known_agent_is_resaved_in_tile() {
        use crate::state::SavedTile;
        let tile = SavedTile {
            pane_id: 1,
            weight: 1.0,
            cwd: None,
            title: None,
            agent: Some(SavedAgent {
                kind: AgentKind::ClaudeCode,
            }),
        };
        let json = serde_json::to_string(&tile).unwrap();
        assert!(
            json.contains("claude-code"),
            "Known agent should be serialized: {json}"
        );
    }

    #[test]
    fn saved_tile_without_agent_field() {
        // Backward compat: old session files have no "agent" field
        let json = r#"{"pane_id":1,"weight":1.0}"#;
        let tile: crate::state::SavedTile = serde_json::from_str(json).unwrap();
        assert!(tile.agent.is_none());
        assert!(tile.cwd.is_none());
    }
}
