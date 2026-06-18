# macOS packaging

Builds a `loomtty.app` bundle and a drag-to-Applications `.dmg` for Apple
Silicon, plus a Homebrew cask.

- [`make-app.sh`](make-app.sh) — assembles `loomtty.app` (both binaries,
  [`Info.plist`](Info.plist), icon, shell-integration), **ad-hoc** code-signs
  it (`codesign --sign -`), and packages `loomtty-<version>-macos-arm64.dmg`.
- [`Info.plist`](Info.plist) — bundle metadata template (`@VERSION@` is
  substituted at build time). Bundle id `dev.loomtty`.
- [`loomtty.rb`](loomtty.rb) — Homebrew cask: installs the `.app` and symlinks
  the `loomtty` / `loomtty-server` CLI binaries onto the PATH.

The same `loomtty` binary is both the `.app` (Finder/Launchpad launch — no
terminal) and the CLI; macOS has no console-subsystem split, so unlike Windows
there's no separate GUI binary.

## Build locally (on a Mac)

```sh
cargo build --release -p loomtty -p loomtty-server
dist/macos/make-app.sh                      # → target/macos/loomtty.app + .dmg
# real signing identity instead of ad-hoc:
dist/macos/make-app.sh --sign-id "Developer ID Application: Name (TEAMID)"
```

## Signing / notarization

The release ships an **ad-hoc** signature (no Apple Developer account needed),
so Gatekeeper blocks the first launch — users right-click → Open, or run
`xattr -dr com.apple.quarantine /Applications/loomtty.app`. For a double-click
experience, sign with a Developer ID and notarize (`xcrun notarytool submit`),
then `xcrun stapler staple loomtty.app` before building the `.dmg`.

## CI

The [`Release`](../../.github/workflows/release.yml) workflow runs `make-app.sh`
on the `macos-latest` (arm64) runner for every `v*` tag and attaches the `.dmg`
to the GitHub Release.
