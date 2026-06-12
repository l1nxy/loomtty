# System Tray

When `loomtty-server` runs with a desktop session available, it shows a **system
tray icon** for managing sessions without a terminal open. This is the server's
default mode — the client auto-spawns the server, so in a graphical session the
tray appears on its own.

## The tray menu

| Item | Action |
| ---- | ------ |
| **Running sessions** | Each running session is listed with its pane count. An `- attached` suffix means a client is already connected. Clicking switches an attached client to it, or opens a new window for an unattached session. |
| **Saved Sessions** | A submenu of saved (inactive) sessions you can reopen. |
| **New Session** | Create a fresh session. |
| **Start on Login** | Toggle [autostart](#autostart) (a checkbox). |
| **Quit** | Shut down the `loomtty-server` daemon. |

The menu refreshes automatically as sessions come and go.

## Headless / server mode

On a machine with no display — a remote host, a CI box, an SSH-only server —
start the daemon **without** the tray:

```bash
loomtty-server --headless     # no tray; run in the foreground
loomtty-server --daemonize    # Unix: fork into the background (also no tray)
```

This matters for [remote attach](/guide/remote-attach): on a headless remote, run
`loomtty-server --headless` (or `--daemonize`) so it doesn't try to initialize a
tray (which requires a desktop — GTK on Linux).

## Autostart

Toggling **Start on Login** registers (or removes) an OS autostart entry so the
tray/server launches when you log in:

| Platform | Entry |
| -------- | ----- |
| Linux | `~/.config/autostart/loomtty-tray.desktop` |
| macOS | `~/Library/LaunchAgents/dev.loomtty.tray.plist` |
| Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\LoomttyTray` |

The checkbox reflects whether that entry currently exists; toggling it writes or
deletes the entry.

::: tip
Quitting from the tray shuts the **server** down. Detached sessions are persisted
and come back the next time the server starts (see
[`server.idle_timeout_secs`](/reference/configuration#server) and
[`session`](/reference/configuration#session)).
:::
