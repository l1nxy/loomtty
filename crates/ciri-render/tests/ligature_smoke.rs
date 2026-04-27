//! End-to-end check that the row shaper produces substituted glyphs for a
//! known programming-ligature font, even when the substitution uses
//! `calt`-style same-cluster pairs (Cascadia Code's `==`) instead of true
//! cluster-merging ligatures.
//!
//! Skipped when no known ligature font is installed.

use ciri_render::shaper::{ShapingOptions, TextShaper};
use std::path::Path;
use std::process::Command;

fn fontconfig_match(family: &str) -> Option<String> {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    {
        let output = Command::new("fc-match")
            .args(["--format=%{file}", family])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() || !Path::new(&path).exists() {
            return None;
        }
        Some(path)
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
    {
        let _ = family;
        None
    }
}

fn find_ligature_font() -> Option<(String, String)> {
    for family in [
        "Google Sans Code NF",
        "FiraCode Nerd Font",
        "Fira Code",
        "Cascadia Code",
        "JetBrains Mono",
    ] {
        if let Some(path) = fontconfig_match(family) {
            return Some((family.to_string(), path));
        }
    }

    let user_fonts = std::env::var("USERPROFILE")
        .map(|h| format!("{h}/AppData/Local/Microsoft/Windows/Fonts"))
        .ok();
    for c in [
        "C:/Windows/Fonts/CascadiaCode.ttf",
        "C:/Windows/Fonts/CascadiaMono.ttf",
    ] {
        if Path::new(c).exists() {
            return Some(("Cascadia Code".to_string(), c.to_string()));
        }
    }
    if let Some(user) = user_fonts {
        for (family, name) in [
            ("FiraCode Nerd Font", "FiraCodeNerdFont-Regular.ttf"),
            ("Google Sans Code NF", "GoogleSansCodeNF-Regular.ttf"),
            ("Cascadia Code", "CascadiaCode.ttf"),
        ] {
            let p = format!("{user}/{name}");
            if Path::new(&p).exists() {
                return Some((family.to_string(), p));
            }
        }
    }

    None
}

#[test]
fn run_shape_substitutes_double_equals_glyphs() {
    let Some((family, p)) = find_ligature_font() else {
        eprintln!("no ligature font found, skipping");
        return;
    };
    eprintln!("font: {family} -> {p}");

    let data = std::fs::read(&p).unwrap();
    let bare_eq: u32 = ttf_parser::Face::parse(&data, 0)
        .unwrap()
        .glyph_index('=')
        .map(|g| g.0 as u32)
        .unwrap_or(0);

    // Drive the actual ciri shaper over the family this file belongs to.
    // We try a few known family names — whichever resolves first.
    let shaper = TextShaper::with_options(&family, &ShapingOptions::default());
    let Some(font_id) = shaper.primary_font_id() else {
        eprintln!("ciri shaper couldn't load Cascadia Code; skipping");
        return;
    };

    #[cfg(not(target_os = "macos"))]
    {
        let face_set = shaper.face_set().expect("face_set");
        let shaped = shaper.shape_run_with_face("==", face_set.primary, font_id);
        eprintln!("shape_run_with_face('=='): {} entries", shaped.len());
        for (i, g) in shaped.iter().enumerate() {
            eprintln!(
                "  [{i}] start_col={} char_count={} glyph_id={} font_id={:?}",
                g.start_col, g.char_count, g.glyph_id, g.font_id
            );
        }
        // Either we get one cluster-merging ligature glyph, or we get two
        // single-cluster glyphs whose IDs differ from `bare_eq`. In both
        // cases the cell renderer must use the *shaped* glyph IDs — and
        // that means at least one entry's glyph_id must differ from the
        // bare cmap mapping of '='.
        assert!(!shaped.is_empty(), "shaper produced zero glyphs for '=='");
        let any_substituted = shaped.iter().any(|g| g.glyph_id != bare_eq);
        assert!(
            any_substituted,
            "no shaped glyph differed from bare '=' cmap glyph {bare_eq}; ligature wiring is broken"
        );
    }
    let _ = (font_id, bare_eq);
}

fn assert_run_substitutes_input_glyph(family: &str, font_path: &str, text: &str, bare_ch: char) {
    let data = std::fs::read(font_path).unwrap();
    let bare_glyph: u32 = ttf_parser::Face::parse(&data, 0)
        .unwrap()
        .glyph_index(bare_ch)
        .map(|g| g.0 as u32)
        .unwrap_or(0);

    let shaper = TextShaper::with_options(family, &ShapingOptions::default());
    let Some(font_id) = shaper.primary_font_id() else {
        eprintln!("ciri shaper couldn't load {family}; skipping");
        return;
    };

    #[cfg(not(target_os = "macos"))]
    {
        let face_set = shaper.face_set().expect("face_set");
        let shaped = shaper.shape_run_with_face(text, face_set.primary, font_id);
        eprintln!("shape_run_with_face('{text}'): {} entries", shaped.len());
        for (i, g) in shaped.iter().enumerate() {
            eprintln!(
                "  [{i}] start_col={} char_count={} glyph_id={} font_id={:?}",
                g.start_col, g.char_count, g.glyph_id, g.font_id
            );
        }
        assert!(
            !shaped.is_empty(),
            "shaper produced zero glyphs for '{text}'"
        );
        assert!(
            shaped.iter().any(|g| g.glyph_id != bare_glyph),
            "no shaped glyph differed from bare '{bare_ch}' cmap glyph {bare_glyph}; ligature wiring is broken"
        );
    }
    let _ = (font_id, bare_glyph);
}

#[test]
fn run_shape_substitutes_double_plus_glyphs() {
    let Some((family, p)) = find_ligature_font() else {
        eprintln!("no ligature font found, skipping");
        return;
    };
    eprintln!("font: {family} -> {p}");
    assert_run_substitutes_input_glyph(&family, &p, "++", '+');
}

#[test]
fn render_double_plus_does_not_fall_back_to_bare_plus() {
    use ciri_config::config::CiriConfig;
    use ciri_protocol::message::{CURSOR_HIDDEN, PackedCell};
    use ciri_render::glyph_cache::{FontInitParams, FontStyle, GlyphCache};
    use ciri_render::terminal::{ColorTable, PackedViewInputs, build_view_from_grid};
    use std::collections::HashMap;

    let Some((family, _)) = find_ligature_font() else {
        eprintln!("no ligature font found, skipping");
        return;
    };
    let config = CiriConfig::default();
    let shaper = TextShaper::with_options(&family, &ShapingOptions::default());
    let Some(font_id) = shaper.primary_font_id() else {
        eprintln!("ciri shaper couldn't load {family}; skipping");
        return;
    };
    let mut config = config;
    config.font.family = family.clone();
    let mut atlas = GlyphCache::new(&FontInitParams {
        font_size_pt: config.font.size,
        dpi_scale: 1.0,
        family_name: &family,
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

    let face_set = shaper.face_set().expect("face_set");
    let expected_visible_shaped = shaper
        .shape_run_with_face("++", face_set.primary, font_id)
        .into_iter()
        .filter(|g| {
            atlas
                .ensure_glyph_id(g.glyph_id, g.font_id, FontStyle::Regular, false)
                .is_some_and(|entry| entry.width > 0 && entry.height > 0)
        })
        .count();

    let cells = vec![PackedCell::with_ch('+'), PackedCell::with_ch('+')];
    let colors = ColorTable::new(&config);
    let graphemes = HashMap::new();
    let inputs = PackedViewInputs {
        cells: &cells,
        cols: 2,
        rows: 1,
        cursor_line: -1,
        cursor_col: 0,
        cursor_shape: CURSOR_HIDDEN,
        config: &config,
        shaper: &shaper,
        colors: &colors,
        grapheme_map: &graphemes,
    };
    let view = build_view_from_grid(&mut atlas, &inputs);

    assert_eq!(
        view.row_glyphs(0).len(),
        expected_visible_shaped,
        "invisible shaped glyphs must not fall back to bare '+' rendering"
    );
}

#[test]
fn render_ascii_before_cjk_stays_in_own_cells() {
    use ciri_config::config::CiriConfig;
    use ciri_protocol::message::{
        CURSOR_HIDDEN, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER, PackedCell,
    };
    use ciri_render::glyph_cache::{FontInitParams, GlyphCache};
    use ciri_render::terminal::{ColorTable, PackedViewInputs, build_view_from_grid};
    use std::collections::HashMap;

    let Some((family, _)) = find_ligature_font() else {
        eprintln!("no ligature font found, skipping");
        return;
    };
    let mut config = CiriConfig::default();
    config.font.family = family.clone();
    let shaper = TextShaper::with_options(&family, &ShapingOptions::default());
    let mut atlas = GlyphCache::new(&FontInitParams {
        font_size_pt: config.font.size,
        dpi_scale: 1.0,
        family_name: &family,
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

    let mut zhong = PackedCell::with_ch('中');
    zhong.flags = FLAG_WIDE_CHAR.to_le_bytes();
    let mut wen = PackedCell::with_ch('文');
    wen.flags = FLAG_WIDE_CHAR.to_le_bytes();
    let mut spacer = PackedCell::default();
    spacer.flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
    let cells = vec![
        PackedCell::with_ch('a'),
        PackedCell::with_ch('b'),
        PackedCell::with_ch('c'),
        zhong,
        spacer,
        wen,
        spacer,
    ];
    let colors = ColorTable::new(&config);
    let graphemes = HashMap::new();
    let inputs = PackedViewInputs {
        cells: &cells,
        cols: cells.len() as u16,
        rows: 1,
        cursor_line: -1,
        cursor_col: 0,
        cursor_shape: CURSOR_HIDDEN,
        config: &config,
        shaper: &shaper,
        colors: &colors,
        grapheme_map: &graphemes,
    };
    let view = build_view_from_grid(&mut atlas, &inputs);
    let glyphs = view.row_glyphs(0);

    assert!(
        glyphs.len() >= 5,
        "mixed ASCII+CJK row should render all visible characters"
    );
    for (idx, glyph) in glyphs.iter().take(3).enumerate() {
        let left = idx as f32 * atlas.cell_width;
        let right = left + atlas.cell_width;
        assert!(
            glyph.px >= left - 1.0 && glyph.px < right,
            "ASCII glyph {idx} should stay in its own cell: px={} cell=[{}, {})",
            glyph.px,
            left,
            right
        );
    }
}

#[test]
fn render_cjk_before_ascii_keeps_cjk_double_cell_size() {
    use ciri_config::config::CiriConfig;
    use ciri_protocol::message::{
        CURSOR_HIDDEN, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER, PackedCell,
    };
    use ciri_render::glyph_cache::{FontInitParams, GlyphCache};
    use ciri_render::terminal::{ColorTable, PackedViewInputs, build_view_from_grid};
    use std::collections::HashMap;

    let Some((family, _)) = find_ligature_font() else {
        eprintln!("no ligature font found, skipping");
        return;
    };
    let mut config = CiriConfig::default();
    config.font.family = family.clone();
    let shaper = TextShaper::with_options(&family, &ShapingOptions::default());
    let mut atlas = GlyphCache::new(&FontInitParams {
        font_size_pt: config.font.size,
        dpi_scale: 1.0,
        family_name: &family,
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

    let mut zhong = PackedCell::with_ch('中');
    zhong.flags = FLAG_WIDE_CHAR.to_le_bytes();
    let mut wen = PackedCell::with_ch('文');
    wen.flags = FLAG_WIDE_CHAR.to_le_bytes();
    let mut spacer = PackedCell::default();
    spacer.flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
    let cells = vec![
        zhong,
        spacer,
        wen,
        spacer,
        PackedCell::with_ch('a'),
        PackedCell::with_ch('b'),
        PackedCell::with_ch('c'),
    ];
    let colors = ColorTable::new(&config);
    let graphemes = HashMap::new();
    let inputs = PackedViewInputs {
        cells: &cells,
        cols: cells.len() as u16,
        rows: 1,
        cursor_line: -1,
        cursor_col: 0,
        cursor_shape: CURSOR_HIDDEN,
        config: &config,
        shaper: &shaper,
        colors: &colors,
        grapheme_map: &graphemes,
    };
    let view = build_view_from_grid(&mut atlas, &inputs);
    let glyphs = view.row_glyphs(0);

    assert!(
        glyphs[0].glyph_w > atlas.cell_width,
        "first CJK glyph should occupy more than one narrow cell"
    );
    assert!(
        glyphs[1].glyph_w > atlas.cell_width,
        "second CJK glyph should occupy more than one narrow cell"
    );
}

#[test]
fn cjk_glyph_metrics_do_not_change_when_ascii_follows() {
    use ciri_config::config::CiriConfig;
    use ciri_protocol::message::{
        CURSOR_HIDDEN, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER, PackedCell,
    };
    use ciri_render::glyph_cache::{FontInitParams, GlyphCache};
    use ciri_render::terminal::{ColorTable, PackedViewInputs, build_view_from_grid};
    use std::collections::HashMap;

    let Some((family, _)) = find_ligature_font() else {
        eprintln!("no ligature font found, skipping");
        return;
    };
    let mut config = CiriConfig::default();
    config.font.family = family.clone();
    let shaper = TextShaper::with_options(&family, &ShapingOptions::default());
    let mut atlas = GlyphCache::new(&FontInitParams {
        font_size_pt: config.font.size,
        dpi_scale: 1.0,
        family_name: &family,
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

    let mut zhong = PackedCell::with_ch('中');
    zhong.flags = FLAG_WIDE_CHAR.to_le_bytes();
    let mut wen = PackedCell::with_ch('文');
    wen.flags = FLAG_WIDE_CHAR.to_le_bytes();
    let mut spacer = PackedCell::default();
    spacer.flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();

    let row_without_ascii = vec![
        zhong,
        spacer,
        wen,
        spacer,
        PackedCell::default(),
        PackedCell::default(),
        PackedCell::default(),
    ];
    let row_with_ascii = vec![
        zhong,
        spacer,
        wen,
        spacer,
        PackedCell::with_ch('b'),
        PackedCell::with_ch('a'),
        PackedCell::with_ch('c'),
    ];
    let cells = [row_without_ascii, row_with_ascii].concat();
    let colors = ColorTable::new(&config);
    let graphemes = HashMap::new();
    let inputs = PackedViewInputs {
        cells: &cells,
        cols: 7,
        rows: 2,
        cursor_line: -1,
        cursor_col: 0,
        cursor_shape: CURSOR_HIDDEN,
        config: &config,
        shaper: &shaper,
        colors: &colors,
        grapheme_map: &graphemes,
    };
    let view = build_view_from_grid(&mut atlas, &inputs);
    let without_ascii = view.row_glyphs(0);
    let with_ascii = view.row_glyphs(1);

    assert!(without_ascii.len() >= 2);
    assert!(with_ascii.len() >= 2);
    for i in 0..2 {
        assert_eq!(without_ascii[i].px, with_ascii[i].px);
        assert_eq!(without_ascii[i].glyph_w, with_ascii[i].glyph_w);
        assert_eq!(without_ascii[i].glyph_h, with_ascii[i].glyph_h);
    }
}
