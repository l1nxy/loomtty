//! OSC 133 shell integration parser.

use winnow::prelude::*;
use winnow::token::{any, rest};

use crate::esc_scanner::scan_osc;
use crate::pane::{SemanticZone, ShellState};
use crate::partial_buf::PartialBuf;

/// Parsed OSC 133 subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Osc133Command<'a> {
    PromptStart,
    CommandInput,
    CommandOutput,
    Done { params: &'a str },
    Unknown(u8),
}

/// Parse an OSC 133 payload: `<kind>[;<params>]`
fn parse_command<'a>(input: &mut &'a [u8]) -> winnow::error::ModalResult<Osc133Command<'a>> {
    let kind = any.parse_next(input)?;
    // Optional ";params" suffix
    let params = winnow::combinator::opt((b';', rest))
        .parse_next(input)?
        .and_then(|(_, rest)| std::str::from_utf8(rest).ok())
        .unwrap_or("");

    Ok(match kind {
        b'A' => Osc133Command::PromptStart,
        b'B' => Osc133Command::CommandInput,
        b'C' => Osc133Command::CommandOutput,
        b'D' => Osc133Command::Done { params },
        other => Osc133Command::Unknown(other),
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
            let mut input: &[u8] = payload;
            let Ok(command) = parse_command(&mut input) else {
                continue;
            };

            match command {
                Osc133Command::PromptStart => {
                    shell_state.zone = SemanticZone::Prompt;
                    shell_state.prompt_line = Some(0);
                    shell_state.command_start = None;
                    log::debug!("OSC 133;A prompt start");
                }
                Osc133Command::CommandInput => {
                    shell_state.zone = SemanticZone::Input;
                    log::debug!("OSC 133;B command input");
                }
                Osc133Command::CommandOutput => {
                    shell_state.zone = SemanticZone::Output;
                    shell_state.output_line = Some(0);
                    shell_state.command_start = Some(std::time::Instant::now());
                    log::debug!("OSC 133;C command output");
                }
                Osc133Command::Done { params } => {
                    shell_state.zone = SemanticZone::Prompt;
                    let exit_code = params.trim().parse::<i32>().ok();
                    shell_state.last_exit_code = exit_code;
                    if let Some(start) = shell_state.command_start.take() {
                        *last_command_duration = Some(start.elapsed());
                    }
                    log::debug!("OSC 133;D command done, exit={exit_code:?}");
                }
                Osc133Command::Unknown(b) => {
                    log::trace!("OSC 133;{} unknown subcommand", b as char);
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
    fn parse_command_variants() {
        let mut input: &[u8] = b"A";
        assert_eq!(
            parse_command(&mut input).unwrap(),
            Osc133Command::PromptStart
        );

        let mut input: &[u8] = b"D;42";
        assert_eq!(
            parse_command(&mut input).unwrap(),
            Osc133Command::Done { params: "42" }
        );
    }

    #[test]
    fn fragmented_sequence_updates_zone_and_exit_code() {
        let mut parser = Osc133Parser::new();
        let mut shell_state = shell_state();
        let mut duration: Option<std::time::Duration> = None;

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
        data.extend(std::iter::repeat_n(b'a', 4097));

        parser.scan(&data, &mut shell_state, &mut duration);
        assert!(parser.partial.is_empty());
        assert_eq!(shell_state.zone, SemanticZone::Prompt);
        assert!(duration.is_none());
    }
}
