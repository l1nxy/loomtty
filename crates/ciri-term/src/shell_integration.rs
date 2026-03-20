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
    pub fn scan(&mut self, data: &[u8], shell_state: &mut ShellState) {
        // If we have a partial OSC from a previous read, prepend it
        let working_data;
        let data = if !self.partial.is_empty() {
            self.partial.extend_from_slice(data);
            working_data = std::mem::take(&mut self.partial);
            &working_data[..]
        } else {
            data
        };

        let mut i = 0;
        while i + 6 < data.len() {
            // Look for ESC ] 1 3 3 ;
            if data[i] == 0x1b
                && data[i + 1] == b']'
                && data[i + 2] == b'1'
                && data[i + 3] == b'3'
                && data[i + 4] == b'3'
                && data[i + 5] == b';'
            {
                let cmd = data[i + 6];
                // Find the string terminator and collect params
                let mut end = i + 7;
                let mut params = String::new();
                let mut found_terminator = false;
                while end < data.len() {
                    if data[end] == 0x07 {
                        found_terminator = true;
                        break;
                    }
                    if data[end] == 0x1b && data.get(end + 1) == Some(&b'\\') {
                        found_terminator = true;
                        break;
                    }
                    if data[end] == b';' && params.is_empty() {
                        let rest_start = end + 1;
                        let mut rest_end = rest_start;
                        while rest_end < data.len() {
                            if data[rest_end] == 0x07
                                || (data[rest_end] == 0x1b
                                    && data.get(rest_end + 1) == Some(&b'\\'))
                            {
                                break;
                            }
                            rest_end += 1;
                        }
                        if rest_end < data.len() {
                            params = String::from_utf8_lossy(&data[rest_start..rest_end])
                                .to_string();
                            end = rest_end;
                            found_terminator = true;
                        } else {
                            end = rest_end;
                        }
                        break;
                    }
                    end += 1;
                }

                if !found_terminator {
                    // Incomplete sequence — buffer from the OSC start for next read
                    let partial = &data[i..];
                    if partial.len() > MAX_OSC_PARTIAL_SIZE {
                        log::warn!(
                            "OSC 133 partial buffer exceeded {}B limit, discarding",
                            MAX_OSC_PARTIAL_SIZE
                        );
                        self.partial.clear();
                    } else {
                        self.partial = partial.to_vec();
                    }
                    return;
                }

                match cmd {
                    b'A' => {
                        shell_state.zone = SemanticZone::Prompt;
                        shell_state.prompt_line = Some(0); // exact line resolved at snapshot time
                        log::debug!("OSC 133;A prompt start");
                    }
                    b'B' => {
                        shell_state.zone = SemanticZone::Input;
                        log::debug!("OSC 133;B command input");
                    }
                    b'C' => {
                        shell_state.zone = SemanticZone::Output;
                        shell_state.output_line = Some(0);
                        log::debug!("OSC 133;C command output");
                    }
                    b'D' => {
                        shell_state.zone = SemanticZone::Prompt;
                        let exit_code = params.trim().parse::<i32>().ok();
                        shell_state.last_exit_code = exit_code;
                        log::debug!("OSC 133;D command done, exit={exit_code:?}");
                    }
                    _ => {
                        log::trace!("OSC 133;{} unknown subcommand", cmd as char);
                    }
                }
                i = end + 1;
                if i < data.len() && data[i - 1] == 0x1b {
                    i += 1; // skip the backslash in ESC \ terminator
                }
            } else {
                // Check if we're at a potential partial match at the end of data
                // (ESC at the tail that could start an OSC 133 sequence)
                if data[i] == 0x1b && i + 6 >= data.len() {
                    let partial = &data[i..];
                    if partial.len() > MAX_OSC_PARTIAL_SIZE {
                        log::warn!(
                            "OSC 133 partial buffer exceeded {}B limit, discarding",
                            MAX_OSC_PARTIAL_SIZE
                        );
                        self.partial.clear();
                    } else {
                        self.partial = partial.to_vec();
                    }
                    return;
                }
                i += 1;
            }
        }
    }
}
