# Predictive echo under a slow link

Run the reproducible scenarios from the workspace root:

```sh
cargo test -p loom-integration-tests --test prediction_weak_network
cargo test -p loom-app prediction
cargo bench -p loom-app --bench prediction
```

Real TUI coverage is opt-in because it launches external applications:

```sh
# Requires Vim and Neovim on PATH; uses temporary buffers and disables configs/plugins.
cargo test -p loom-integration-tests --test prediction_weak_network tui::editors -- --ignored --nocapture

# Requires an authenticated Codex CLI and an already trusted directory.
# Override LOOM_TEST_CODEX_BIN with the executable path if codex is not on PATH.
LOOM_TEST_CODEX_CWD=/path/to/trusted/project cargo test -p loom-integration-tests --test prediction_weak_network tui::codex_inline -- --ignored --nocapture
```

The TUI tests were run locally with Vim 9.1, Neovim 0.12.5 and Codex CLI
0.158.0-alpha.2.1. They exercise real programs through `loom-term` PTYs and the
same delayed/fragmented wire path, including terminal mode flags. Snapshots wait
until DEC 2026 synchronized redraws finish. These are headless composition tests,
not GUI/GPU screenshot tests.

Vim/Neovim coverage includes normal-mode commands under Never/Always/Adaptive,
and insert/delete/Escape/alternate-screen exit with Always/Adaptive at 50/200/800 ms RTT plus a 300 ms
stall. Codex runs with `--no-alt-screen --no-daemon --sandbox read-only`, testing
its real composer under Never/Always/Adaptive through typing, delayed Backspace,
multiline bracketed paste and resize. Test text is never submitted to the model. The test does not accept
trust dialogs, modify project files or change user settings.

This exposed an additional bug: the production hidden-text/forced-Backspace
paths bypassed the usual TUI mode guard. Vim navigation could become visible
text or pin the predicted cursor on an old row. App-owned screens now require a
matching typed-character echo before a local edit run can be displayed; shifted
existing cells and blank deletions do not establish that evidence. Verified TUI
editing retains prediction. Controls, screen-mode changes and prompt relocation
discard the old edit run, and keyboard invalidation repaints the removed overlay.

`prediction_weak_network` schedules framed server updates on a virtual clock.
It covers 50/200/800 ms round trips, uneven delivery, a 300 ms stall, partial
echoes, and coalesced cursor history. Each frame goes through a bounded byte
stream with 7-byte writes and a 17-byte buffer, alternating FullPaneSync and
CellDelta. This models ordered reliable delivery (TCP/SSH); retransmission
delays bytes rather than dropping protocol frames. It does not modify host
network settings or require a running GUI/server.

The sustained-edit scenario checks every visible cell and cursor against an
independent line editor after each of 120 insert/delete/arrow operations, then
checks exact convergence after the link drains. On Unix, an additional test
starts an isolated real PTY with canonical echo/erase, feeds it through
`loom-term` and the wire codecs, and delays the final echo until 1.6 seconds.
It also checks held Backspace at the known edit boundary and outage expiry.
The virtual link does not exercise the daemon's event scheduling or GPU output.

Three initial regressions failed before the fix: a receipt-only frame cancelled
a valid Backspace, cancelled a valid Left, and discarded pending text when an
older cursor lagged behind a coalesced server frame. Receipt does not prove
shell execution. Cursor reconciliation now waits for echo acknowledgments and
discards old cursor history without killing newer predictions.

The server's existing `echo_ack` remains a PTY-drain watermark, not a per-input
shell execution receipt: a partial drain can still acknowledge too much of a
burst. Such mature mismatches continue to fall back to server state. No wire
format change is made here. When an edit has no echo, the known local edit
anchor bounds held Backspace; absent an anchor, receipt alone cannot safely
establish a prompt boundary. The existing eight-second timeout now runs even
with no incoming frames and cursor blinking disabled.

The compositor visits predicted rows once, instead of querying two hash maps
for every screen cell. It copies the viewport only when a visible cell needs
replacement, and excludes scrollback views. Width flags survive shifts so
subsequent edits can still recognize a CJK glyph and its spacer.

The benchmark compares the former full-viewport loop with the new compositor
in the same release binary, checking output equality first. It measures CPU
composition only, not total frame time or network latency. Hidden and tentative
overlays are also measured; these no longer allocate a viewport copy.

Sample from the local release build (nanoseconds per composition; timing varies
by machine and load):

| Case | Viewport | Former loop | Row compositor | Speedup |
| --- | --- | ---: | ---: | ---: |
| Visible | 80×24 | 24,429 | 1,517 | 16.1× |
| Visible | 240×80 | 202,825 | 10,708 | 18.9× |
| Hidden | 80×24 | 3,347 | 7 | 496× |
| Tentative, 100 ms RTT | 80×24 | 21,115 | 44 | 476× |

Validation: `cargo test --workspace` completed with 1,605 passed and 15 ignored;
the five default weak-network integration tests passed. All three opt-in real TUI
tests were then explicitly run and passed (including their app/mode/RTT matrices). `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets` completed successfully; Clippy still
reports existing warnings outside the changed implementation.
