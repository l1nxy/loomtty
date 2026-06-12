# Remote & Predictive Echo

Because loomtty is split into a **client** and a **server**, the client doesn't
have to run on the same machine as your shells. You run `loomtty-server` on a
remote host, and attach to it from your laptop — the panes render locally on the
GPU, while the shells live on the remote and survive disconnects.

## How it works

The client talks to the remote server over loomtty's **binary protocol, carried
inside an SSH tunnel**. Concretely, the client runs:

```
ssh -p <ssh_port> -W localhost:<remote_port> -- <user@host>
```

`ssh -W` forwards the protocol stream to the remote's **loopback** address, where
`loomtty-server` is listening. That has two consequences worth understanding:

- The remote TCP listener binds to **`127.0.0.1` only** and has **no
  authentication of its own** — it is reachable solely through the SSH tunnel (or
  from localhost on the remote). It is never exposed to the network directly.
- **Authentication and encryption are just SSH.** loomtty adds nothing on top; it
  reuses your SSH keys, `known_hosts`, and config.

## Step 1 — Enable the listener on the remote host

On the **remote** machine, install loomtty and turn on the remote listener in
`~/.config/loom/config.toml`:

```toml
[remote]
enabled = true
port = 7890        # optional; 7890 is the default
```

Then start the server by creating a session and detaching from it, so the server
keeps running with the listener open:

```bash
loomtty work       # start a session named "work" on the remote
# press Alt q to detach — the session (and the server) stay alive
```

A detached session keeps `loomtty-server` running, so the listener stays up and
ready for you to attach from elsewhere. (With no sessions and no clients, the
server shuts down after [`server.idle_timeout_secs`](/reference/configuration#server),
5 minutes by default.)

::: tip
`[remote] enabled` belongs on the **remote host** (the machine running the
server). You do **not** set it on your laptop just to connect out.
:::

## Step 2 — Connect from your local machine

### Option A — one-off, from the CLI

```bash
loomtty remote user@remote-host work
```

Arguments and flags:

| Argument | Default | Meaning |
| -------- | ------- | ------- |
| `user@remote-host` | — | SSH target (required). |
| `[session]` | — | Session to attach to / create on the remote. |
| `--port <n>` | `7890` | The remote's `[remote] port`. |
| `--ssh-port <n>` | `22` | SSH port for the tunnel. |

### Option B — saved hosts + the session palette

On your **local** machine, list the remotes you connect to under
`[[remote.hosts]]` — this is purely client-side, to populate the palette:

```toml
[[remote.hosts]]
name = "devbox"
host = "user@dev.example.com"
port = 7890        # the remote's [remote] port
ssh_port = 22
```

Then open the **session palette** with `Alt Shift+p`. loomtty probes each saved
host for its running sessions and lets you pick one to attach to (you can also
type a host into the connect prompt). See the
[`[remote]` reference](/reference/configuration#remote).

## Requirements & gotchas

- **`ssh` must be on your PATH.** loomtty shells out to the system OpenSSH client.
- **Use key-based auth.** The client drives `ssh` non-interactively (its stdin is
  the protocol stream), so password prompts can't be answered. Set up an SSH key
  or `ssh-agent`. A good check: `ssh user@host` should connect without typing a
  password.
- **Accept the host key first.** If the host is new, run a plain `ssh user@host`
  once so its key lands in `known_hosts`; otherwise the tunnel fails host-key
  verification.
- **Don't collide ports.** If the remote also runs the [web gateway](/guide/web-ui),
  `[remote] port` and `[web] port` must differ (defaults are `7890` / `7891`); the
  server refuses to start otherwise.

## Predictive echo

Over a high-latency link, waiting a full round-trip to see each keystroke is
painful. loomtty borrows **mosh's predictive echo**: it echoes your typing
locally and reconciles with the server as updates arrive, so input stays
responsive even when the connection is slow.

It's off by default — enable it (on the **client**) under `[prediction]`:

```toml
[prediction]
mode = "adaptive"     # "never" (default) | "always" | "adaptive"
threshold_ms = 30     # adaptive: latency above which prediction kicks in
show_underline = true # underline not-yet-confirmed characters
```

- **`adaptive`** predicts only when measured latency exceeds `threshold_ms` — the
  recommended setting for mixed local/remote use.
- **`always`** predicts unconditionally; **`never`** disables it.

See the [`[prediction]` reference](/reference/configuration#prediction).

## Troubleshooting

When a connection drops, loomtty classifies the SSH failure and shows the reason:

| Symptom | Likely cause |
| ------- | ------------ |
| `ssh binary not found` | OpenSSH isn't on your PATH. |
| `Permission denied` | SSH auth failed — set up key-based auth (password prompts can't be answered). |
| `Connection refused` | No `loomtty-server` listening on the remote, or `[remote] enabled` is false / the wrong `port`. |
| `Host key verification failed` | The remote's host key is unknown or changed — `ssh user@host` once to resolve. |
| `Could not resolve hostname` | DNS / hostname typo. |
| `connection timed out` | Network/firewall, or the wrong `--ssh-port`. |

::: warning Work in progress
loomtty is early (v0.1) and remote attach is among the rougher edges. Expect the
setup flow and options to change.
:::
