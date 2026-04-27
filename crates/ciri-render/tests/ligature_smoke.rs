//! End-to-end check that the row shaper produces substituted glyphs for a
//! known programming-ligature font, even when the substitution uses
//! `calt`-style same-cluster pairs (Cascadia Code's `==`) instead of true
//! cluster-merging ligatures.
//!
//! Skipped when no known ligature font is installed.

use ciri_render::shaper::{ShapingOptions, TextShaper};
use std::path::Path;

#[test]
fn run_shape_substitutes_double_equals_glyphs() {
    let user_fonts = std::env::var("USERPROFILE")
        .map(|h| format!("{h}/AppData/Local/Microsoft/Windows/Fonts"))
        .ok();
    let mut path: Option<String> = None;
    for c in [
        "C:/Windows/Fonts/CascadiaCode.ttf",
        "C:/Windows/Fonts/CascadiaMono.ttf",
    ] {
        if Path::new(c).exists() {
            path = Some(c.to_string());
            break;
        }
    }
    if path.is_none() {
        if let Some(user) = user_fonts {
            for name in [
                "FiraCodeNerdFont-Regular.ttf",
                "GoogleSansCodeNF-Regular.ttf",
                "CascadiaCode.ttf",
            ] {
                let p = format!("{user}/{name}");
                if Path::new(&p).exists() {
                    path = Some(p);
                    break;
                }
            }
        }
    }
    let Some(p) = path else {
        eprintln!("no ligature font found, skipping");
        return;
    };
    eprintln!("font: {p}");

    let data = std::fs::read(&p).unwrap();
    let bare_eq: u32 = ttf_parser::Face::parse(&data, 0)
        .unwrap()
        .glyph_index('=')
        .map(|g| g.0 as u32)
        .unwrap_or(0);

    // Drive the actual ciri shaper over the family this file belongs to.
    // We try a few known family names — whichever resolves first.
    let shaper = TextShaper::with_options("Cascadia Code", &ShapingOptions::default());
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
        assert!(
            !shaped.is_empty(),
            "shaper produced zero glyphs for '=='"
        );
        let any_substituted = shaped.iter().any(|g| g.glyph_id != bare_eq);
        assert!(
            any_substituted,
            "no shaped glyph differed from bare '=' cmap glyph {bare_eq}; ligature wiring is broken"
        );
    }
    let _ = (font_id, bare_eq);
}
