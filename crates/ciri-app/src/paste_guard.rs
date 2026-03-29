/// Paste protection: warn when pasted content exceeds a size threshold.

/// Returns `Some(PasteInfo)` if the paste exceeds the threshold and needs
/// user confirmation. Returns `None` if the paste is small enough to proceed.
/// A threshold of 0 disables the check entirely.
pub fn check_paste_size(text: &str, threshold: usize) -> Option<PasteInfo> {
    if threshold == 0 || text.len() <= threshold {
        return None;
    }
    Some(PasteInfo {
        text: text.to_string(),
        size: text.len(),
        line_count: text.lines().count(),
    })
}

#[derive(Debug, Clone)]
pub struct PasteInfo {
    pub text: String,
    pub size: usize,
    pub line_count: usize,
}

/// Format a byte size for display (e.g. "1.2 KB", "3.4 MB").
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_threshold_is_none() {
        assert!(check_paste_size("hello", 100).is_none());
    }

    #[test]
    fn at_threshold_is_none() {
        let text = "a".repeat(100);
        assert!(check_paste_size(&text, 100).is_none());
    }

    #[test]
    fn above_threshold_returns_info() {
        let text = "a".repeat(101);
        let info = check_paste_size(&text, 100).unwrap();
        assert_eq!(info.size, 101);
    }

    #[test]
    fn zero_threshold_disables_check() {
        let text = "a".repeat(10000);
        assert!(check_paste_size(&text, 0).is_none());
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn format_size_kb() {
        assert_eq!(format_size(5000), "4.9 KB");
    }

    #[test]
    fn format_size_mb() {
        assert_eq!(format_size(2_000_000), "1.9 MB");
    }
}
