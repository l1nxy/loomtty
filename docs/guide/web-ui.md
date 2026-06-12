# Web UI

`loomtty web` turns the multiplexer into a **browser terminal**: the same session
you'd attach to from the native client, served as an HTTP single-page app and
installable as a **PWA**.

![The browser client attached to a live session, showing its Claude Code pane](/demo/web-claude.png)

## Run it

```bash
loomtty web
```

This enables the `[web]` server, mints and saves an access token, prints the URL,
and runs the server in the foreground. Open the URL, log in with the token, and
you're in your session.

## Demo mode (no token)

For a local try-out you can skip authentication entirely:

```toml
[web]
enabled = true
auth = "none"   # demo mode — loopback only
```

The gateway then accepts unauthenticated connections and the browser UI skips
the login screen — open the URL and you're attached. Demo mode is **only
honored on a loopback `bind`**; the daemon refuses to start an unauthenticated
gateway on a reachable interface. Switch back by setting `auth = "token"` (the
default) with a real `token`.

## How it works

In production there is **no separate web host and no dev bridge**.
`loomtty-server` itself serves the built SPA over plain HTTP and upgrades `/ws`
to the binary protocol — both on the one `[web]` port. The browser connects
**same-origin** and authenticates through a token login screen (the token lives
in `sessionStorage`, never in the URL).

## Security

::: danger Put TLS in front before exposing it
The gateway speaks plain **`http` / `ws`**. That's fine on loopback, but **do not
expose it past `localhost` without TLS.** Run it behind a reverse proxy
(Caddy / nginx) or an SSH tunnel that terminates TLS.
:::

## Building the bundle

The web UI is a small npm/Vite/TypeScript workspace under
[`web/`](https://github.com/l1nxy/loomtty/tree/main/web). For a source build you
install dependencies and copy the bundle into the directory the server serves
from:

```bash
cd web
npm install
node scripts/install-assets.mjs        # → <repo>/target/debug/web
# release build:
# node scripts/install-assets.mjs path/to/target/release/web
```

`loomtty-server` serves `web.static_dir` (empty → `<server-exe-dir>/web`). See
[`web/README.md`](https://github.com/l1nxy/loomtty/blob/main/web/README.md) for
the full architecture, the package layout, and the dev workflow.
