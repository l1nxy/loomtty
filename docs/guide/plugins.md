# Plugins <Badge type="warning" text="planned" />

::: danger Not yet enabled
The Lua plugin engine exists in the codebase (`loom-plugin`) with a sandboxed
VM, an event API, and tests — but it is **not yet wired into the running
server**. Nothing below works in the current build yet. This page documents the
**planned** design so it's clear where loomtty is headed; treat it as a preview,
not a contract.
:::

loomtty's plugin system is a sandboxed **Lua 5.4** engine, owned by the server.
Plugins register **event handlers** and customize behavior — agent detection,
tab titles, the status bar — without recompiling.

## Where plugins live (planned)

Loaded in this order, last-registered handler winning:

1. **Built-in plugins** (embedded) — e.g. agent detection.
2. **User plugins** — each subdirectory of `~/.config/loom/plugins/` with an
   `init.lua` is loaded: `~/.config/loom/plugins/<name>/init.lua`.
3. **User init** — `~/.config/loom/init.lua`.

## The `loom` API (planned)

| Function | Description |
| -------- | ----------- |
| `loom.on(event, fn)` | Register a handler for an event. |
| `loom.log(msg)` | Write an info line to the server log. |
| `loom.warn(msg)` | Write a warning to the server log. |

```lua
-- ~/.config/loom/init.lua
loom.on("format-tab-title", function(info)
  return info.cwd .. " > " .. info.title
end)
```

## Events (planned)

| Event | Argument | Return | Purpose |
| ----- | -------- | ------ | ------- |
| `detect-agent` | `(exe_name, argv)` | `{ name, resume_command }` or nil | Recognize an AI agent in a pane so it can be restored on reattach. |
| `format-tab-title` | `{ pane_id, title, cwd, is_active }` | string or nil | Customize a pane's tab label. |
| `format-status-bar` | `{ session_name, pane_count, mode }` | string or nil | Customize the status bar content. |

A handler returning `nil` defers to the next handler (so a user handler can
selectively override a built-in). For value-returning events, the
last-registered handler that returns a non-nil value wins.

```lua
-- Recognize a custom agent CLI for session restore
loom.on("detect-agent", function(exe_name, argv)
  if exe_name == "myagent" then
    return { name = "myagent", resume_command = "myagent --resume" }
  end
end)
```

## Sandbox (planned)

The VM runs handlers under guardrails so a buggy plugin can't take the server
down:

- **Instruction limit** — a runaway handler (e.g. an infinite loop) is killed.
- **Failure backoff** — a handler that errors on three consecutive calls is
  disabled for the rest of the session.
- **Single-threaded** — the engine is server-owned and not shared across threads.

## Today: agent detection without Lua

Agent-aware session restore already works in the current build — it's
implemented natively (not via Lua). loomtty recognizes Claude Code, Codex,
opencode, and Droid and restores their panes on reattach. Toggle it with
[`session.restore_agents`](/reference/configuration#session). The plugin engine
above will make that list user-extensible once it's wired in.
