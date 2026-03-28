use crate::esc_scanner::scan_osc;
use crate::pane::{SemanticZone, ShellState};
use crate::partial_buf::PartialBuf;

/// Parser for OSC 133 shell integration sequences.
/// Handles sequences split across PTY read boundaries.
pub(crate) struct Osc133Parser {
    partial: PartialBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Osc133Command<'a> {
    kind: u8,
    params: &'a str,
}

fn parse_command(payload: &[u8]) -> Option<Osc133Command<'_>> {
    let (&kind, rest) = payload.split_first()?;
    let params = if let Some(rest) = rest.strip_prefix(b";") {
        std::str::from_utf8(rest).unwrap_or("")
    } else {
        ""
    };
    Some(Osc133Command { kind, params })
}

impl Osc133Parser {
    pub fn new() -> Self {
        Osc133Parser {
            partial: PartialBuf::new(4096, "OSC 133"),
        }
    }

    /// Scan data for OSC 133 sequences, updating shell_state.
    /// If a command completion (OSC 133;D) is detected, `last_command_duration` is set
    /// with the elapsed time since the command started (OSC 133;C).
    pub fn scan(
        &mut self,
        data: &[u8],
        shell_state: &mut ShellState,
        last_command_duration: &mut Option<std::time::Duration>,
    ) {
        let mut tmp = Vec::new();
        let data = self.partial.prepend_to(data, &mut tmp);

        let result = scan_osc(data, b"133");

        for (_offset, payload) in &result.sequences {
            // payload is everything between "133;" and ST.
            // First byte is the command (A/B/C/D), optionally followed by ";<params>".
            let Some(command) = parse_command(payload) else {
                continue;
            };

            match command.kind {
                b'A' => {
                    shell_state.zone = SemanticZone::Prompt;
                    shell_state.prompt_line = Some(0); // exact line resolved at snapshot time
                    shell_state.command_start = None;
                    log::debug!("OSC 133;A prompt start");
                }
                b'B' => {
                    shell_state.zone = SemanticZone::Input;
                    log::debug!("OSC 133;B command input");
                }
                b'C' => {
                    shell_state.zone = SemanticZone::Output;
                    shell_state.output_line = Some(0);
                    shell_state.command_start = Some(std::time::Instant::now());
                    log::debug!("OSC 133;C command output");
                }
                b'D' => {
                    shell_state.zone = SemanticZone::Prompt;
                    let exit_code = command.params.trim().parse::<i32>().ok();
                    shell_state.last_exit_code = exit_code;
                    if let Some(start) = shell_state.command_start.take() {
                        *last_command_duration = Some(start.elapsed());
                    }
                    log::debug!("OSC 133;D command done, exit={exit_code:?}");
                }
                _ => {
                    log::trace!("OSC 133;{} unknown subcommand", command.kind as char);
                }
            }
        }

        if let Some(partial_start) = result.partial_start {
            self.partial.store(&data[partial_start..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::SemanticZone;

    fn shell_state() -> ShellState {
        ShellState {
            zone: SemanticZone::Prompt,
            last_exit_code: None,
            prompt_line: None,
            output_line: None,
            command_start: None,
        }
    }

    #[test]
    fn fragmented_sequence_updates_zone_and_exit_code() {
        let mut parser = Osc133Parser::new();
        let mut shell_state = shell_state();
        let mut duration = None;

        parser.scan(b"prefix\x1b]133;C", &mut shell_state, &mut duration);
        assert_eq!(shell_state.zone, SemanticZone::Prompt);
        parser.scan(b"\x07suffix", &mut shell_state, &mut duration);
        assert_eq!(shell_state.zone, SemanticZone::Output);

        parser.scan(b"\x1b]133;D;7\x07", &mut shell_state, &mut duration);
        assert_eq!(shell_state.zone, SemanticZone::Prompt);
        assert_eq!(shell_state.last_exit_code, Some(7));
        assert!(duration.is_some());
    }

    #[test]
    fn oversized_partial_is_discarded() {
        let mut parser = Osc133Parser::new();
        let mut shell_state = shell_state();
        let mut duration = None;
        let mut data = vec![0x1b, b']', b'1', b'3', b'3', b';'];
        data.extend(std::iter::repeat_n(b'a', 4097)); // exceeds 4096 limit

        parser.scan(&data, &mut shell_state, &mut duration);
        assert!(parser.partial.is_empty());
        assert_eq!(shell_state.zone, SemanticZone::Prompt);
        assert!(duration.is_none());
    }
}
