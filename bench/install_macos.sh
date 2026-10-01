#!/usr/bin/env bash
# Explicitly invoked installer. Never changes Gatekeeper or overwrites an app.
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
    echo "This installer requires macOS." >&2
    exit 1
fi

command -v brew >/dev/null
if [[ ! -d /Applications/kitty.app ]]; then
    brew install --cask kitty
fi
if [[ ! -d /Applications/Ghostty.app ]]; then
    brew install --cask ghostty
fi

if [[ ! -d /Applications/Alacritty.app ]]; then
    # The Homebrew Alacritty cask was disabled on 2026-09-01. Build the
    # official pinned release using its native app packaging target instead.
    command -v cargo >/dev/null
    command -v scdoc >/dev/null || brew install scdoc
    alacritty_build_dir="$(mktemp -d /tmp/loom-alacritty-build.XXXXXX)"
    git clone --depth 1 --branch v0.17.0 https://github.com/alacritty/alacritty.git "$alacritty_build_dir/source"
    (
        cd "$alacritty_build_dir/source"
        MACOSX_DEPLOYMENT_TARGET="10.12" cargo build --release --locked
        make app
        codesign --verify --deep target/release/osx/Alacritty.app
        cp -R target/release/osx/Alacritty.app /Applications/Alacritty.app
    )
    echo "Alacritty source/build retained at $alacritty_build_dir"
fi

/Applications/Alacritty.app/Contents/MacOS/alacritty --version
/Applications/kitty.app/Contents/MacOS/kitty --version
/Applications/Ghostty.app/Contents/MacOS/ghostty +version
