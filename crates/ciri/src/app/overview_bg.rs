//! Overview-mode wallpaper loader.
//!
//! Decodes the user-configured `appearance.overview_background_image`
//! into RGBA8 bytes the GPU backends can upload directly. The texture
//! upload + fullscreen-quad draw lives in each backend (DX, GL, blade).
//!
//! Decoding runs on a worker thread spawned by
//! `App::reload_background_image` so a multi-MB wallpaper doesn't
//! stall the event loop. The worker sends its result back through a
//! channel + an `EventLoopProxy` wake; the main thread applies it via
//! `App::apply_pending_overview_bg` next event-loop iteration.

use anyhow::{Context, Result};
use ciri_config::config::expand_config_path;
use std::path::PathBuf;

/// Decoded RGBA8 image ready for `Renderer::set_overview_background_image`.
pub(crate) struct DecodedImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decode the configured wallpaper from disk.
///
/// Empty config string → `Ok(None)`. Decode errors propagate (caller
/// logs a warning so a bad path doesn't silently fall through to a
/// solid backdrop with no explanation).
pub(crate) fn load_background_image(config_value: &str) -> Result<Option<DecodedImage>> {
    if config_value.is_empty() {
        return Ok(None);
    }
    let path: PathBuf = expand_config_path(config_value);
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read overview wallpaper {}", path.display()))?;
    let dynamic = image::load_from_memory(&bytes)
        .with_context(|| format!("decode overview wallpaper {}", path.display()))?;
    let rgba = dynamic.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Some(DecodedImage {
        rgba: rgba.into_raw(),
        width,
        height,
    }))
}
