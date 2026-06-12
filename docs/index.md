---
layout: home
title: GPU-accelerated terminal multiplexer

hero:
  name: loomtty
  text: A terminal multiplexer, GPU-accelerated.
  tagline: Column-based layouts and workspaces, a built-in browser client, and remote attach over a binary protocol — written in Rust.
  image:
    src: /logo.svg
    alt: loomtty
  actions:
    - theme: brand
      text: Get Started
      link: /guide/
    - theme: alt
      text: Install
      link: /guide/installation
    - theme: alt
      text: View on GitHub
      link: https://github.com/l1nxy/loomtty

features:
  - icon: 🧱
    title: Column + workspace layout
    details: tmux/zellij-style multiplexing, organized in columns and workspaces.
  - icon: ⚡
    title: GPU rendering
    details: Vulkan / Metal / OpenGL / DirectX 11, with ligatures and inline images (Kitty/Sixel).
  - icon: 🌐
    title: Remote attach
    details: Connect to a session on another host over a binary protocol and SSH tunnel.
  - icon: 🖥️
    title: Browser client
    details: <code>loomtty web</code> serves the same session as an installable PWA.
  - icon: 🔮
    title: Predictive echo
    details: mosh-style local echo keeps typing responsive over high-latency links.
  - icon: 🤖
    title: Agent-aware sessions
    details: Auto-restores agent panes (Claude Code, Codex, opencode, Droid) on reattach.
  - icon: 🧩
    title: Lua plugins
    details: Sandboxed scripting with an event API.
  - icon: 🎨
    title: Themeable
    details: loom_dark, catppuccin, tokyo_night, dracula, nord, gruvbox, ghostty, and more.
---

<div class="vp-doc" style="max-width: 960px; margin: 4rem auto 0; padding: 0 24px;">

> 🚧 **Early and experimental (v0.1).** Expect rough edges — not everything is
> guaranteed to work yet. Primarily developed on Linux and Windows; **macOS is
> untested, treat it as work in progress.**

## Try it in 30 seconds

```bash
# Build from source (Rust 1.82+)
git clone https://github.com/l1nxy/loomtty.git && cd loomtty
cargo build --release

# Launch — attaches to the last session, or creates one
./target/release/loomtty
```

Then run `loomtty init` for an interactive config wizard, or head to the
[Quick Start](/guide/quick-start) to learn the workflow.

</div>
