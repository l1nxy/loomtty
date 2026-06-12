# Shell Integration

Sourcing loomtty's shell snippet enables **OSC 133 prompt marks**. With them,
loomtty knows where each prompt and command begins and ends, which unlocks:

- **Jump-to-prompt** in scroll mode — hop between previous prompts instead of
  scrolling line by line.
- **Command status** — see which command is running and its **exit code**.

loomtty also emits **OSC 7** so it can track each pane's working directory.

## Scripts

Integration scripts live in the repository under
[`crates/loom-server/shell-integration/`](https://github.com/l1nxy/loomtty/tree/main/crates/loom-server/shell-integration):

| Shell | Script |
| ----- | ------ |
| Bash | `loom.bash` |
| Zsh | `loom.zsh` |
| Fish | `loom.fish` |

The release tarball includes these under a `shell-integration/` directory.

## Setup

Source the script for your shell from its startup file.

::: code-group

```bash [bash]
# ~/.bashrc
source ~/.config/loom/shell-integration/loom.bash
```

```zsh [zsh]
# ~/.zshrc
source ~/.config/loom/shell-integration/loom.zsh
```

```fish [fish]
# Fish auto-loads files in conf.d:
cp loom.fish ~/.config/fish/conf.d/
```

:::

Restart your shell (or `source` the rc file) and start a new loomtty session. In
scroll mode (`Alt s`) you can now jump between prompts.

::: tip
See [INSTALL.md](https://github.com/l1nxy/loomtty/blob/main/INSTALL.md) for the
exact copy/source commands per platform, including how to wire it up from a
release tarball.
:::
