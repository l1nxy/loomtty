/// Embedded shell integration scripts.
pub const BASH_INTEGRATION: &str = include_str!("../shell-integration/loom.bash");
pub const ZSH_INTEGRATION: &str = include_str!("../shell-integration/loom.zsh");
pub const FISH_INTEGRATION: &str = include_str!("../shell-integration/loom.fish");

/// Write shell integration scripts to a temporary directory and return its path.
/// The directory is created under `$XDG_RUNTIME_DIR/loom/shell-integration/`.
pub fn ensure_integration_dir() -> std::io::Result<std::path::PathBuf> {
    let dir = loom_protocol::transport::runtime_dir()
        .join("loom")
        .join("shell-integration");
    std::fs::create_dir_all(&dir)?;

    std::fs::write(dir.join("loom.bash"), BASH_INTEGRATION)?;
    std::fs::write(dir.join("loom.zsh"), ZSH_INTEGRATION)?;
    std::fs::write(dir.join("loom.fish"), FISH_INTEGRATION)?;

    Ok(dir)
}
