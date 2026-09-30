# macOS packaging

Builds a `loomtty.app` bundle and a drag-to-Applications `.dmg` for Apple
Silicon, plus a Homebrew cask.

- [`make-app.sh`](make-app.sh) — assembles `loomtty.app` (both binaries,
  [`Info.plist`](Info.plist), icon, shell-integration), **ad-hoc** code-signs
  it (`codesign --sign -`), and packages `loomtty-<version>-macos-arm64.dmg`.
- [`Info.plist`](Info.plist) — bundle metadata template (`@VERSION@` is
  substituted at build time). Bundle id `dev.loomtty`.
- [`ServerInfo.plist`](ServerInfo.plist) — the tray daemon's separate helper
  identity, `dev.loomtty.server`. It lives under `Contents/Helpers`; the sibling
  `Contents/MacOS/loomtty-server` entry remains a symlink for CLI compatibility.
  This prevents Apple Events and app launch requests from selecting the daemon
  as though it were the GUI. Existing running daemons need to exit normally
  before the new identity takes effect.
- [`loomtty.rb`](loomtty.rb) — Homebrew cask: installs the `.app` and symlinks
  the `loomtty` / `loomtty-server` CLI binaries onto the PATH.

The same `loomtty` binary is both the `.app` (Finder/Launchpad launch — no
terminal) and the CLI; macOS has no console-subsystem split, so unlike Windows
there's no separate GUI binary.

## Build locally (on a Mac)

```sh
# Full Xcode 16+ is needed for App Intents discovery metadata.
# Set DEVELOPER_DIR if Xcode is not the selected developer directory.
cargo build --release -p loomtty -p loomtty-server --features loomtty/macos-app-intents
dist/macos/make-app.sh                      # → target/macos/loomtty.app + .dmg
# Faster local app testing with Command Line Tools only (no Shortcuts discovery):
cargo build -p loomtty -p loomtty-server
dist/macos/make-app.sh --bindir "$PWD/target/debug" --app-only --without-app-intents
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

Terminal right-click menus use AppKit's native `NSMenu`, including system
appearance, keyboard navigation, disabled items, and dismissal. Copy, guarded
Paste, Select All, Search, link actions, splits, and Close Pane operate on the
clicked pane. TUI mouse reporting still receives right clicks; hold Shift to
show the terminal menu instead. Output and rendering continue while the menu
is open. Actions retain their window/session/connection identity and reject
stale targets after a reconnect; splits target the original pane atomically.
Password input disables text extraction, selection, search, and link actions.

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
macos_app_intents = true
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
Both services declare an empty `NSRequiredContext` so macOS exposes them by
default; `NSSendFileTypes` limits their input to files and folders. Omitting
that context registers the services without automatically showing them in
Finder's menu.

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

### Text Services

The loomtty → Services menu can send the active terminal's selected text to
installed macOS text services and accept text returned by a service. Returned
text goes through the normal paste path, including bracketed paste and the
configured large-paste confirmation. It does not rewrite terminal output.

Requests stay bound to their original pane and connection; switching panes,
reconnecting, opening a modal, or entering password input invalidates them.
Selections containing concealed cells are not offered to Services. Transfers
are limited to 1 MiB of UTF-8 text in either direction. The service's private
pasteboard is used, preserving the normal clipboard.

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

The GPU chrome also exposes native accessibility controls: pane tabs, session
and workspace buttons, overview terminals, settings fields and dropdowns,
command-palette and search input, context-menu items, paste confirmation,
keyboard help, and connection status. Long settings/menu/result lists provide
adjustable scroll controls. Field labels, descriptions, values, and bounds come
from the existing settings schema and UI layout. A covered dialog's controls
are removed until it becomes visible again.

Actions return through the normal UI dispatcher. Closing a dialog, replacing
a menu command, or changing a paste payload invalidates old actions. Input
fields support setting their text value; ordinary buttons use their press
action and do not claim keyboard focus. The adapter caches unchanged controls
and does not add a polling timer.

This does **not** claim full [Ghostty feature parity](https://ghostty.org/docs/features).
App Intents metadata extraction and live Shortcuts execution still need to be
verified with full Xcode and an unlocked desktop. The accessibility adapter
still needs live VoiceOver acceptance testing. Terminal splits and settings use loom's GPU UI;
the native shell and server session model remain integrated with it.

### Shortcuts / App Intents

Feature-enabled builds include Swift App Intents in the same client executable.
They queue work on the Rust event loop and share the AppleScript creation/input
engine. No Apple Events, subprocess shell commands, or extra IPC server are used.
Non-default AppKit launches (such as Services/automation) defer the default
window to the requested action and leave saved desktop restoration data intact.
Startup does not force activation over another app. Normal launches and explicit
new-window/focus actions activate loomtty; background queries and input do not.
App Intents requires macOS 13+; earlier systems retain the normal native client.
The `.app` needs Xcode-generated `Metadata.appintents` to expose these actions in
Shortcuts. A bare Cargo binary or `--without-app-intents` bundle does not include
that discovery metadata.

- **New Terminal** creates a window, native tab, split right, or split down and
  returns a connected terminal for subsequent actions. A parent is required for
  a split and optional for a tab. New windows/tabs accept an existing absolute
  working-directory path (which explicitly creates a local session); splits
  inherit their terminal's directory.
- **Find Terminals** searches title, session and directory. Empty text lists all
  connected normal-window terminals. Terminal entities also expose these three
  fields to Shortcuts. Quick Terminal is excluded, as with AppleScript.
- **Focus Terminal**, **Send Text to Terminal**, and **Close Terminal** operate
  on a specific terminal entity. Send Text sends exact UTF-8 (at most 256 KiB),
  including supplied newlines; it adds no Return key. Close ends the pane's
  process. Dialogs/overlays reject these actions until dismissed.
- An App Shortcut provides **New Terminal** with the phrase “Open a terminal in
  loomtty.” Siri/Shortcuts discovery still requires the metadata and live
  acceptance checks below.

Actions use Apple's `requiresLocalDeviceAuthentication` policy: the Mac must be
unlocked before an action runs, including requests originating on another device.

`window.macos_app_intents = false` disables queries and actions independently of
`window.macos_applescript`. Entity IDs expire when the terminal's connection or
session changes and across app restarts. Find the terminal again in each workflow
instead of treating a saved entity as a persistent session reference. Cancellation
removes pending callbacks; it cannot undo an operation already sent to the server.
Creation times out after 30 seconds, with an outer 35-second bridge deadline; errors
ask the user to inspect the terminal before retrying to avoid duplicate actions.

The bundle builder obtains compiler metadata from the exact binary being packaged,
requires the extraction output, copies any required Swift compatibility runtimes,
and signs inside-out. A failed build keeps the previous `.app`. macOS CI builds
this feature, runs the Swift bridge tests, and requires real metadata extraction.

```sh
# Foundation-only tests; no terminal windows or server sessions are opened:
dist/macos/app-intents/test.sh
cargo test -p loomtty --lib --features macos-app-intents macos::scripting
```

### Manual regression checks

- Build with full Xcode, install the signed app, and find its actions in
  Shortcuts. Chain New Terminal → Send Text → Focus Terminal, using a path with
  spaces and Chinese/emoji input. Exercise tabs and both split directions, closed
  entities, cancellation, disabled configuration and a stalled connection. Run
  with loomtty already open, closed, and with no windows; check that cold launch
  does not create an unintended extra window. Confirm Siri discovers New Terminal
  and requires unlocking this Mac when it is locked. Run Find Terminals and Send
  Text while another app is focused; they must not steal focus. New Terminal and
  Focus Terminal should activate loomtty, including a minimized or hidden window.
- With VoiceOver, navigate between terminal panes and native tabs. Read Chinese,
  combining accents, and emoji; select and copy them. Read scrollback, resize
  the window, and check character bounds on Retina/external displays. Verify
  cursor/selection announcements during output and focus changes. A password
  prompt must expose no text, and hidden Quick Terminal panes must disappear.
  Close/reconnect panes and verify old accessibility elements become invalid.
- Use VoiceOver to open Settings, change a toggle/stepper/dropdown, and scroll
  to the last field. Read keyboard help and connection errors. Edit the command
  palette and Find fields, invoke results, and activate pane tabs in all three
  tab-bar positions. In overview, choose a terminal. Open a large-paste dialog
  over the palette and verify only the confirmation controls are reachable;
  cancelling must restore the palette. Repeat after moving between displays.
- Select text and invoke an installed text Service from the application menu.
  Test send-only, return-only, and text-transform services, with Chinese and
  emoji. Verify returned text uses bracketed paste and large-paste confirmation.
  Switch panes or reconnect while a service is open; its late result must not
  enter the newly focused terminal. Repeat without selection, in a password
  prompt, and while a modal is open. The normal clipboard must stay intact.
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
- Right-click active and inactive panes. Test native menu keyboard navigation,
  Copy/Paste, Select All, Search, links, both splits, and Close Pane. Keep output
  streaming while the menu is open; both output and rendering must continue.
  Reconnect or close the target window during tracking and verify stale actions
  do not affect another terminal. Check Retina positioning and Shift-right-click
  in mouse-reporting TUIs.
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

On Apple Silicon macOS, `cargo test --workspace --features loomtty/macos-app-intents`
passed **1,692 tests**
(12 documentation examples and one bundle-only test ignored), including
**369 client tests**. Coverage includes Services request identity, shared
paste-path regressions, Quick Terminal geometry,
animation reversal, shortcut replacement, session exclusion, appearance
switching, user color overrides, dictionary query bounds and password/modal
exclusion, restoration locking/atomic saves, corrupt records, disconnected
displays, configuration validation, Finder literal-path handling, scripting
identity/backpressure, and accessibility Unicode/range/geometry handling.
The accessibility cache tests also verify that renderer damage cannot cause
idle polling and that password state clears cached text without delay.
Chrome regressions check modal layering, stale commands/paste payloads, text
editing, settings scroll bounds, and pointer/accessibility geometry across tab
bar positions. Cocoa callbacks for detached terminal/control objects are tested
without launching the UI.
Incremental grid synchronization now sends grapheme overflow when needed,
detects combining marks arriving in a later PTY chunk, evicts overwritten
graphemes, and preserves their indices through history-only updates and trimming.
This keeps the text consumed by Copy and accessibility consistent with the PTY.
The fix requires the updated server as well as the client.
`cargo clippy --workspace --all-targets` completed with existing warnings both
with and without `loomtty/macos-app-intents`;
`cargo fmt`, plist validation, script syntax validation, and ad-hoc bundle
signature verification also completed.

A bundled startup smoke test exposed the pinned winit fork's requirement to
retain its own application delegate. The integration now extends that delegate
without changing its ivars or replacing its lifecycle handlers. The process
stayed alive after that correction. On an unlocked desktop, live checks now
cover new windows and native tabs, independent session input, moving a tab to
a window, closing sibling windows, and reopening the last session after all
windows close. Settings controls and its theme menu appear in the accessibility
tree; the Find field accepts Chinese via its accessibility setter and reports
matching results. Select All, Copy and paste into Find preserve the selected text.

An isolated client/server run verified Chinese, combining accents arriving in
separate output chunks, removal of an overwritten accent, and preservation of
the complete emoji ZWJ sequence in accessibility text and copied text. Live release checks also verify joined woman-technologist and family emoji,
regional-indicator flags, and keycaps. Shaping joins extended graphemes across
terminal cells and checks each CoreText run's actual font before rasterizing
its glyph ID. The emulator's allocated columns remain unchanged, so joined
sequences can leave space before following text. Regression tests cover these
clusters with ligatures enabled/disabled and font substitution for keycaps.

An isolated release client/server also passed live Apple-event checks from
Script Editor: new windows, native tabs, both split directions, exact target
input, focus, object properties, and window/tab/terminal counts. The working
directory contained Chinese, a space, an apostrophe, and literal `$()`; `pwd`
confirmed it was passed unchanged. Closing terminals/tabs/windows preserved the
other test window; input to an expired terminal ID failed with `-1728`.
The bundled dictionary regression now verifies actual Objective-C class names,
object command handlers, and the hidden count argument required by Cocoa.

Native context menus passed live release checks for disabled Copy, Unicode
Copy/Paste with bracketed-paste markers, keyboard navigation, both split
directions, Search and Close Pane on an inactive pane, and Escape dismissal.
The native menu's position and appearance were checked on Retina; a timed PTY
counter continued updating terminal pixels while the menu remained open.
Unit tests cover coordinate conversion, replaced session/connection rejection,
and password/disconnection permissions.

IME candidate placement now shares the renderer's window coordinates, including
top status-bar and side tab-bar offsets. Its cache includes cell size and display
scale and is invalidated when the input context changes. Terminal composition
anchors to the text cell independently of block, underline, or hidden cursor
painting. Regressions cover all tab-bar/status-bar placements, painted preedit
alignment, fixed-position font/scale changes, and hidden/underline cursors.
The current UI automation attempt delivered literal `nihao` without entering
system Pinyin composition, so it does not count as a live IME pass.

Composition now retains its original pane/text field, session, connection, and
broadcast recipients across AppKit's empty-preedit/commit sequence. Focus or
modal changes cancel stale preedit instead of transferring it to another input;
reopening the same field also expires the old composition. A paste-confirmation
dialog blocks IME input to a palette underneath it. macOS window focus loss
disables the native input context and clears winit's marked text; focus return
reenables it. Regressions reproduce the previous wrong-pane/wrong-palette
delivery and cover reconnects, pending session changes, cancellation, exact
Unicode delivery, and password broadcast suppression. A live release check
opened and closed another native window while a raw PTY recorded keystrokes;
the original terminal received exactly `ab` across the focus round trip.

The installed macOS Simplified-to-Traditional Chinese text Service returned
the expected conversion to an isolated raw PTY, preserving emoji and combining
accents and enclosing the response in bracketed-paste markers. A test password
prompt removed AX text and acquired Secure Input with the client's PID visible
in `ioreg`; hiding the app released it, as did finishing the prompt. The native
menu labelled automatic protection and disabled text-transform services.

Native pasteboard regressions additionally exercise send/receive and receive-only
callbacks with both modern and legacy string types, emoji/combining marks, empty
results, and the 1 MiB UTF-8 boundary. Each uses uniquely named AppKit pasteboards
without accessing the general clipboard. Queued results retain their requestor
identity until delivery, so invalidation followed by a return to the same pane
cannot revive a stale result. The regression checks both an already-queued result
and a callback that arrives after invalidation. These callback tests supplement
the live conversion check; they do not replace the remaining system-provider
and clipboard-preservation checks.

Secure Input received a further live release check on 2026-09-30 using the
`6dbc5b4` client in an isolated server runtime. Only an empty test password was
submitted. `ioreg` ownership was checked after each transition:

| Transition | Observed Secure Input owner |
| --- | --- |
| Test password prompt focused | QA client PID |
| New ordinary sibling window focused | None |
| Sibling closed; password window focused again | QA client PID |
| Settings opened over the password prompt | None |
| Settings dismissed | QA client PID |
| Empty password submitted | None |
| Native Secure Keyboard Entry menu enabled | QA client PID |
| Application hidden | None |
| Window raised and terminal focused again | QA client PID |
| Last window closed | None; client process stayed alive |
| Last session reopened | Same QA client PID |
| Client quit | None; client process exited |

Password AX elements exposed no value. The isolated server still listed both
test sessions with zero attached clients after Quit; it was then shut down
separately. This checks the OS's actual ownership rather than inferring it from
the menu state. The automation could not capture the open application menu's
checkmark, so that visual check remains manual.

Finder Services passed live release checks on 2026-09-30 with a signed QA app
installed in `~/Applications`. The checks first exposed the missing
`NSRequiredContext`; after adding it and refreshing Services registration,
both entries appeared in Finder's context menu. Selecting a folder opened a
new window, and selecting two files in the same folder opened exactly one new
native tab. Those warm-client checks used a separate server socket. After the
QA client quit, invoking the window service on one file cold-launched it with
one attached session in the file's parent directory, using the already-running
local server. In all three cases, shell `pwd` preserved Chinese, spaces, an
apostrophe, and literal `$()` in the directory name. The temporary installed
QA copy was removed after validation.

VoiceOver speech/navigation, IME composition, the Secure Input menu checkmark,
physical Look Up gestures, multi-display/Spaces restoration,
Finder Services with a remote window active, folder Open With, other
text-Service modes/late results/clipboard preservation, and
Shortcuts discovery/execution still need their manual checks above. Full Xcode remains necessary for App Intents metadata;
Command Line Tools alone cannot finish that validation.
