# What is loomtty?

**loomtty** is a GPU-accelerated terminal multiplexer written in Rust. It gives
you tmux/zellij-style multiplexing — many shells inside one window — but
organized in **scrollable columns and workspaces**, rendered on the GPU, with a
client-server architecture you can attach to and detach from at will.

It also goes a few places traditional multiplexers don't: a built-in
**browser client**, **remote attach** to a session on another host over a binary
protocol, **predictive echo** for high-latency links, and **agent-aware
sessions** that restore your AI coding panes on reattach.

::: warning Early and experimental (v0.1)
Expect rough edges — not everything is guaranteed to work yet. loomtty is
primarily developed on **Linux and Windows**. **macOS is untested** and should
be treated as work in progress.
:::

## Highlights

- **Column + workspace layout** — multiplexing organized in columns, niri-style.
- **GPU rendering** — Vulkan / Metal / OpenGL / DirectX 11, with ligatures and
  inline images (Kitty / Sixel graphics).
- **Remote attach** — connect to a session on another host over a binary
  protocol plus an SSH tunnel.
- **Browser client** — `loomtty web` serves the same session as an installable
  PWA.
- **Predictive echo** — mosh-style local echo over high-latency links.
- **Agent-aware sessions** — auto-restores agent panes (Claude Code, Codex,
  opencode, Droid) on reattach.
- **Lua plugins** — sandboxed scripting with an event API.

## Architecture

loomtty uses a **client-server** split, like tmux and zellij:

- **`loomtty`** — the client. Owns the window, GPU rendering, input, and the
  layout you see.
- **`loomtty-server`** — the daemon. Owns the PTYs and terminal state, so your
  sessions survive when the client detaches (or crashes).

The two talk over a binary protocol — the same protocol the browser client and
remote attach ride on. Under the hood it's a 13-crate Rust workspace; terminal
emulation builds on `alacritty_terminal` with extensions for the Kitty keyboard
protocol, Sixel/Kitty graphics, and OSC 133 shell integration. Rendering goes
through `blade-graphics` (Vulkan / Metal / OpenGL / DirectX 11).

## Platform support

| Platform | Status | Backend |
| -------- | ------ | ------- |
| Linux (Wayland / X11) | Supported | Vulkan / OpenGL |
| Windows | Supported | DirectX 11 |
| macOS | **Untested (WIP)** | Metal |

## Next steps

<div class="vp-doc" style="display:flex; gap:1rem; flex-wrap:wrap;">

- [**Installation →**](/guide/installation) — build from source or grab a release
- [**Quick Start →**](/guide/quick-start) — sessions, columns, and the basics
- [**Layout & Workspaces →**](/guide/layout) — the column / workspace / tile model
- [**Keybindings →**](/guide/keybindings) — the leader key and the default map
- [**Configuration →**](/guide/configuration) — every `config.toml` option
- [**Theming →**](/guide/theming) — presets and color overrides

</div>
