/// Embedded shell integration scripts.
pub const BASH_INTEGRATION: &str = include_str!("../shell-integration/ciri.bash");
pub const ZSH_INTEGRATION: &str = include_str!("../shell-integration/ciri.zsh");
pub const FISH_INTEGRATION: &str = include_str!("../shell-integration/ciri.fish");

/// Write shell integration scripts to a temporary directory and return its path.
/// The directory is created under `$XDG_RUNTIME_DIR/ciri/shell-integration/`.
pub fn ensure_integration_dir() -> std::io::Result<std::path::PathBuf> {
    let dir = ciri_protocol::transport::runtime_dir()
        .join("ciri")
        .join("shell-integration");
    std::fs::create_dir_all(&dir)?;

    std::fs::write(dir.join("ciri.bash"), BASH_INTEGRATION)?;
    std::fs::write(dir.join("ciri.zsh"), ZSH_INTEGRATION)?;
    std::fs::write(dir.join("ciri.fish"), FISH_INTEGRATION)?;

    Ok(dir)
}
