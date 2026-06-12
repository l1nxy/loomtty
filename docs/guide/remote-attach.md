# Remote Attach

Because loomtty is split into a **client** and a **server**, the client doesn't
have to run on the same machine as your shells. You can attach to a session
living on another host.

## How it works

The client and server talk over loomtty's **binary protocol** — the same
protocol the [browser client](/guide/web-ui) uses. For remote sessions, that
protocol is carried over an **SSH tunnel**, so traffic rides your existing SSH
trust and encryption: you connect to a `loomtty-server` on a remote box, and
your panes render locally on the GPU as if they were on your machine.

This is the same detach/attach model as a local session — your shells live in
the remote server and survive client disconnects — but across the network.

## Remote configuration

On the **remote** host, enable the listener under `[remote]`. You can also
pre-register hosts so they show up in the session palette (`Alt Shift+p`):

```toml
[remote]
enabled = true
port = 7890          # loomtty listener port

[[remote.hosts]]
name = "devbox"
host = "dev.example.com"
port = 7890          # remote loomtty port
ssh_port = 22        # SSH port for the tunnel
```

See the [`[remote]` reference](/reference/configuration#remote) for every field.

## Predictive echo

Over a high-latency link, waiting a full round-trip to see each keystroke is
painful. loomtty borrows **mosh's predictive echo**: it echoes your typing
locally and reconciles with the server as updates arrive, so input feels
responsive even when the connection is slow.

It's off by default — enable it under `[prediction]`:

```toml
[prediction]
mode = "adaptive"    # "never" (default) | "always" | "adaptive"
threshold_ms = 30    # adaptive: latency above which prediction kicks in
show_underline = true # underline not-yet-confirmed characters
```

- **`adaptive`** predicts only when measured latency exceeds `threshold_ms` —
  the recommended setting for mixed local/remote use.
- **`always`** predicts unconditionally; **`never`** disables it.

See the [`[prediction]` reference](/reference/configuration#prediction).

::: warning Work in progress
loomtty is early (v0.1) and remote attach is among the rougher edges. Expect the
setup flow and options to change.
:::
