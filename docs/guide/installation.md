# Installation

loomtty ships two binaries: **`loomtty`** (the client) and **`loomtty-server`**
(the daemon). The client auto-starts the server, so once both are on your `PATH`
you only ever run `loomtty`.

## From a release

Prebuilt binaries are published on the
[GitHub Releases](https://github.com/l1nxy/loomtty/releases) page.

::: code-group

```sh [Linux]
# Pick the artifact for your architecture (x86_64 / aarch64)
ARTIFACT="loomtty-linux-x86_64"
curl -sL "https://github.com/l1nxy/loomtty/releases/latest/download/${ARTIFACT}.tar.gz" \
  -o "/tmp/${ARTIFACT}.tar.gz"

mkdir -p ~/.local/bin
tar xzf "/tmp/${ARTIFACT}.tar.gz" -C /tmp/
cp "/tmp/${ARTIFACT}/loomtty" "/tmp/${ARTIFACT}/loomtty-server" ~/.local/bin/
chmod +x ~/.local/bin/loomtty ~/.local/bin/loomtty-server

loomtty --version
```

```sh [macOS]
# ⚠️ macOS is untested — treat as work in progress.
ARTIFACT="loomtty-macos-aarch64"   # or loomtty-macos-x86_64
curl -sL "https://github.com/l1nxy/loomtty/releases/latest/download/${ARTIFACT}.tar.gz" \
  -o "/tmp/${ARTIFACT}.tar.gz"

mkdir -p ~/.local/bin
tar xzf "/tmp/${ARTIFACT}.tar.gz" -C /tmp/
cp "/tmp/${ARTIFACT}/loomtty" "/tmp/${ARTIFACT}/loomtty-server" ~/.local/bin/
chmod +x ~/.local/bin/loomtty ~/.local/bin/loomtty-server

loomtty --version
```

```powershell [Windows]
# Download loomtty-windows-x86_64.zip from the Releases page, then extract it
# to a directory on your PATH, e.g.:
Expand-Archive loomtty-windows-x86_64.zip -DestinationPath "$HOME\bin\loomtty"
# Add that folder to PATH, then:
loomtty --version
```

:::

Make sure the install directory (`~/.local/bin` above) is on your `PATH`.

## From source

Building requires **Rust 1.82+** (edition 2024).

```bash
git clone https://github.com/l1nxy/loomtty.git && cd loomtty
cargo build --release
# → target/release/loomtty (client) and target/release/loomtty-server (daemon)

cp target/release/loomtty target/release/loomtty-server ~/.local/bin/
```

### System dependencies

On Linux you'll need the usual graphics / font development libraries. If the
build fails, the compiler error names the missing package — for example
`freetype2`, `fontconfig`, or `xcb`. Install the matching `-dev` / `-devel`
package for your distribution and retry.

## Shell integration (recommended)

Sourcing the snippet for your shell enables OSC 133 prompt marks — jump-to-prompt
in scroll mode, command status, and exit codes. See
[Shell Integration](/guide/shell-integration) for setup.

## Initial setup

Run the interactive wizard to choose your keybinding style, leader key, theme,
and status bar position:

```bash
loomtty init
```

Shell and font are auto-detected. If you skip it, loomtty works with all
defaults. See [Configuration](/guide/configuration) for the file it writes.

## Uninstall

```sh
rm -f ~/.local/bin/loomtty ~/.local/bin/loomtty-server
rm -rf ~/.config/loom
```
