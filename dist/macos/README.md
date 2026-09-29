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
# Faster local app testing without creating a DMG:
dist/macos/make-app.sh --bindir "$PWD/target/debug" --app-only
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

## Native application behavior

The macOS client uses AppKit windows, native tab groups, the system menu bar,
traffic lights, and native fullscreen/Spaces, with the existing Rust client
core and Metal renderer inside each window. Each new window or native tab gets
an independent loom session; the session/connection switcher remains available
inside it. New windows/tabs inherit the active window's remote connection.

| Shortcut | Behavior |
| --- | --- |
| Cmd+N / Cmd+T | New window / native tab |
| Cmd+Shift+W | Close the current tab/window and detach; keep its session running |
| Cmd+W | Close the active terminal pane (ends that pane's process) |
| Cmd+D / Cmd+Shift+D | New column / stacked tile |
| Cmd+Shift+P | Command palette |
| Cmd+Q | Quit all client windows; leave server sessions running |
| Cmd+, / Cmd+Shift+, | Settings / reload configuration in every window |
| Cmd+C / Cmd+V / Cmd+F | Copy / guarded paste / find |
| Cmd+A | Select the active terminal's scrollback and screen |
| Cmd+= / Cmd+- / Cmd+0 | Increase / decrease / reset font size (temporary) |
| Cmd+Shift+[ / Cmd+Shift+] | Previous / next native tab |
| Cmd+M / Ctrl+Cmd+F | Minimize / native fullscreen |
| Cmd+H / Option+Cmd+H | Hide loomtty / hide other applications |

Terminal menu shortcuts (Copy, Paste, Find, Close Pane, etc.) follow
`keys.direct_bindings`, including custom mappings and removals. Application
and window shortcuts in the table are reserved by the native menu. The Window
menu also supports moving a tab into a window, merging windows, showing the
tab bar, and bringing all windows forward. AppKit's tab-bar **+** creates a tab.

Closing the last window keeps loomtty running in the Dock. Clicking the Dock
icon reopens the last session, or brings an existing window forward. Closing
one window or detaching one session does not exit sibling windows. Local pane
working directories populate the titlebar proxy icon; remote paths are never
interpreted as local files.

```toml
[window]
macos_option_as_alt = "left" # "none", "left", "right", or "both"
macos_secure_input = true
macos_quit_after_last_window_closed = false
macos_initial_window = true
```

Option-key and Secure Input settings hot reload. By default left Option sends terminal Alt and right
Option retains native text entry. Secure Event Input is enabled only while a
focused terminal is marked as reading a password by the server; it is released
on focus loss, modal UI, password completion, window close, and application
exit. This relies on the existing terminal password detection, not on matching
prompt text. The native titlebar follows the configured terminal background's
light/dark contrast.

Default interactive shells start as login shells on macOS so Finder launches
load PATH and login configuration. Explicit commands and configured shell
programs retain their arguments. A bundled launch from `/` starts in the home directory;
CLI working directories are preserved.

### System appearance

Terminal colors, floating chrome, and native window appearance can follow
macOS light/dark mode, including an automatic sunset/sunrise change:

```toml
[theme]
preset = "loom_dark"       # fallback; also used on other platforms
light_preset = "loom_light"
dark_preset = "loom_dark"
# accent = "#286B86"       # explicit overrides apply in both appearances
```

Leave both appearance presets empty (the default) to keep a fixed theme. If
only one is set, the other uses `preset`. Explicit color overrides, including
empty values, are preserved across switches. `loom_light` includes light
terminal and chrome colors and is also available as a fixed preset. Choosing
a preset in the settings panel temporarily stops following the system until
configuration reload. Fixed themes continue to set titlebar contrast from the
terminal background. Appearance changes invalidate every pane's cached colors.

### Quick Terminal

View → Quick Terminal opens a separate local terminal over the current
application. It keeps its connection and grid while hidden, supports all four
screen edges and multiple displays, and respects the menu bar, Dock, and camera
housing. Ctrl+Cmd+F fills the available screen without entering a fullscreen
Space. It does not participate in native tab groups.

```toml
[window]
macos_initial_window = false # optional: start in the Dock with no regular window

[window.macos_quick_terminal]
shortcut = "Control+Super+Backquote" # global; empty (default) disables registration
position = "top"                    # top, bottom, left, right
screen = "mouse"                    # mouse or main (menu-bar display)
width = 1.0                         # fraction of usable display, 0.1–1.0
height = 0.4                        # fraction of usable display, 0.1–1.0
animation_ms = 180                  # 0–1000; Reduce Motion uses zero
autohide = true                    # hide when another window/app receives focus
```

`Super` means Command. The global shortcut is opt-in and does not need
Accessibility permission. An invalid or conflicting replacement keeps the
previous working registration; the View menu indicates the failure. Explicit
shortcut/menu dismissal returns focus to the previous application or loom
window; automatic dismissal never takes focus back. Animation frames do not
resize the shell; only the final dimensions are sent to the server.

`macos_initial_window` applies on the next default launch; explicit CLI session
commands still create windows. The other Quick Terminal settings hot reload;
position and dimensions apply on the next show. Closing the popup hides it;
quitting the application detaches it like other loom windows. Its `quick-*`
session remains available in the session list for an explicit attach, but is
excluded from automatic normal-window session selection. A later application
launch creates a new Quick Terminal session. This follows loom's persistent
server model rather than terminating the shell when the app quits.

This does **not** claim full [Ghostty feature parity](https://ghostty.org/docs/features).
Quick Look/Force Touch, AppleScript/App Intents, Finder Services providers,
VoiceOver terminal content and OS window geometry restoration remain future work. Terminal splits and the settings
panel still use loom's GPU UI. Session restoration is provided by the existing
loom server/session layer.

### Manual regression checks

- Open multiple windows and native tabs; type different commands in each.
  Switch tabs, move a tab to a window, and merge windows. Input and output must
  stay associated with the correct session.
- Close one tab/window, then the last window; reopen from the Dock. The process
  remains alive and the server session survives. Cmd+Q and Dock → Quit exit
  the client without killing the server.
- Use menu Copy/Paste/Find, paste into search/palette, and verify large-paste
  confirmation. Drag paths containing spaces/apostrophes into the terminal;
  an open modal must not let a drop type into a hidden terminal.
- Compose Chinese/Japanese text, cancel composition, switch windows, and type
  again. Verify Option-key modes and candidate placement on Retina displays.
- Exercise a password prompt, switch away/back, and close its window; inspect
  Secure Input ownership with `ioreg -l -w 0 | rg SecureInput`.
- Test fullscreen/Spaces, minimize, Hide/Show, resize and move between displays.
  Test Finder launch, shell PATH, a custom theme, and live config reload.
  Configure appearance presets and switch macOS Light/Dark/Auto; terminal cells,
  palette, settings, native tabs, and the Quick Terminal must all update. Test
  a fixed preset and explicit color overrides too.

- Set a global Quick Terminal shortcut; invoke it from another application and
  over a fullscreen Space. Toggle repeatedly during animation and check focus
  restoration. Click another app to test autohide. Repeat on all display edges,
  with the pointer on each display, an auto-hidden menu bar/Dock, and Reduce Motion.
- Run `stty size` in the popup before/after hiding. It must keep its final grid
  size. Test Ctrl+Cmd+F, disabling/rebinding the shortcut, and a conflicting
  shortcut. Launch with `macos_initial_window = false`, use the View menu, then
  open a normal window from the Dock. A normal default launch must not attach
  to a saved `quick-*` session automatically.

### Validation for this implementation

On Apple Silicon macOS, `cargo test --workspace` passed **1,611 tests**
(12 documentation examples ignored). This includes the Quick Terminal geometry, animation reversal, shortcut
replacement, session exclusion, appearance switching, user color overrides,
and configuration validation tests.
`cargo clippy --workspace --all-targets` completed with existing warnings;
`cargo fmt`, plist validation, script syntax validation, and ad-hoc bundle
signature verification also completed.

A bundled startup smoke test exposed the pinned winit fork's requirement to
retain its own application delegate. The integration now extends that delegate
without changing its ivars or replacing its lifecycle handlers. The process
stayed alive after that correction, but the desktop was locked, so window
presentation, menu clicks, tab interactions, Secure Input transitions, and
Dock reopen still need the manual checks above. Compilation and unit tests
are not a substitute for those checks.
