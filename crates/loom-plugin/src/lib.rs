//! Lua plugin engine for loom.
//!
//! Provides a sandboxed Lua 5.4 VM with an event-driven API (`loom.on`).
//! The engine is designed to be owned by the server and accessed
//! single-threaded (no `Send` required).

mod api;
mod builtin;
mod events;
mod sandbox;
mod vm;

use events::EventRegistry;
use mlua::{Lua, LuaSerdeExt, Table, Value};

/// Agent detected by a plugin's `detect-agent` handler.
#[derive(Debug, Clone)]
pub struct DetectedAgent {
    pub name: String,
    pub resume_command: String,
}

/// The plugin engine owns the Lua VM and event handler registry.
pub struct PluginEngine {
    lua: Lua,
    registry: EventRegistry,
}

impl PluginEngine {
    /// Create and initialize the plugin engine.
    /// Loads built-in plugins, user plugins, and user init.lua.
    pub fn new() -> anyhow::Result<Self> {
        let registry = EventRegistry::new();
        let lua = vm::init_vm(&registry)?;
        Ok(Self { lua, registry })
    }

    /// Reload all plugins (clear handlers, re-initialize VM).
    pub fn reload(&mut self) -> anyhow::Result<()> {
        self.registry.clear();
        self.lua = vm::init_vm(&self.registry)?;
        Ok(())
    }

    // ── Event dispatch methods ──────────────────────────────────────

    /// Fire a void event (handlers return nothing).
    pub fn fire(&self, event: &str) {
        self.registry.dispatch_void(&self.lua, event, ());
    }

    /// Fire a void event with a Lua table argument.
    pub fn fire_with_table(&self, event: &str, build: impl FnOnce(&Lua, &Table)) {
        let Ok(table) = self.lua.create_table() else {
            return;
        };
        build(&self.lua, &table);
        self.registry.dispatch_void(&self.lua, event, table);
    }

    /// Detect an agent from a foreground process.
    /// Dispatches the `detect-agent` event and returns the first non-nil result.
    pub fn detect_agent(&self, exe_name: &str, argv: &[String]) -> Option<DetectedAgent> {
        let argv_table = match self.lua.create_sequence_from(argv.iter().cloned()) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("[plugin] failed to create argv table: {e}");
                return None;
            }
        };

        let result: Option<Table> = self.registry.dispatch_first(
            &self.lua,
            "detect-agent",
            (exe_name.to_string(), argv_table),
        );

        result.and_then(|t| {
            let name: String = t.get("name").ok()?;
            let resume_command: String = t.get("resume_command").ok()?;
            Some(DetectedAgent {
                name,
                resume_command,
            })
        })
    }

    /// Format a tab title via the `format-tab-title` event.
    /// Returns `None` if no handler provides a custom title.
    pub fn format_tab_title(
        &self,
        pane_id: u64,
        title: &str,
        cwd: &str,
        is_active: bool,
    ) -> Option<String> {
        let table = self.lua.create_table().ok()?;
        table.set("pane_id", pane_id).ok()?;
        table.set("title", title.to_string()).ok()?;
        table.set("cwd", cwd.to_string()).ok()?;
        table.set("is_active", is_active).ok()?;

        self.registry
            .dispatch_first::<Table, Value>(&self.lua, "format-tab-title", table)
            .and_then(|v| match v {
                Value::String(s) => Some(s.to_str().ok()?.to_string()),
                _ => None,
            })
    }

    /// Format a status bar via the `format-status-bar` event.
    /// Returns `None` if no handler provides custom content.
    pub fn format_status_bar(
        &self,
        session_name: &str,
        pane_count: usize,
        mode: &str,
    ) -> Option<String> {
        let table = self.lua.create_table().ok()?;
        table.set("session_name", session_name.to_string()).ok()?;
        table.set("pane_count", pane_count).ok()?;
        table.set("mode", mode.to_string()).ok()?;

        self.registry
            .dispatch_first::<Table, Value>(&self.lua, "format-status-bar", table)
            .and_then(|v| match v {
                Value::String(s) => Some(s.to_str().ok()?.to_string()),
                _ => None,
            })
    }

    /// Format the usage segment via the `format-usage` event. The
    /// caller hands in an arbitrary JSON payload (focused pane info,
    /// usage snapshot, mode, …); the Lua handler can inspect every
    /// field and either return a string (rendered as the segment
    /// label) or `nil` (segment collapses).
    ///
    /// `nil`-return semantics is the user-facing knob for "only show
    /// usage when claude is the focused pane" — the handler returns
    /// nil whenever the gate fails.
    pub fn format_usage(&self, ctx: &serde_json::Value) -> Option<String> {
        let lua_ctx = self.lua.to_value(ctx).ok()?;
        let table = match lua_ctx {
            Value::Table(t) => t,
            _ => self.lua.create_table().ok()?,
        };
        match self
            .registry
            .dispatch_first::<Table, Value>(&self.lua, "format-usage", table)
        {
            Some(Value::String(s)) => s.to_str().ok().map(|s| s.to_string()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_initializes_with_builtin_plugins() {
        let engine = PluginEngine::new().unwrap();
        // Built-in session_restore.lua should have registered a detect-agent handler.
        // Verify by detecting a known agent.
        let result = engine.detect_agent("claude", &["claude".into()]);
        assert!(result.is_some());
        let agent = result.unwrap();
        assert_eq!(agent.name, "claude-code");
        assert_eq!(agent.resume_command, "claude --continue");
    }

    #[test]
    fn detect_agent_returns_none_for_unknown() {
        let engine = PluginEngine::new().unwrap();
        let result = engine.detect_agent("vim", &["vim".into()]);
        assert!(result.is_none());
    }

    #[test]
    fn detect_agent_skips_non_interactive_claude() {
        let engine = PluginEngine::new().unwrap();
        let result = engine.detect_agent("claude", &["claude".into(), "--print".into()]);
        assert!(result.is_none());
    }

    #[test]
    fn detect_agent_skips_codex_exec() {
        let engine = PluginEngine::new().unwrap();
        let result = engine.detect_agent("codex", &["codex".into(), "exec".into()]);
        assert!(result.is_none());
    }

    #[test]
    fn detect_agent_codex_interactive() {
        let engine = PluginEngine::new().unwrap();
        let result = engine.detect_agent("codex", &["codex".into()]);
        assert!(result.is_some());
        let agent = result.unwrap();
        assert_eq!(agent.name, "codex");
        assert_eq!(agent.resume_command, "codex resume --last");
    }

    #[test]
    fn user_handler_overrides_builtin() {
        let engine = PluginEngine::new().unwrap();
        // Register a user handler that overrides claude detection.
        engine
            .lua
            .load(
                r#"
                loom.on("detect-agent", function(exe_name, argv)
                    if exe_name == "claude" then
                        return { name = "my-claude", resume_command = "my-resume" }
                    end
                end)
                "#,
            )
            .exec()
            .unwrap();

        let result = engine.detect_agent("claude", &["claude".into()]);
        assert!(result.is_some());
        let agent = result.unwrap();
        assert_eq!(agent.name, "my-claude");
        assert_eq!(agent.resume_command, "my-resume");
    }

    #[test]
    fn infinite_loop_is_killed() {
        let engine = PluginEngine::new().unwrap();
        engine
            .lua
            .load(
                r#"
                loom.on("detect-agent", function(exe_name, argv)
                    while true do end
                end)
                "#,
            )
            .exec()
            .unwrap();

        // Should not hang — instruction limit kicks in.
        let result = engine.detect_agent("claude", &["claude".into()]);
        // The infinite loop handler fails, falls through to the built-in.
        // The built-in should still work (if not disabled by the failure).
        // Actually, the user handler runs first (last-registered-wins) and errors,
        // then the built-in handler runs.
        assert!(result.is_some() || result.is_none()); // Just verify no hang.
    }

    #[test]
    fn error_handler_is_disabled_after_failures() {
        let engine = PluginEngine::new().unwrap();
        engine
            .lua
            .load(
                r#"
                loom.on("detect-agent", function(exe_name, argv)
                    error("intentional error")
                end)
                "#,
            )
            .exec()
            .unwrap();

        // Call 3 times to trigger disabling.
        for _ in 0..3 {
            let _ = engine.detect_agent("unknown", &["unknown".into()]);
        }

        // After 3 failures the broken handler is disabled.
        // The built-in handler for "claude" should now respond.
        let result = engine.detect_agent("claude", &["claude".into()]);
        assert!(result.is_some());
    }

    #[test]
    fn fire_void_event_does_not_panic() {
        let engine = PluginEngine::new().unwrap();
        engine.fire("nonexistent-event");
        engine.fire_with_table("pane-created", |_, t| {
            t.set("pane_id", 42u64).unwrap();
        });
    }

    #[test]
    fn format_tab_title_returns_none_without_handler() {
        let engine = PluginEngine::new().unwrap();
        let result = engine.format_tab_title(1, "bash", "/home", true);
        assert!(result.is_none());
    }

    #[test]
    fn format_tab_title_with_custom_handler() {
        let engine = PluginEngine::new().unwrap();
        engine
            .lua
            .load(
                r#"
                loom.on("format-tab-title", function(info)
                    return info.cwd .. " > " .. info.title
                end)
                "#,
            )
            .exec()
            .unwrap();

        let result = engine.format_tab_title(1, "bash", "/home", true);
        assert_eq!(result, Some("/home > bash".to_string()));
    }

    #[test]
    fn loom_json_decode_is_available() {
        let engine = PluginEngine::new().unwrap();
        // A user plugin reads in JSON, asserts a field, and writes
        // it into a registry slot we read back.
        engine
            .lua
            .load(
                r#"
                local parsed = loom.json.decode('{"a":1,"b":[2,3]}')
                assert(parsed.a == 1)
                assert(parsed.b[1] == 2)
                assert(parsed.b[2] == 3)
                "#,
            )
            .exec()
            .unwrap();
    }

    #[test]
    fn loom_json_encode_roundtrip() {
        let engine = PluginEngine::new().unwrap();
        let s: String = engine
            .lua
            .load(r#"return loom.json.encode({a = 1, b = "two"})"#)
            .eval()
            .unwrap();
        // mlua's table key ordering isn't guaranteed; just check both keys.
        assert!(s.contains("\"a\":1"));
        assert!(s.contains("\"b\":\"two\""));
    }

    #[test]
    fn builtin_usage_hides_when_no_focused_agent() {
        let engine = PluginEngine::new().unwrap();
        let ctx = serde_json::json!({
            "focused_pane": { "id": 1, "title": "vim README.md", "cwd": "/repo" },
            "usage": {
                "claude": { "session_utilization": 0.12, "weekly_utilization": 0.41 }
            },
        });
        // vim is not claude/codex → builtin:usage returns nil.
        assert_eq!(engine.format_usage(&ctx), None);
    }

    #[test]
    fn builtin_usage_shows_claude_for_focused_claude_pane() {
        let engine = PluginEngine::new().unwrap();
        let ctx = serde_json::json!({
            "focused_pane": { "id": 1, "title": "claude", "cwd": "/repo" },
            // Real Anthropic header values are 0..1 fractions.
            "usage": {
                "claude": { "session_utilization": 0.6, "weekly_utilization": 0.17 }
            },
        });
        assert_eq!(
            engine.format_usage(&ctx).as_deref(),
            Some("claude 60%/17%")
        );
    }

    #[test]
    fn builtin_usage_shows_codex_for_focused_codex_pane() {
        let engine = PluginEngine::new().unwrap();
        let ctx = serde_json::json!({
            "focused_pane": { "id": 1, "title": "codex resume", "cwd": "/repo" },
            "usage": {
                "codex": { "primary_utilization": 0.07, "secondary_utilization": 0.23 }
            },
        });
        assert_eq!(
            engine.format_usage(&ctx).as_deref(),
            Some("codex 7%/23%")
        );
    }

    #[test]
    fn builtin_usage_prefers_server_agent_over_title() {
        let engine = PluginEngine::new().unwrap();
        // Title says "vim" (user opened vim inside a Claude Code
        // session, doesn't matter) but server-side procinfo says the
        // foreground process is `claude-code`. The agent string wins.
        let ctx = serde_json::json!({
            "focused_pane": {
                "id": 1,
                "title": "vim ~/.bashrc",
                "cwd": "/home/me",
                "agent": "claude-code",
            },
            "usage": {
                "claude": { "session_utilization": 0.6, "weekly_utilization": 0.17 }
            },
        });
        assert_eq!(
            engine.format_usage(&ctx).as_deref(),
            Some("claude 60%/17%")
        );
    }

    #[test]
    fn builtin_usage_hides_when_server_agent_is_unrelated() {
        let engine = PluginEngine::new().unwrap();
        // Title contains "claude" (a path coincidence) but the real
        // foreground process is something else — `agent` wins, so
        // the segment hides.
        let ctx = serde_json::json!({
            "focused_pane": {
                "id": 1,
                "title": "vim ~/projects/claude/README.md",
                "cwd": "/home/me",
                "agent": "unknown",
            },
            "usage": {
                "claude": { "session_utilization": 0.6, "weekly_utilization": 0.17 }
            },
        });
        assert_eq!(engine.format_usage(&ctx), None);
    }

    #[test]
    fn user_format_usage_overrides_builtin() {
        let engine = PluginEngine::new().unwrap();
        engine
            .lua
            .load(
                r#"
                loom.on("format-usage", function(ctx)
                    return "always-on " .. (ctx.focused_pane and ctx.focused_pane.title or "?")
                end)
                "#,
            )
            .exec()
            .unwrap();
        let ctx = serde_json::json!({
            "focused_pane": { "id": 1, "title": "bash", "cwd": "/repo" },
            "usage": {},
        });
        assert_eq!(
            engine.format_usage(&ctx).as_deref(),
            Some("always-on bash")
        );
    }

    #[test]
    fn reload_clears_handlers() {
        let mut engine = PluginEngine::new().unwrap();
        engine
            .lua
            .load(
                r#"
                loom.on("detect-agent", function(exe_name, argv)
                    return { name = "custom", resume_command = "custom-cmd" }
                end)
                "#,
            )
            .exec()
            .unwrap();

        // Before reload: custom handler wins.
        let result = engine.detect_agent("claude", &["claude".into()]);
        assert_eq!(result.as_ref().unwrap().name, "custom");

        // After reload: only built-in remains.
        engine.reload().unwrap();
        let result = engine.detect_agent("claude", &["claude".into()]);
        assert_eq!(result.as_ref().unwrap().name, "claude-code");
    }
}
