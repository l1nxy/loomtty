//! Probe what glyph metrics the box-drawing horizontal line gets — both via
//! cmap and via the ciri shaper. The user reports that `─────────` shows
//! visible bright seams at cell boundaries, suggesting either glyph overlap
//! or sub-pixel mis-alignment.

use std::path::Path;

#[test]
fn inspect_box_drawing_metrics() {
    let path = "C:/Windows/Fonts/CascadiaCode.ttf";
    if !Path::new(path).exists() {
        eprintln!("Cascadia Code not found, skipping");
        return;
    }
    let data = std::fs::read(path).unwrap();
    let face = ttf_parser::Face::parse(&data, 0).unwrap();
    let units_per_em = face.units_per_em();

    let cmap_dash: u16 = face.glyph_index('\u{2500}').map(|g| g.0).unwrap_or(0);
    let cmap_eq: u16 = face.glyph_index('=').map(|g| g.0).unwrap_or(0);

    let dash_advance = face.glyph_hor_advance(ttf_parser::GlyphId(cmap_dash)).unwrap_or(0);
    let dash_lsb = face.glyph_hor_side_bearing(ttf_parser::GlyphId(cmap_dash)).unwrap_or(0);
    let dash_bbox = face.glyph_bounding_box(ttf_parser::GlyphId(cmap_dash));
    let eq_advance = face.glyph_hor_advance(ttf_parser::GlyphId(cmap_eq)).unwrap_or(0);
    let eq_bbox = face.glyph_bounding_box(ttf_parser::GlyphId(cmap_eq));

    eprintln!("units_per_em={units_per_em}");
    eprintln!(
        "─ (U+2500): glyph={cmap_dash} advance={dash_advance} lsb={dash_lsb} bbox={dash_bbox:?}"
    );
    eprintln!(
        "= (U+003D): glyph={cmap_eq} advance={eq_advance} bbox={eq_bbox:?}"
    );
}
