//! Reproducible font startup/rasterization workload, without a window or server.
//! Build first, then measure the binary with `/usr/bin/time -l` on macOS:
//! cargo build --release -p loom-render --example font_profile
//! /usr/bin/time -l target/release/examples/font_profile

use loom_config::config::LoomConfig;
use loom_render::glyph_cache::{FontInitParams, FontStyle, GlyphCache};
use loom_render::shaper::TextShaper;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let config = LoomConfig::default();
    let start = Instant::now();
    let shaper = TextShaper::new(&config.font.family);
    let discovery = start.elapsed();
    let mut cache = GlyphCache::new(&FontInitParams {
        font_size_pt: config.font.size,
        dpi_scale: 2.0,
        family_name: &config.font.family,
        ui_family_name: None,
        primary_font_path: shaper.primary_font_path(),
        emoji_font_path: shaper.emoji_font_path(),
        emoji_font_id: shaper.emoji_font_id(),
        cjk_font_path: shaper.cjk_font_path(),
        cjk_font_id: shaper.cjk_font_id(),
        ui_font_path: None,
        ui_font_id: None,
        ui_pixel_size: None,
        render_config: &config.render,
        font_resolver: shaper.font_resolver(),
        #[cfg(windows)]
        dwrite_resolver: shaper.dwrite_resolver(),
        cell_width_scale: None,
        cell_height_scale: None,
    });
    let init = start.elapsed();
    let faces = shaper.face_set().expect("primary font");
    let text = "fn main() { println!(\"Hello 世界 😀 🚀\"); } => !=";
    for _ in 0..1000 {
        for ch in text.chars() {
            black_box(cache.ensure_styled_char(ch, FontStyle::Regular));
            black_box(shaper.shape_char_with_fallback(ch, &faces));
        }
    }
    println!(
        "discovery_ms={:.2} init_ms={:.2} workload_ms={:.2} cell={:.1}x{:.1}",
        discovery.as_secs_f64() * 1000.0,
        init.as_secs_f64() * 1000.0,
        (start.elapsed() - init).as_secs_f64() * 1000.0,
        cache.cell_width,
        cache.cell_height,
    );
    black_box((&shaper, &cache));
}
