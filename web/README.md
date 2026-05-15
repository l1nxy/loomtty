# ciri-web — browser client for ciritty

Packages:

| package         | role                                                            |
| --------------- | --------------------------------------------------------------- |
| `@ciri/codec`   | ciri-protocol frame encode/decode, shared with the Rust client. |
| `@ciri/client`  | WebSocket transport + typed message dispatch.                   |
| `@ciri/dom`     | Pane grid model + DOM renderer (rows, SGR runs, cursor, …).     |
| `@ciri/app`     | Orchestrator: layout, keyboard, mouse, IME, clipboard, resize.  |
| `@ciri/bridge`  | Dev-time WebSocket ↔ TCP bridge to ciri-server.                 |
| `@ciri/demo`    | Dev-time HTML + vite page that mounts `CiriApp`.                |

## Running the demo

Three pieces have to be up at the same time:

1. **`ciri-server`** — your normal ciritty backend, configured to expose its TCP listener.

   Add to your config (`~/.config/ciritty/config.toml` on Linux/macOS, `%APPDATA%\ciritty\config.toml` on Windows):

   ```toml
   [remote]
   enabled = true
   port = 7890     # default
   ```

   Start `ciri-server` as you normally would. The log line `ciritty-server TCP listener on 127.0.0.1:7890 (remote enabled)` confirms it's listening.

2. **`@ciri/bridge`** — WebSocket gateway. The browser can't speak raw TCP, so this script forwards `ws://localhost:8090` to `tcp://127.0.0.1:7890`. Pure byte-for-byte pipe; no protocol awareness.

3. **`@ciri/demo`** — vite dev server. Hosts `index.html` + `src/main.ts` at `http://localhost:5173`.

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
| `session` | `default`              | Session name to attach to.             |
| `token` | (none)                   | Auth token forwarded to the transport. |

Example: `http://localhost:5173/?session=work&ws=ws://192.168.1.10:8090`.

### Troubleshooting

- **Status stuck on "connecting"**: the bridge or ciri-server isn't up. Check the bridge log for `tcp connected` lines; if you see `tcp error: ECONNREFUSED`, ciri-server isn't listening on the configured TCP port.
- **`ciri-server` log says "remote disabled"**: the config change didn't take effect. Confirm the config path it loaded (usually printed near the top of its log) and re-check `[remote] enabled = true`.
- **Mismatched cell metrics on first paint**: the page re-measures once the web font finishes loading; if it doesn't settle, force a `Resize` via `window.__ciri.remeasureCells()` from the devtools console.
- **No bytes go anywhere**: open the devtools console — the demo exposes the live app as `window.__ciri`. `__ciri.paneCount` should be ≥1 after the first `FullPaneSync`.

### Security note

The bridge has **no authentication** and the demo page has **no transport encryption**. Both bind to `127.0.0.1` by default. Don't expose either to a network you don't trust. Production deployments should put `wss://` + auth in front of `ciri-server`'s TCP listener via a real reverse proxy — that's out of scope for this dev tooling.
