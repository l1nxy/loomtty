//! UI text shaper — used only by UI chrome (palette, tab bar, status bar, …),
//! never by the terminal grid. Lets UI fonts be proportional (Inter, SF Pro,
//! Segoe UI) while the terminal keeps its monospaced grid.
//!
//! Platform backends:
//!   - Linux / Windows: `rustybuzz` (HarfBuzz port).
//!   - macOS: CoreText via `core_text::font::CTFont::get_glyphs_for_characters`
//!     and advances from `CTFontGetAdvancesForGlyphs`.
//!
//! A small LRU keeps the last N shaped runs so painting the same label each
//! frame doesn't re-shape. UI text has very high repeat rate.
//!
//! What callers get back (`UiShapedGlyph`):
//!   - `glyph_id` + `font_id` suitable for `GlyphCache::ensure_glyph_id`,
//!   - `x_advance` (pixels) for pen advance,
//!   - `cluster` (byte offset into the original `&str`) for hit-test /
//!     truncation decisions,
//!   - `x_offset` / `y_offset` (pixels) — rustybuzz's positioning output.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::font_resolver::{FontResolver, ResolvedFont};

#[cfg(target_os = "macos")]
use core_text::font::CTFont;

/// Resolve a UI font family name to a (font file path, face index, fontdb ID).
/// Used by the app startup path to hand `ui_font_path` / `ui_font_id` into
/// both `UiTextShaper::new` and `FontInitParams`. Returns `None` if the
/// family can't be found on the system.
///
/// When `family` is empty, queries the platform's default sans-serif font
/// via `fontdb::Family::SansSerif` (fontconfig `sans-serif` on Linux,
/// system default on macOS/Windows).
pub fn resolve_ui_font(family: &str) -> Option<(String, u32, fontdb::ID)> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let families: Vec<fontdb::Family<'_>> = if family.is_empty() {
        vec![fontdb::Family::SansSerif]
    } else {
        vec![fontdb::Family::Name(family), fontdb::Family::SansSerif]
    };

    let query = fontdb::Query {
        families: &families,
        ..Default::default()
    };
    let id = db.query(&query)?;
    let face = db.face(id)?;
    match &face.source {
        fontdb::Source::File(path) => Some((path.to_string_lossy().to_string(), face.index, id)),
        _ => None,
    }
}

/// Shaped glyph produced by [`UiTextShaper::shape`].
#[derive(Debug, Clone, Copy)]
pub struct UiShapedGlyph {
    pub glyph_id: u32,
    pub font_id: fontdb::ID,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    /// Byte offset into the original `&str` this glyph starts at. Used for
    /// hit-testing and prefix truncation.
    pub cluster: u32,
}

/// Owned font data with pre-parsed metrics. The rustybuzz `Face` is rebuilt
/// on each `shape()` call via `borrow_face()` — this only re-indexes table
/// pointers (no allocation or IO), since the byte data is already validated.
#[cfg(not(target_os = "macos"))]
struct UiFace {
    data: Arc<Vec<u8>>,
    index: u32,
    units_per_em: f32,
    ascent: f32,
    descent: f32,
    line_gap: f32,
}

#[cfg(not(target_os = "macos"))]
impl UiFace {
    /// Create from already-loaded font data (shared via Arc — no disk IO).
    fn from_arc(data: Arc<Vec<u8>>, index: u32) -> Option<Self> {
        let face = rustybuzz::Face::from_slice(&data, index)?;
        Some(UiFace {
            units_per_em: face.units_per_em() as f32,
            ascent: face.ascender() as f32,
            descent: face.descender() as f32,
            line_gap: face.line_gap() as f32,
            data,
            index,
        })
    }

    /// Create by reading a font file from disk. Used only for UI font
    /// overrides whose data isn't in the terminal shaper.
    fn from_path(path: &str, index: u32) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        Self::from_arc(Arc::new(data), index)
    }

    fn borrow_face(&self) -> rustybuzz::Face<'_> {
        rustybuzz::Face::from_slice(&self.data, self.index)
            .expect("UiFace data parsed once in new(), should reparse")
    }
}

/// Small fixed-size LRU used by [`UiTextShaper`] to cache shape results.
///
/// NOTE: The cache key is the text string only, not (text, font_size, dpi).
/// This is safe because the entire `UiTextShaper` is rebuilt on DPI/config
/// changes, which creates a fresh `ShapeLru`. Do not add a `set_pixel_size()`
/// method without also invalidating the cache.
struct ShapeLru {
    cap: usize,
    entries: VecDeque<(String, Vec<UiShapedGlyph>)>,
}

impl ShapeLru {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            entries: VecDeque::with_capacity(cap),
        }
    }

    fn get(&mut self, key: &str) -> Option<&Vec<UiShapedGlyph>> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        // Promote to back (MRU) by rotating.
        if pos < self.entries.len() - 1 {
            let item = self.entries.remove(pos).unwrap();
            self.entries.push_back(item);
        }
        self.entries.back().map(|(_, v)| v)
    }

    fn put(&mut self, key: String, value: Vec<UiShapedGlyph>) {
        // Check for existing entry.
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == &key) {
            self.entries.remove(pos);
        } else if self.entries.len() == self.cap {
            self.entries.pop_front();
        }
        self.entries.push_back((key, value));
    }
}

/// Font data source — either shared from the terminal shaper or a file path
/// for UI font overrides.
pub enum UiFontData {
    /// Shared data already loaded by the terminal shaper (preferred — zero IO).
    Shared(Arc<Vec<u8>>, u32),
    /// File path; will be read from disk during construction.
    Path(String, u32),
}

/// Parameters for constructing a [`UiTextShaper`] with full fallback support.
pub struct UiShaperParams {
    /// Primary UI font (proportional or terminal font).
    pub primary: Option<UiFontData>,
    pub primary_id: Option<fontdb::ID>,
    /// Terminal primary font — used as last-resort fallback for characters
    /// the UI font lacks (e.g. Braille, box drawing, Nerd Font glyphs).
    pub terminal_primary: Option<UiFontData>,
    pub terminal_primary_id: Option<fontdb::ID>,
    /// CJK fallback font (from terminal shaper).
    pub cjk: Option<UiFontData>,
    pub cjk_id: Option<fontdb::ID>,
    /// Emoji fallback font (from terminal shaper).
    pub emoji: Option<UiFontData>,
    pub emoji_id: Option<fontdb::ID>,
    /// Font resolver for per-character font selection.
    pub resolver: Option<Arc<dyn FontResolver>>,
    pub pixel_size: f32,
    pub fallback_advance: f32,
    pub fallback_line_height: f32,
}

/// UI text shaper — see module-level docs.
pub struct UiTextShaper {
    #[cfg(not(target_os = "macos"))]
    face: Option<UiFace>,
    #[cfg(not(target_os = "macos"))]
    terminal_face: Option<UiFace>,
    #[cfg(not(target_os = "macos"))]
    cjk_face: Option<UiFace>,
    #[cfg(not(target_os = "macos"))]
    emoji_face: Option<UiFace>,
    #[cfg(target_os = "macos")]
    font: Option<CTFont>,
    #[cfg(target_os = "macos")]
    #[allow(dead_code)]
    units_per_em: f32,
    #[cfg(target_os = "macos")]
    ct_ascent: f32,
    #[cfg(target_os = "macos")]
    ct_descent: f32,
    #[cfg(target_os = "macos")]
    ct_leading: f32,
    font_id: Option<fontdb::ID>,
    terminal_font_id: Option<fontdb::ID>,
    cjk_font_id: Option<fontdb::ID>,
    emoji_font_id: Option<fontdb::ID>,
    resolver: Option<Arc<dyn FontResolver>>,
    pixel_size: f32,
    cache: ShapeLru,
    fallback_advance: f32,
    fallback_line_height: f32,
}

#[cfg(not(target_os = "macos"))]
fn load_ui_face(src: Option<UiFontData>) -> Option<UiFace> {
    match src? {
        UiFontData::Shared(data, idx) => UiFace::from_arc(data, idx),
        UiFontData::Path(path, idx) => UiFace::from_path(&path, idx),
    }
}

impl UiTextShaper {
    /// Capacity of the shape-result LRU. 256 is enough for the small handful
    /// of repeated UI strings we paint per frame (palette rows, tab titles,
    /// status segments) without blowing memory on rarely-seen strings.
    const CACHE_CAP: usize = 256;

    /// Build a shaper with full font fallback support.
    pub fn new(params: UiShaperParams) -> Self {
        #[cfg(not(target_os = "macos"))]
        let face = load_ui_face(params.primary);
        #[cfg(not(target_os = "macos"))]
        let terminal_face = load_ui_face(params.terminal_primary);
        #[cfg(not(target_os = "macos"))]
        let cjk_face = load_ui_face(params.cjk);
        #[cfg(not(target_os = "macos"))]
        let emoji_face = load_ui_face(params.emoji);

        #[cfg(not(target_os = "macos"))]
        {
            log::info!(
                "UiTextShaper: primary={} terminal={} cjk={} emoji={}",
                if face.is_some() { "loaded" } else { "NONE" },
                if terminal_face.is_some() {
                    "loaded"
                } else {
                    "NONE"
                },
                if cjk_face.is_some() { "loaded" } else { "NONE" },
                if emoji_face.is_some() {
                    "loaded"
                } else {
                    "NONE"
                },
            );
        }

        #[cfg(target_os = "macos")]
        let (font, upem, ct_asc, ct_desc, ct_lead) = {
            let path_idx = match &params.primary {
                Some(UiFontData::Path(p, i)) => Some((p.as_str(), *i)),
                Some(UiFontData::Shared(..)) => None, // macOS needs file path for CTFont
                None => None,
            };
            match path_idx {
                Some((path, idx)) => {
                    let f =
                        crate::shaper::load_ct_font_from_path(path, idx, params.pixel_size as f64);
                    if let Some(f) = f {
                        let upem = f.units_per_em() as f32;
                        let asc = f.ascent() as f32;
                        let desc = f.descent() as f32;
                        let lead = f.leading() as f32;
                        (Some(f), upem, asc, desc, lead)
                    } else {
                        (None, 1000.0, 0.0, 0.0, 0.0)
                    }
                }
                None => (None, 1000.0, 0.0, 0.0, 0.0),
            }
        };

        Self {
            #[cfg(not(target_os = "macos"))]
            face,
            #[cfg(not(target_os = "macos"))]
            terminal_face,
            #[cfg(not(target_os = "macos"))]
            cjk_face,
            #[cfg(not(target_os = "macos"))]
            emoji_face,
            #[cfg(target_os = "macos")]
            font,
            #[cfg(target_os = "macos")]
            units_per_em: upem,
            #[cfg(target_os = "macos")]
            ct_ascent: ct_asc,
            #[cfg(target_os = "macos")]
            ct_descent: ct_desc,
            #[cfg(target_os = "macos")]
            ct_leading: ct_lead,
            font_id: params.primary_id,
            terminal_font_id: params.terminal_primary_id,
            cjk_font_id: params.cjk_id,
            emoji_font_id: params.emoji_id,
            resolver: params.resolver,
            pixel_size: params.pixel_size,
            cache: ShapeLru::new(Self::CACHE_CAP),
            fallback_advance: params.fallback_advance,
            fallback_line_height: params.fallback_line_height,
        }
    }

    pub fn pixel_size(&self) -> f32 {
        self.pixel_size
    }

    pub fn font_id(&self) -> Option<fontdb::ID> {
        self.font_id
    }

    pub fn has_face(&self) -> bool {
        #[cfg(not(target_os = "macos"))]
        {
            self.face.is_some()
        }
        #[cfg(target_os = "macos")]
        {
            self.font.is_some()
        }
    }

    /// Total x-advance of `text` in pixels.
    pub fn measure(&mut self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        // Sum from cached result without cloning.
        if let Some(cached) = self.cache.get(text) {
            return cached.iter().map(|g| g.x_advance).sum();
        }
        let shaped = self.shape_uncached(text);
        let sum: f32 = shaped.iter().map(|g| g.x_advance).sum();
        self.cache.put(text.to_string(), shaped);
        sum
    }

    /// Shape `text`. Returns cached result when possible.
    pub fn shape(&mut self, text: &str) -> Vec<UiShapedGlyph> {
        if text.is_empty() {
            return Vec::new();
        }
        if let Some(cached) = self.cache.get(text) {
            return cached.clone();
        }
        let shaped = self.shape_uncached(text);
        self.cache.put(text.to_string(), shaped.clone());
        shaped
    }

    fn shape_uncached(&self, text: &str) -> Vec<UiShapedGlyph> {
        #[cfg(not(target_os = "macos"))]
        {
            if self.face.is_some() && self.font_id.is_some() {
                return self.shape_rustybuzz_segmented(text);
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(font) = self.font.as_ref()
                && let Some(fid) = self.font_id
            {
                return self.shape_coretext(font, fid, text);
            }
        }
        self.shape_fallback(text)
    }

    /// Fallback when no face is loaded: pretend each char takes
    /// `fallback_advance * unicode_width(ch)`. Glyph id is 0 so callers
    /// emit nothing from the atlas; they'll still advance the pen correctly.
    fn shape_fallback(&self, text: &str) -> Vec<UiShapedGlyph> {
        use unicode_width::UnicodeWidthChar;
        let mut out = Vec::with_capacity(text.len());
        let mut byte = 0usize;
        for ch in text.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0).max(1) as f32;
            out.push(UiShapedGlyph {
                glyph_id: 0,
                font_id: fontdb::ID::dummy(),
                x_advance: self.fallback_advance * w,
                x_offset: 0.0,
                y_offset: 0.0,
                cluster: byte as u32,
            });
            byte += ch.len_utf8();
        }
        out
    }

    /// Shape text by segmenting into grapheme-cluster-aligned runs per the
    /// font resolver, then shaping each run with the appropriate face.
    ///
    /// Segmentation uses `unicode-segmentation` grapheme clusters to avoid
    /// splitting combining characters, ZWJ emoji sequences, or variation
    /// selectors across font runs.
    #[cfg(not(target_os = "macos"))]
    fn shape_rustybuzz_segmented(&self, text: &str) -> Vec<UiShapedGlyph> {
        use unicode_segmentation::UnicodeSegmentation;

        let primary_face = self.face.as_ref().unwrap();
        let primary_id = self.font_id.unwrap();

        // Without a resolver, shape everything with the primary face.
        let Some(resolver) = self.resolver.as_ref() else {
            return self.shape_rustybuzz_run(primary_face, primary_id, text, 0);
        };

        // Split text into contiguous runs sharing the same resolved font.
        // Resolution is per-grapheme-cluster: we resolve the first char of
        // each cluster and keep the entire cluster (combining marks, ZWJ
        // sequences, variation selectors) in the same run.
        let mut out = Vec::new();
        let mut run_start = 0usize;
        let mut run_font = None::<ResolvedFont>;

        for (byte_off, grapheme) in text.grapheme_indices(true) {
            let first_ch = grapheme.chars().next().unwrap();
            let resolved = resolver.resolve_char(first_ch);

            if run_font.is_some() && run_font != Some(resolved) {
                let run_text = &text[run_start..byte_off];
                let (face, fid) = self.face_for_resolved(run_font.unwrap());
                out.extend(self.shape_rustybuzz_run(face, fid, run_text, run_start));
                run_start = byte_off;
            }
            run_font = Some(resolved);
        }

        // Flush last run.
        if run_start < text.len() {
            let run_text = &text[run_start..];
            let (face, fid) = self.face_for_resolved(run_font.unwrap());
            out.extend(self.shape_rustybuzz_run(face, fid, run_text, run_start));
        }

        out
    }

    /// Pick the face + font_id for a resolved font class, falling back to
    /// primary when the fallback face isn't loaded.
    #[cfg(not(target_os = "macos"))]
    fn face_for_resolved(&self, resolved: ResolvedFont) -> (&UiFace, fontdb::ID) {
        let primary_face = self.face.as_ref().unwrap();
        let primary_id = self.font_id.unwrap();
        match resolved {
            ResolvedFont::Cjk => {
                if let (Some(f), Some(id)) = (self.cjk_face.as_ref(), self.cjk_font_id) {
                    (f, id)
                } else {
                    log::warn!("UI shaper: CJK face not loaded, falling back to primary");
                    (primary_face, primary_id)
                }
            }
            ResolvedFont::Emoji => {
                if let (Some(f), Some(id)) = (self.emoji_face.as_ref(), self.emoji_font_id) {
                    (f, id)
                } else {
                    log::warn!("UI shaper: emoji face not loaded, falling back to primary");
                    (primary_face, primary_id)
                }
            }
            ResolvedFont::Primary => (primary_face, primary_id),
        }
    }

    /// Shape a single text run with a specific face. `byte_base` is added
    /// to cluster values so they're correct within the full original string.
    ///
    /// When a glyph comes back as `.notdef` (glyph_id=0) and the terminal
    /// primary face is available, we re-shape the failing character with the
    /// terminal face. This covers Braille spinners, box drawing, Nerd Font
    /// icons, and other glyphs the proportional UI font lacks.
    #[cfg(not(target_os = "macos"))]
    fn shape_rustybuzz_run(
        &self,
        face: &UiFace,
        font_id: fontdb::ID,
        text: &str,
        byte_base: usize,
    ) -> Vec<UiShapedGlyph> {
        let ui_features = [
            rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"kern"), 1, ..),
            rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"liga"), 1, ..),
            rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"calt"), 1, ..),
        ];
        let rb_face = face.borrow_face();
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        let output = rustybuzz::shape(&rb_face, &ui_features, buffer);
        let infos = output.glyph_infos();
        let positions = output.glyph_positions();
        let scale = self.pixel_size / face.units_per_em.max(1.0);
        let mut out = Vec::with_capacity(infos.len());
        for (info, pos) in infos.iter().zip(positions.iter()) {
            let mut gid = info.glyph_id;
            let mut fid = font_id;
            let mut adv = pos.x_advance as f32 * scale;
            let mut xoff = pos.x_offset as f32 * scale;
            let mut yoff = pos.y_offset as f32 * scale;

            // .notdef → try terminal primary face as last-resort fallback.
            if gid == 0 {
                let cluster_byte = info.cluster as usize;
                if let (Some(tf), Some(tid)) = (self.terminal_face.as_ref(), self.terminal_font_id)
                {
                    if let Some(ch) = text[cluster_byte..].chars().next() {
                        let mut buf2 = rustybuzz::UnicodeBuffer::new();
                        buf2.push_str(&ch.to_string());
                        let tf_face = tf.borrow_face();
                        let out2 = rustybuzz::shape(&tf_face, &[], buf2);
                        if let (Some(i2), Some(p2)) =
                            (out2.glyph_infos().first(), out2.glyph_positions().first())
                        {
                            if i2.glyph_id != 0 {
                                let tscale = self.pixel_size / tf.units_per_em.max(1.0);
                                gid = i2.glyph_id;
                                fid = tid;
                                adv = p2.x_advance as f32 * tscale;
                                xoff = p2.x_offset as f32 * tscale;
                                yoff = p2.y_offset as f32 * tscale;
                            }
                        }
                    }
                }
            }

            out.push(UiShapedGlyph {
                glyph_id: gid,
                font_id: fid,
                x_advance: adv,
                x_offset: xoff,
                y_offset: yoff,
                cluster: info.cluster + byte_base as u32,
            });
        }
        out
    }

    /// Shape text via CoreText. CoreText's `CTLine` performs automatic font
    /// fallback — when the primary font lacks a glyph, it substitutes a
    /// system font. We extract the actual font used per `CTRun` and map it
    /// back to primary/CJK/emoji `font_id` so the glyph cache rasterizes
    /// each glyph with the correct face.
    #[cfg(target_os = "macos")]
    fn shape_coretext(&self, font: &CTFont, font_id: fontdb::ID, text: &str) -> Vec<UiShapedGlyph> {
        use core_foundation::attributed_string::CFMutableAttributedString;
        use core_foundation::base::{CFRange, TCFType};
        use core_foundation::string::CFString;
        use core_text::font::CTFont as CTFontType;
        use core_text::line::CTLine;
        use core_text::string_attributes::kCTFontAttributeName;

        if text.is_empty() {
            return Vec::new();
        }
        let cf_string = CFString::new(text);
        let mut attr_string = CFMutableAttributedString::new();
        attr_string.replace_str(&cf_string, CFRange::init(0, 0));
        let len = attr_string.char_len();
        unsafe {
            attr_string.set_attribute(CFRange::init(0, len), kCTFontAttributeName, font);
        }
        let line = CTLine::new_with_attributed_string(attr_string.as_concrete_TypeRef());

        // UTF-16 index → UTF-8 byte offset map.
        let mut utf16_to_byte: Vec<usize> = Vec::with_capacity(text.len());
        for (b, c) in text.char_indices() {
            for _ in 0..c.len_utf16() {
                utf16_to_byte.push(b);
            }
        }

        // Cache the primary font's PostScript name for comparison.
        let primary_ps = font.postscript_name();

        let mut out = Vec::new();
        for run in line.glyph_runs().iter() {
            let glyphs = run.glyphs();
            let positions = run.positions();
            let indices = run.string_indices();
            let n = glyphs.len();

            // Determine which font_id this run uses by inspecting the run's
            // font attribute. CoreText may have substituted a fallback font.
            let run_font_id = self.classify_ct_run_font(&run, &primary_ps, font_id);

            let run_total_w = unsafe {
                let r =
                    core_foundation::base::CFRange::init(0, n as core_foundation::base::CFIndex);
                CTRunGetTypographicBounds(
                    run.as_concrete_TypeRef(),
                    r,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) as f32
            };
            for i in 0..n {
                let idx16 = indices[i] as usize;
                let advance = if i + 1 < n {
                    positions[i + 1].x as f32 - positions[i].x as f32
                } else {
                    run_total_w - positions[i].x as f32
                };
                let cluster = utf16_to_byte.get(idx16).copied().unwrap_or(text.len());
                out.push(UiShapedGlyph {
                    glyph_id: glyphs[i] as u32,
                    font_id: run_font_id,
                    x_advance: advance,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    cluster: cluster as u32,
                });
            }
        }
        out
    }

    /// Classify a CoreText glyph run's actual font against known font IDs.
    /// CoreText may substitute the primary font with a system fallback for
    /// CJK/emoji; we detect this by extracting the run's font attribute.
    #[cfg(target_os = "macos")]
    fn classify_ct_run_font(
        &self,
        run: &core_text::run::CTRun,
        primary_ps: &str,
        primary_font_id: fontdb::ID,
    ) -> fontdb::ID {
        use core_foundation::base::TCFType;
        use core_foundation::dictionary::CFDictionaryRef;
        use core_foundation::string::CFString;
        use core_text::font::CTFont as CTFontType;
        use core_text::string_attributes::kCTFontAttributeName;

        unsafe {
            let attrs: CFDictionaryRef = CTRunGetAttributes(run.as_concrete_TypeRef());
            if attrs.is_null() {
                return primary_font_id;
            }
            let font_key = kCTFontAttributeName as *const std::ffi::c_void;
            let mut font_val: *const std::ffi::c_void = std::ptr::null();
            if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
                attrs,
                font_key,
                &mut font_val,
            ) == 0
                || font_val.is_null()
            {
                return primary_font_id;
            }

            // font_val is a CTFontRef. Get its PostScript name to classify.
            let run_font = CTFontType::wrap_under_get_rule(font_val as _);
            let run_ps = run_font.postscript_name();

            if run_ps == primary_ps {
                return primary_font_id;
            }

            // Check if it matches known CJK or emoji fonts by name heuristics.
            let run_ps_lower = run_ps.to_lowercase();
            if self.emoji_font_id.is_some()
                && (run_ps_lower.contains("emoji") || run_ps_lower.contains("color"))
            {
                return self.emoji_font_id.unwrap();
            }
            if self.cjk_font_id.is_some()
                && (run_ps_lower.contains("cjk")
                    || run_ps_lower.contains("pingfang")
                    || run_ps_lower.contains("hiragino")
                    || run_ps_lower.contains("heiti")
                    || run_ps_lower.contains("songti")
                    || run_ps_lower.contains("gothic")
                    || run_ps_lower.contains("mincho"))
            {
                return self.cjk_font_id.unwrap();
            }

            // Unknown fallback font — use primary; glyph cache may .notdef
            // but that's better than crashing.
            log::debug!(
                "UI shaper: CoreText substituted unknown font '{}', using primary font_id",
                run_ps,
            );
            primary_font_id
        }
    }

    /// Line height in pixels (ascent − descent + line gap). If no face is
    /// loaded, returns the fallback value.
    pub fn line_height(&self) -> f32 {
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(face) = self.face.as_ref() {
                let scale = self.pixel_size / face.units_per_em.max(1.0);
                return (face.ascent - face.descent + face.line_gap) * scale;
            }
        }
        #[cfg(target_os = "macos")]
        {
            if self.font.is_some() {
                return self.ct_ascent + self.ct_descent + self.ct_leading;
            }
        }
        self.fallback_line_height
    }

    /// Ascent in pixels. Used to position the baseline inside a row.
    pub fn ascent(&self) -> f32 {
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(face) = self.face.as_ref() {
                let scale = self.pixel_size / face.units_per_em.max(1.0);
                return face.ascent * scale;
            }
        }
        #[cfg(target_os = "macos")]
        {
            if self.font.is_some() {
                return self.ct_ascent;
            }
        }
        self.fallback_line_height * 0.8
    }

    /// Given a byte offset into the original text, return the pen-x position
    /// at that boundary. Used for cursor positioning inside text inputs.
    pub fn pen_x_at_byte(&mut self, text: &str, byte_offset: usize) -> f32 {
        let shaped = self.shape(text);
        let mut pen = 0.0_f32;
        for g in &shaped {
            if (g.cluster as usize) >= byte_offset {
                break;
            }
            pen += g.x_advance;
        }
        pen
    }

    /// Longest prefix (in bytes) that fits inside `max_w`. Returns the byte
    /// length of the prefix and its pixel width. Ideal for truncating labels
    /// with an ellipsis appended.
    pub fn prefix_fit(&mut self, text: &str, max_w: f32) -> (usize, f32) {
        let shaped = self.shape(text);
        let mut pen = 0.0_f32;
        let mut last_cluster = 0usize;
        for g in &shaped {
            if pen + g.x_advance > max_w {
                return (last_cluster, pen);
            }
            pen += g.x_advance;
            last_cluster = g.cluster as usize + glyph_byte_len(text, g.cluster as usize);
        }
        (text.len(), pen)
    }
}

/// Byte length of the grapheme cluster that starts at `byte` in `text`.
/// Uses `unicode-segmentation` so multi-codepoint clusters (ZWJ emoji,
/// combining marks) aren't split.
fn glyph_byte_len(text: &str, byte: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    text[byte..]
        .graphemes(true)
        .next()
        .map(|g| g.len())
        .unwrap_or(1)
}

// ─── CoreText FFI (only bits the shaper needs) ───────────────────────

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn CTRunGetTypographicBounds(
        run: core_text::run::CTRunRef,
        range: core_foundation::base::CFRange,
        ascent: *mut core_graphics::base::CGFloat,
        descent: *mut core_graphics::base::CGFloat,
        leading: *mut core_graphics::base::CGFloat,
    ) -> f64;

    fn CTRunGetAttributes(
        run: core_text::run::CTRunRef,
    ) -> core_foundation::dictionary::CFDictionaryRef;
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_shaper(fallback_advance: f32) -> UiTextShaper {
        UiTextShaper::new(UiShaperParams {
            primary: None,
            primary_id: None,
            terminal_primary: None,
            terminal_primary_id: None,
            cjk: None,
            cjk_id: None,
            emoji: None,
            emoji_id: None,
            resolver: None,
            pixel_size: 12.0,
            fallback_advance,
            fallback_line_height: 16.0,
        })
    }

    #[test]
    fn measure_empty_is_zero() {
        let mut s = test_shaper(8.0);
        assert_eq!(s.measure(""), 0.0);
    }

    #[test]
    fn fallback_measure_matches_sum_of_advances() {
        let mut s = test_shaper(8.0);
        let m = s.measure("abc");
        let shaped = s.shape("abc");
        let sum: f32 = shaped.iter().map(|g| g.x_advance).sum();
        assert!((m - sum).abs() < 0.001);
    }

    #[test]
    fn fallback_prefix_fit_respects_max_width() {
        let mut s = test_shaper(10.0);
        // 3 chars * 10 = 30 px; cap at 25 → fit 2 chars.
        let (bytes, w) = s.prefix_fit("abcd", 25.0);
        assert_eq!(bytes, 2);
        assert!((w - 20.0).abs() < 0.001);
    }

    #[test]
    fn lru_evicts_oldest() {
        let mut lru = ShapeLru::new(2);
        lru.put("a".into(), vec![]);
        lru.put("b".into(), vec![]);
        lru.put("c".into(), vec![]);
        assert!(lru.get("a").is_none());
        assert!(lru.get("b").is_some());
        assert!(lru.get("c").is_some());
    }

    #[test]
    fn lru_touches_on_get() {
        let mut lru = ShapeLru::new(2);
        lru.put("a".into(), vec![]);
        lru.put("b".into(), vec![]);
        let _ = lru.get("a");
        lru.put("c".into(), vec![]);
        assert!(lru.get("a").is_some(), "a was touched, should survive");
        assert!(lru.get("b").is_none(), "b is LRU, should be evicted");
    }
}
