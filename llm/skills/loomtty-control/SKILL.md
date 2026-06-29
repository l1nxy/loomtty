---
name: loomtty-control
description: >-
  Drive and inspect a running loomtty terminal from the command line: list
  sessions, send keystrokes, run commands, capture pane output, and read
  shell-prompt (OSC 133) history via `loomtty msg`. Use when an agent needs to
  control or observe a loomtty session programmatically — e.g. typing into a
  pane, launching a process, reading what a command printed, or checking whether
  a command finished and with what exit code.
---

# Controlling loomtty from the CLI

loomtty is a GPU terminal multiplexer with a **server–client** architecture: a
background daemon owns the sessions/panes/PTYs; GUI windows and the `loomtty
msg` IPC commands are just clients of that daemon. Because the daemon holds the
real terminal state, an agent can drive and observe a session from any shell
without attaching a window.

Everything below assumes the daemon is running (it auto-starts when a GUI
session is open). If no server is running, commands print `server is not
running` to stderr and exit 1 (except `list`, which falls back to saved
sessions on disk).

## Orientation: find the session and pane you want

```sh
loomtty list                       # or `ls`. Columns: NAME STATUS PANES CLIENTS
loomtty msg info <session>         # session summary (add --json for a struct)
loomtty msg list-panes <session>   # the pane table — get pane IDs here
loomtty msg list-panes <session> --json
```

`list-panes` is the entry point for almost everything else: it gives you each
pane's **ID** (the `<pane_id>` argument the other commands need), size
(`COLS×ROWS`), title, which one is active (`*`), and its grid position
(`WS`/`COL`/`TILE` = workspace, column, tile indices).

## The `msg` commands

All scripting/automation runs through `loomtty msg <subcommand>`. The global
`--json` flag switches most subcommands from a human table to machine-readable
JSON.

| Command | Args | Output |
| --- | --- | --- |
| `send-keys` | `<session> <pane_id> <keys>` | `CommandResult` (text `ok (pane N)` / `--json`) |
| `run-command` | `<session> <command> [--cwd DIR]` | `CommandResult` with the **new** pane's id |
| `capture-pane` | `<session> <pane_id> [flags]` | raw pane text on stdout (or `--json`) |
| `list-panes` | `<session>` | pane table / `--json` array |
| `info` | `<session>` | session summary / `--json` |
| `get-layout` | `<session>` | **always JSON** (`{session_name, layout}`) |
| `list-prompts` | `<session> <pane_id>` | OSC 133 prompt table / `--json` |
| `focus-pane` | `<session> <pane_id>` | `CommandResult` |
| `close-pane` | `<session> <pane_id>` | `CommandResult` |
| `create-pane` | `<session>` | `CommandResult` with the new pane's id |

### send-keys — type raw bytes into a pane

`send-keys` writes the `<keys>` string **verbatim to the PTY**. There is **no
tmux-style key syntax** (`Enter`, `C-c`, …): you send the actual bytes.

```sh
# Type a command WITHOUT running it (no trailing newline):
loomtty msg send-keys work 3 'git status'

# Type AND submit — append a newline. In bash/zsh use $'...':
loomtty msg send-keys work 3 $'git status\n'

# Send Ctrl-C (0x03), Escape (0x1b), a bare Enter:
loomtty msg send-keys work 3 $'\x03'
loomtty msg send-keys work 3 $'\x1b'
loomtty msg send-keys work 3 $'\n'      # if a program ignores \n, try $'\r'
```

The keys land in whatever is foregrounded in that pane (shell, vim, a REPL…).
loomtty does not wait for the program to react — see *Knowing when a command
finished* below.

> Shell escaping is the caller's job. In PowerShell, `` `n `` / `` `e `` aren't
> interpreted inside single quotes — build the string with `"...\`n"` or pass a
> here-string; the simplest portable path is often `run-command` (below).

### run-command — launch a process in a fresh pane

```sh
loomtty msg run-command work 'npm run dev'
loomtty msg run-command work 'cargo build' --cwd /path/to/project
```

Spawns a **new pane** that runs `command` through the shell (`sh -c` on Unix,
`cmd /C` on Windows) and returns the new pane's id in the `CommandResult`
(`--json` → `{"success", "message", "pane_id"}`). When the command finishes the
shell exits and the pane closes. `--cwd <dir>` sets the working directory
(default: inherited by the new pane). Use this instead of `send-keys` when you
want a clean, dedicated pane for a process rather than typing into an existing
shell.

### capture-pane — read what's on screen / in scrollback

```sh
loomtty msg capture-pane work 3                       # visible grid only
loomtty msg capture-pane work 3 --scrollback-rows 500 # + up to 500 rows of history
loomtty msg capture-pane work 3 --join-wrapped        # unwrap soft-wrapped lines
loomtty msg capture-pane work 3 --json                # {session_name,pane_id,text,truncated}
```

This is how an agent **reads command output**. Notes:

- Trailing ASCII spaces are trimmed per row by default; `--preserve-trailing-spaces` keeps them.
- `--scrollback-rows` is an unsigned count *above* the viewport (NOT tmux's signed `-S`). It's clamped to the pane's real history, ~100k rows, and a ~900 KiB frame budget — a very wide pane gets fewer rows than asked.
- When an **alt-screen** TUI is foregrounded (vim/less/htop), the alt buffer is captured and `--scrollback-rows` has no effect (the primary buffer's history isn't reachable).
- If the response was clipped to fit the frame budget, a warning goes to **stderr** and (`--json`) `truncated` is `true`.

### list-prompts — shell-integration command history (OSC 133)

```sh
loomtty msg list-prompts work 3            # table: PROMPT OUTPUT END EXIT DURATION
loomtty msg list-prompts work 3 --json
```

Each entry is one shell command boundary: the absolute line of the prompt
(`133;A`), output start (`133;C`), done (`133;D`), plus the command's **exit
code** and **duration**. Empty output means the pane has never seen OSC 133 —
i.e. shell integration isn't active there (see `crates/loom-server/shell-integration/`).

### get-layout / focus-pane / close-pane / create-pane

```sh
loomtty msg get-layout work          # full workspace→column→tile→pane tree (JSON)
loomtty msg focus-pane work 3        # make pane 3 the active one
loomtty msg create-pane work         # new empty pane, returns its id
loomtty msg close-pane work 3
```

## Common agent workflows

**Run a command in an existing shell and read its output:**

```sh
loomtty msg send-keys work 3 $'ls -la\n'
# give it a moment, then:
loomtty msg capture-pane work 3 --scrollback-rows 200
```

**Run a command and know when it finished + its exit code** (requires shell
integration in that pane):

```sh
loomtty msg send-keys work 3 $'make build\n'
# poll until a new prompt boundary appears with a done line + exit code:
loomtty msg list-prompts work 3 --json
# the newest entry's exit_code tells you pass/fail; duration_ms how long it took.
```

**Spin up a dedicated process pane and watch it:**

```sh
pid=$(loomtty msg run-command work 'npm run dev' --json | jq .pane_id)
loomtty msg capture-pane work "$pid" --scrollback-rows 300
```

## Top-level session commands (for reference)

| Command | Alias | Purpose |
| --- | --- | --- |
| `loomtty [<session>]` | | Attach to (or create) a session in a GUI window |
| `loomtty new` | | Create a new session and connect |
| `loomtty attach <name>` | `a` | Attach to an existing session (errors if missing) |
| `loomtty list [--all]` | `ls` | List sessions (`--all` includes saved/inactive) |
| `loomtty kill <name>` | `k` | Kill a running session |
| `loomtty kill-server` | `ks` | Shut the daemon down |
| `loomtty delete <name>` | `rm` | Delete a saved session |
| `loomtty remote <user@host> [session]` | | Attach to a remote server over SSH |
| `loomtty template <ls\|apply\|save>` | `tpl` | Layout template management |
| `loomtty web [--port --bind --token --open]` | | Serve the browser UI + run the server |
| `loomtty init` | | Interactive config wizard |

## Gotchas for agents

- **`send-keys` ≠ submit.** Append `\n` (or `\r`) to actually run a typed command.
- **No key-name syntax.** Control keys are raw bytes (`$'\x03'` for Ctrl-C, `$'\x1b'` for Esc).
- **`get-layout` is always JSON**, even without `--json`. Capture-pane writes raw text (no `--json` needed for plain reads).
- **Capture is point-in-time**, and there's no built-in "wait for command to finish" — poll `capture-pane` for output, or `list-prompts` for completion + exit code (the reliable signal, if shell integration is on).
- **Errors go to stderr + exit 1.** Check the exit status; don't only parse stdout.
- **Pane IDs are per-session u64s** from `list-panes` / the `pane_id` returned by `run-command`/`create-pane`. They are not stable across closes.
