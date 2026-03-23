use crate::esc_scanner::scan_osc;
use crate::pane::{SemanticZone, ShellState};

const MAX_OSC_PARTIAL_SIZE: usize = 4096; // OSC 133 sequences are tiny

/// Parser for OSC 133 shell integration sequences.
/// Handles sequences split across PTY read boundaries.
pub(crate) struct Osc133Parser {
    partial: Vec<u8>,
}

impl Osc133Parser {
    pub fn new() -> Self {
        Osc133Parser {
            partial: Vec::new(),
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
        // If we have a partial OSC from a previous read, prepend it
        let working_data;
        let data = if !self.partial.is_empty() {
            self.partial.extend_from_slice(data);
            working_data = std::mem::take(&mut self.partial);
            &working_data[..]
        } else {
            data
        };

        let result = scan_osc(data, b"133");

        for (_offset, payload) in &result.sequences {
            // payload is everything between "133;" and ST.
            // First byte is the command (A/B/C/D), optionally followed by ";<params>".
            if payload.is_empty() {
                continue;
            }

            let cmd = payload[0];
            let params = if payload.len() > 1 && payload[1] == b';' {
                std::str::from_utf8(&payload[2..])
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            };

            match cmd {
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
                    let exit_code = params.trim().parse::<i32>().ok();
                    shell_state.last_exit_code = exit_code;
                    if let Some(start) = shell_state.command_start.take() {
                        *last_command_duration = Some(start.elapsed());
                    }
                    log::debug!("OSC 133;D command done, exit={exit_code:?}");
                }
                _ => {
                    log::trace!("OSC 133;{} unknown subcommand", cmd as char);
                }
            }
        }

        // Handle partial buffering when the scanner reports an incomplete sequence
        if let Some(partial_start) = result.partial_start {
            let partial = &data[partial_start..];
            if partial.len() > MAX_OSC_PARTIAL_SIZE {
                log::warn!(
                    "OSC 133 partial buffer exceeded {}B limit, discarding",
                    MAX_OSC_PARTIAL_SIZE
                );
                self.partial.clear();
            } else {
                self.partial = partial.to_vec();
            }
        }
    }
}
