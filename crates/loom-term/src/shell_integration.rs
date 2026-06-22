//! OSC 133 shell integration parser.
//!
//! Returns a list of [`Osc133Event`]s for the caller to apply, rather than
//! mutating shell state directly. This lets the caller resolve the current
//! grid line at the moment each event was observed (so prompt/output line
//! numbers can be recorded into a [`crate::pane::PromptMarkRing`]).

use winnow::prelude::*;
use winnow::token::{any, rest};

use crate::esc_scanner::scan_osc;
use crate::partial_buf::PartialBuf;

/// An OSC 133 event surfaced by [`Osc133Parser::scan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Osc133Event {
    /// `OSC 133;A` — a new prompt is about to be drawn.
    PromptStart,
    /// `OSC 133;B` — the cursor is now at the start of command input.
    CommandInput,
    /// `OSC 133;C` — command output starts on the next byte.
    CommandOutput,
    /// `OSC 133;D[;exit_code]` — command finished.
    Done { exit_code: Option<i32> },
}

/// Parse an OSC 133 payload: `<kind>[;<params>]`. Returns `None` for kinds
/// we don't recognise (e.g. iTerm's `OSC 133;P` continuation marker).
fn parse_command(input: &mut &[u8]) -> winnow::error::ModalResult<Option<Osc133Event>> {
    let kind = any.parse_next(input)?;
    let params = winnow::combinator::opt((b';', rest))
        .parse_next(input)?
        .and_then(|(_, rest)| std::str::from_utf8(rest).ok())
        .unwrap_or("");

    Ok(match kind {
        b'A' => Some(Osc133Event::PromptStart),
        b'B' => Some(Osc133Event::CommandInput),
        b'C' => Some(Osc133Event::CommandOutput),
        b'D' => Some(Osc133Event::Done {
            exit_code: params.trim().parse::<i32>().ok(),
        }),
        _ => None,
    })
}

/// Parser for OSC 133 shell integration sequences.
pub(crate) struct Osc133Parser {
    partial: PartialBuf,
}

impl Osc133Parser {
    pub fn new() -> Self {
        Osc133Parser {
            partial: PartialBuf::new(4096, "OSC 133"),
        }
    }

    /// Scan `data` and return the OSC 133 events observed, in order.
    /// The caller resolves the current grid line for each event and applies
    /// it to `ShellState` + `PromptMarkRing`.
    ///
    /// Byte offsets of individual events within the chunk are intentionally
    /// **not** returned. Shells emit OSC 133 sequences at chunk boundaries
    /// (precmd / preexec hooks fire as standalone PTY writes), so the
    /// chunk-start cursor recorded by the caller is the correct row for
    /// every event in the chunk. Mid-chunk OSC 133 after content that moves
    /// the cursor can be off by ≤1 row; if that ever becomes a problem,
    /// upgrade by returning ranges and splitting `processor.advance` per
    /// event.
    pub fn scan(&mut self, data: &[u8]) -> Vec<Osc133Event> {
        let mut tmp = Vec::new();
        let data = self.partial.prepend_to(data, &mut tmp);

        let result = scan_osc(data, b"133");
        let mut events = Vec::with_capacity(result.sequences.len());

        for (_offset, payload) in &result.sequences {
            let mut input: &[u8] = payload;
            if let Ok(Some(event)) = parse_command(&mut input) {
                log::debug!("OSC 133 event: {event:?}");
                events.push(event);
            }
        }

        if let Some(partial_start) = result.partial_start {
            self.partial.store(&data[partial_start..]);
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_variants() {
        let mut input: &[u8] = b"A";
        assert_eq!(
            parse_command(&mut input).unwrap(),
            Some(Osc133Event::PromptStart)
        );

        let mut input: &[u8] = b"D;42";
        assert_eq!(
            parse_command(&mut input).unwrap(),
            Some(Osc133Event::Done {
                exit_code: Some(42)
            })
        );

        let mut input: &[u8] = b"D";
        assert_eq!(
            parse_command(&mut input).unwrap(),
            Some(Osc133Event::Done { exit_code: None })
        );

        // Unknown subcommand parses without error but yields no event.
        let mut input: &[u8] = b"X";
        assert_eq!(parse_command(&mut input).unwrap(), None);
    }

    #[test]
    fn fragmented_sequence_emits_events_in_order() {
        let mut parser = Osc133Parser::new();

        let events = parser.scan(b"prefix\x1b]133;C");
        assert!(events.is_empty(), "incomplete sequence should not emit");

        let events = parser.scan(b"\x07suffix");
        assert_eq!(events, vec![Osc133Event::CommandOutput]);

        let events = parser.scan(b"\x1b]133;D;7\x07");
        assert_eq!(events, vec![Osc133Event::Done { exit_code: Some(7) }]);
    }

    #[test]
    fn full_prompt_cycle_in_one_chunk() {
        let mut parser = Osc133Parser::new();
        let chunk =
            b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07file1 file2\r\n\x1b]133;D;0\x07";
        let events = parser.scan(chunk);
        assert_eq!(
            events,
            vec![
                Osc133Event::PromptStart,
                Osc133Event::CommandInput,
                Osc133Event::CommandOutput,
                Osc133Event::Done { exit_code: Some(0) },
            ]
        );
    }

    #[test]
    fn oversized_partial_is_discarded() {
        let mut parser = Osc133Parser::new();
        let mut data = vec![0x1b, b']', b'1', b'3', b'3', b';'];
        data.extend(std::iter::repeat_n(b'a', 4097));
        let events = parser.scan(&data);
        assert!(events.is_empty());
        // After discard, parser state is clean (nothing buffered).
    }
}
