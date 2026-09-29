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
| Ctrl+Cmd+D | Look up selected text or the word under the pointer |
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
macos_restore_windows = true
macos_applescript = true
```

Option-key and Secure Input settings hot reload. By default left Option sends terminal Alt and right
Option retains native text entry. Secure Event Input is enabled only while a
focused terminal is marked as reading a password by the server; it is released
on focus loss, modal UI, password completion, window close, and application
exit. This relies on the existing terminal password detection, not on matching
prompt text. The loomtty → Secure Keyboard Entry menu can keep protection on
manually. The checkmark shows enabled/requested protection; automatic password
protection is labelled and cannot be unchecked during that prompt. Manual
protection is still released when the app loses focus or shows modal UI and
resumes when terminal input regains focus. The native titlebar follows the configured terminal background's
light/dark contrast.

Default interactive shells start as login shells on macOS so Finder launches
load PATH and login configuration. Explicit commands and configured shell
programs retain their arguments. A bundled launch from `/` starts in the home directory;
CLI working directories are preserved.

### Finder integration

Finder → Services provides **New loomtty Window Here** and **New loomtty Tab
Here** for selected local files/folders. Files open their containing folder;
multiple items in one folder are deduplicated. Folders also advertise loomtty
as an alternate Open With application. Installing the `.app` in Applications
lets Launch Services discover these entries; no default file association is
changed by the bundle.

The selected directory is stored as a fresh session's startup cwd before its
first shell launches. Spaces, quotes, and shell metacharacters remain literal
path characters. Existing sessions are never overwritten, and no `cd` command
is pasted into a shell. Normal Cmd+N/Cmd+T local windows also inherit the active
local pane's cwd when available. Finder services always create local sessions,
including when invoked while a remote terminal is active. Remote tab/window
creation continues to use the remote server's session startup directory.

### Window restoration

A default launch restores regular windows, their native tab order/selection,
local or SSH session connections, positions, and minimized/zoomed/fullscreen
state. Pane layouts and processes come from the existing server/session layer.
Quick Terminal is excluded. `window.macos_restore_windows = false` disables
restoration on the next launch. `macos_initial_window = false` takes precedence.
Explicit CLI session commands always open their requested window.

State is stored separately from server sessions in `state_dir()/macos/windows.json`.
The first default-launch process owns a file lock; other processes cannot
replace its state. Snapshots are debounced and atomically replaced. Quit saves
the open windows before detaching them; closing a window normally removes it
from the next snapshot. If all windows were closed, the next default launch
opens its usual initial window. Missing/corrupt state falls back to normal
startup. Window frames use points and are clamped to connected displays, so
unplugged monitors do not leave restored windows off-screen. The OS chooses
new fullscreen Spaces; their previous Space numbers cannot be restored.

### Look Up

Use the macOS three-finger Look Up gesture, Force Touch, or Edit → Look Up
(Ctrl+Cmd+D) for a native dictionary popover. Gestures query the word under
the pointer; the menu prefers selected text. Pointer coordinates account for
Retina scale and the current pane viewport. Queries leave the terminal
selection unchanged and are disabled for password input, overview, and modal
UI. The system Trackpad setting determines which physical gesture is available.

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

### AppleScript

The app bundle includes a scripting dictionary. Open **Script Editor → File →
Open Dictionary → loomtty** to inspect it. `window.macos_applescript = false`
disables loom's object queries and control commands after config reload.
The hierarchy is application → native windows → native tabs → terminals.
Application-level `tabs` and `terminals` collections also support absolute ID
references. Quick Terminal is excluded. Tab IDs survive a session switch;
terminal IDs expire when their connection, session, or pane changes.

```applescript
tell application "loomtty"
    set projectWindow to new window working directory "/tmp"
    set firstTerminal to focused terminal of projectWindow
    set secondTerminal to split terminal firstTerminal direction "down"
    input text ("pwd" & linefeed) to secondTerminal
    focus firstTerminal
    set extraTab to new tab in window projectWindow working directory "/tmp"
    return {id of projectWindow, session name of extraTab}
end tell
```

`new window`, `new tab`, and `split terminal` wait for the new terminal (up to
30 seconds). Split creation is atomic relative to its target and returns the
server's correlated pane ID; it requires the matching server build, including
on remote hosts. If creation times out, inspect the app before retrying: a
window or pane may already exist. New windows and tabs inherit the active
connection; an explicit working directory selects a local session.

`input text` sends exact UTF-8 (up to 256 KiB per call) to the specified terminal, without broadcasting,
bracketed-paste wrapping, or an added newline. Add `linefeed` to execute a
command. Its completion means queued for delivery, not that the shell finished
processing it. Closed objects, unavailable connections, a full input queue,
and modal dialogs return script errors. `close terminal` closes its PTY;
`close tab` and `close window` detach clients and preserve server sessions.

`dist/macos/test-scripting.sh` runs a Foundation-only bundle test that loads the
actual dictionary, checks command argument mappings, and exercises object
getters/specifiers. It requires no Xcode installation and opens no UI. The
normal Rust/server tests cover identity invalidation, backpressure, and atomic
split targeting. GUI Apple-event execution still needs the manual checks below.

### Accessibility

Each visible terminal pane exposes an AppKit text area for VoiceOver, with its
session/title, focus, text, cursor line, selection, and character bounds. Text
ranges use UTF-16, preserving Chinese wide characters, surrogate pairs,
combining marks, and emoji sequences. Assistive selection focuses the named
pane and uses the same selection as Copy; stale selections are rejected after
output or reflow changes their meaning.

The accessible text includes the visible viewport and up to 2,048 recent rows
ending at that viewport, capped at 256 Ki UTF-16 units. Scroll the terminal to
read older history. Output notifications are coalesced; password-input panes
are cleared immediately and concealed cells are exposed as spaces. Hidden
Quick Terminal panes are removed from the accessibility tree.

This does **not** claim full [Ghostty feature parity](https://ghostty.org/docs/features).
App Intents and accessible controls for GPU settings/overlays remain
outstanding. The terminal text adapter still needs live VoiceOver acceptance
testing. Terminal splits and settings use loom's GPU UI; the native shell and
server session model remain integrated with it.

### Manual regression checks

- With VoiceOver, navigate between terminal panes and native tabs. Read Chinese,
  combining accents, and emoji; select and copy them. Read scrollback, resize
  the window, and check character bounds on Retina/external displays. Verify
  cursor/selection announcements during output and focus changes. A password
  prompt must expose no text, and hidden Quick Terminal panes must disappear.
  Close/reconnect panes and verify old accessibility elements become invalid.
- In Script Editor, run the example above, query `every terminal`, then move
  tabs between windows and resolve the saved IDs again. Try scripts while
  another client switches focus; splits and input must still reach the named
  target. Disconnect/reconnect a session and verify old terminal references
  fail. Disable AppleScript and verify queries/commands return errors. Confirm
  creation followed immediately by input delivers the entire text.

- Open multiple windows and native tabs; type different commands in each.
  Switch tabs, move a tab to a window, and merge windows. Input and output must
  stay associated with the correct session.
- Move/resize windows, reorder tabs, minimize/zoom/fullscreen groups, then quit
  and relaunch. Verify sessions and selected tabs. Unplug an external monitor
  before relaunching and verify reachable frames. Run a separate explicit CLI
  session and check that it does not replace the main application's snapshot.
  Test disabled restoration, Quick Terminal exclusion, and corrupt state fallback.
- Close one tab/window, then the last window; reopen from the Dock. The process
  remains alive and the server session survives. Cmd+Q and Dock → Quit exit
  the client without killing the server.
- Use menu Copy/Paste/Find, paste into search/palette, and verify large-paste
  confirmation. Drag paths containing spaces/apostrophes into the terminal;
  an open modal must not let a drop type into a hidden terminal.
- In Finder, invoke New loomtty Window/Tab Here on a folder, a file, and several
  files from the same folder. Test names containing spaces, apostrophes, `$()`,
  and Chinese text. `pwd` must show the chosen directory with no extra shell
  input. Repeat with the app closed and while a remote window is active.
- Look up words with three-finger tap, Force Touch, and Ctrl+Cmd+D. Repeat on
  Retina/external displays and scrolled panes. Test selected Unicode text,
  password prompts, and modal overlays. Verify one popover per force click.
- Toggle Secure Keyboard Entry manually, change focus, and quit. Check both
  the native checkmark and `ioreg` ownership before/after every transition.
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

On Apple Silicon macOS, `cargo test --workspace` passed **1,638 tests**
(12 documentation examples and one bundle-only test ignored). A subsequent
focused run added a passing scrollback accessibility regression test. Coverage includes Quick Terminal geometry,
animation reversal, shortcut replacement, session exclusion, appearance
switching, user color overrides, dictionary query bounds and password/modal
exclusion, restoration locking/atomic saves, corrupt records, disconnected
displays, configuration validation, Finder literal-path handling, scripting
identity/backpressure, and accessibility Unicode/range/geometry handling.
The accessibility cache tests also verify that renderer damage cannot cause
idle polling and that password state clears cached text without delay.
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
