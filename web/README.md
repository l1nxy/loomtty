# loom-web — browser client for loomtty

Packages:

| package         | role                                                            |
| --------------- | --------------------------------------------------------------- |
| `@loom/codec`   | loom-protocol frame encode/decode, shared with the Rust client. |
| `@loom/client`  | WebSocket transport + typed message dispatch.                   |
| `@loom/dom`     | Pane grid model + DOM renderer (rows, SGR runs, cursor, …).     |
| `@loom/app`     | Orchestrator: layout, keyboard, mouse, IME, clipboard, resize.  |
| `@loom/web`     | **Production** SPA: same-origin connect + token login screen.    |
| `@loom/bridge`  | Dev-time WebSocket ↔ TCP bridge to loom-server.                 |
| `@loom/demo`    | Dev-time HTML + vite page that mounts `LoomApp`.                |

## Production: serve the web UI from loom-server

In production there is **no bridge and no separate web host**. `loom-server`
itself serves the built `@loom/web` SPA over plain HTTP and upgrades `/ws`
to the binary protocol — both on the one `[web]` port. The browser connects
same-origin and authenticates through a token login screen (the token lives
in `sessionStorage`, never in the URL).

**One-time setup — build the bundle into the server's static dir:**

```bash
cd web
npm install
node scripts/install-assets.mjs        # → <repo>/target/debug/web
# release build: node scripts/install-assets.mjs path/to/target/release/web
```

`loom-server` serves `web.static_dir` (empty → `<server-exe-dir>/web`). The
install script builds `@loom/web` and copies the bundle there.

**Run it:**

```bash
loomtty web                 # enables [web], mints+saves a token, prints the
                            # URL, runs the server in the foreground
```

`loomtty web` prints something like:

```
    URL:    http://127.0.0.1:7891/
    Token:  3f9c…(32 hex chars)
```

Open the URL, paste the token, and you're in. Flags: `--port`, `--bind`,
`--token`, `--static-dir`, `--open`. The token is saved to your config, so
the desktop app and later `loomtty web` runs share it.

Equivalent manual path (the server reads `[web]` from your config):

```toml
[web]
enabled = true
port = 7891
token = "…32+ hex chars…"   # openssl rand -hex 16
# static_dir = ""            # empty → <server-exe-dir>/web
```

```bash
loomtty-server --headless          # or just run the desktop app
```

**Exposing beyond loopback.** The gateway speaks plain `http://` + `ws://`.
For anything other than `127.0.0.1`, set `web.allowed_origins` to the exact
browser origin(s) and put TLS in front (caddy/nginx) or use a tunnel
(SSH / Tailscale). The static assets are public (only `/ws` is
token-gated), so keep `static_dir` to the web bundle alone — no secrets.

## Development (demo + bridge)

The rest of this file describes the **dev-only** demo, which talks to
`loom-server` through the `@loom/bridge` WebSocket↔TCP shim instead of the
production gateway. It has no auth and no TLS — never expose it.

Three pieces have to be up at the same time:

1. **`loom-server`** — your normal loomtty backend, configured to expose its TCP listener.

   Add to your config (`~/.config/loomtty/config.toml` on Linux/macOS, `%APPDATA%\loomtty\config.toml` on Windows):

   ```toml
   [remote]
   enabled = true
   port = 7899     # see port note below
   ```

   Start `loom-server` as you normally would. The log line `loomtty-server TCP listener on 127.0.0.1:7899 (remote enabled)` confirms it's listening.

   > **Port note.** `loom-server`'s schema-level default is **7890**, but on Windows that collides with Clash / Clash Verge / clash-meta, which claim 7890 for their mixed proxy and silently swallow any non-HTTP bytes (the connection establishes but the server never sees them, so the browser sits at "connecting…" forever). The dev bridge therefore defaults to **7899** — pick the same number in your `[remote] port` (or pass `--tcp-port` to `npm run dev:bridge`).

2. **`@loom/bridge`** — WebSocket gateway. The browser can't speak raw TCP, so this script forwards `ws://localhost:8090` to `tcp://127.0.0.1:7899`. Forwards bytes unmodified in both directions; in dev it also peeks at the first inbound message and logs the decoded `ClientHello` (session name + viewport + cell metrics) to make handshake mismatches easy to spot.

3. **`@loom/demo`** — vite dev server. Hosts `index.html` + `src/main.ts` at `http://localhost:5173`.

One command starts (2) and (3) together (after a prebuild of the libraries):

```bash
cd web
npm install
npm run dev
```

Then open <http://localhost:5173> in a browser. The status strip at the top of the page should flip from "connecting…" to "connected".

### URL parameters

The demo page accepts a few `?key=value` overrides:

| param   | default                  | purpose                                |
| ------- | ------------------------ | -------------------------------------- |
| `ws`    | `ws://localhost:8090`    | Override the bridge URL.               |
| `session` | *(auto-attach)*        | Session name to attach to. When omitted, the client sends the `__auto__` sentinel and the server attaches to the most-recently-used session (or creates a fresh one) — same as bare `loomtty`. The resolved name is then written back into the URL (`history.replaceState`) so a reload/share reattaches to that exact session. |
| `token` | (none)                   | Auth token forwarded to the transport. |

Example: `http://localhost:5173/?session=work&ws=ws://192.168.1.10:8090`.

### Shortcuts & features

- **Find in scrollback**: `Ctrl+Shift+F` (or `Cmd+F`) opens a find bar over the active pane. Type to highlight matches; `Enter` / `Shift+Enter` cycle next/previous; `Esc` closes and restores scroll. Search covers the viewport **and** scrollback, case-insensitively.
- **Copy / paste**: `Ctrl+Shift+C` / `Ctrl+Shift+V` (or `Cmd`). OSC 52 clipboard writes from TUIs (vim/tmux yank) are mirrored to the system clipboard automatically.
- **Inline images**: sixel / kitty / iTerm images are rendered (the server decodes them to RGBA; the browser paints them onto a positioned `<canvas>` that scrolls with the buffer).
- **Sessions / workspaces**: the session dropdown (top-left) switches between running sessions; the `⊞` action creates a new workspace, and the numbered tabs switch between them.
- **Touch / mobile**: tap a pane to focus it and raise the on-screen keyboard (a floating `⌨` button is the fallback if your browser doesn't); the layout shrinks above the keyboard via `visualViewport`. One-finger vertical drag scrolls scrollback; **long-press** opens the Copy/Paste/Find menu, and long-press-then-drag selects text (then Copy). On mouse-reporting TUIs the touch is forwarded to the app instead. *(Soft-keyboard raising is browser/OS-specific — verify on a real device.)*

### Troubleshooting

- **Status stuck on "connecting"**: the bridge or loom-server isn't up. Check the bridge log for `tcp connected` lines; if you see `tcp error: ECONNREFUSED`, loom-server isn't listening on the configured TCP port.
- **Status flips to "connected" but the pane stays blank, no echo, no output**: bridge is forwarding to *something*, but it isn't loom-server. On Windows this is almost always Clash on port 7890 — confirm with `netstat -ano | findstr 7890` and `Get-Process -Id <pid>`. Move `loom-server`'s `[remote] port` (and the bridge's `--tcp-port`) off any port a proxy owns.
- **`loom-server` log says "remote disabled"**: the config change didn't take effect. Confirm the config path it loaded (usually printed near the top of its log) and re-check `[remote] enabled = true`.
- **Mismatched cell metrics on first paint**: the page re-measures once the web font finishes loading; if it doesn't settle, force a `Resize` via `window.__loom.remeasureCells()` from the devtools console.
- **No bytes go anywhere**: open the devtools console — the demo exposes the live app as `window.__loom`. `__loom.paneCount` should be ≥1 after the first `FullPaneSync`.

### Security note

The bridge has **no authentication** and the demo page has **no transport encryption**. Both bind to `127.0.0.1` by default. Don't expose either to a network you don't trust. Production deployments should put `wss://` + auth in front of `loom-server`'s TCP listener via a real reverse proxy — that's out of scope for this dev tooling.
