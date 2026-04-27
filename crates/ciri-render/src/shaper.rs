//! Text shaping via rustybuzz (Linux/Windows) or CoreText (macOS).

use fontdb;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(windows)]
use crate::font_resolver;
use crate::font_resolver::{CmapResolver, FontResolver, ResolvedFont};

/// Font data cached for text shaping.
struct FontData {
    data: Arc<Vec<u8>>,
    face_index: u32,
}

// ─── CachedFace (rustybuzz, non-macOS only) ───────────────────────

/// A `rustybuzz::Face` that owns its backing font data via `Arc`, allowing it
/// to live independently of the `TextShaper` borrow scope.
///
/// # Safety
/// The `Face<'static>` is created by transmuting the lifetime from the `Arc`'s
/// stable pointer. The `Arc` is kept alive as long as this struct exists, so the
/// data reference inside `Face` always points to valid memory.
#[cfg(not(target_os = "macos"))]
struct CachedFace {
    face: rustybuzz::Face<'static>,
    _data: Arc<Vec<u8>>,
}

#[cfg(not(target_os = "macos"))]
impl CachedFace {
    fn new(data: Arc<Vec<u8>>, face_index: u32) -> Option<Self> {
        let face = rustybuzz::Face::from_slice(&data, face_index)?;
        // SAFETY: `data` is an Arc that we hold onto for the lifetime of this
        // struct. The pointer is stable (heap-allocated, ref-counted), so the
        // Face's internal references remain valid.
        let face: rustybuzz::Face<'static> = unsafe { std::mem::transmute(face) };
        Some(CachedFace { face, _data: data })
    }

    fn as_face(&self) -> &rustybuzz::Face<'_> {
        &self.face
    }
}

// ─── FaceSet ──────────────────────────────────────────────────────

/// Bundle of references to cached font faces for the primary, CJK, and emoji fonts.
/// Cheap to create (just borrows from `TextShaper`'s long-lived objects).
pub struct FaceSet<'a> {
    #[cfg(not(target_os = "macos"))]
    pub primary: &'a rustybuzz::Face<'a>,
    #[cfg(target_os = "macos")]
    pub primary: &'a core_text::font::CTFont,

    pub primary_id: fontdb::ID,

    #[cfg(not(target_os = "macos"))]
    pub cjk: Option<&'a rustybuzz::Face<'a>>,
    #[cfg(target_os = "macos")]
    pub cjk: Option<&'a core_text::font::CTFont>,

    pub cjk_id: Option<fontdb::ID>,

    #[cfg(not(target_os = "macos"))]
    pub emoji: Option<&'a rustybuzz::Face<'a>>,
    #[cfg(target_os = "macos")]
    pub emoji: Option<&'a core_text::font::CTFont>,

    pub emoji_id: Option<fontdb::ID>,
}

/// Detected ligature: multiple input characters shaped into a single glyph.
#[derive(Debug, Clone)]
pub struct Ligature {
    /// Starting column (character index) in the input text.
    pub start_col: usize,
    /// Number of input characters consumed by this ligature.
    pub char_count: usize,
    /// The glyph ID produced by the shaper.
    pub glyph_id: u32,
    /// The font ID that produced this glyph.
    pub font_id: fontdb::ID,
}

/// One shaped glyph from a text run: starts at `start_col` (char index in the
/// run), spans `char_count` input characters (≥ 1), maps to `glyph_id`.
///
/// `char_count > 1` means a true cluster-merging ligature (`fi`, Arabic
/// joining, etc). `char_count == 1` may still be a substituted glyph — e.g.
/// Cascadia Code's `==` produces two single-cluster glyphs whose IDs differ
/// from the bare codepoint mapping. Either way the renderer should use the
/// shaped `glyph_id`, not the cmap glyph for the source character.
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub start_col: usize,
    pub char_count: usize,
    pub glyph_id: u32,
    pub font_id: fontdb::ID,
}

// ─── TextShaper ───────────────────────────────────────────────────

/// Text shaper using rustybuzz (Linux/Windows) or CoreText (macOS).
///
/// Owns a standalone `fontdb::Database` for font discovery and raw font byte
/// access. This replaces the previous `cosmic_text::FontSystem` dependency.
pub struct TextShaper {
    db: fontdb::Database,
    fonts: HashMap<fontdb::ID, FontData>,
    primary_font_id: Option<fontdb::ID>,
    emoji_font_id: Option<fontdb::ID>,
    cjk_font_id: Option<fontdb::ID>,

    /// Pre-built rustybuzz `Feature` list applied to terminal shaping.
    /// Empty when the user hasn't configured `font.features`. Not yet wired
    /// through the CoreText path; macOS shaping silently ignores features.
    #[cfg(not(target_os = "macos"))]
    rb_features: Vec<rustybuzz::Feature>,

    /// Long-lived cached font faces — parsed once, reused across all frames.
    #[cfg(not(target_os = "macos"))]
    primary_face: Option<CachedFace>,
    #[cfg(not(target_os = "macos"))]
    cjk_face: Option<CachedFace>,
    #[cfg(not(target_os = "macos"))]
    emoji_face: Option<CachedFace>,

    /// CoreText font objects for shaping (macOS only).
    #[cfg(target_os = "macos")]
    primary_ct_font: Option<core_text::font::CTFont>,
    #[cfg(target_os = "macos")]
    cjk_ct_font: Option<core_text::font::CTFont>,
    #[cfg(target_os = "macos")]
    emoji_ct_font: Option<core_text::font::CTFont>,

    /// Determines which font to use for each character (before shaping).
    /// Shared with GlyphCache so both paths use the same resolution logic.
    resolver: Arc<dyn FontResolver>,
    /// Direct reference to the DWrite resolver for system font face access.
    #[cfg(windows)]
    dwrite_resolver: Option<Arc<font_resolver::DWriteResolver>>,
    /// Cache: char → shaped (glyph_id, font_id). Avoids re-running shaping per char.
    char_shape_cache: RefCell<HashMap<char, Option<(u32, fontdb::ID)>>>,
    /// Cache: grapheme cluster string → shaped (glyph_id, font_id).
    grapheme_shape_cache: RefCell<HashMap<String, Option<(u32, fontdb::ID)>>>,
    /// Cache: (text run, font_id) → detected ligatures.
    ligature_cache: RefCell<HashMap<(String, fontdb::ID), Vec<Ligature>>>,
}

/// Extra hooks that callers (config) can use to influence font discovery and
/// shaping. All fields default to "preserve historical behavior".
#[derive(Debug, Clone, Default)]
pub struct ShapingOptions {
    /// Preferred OpenType weight for the primary face (e.g. 400 for Regular,
    /// 500 for Medium). When `None`, the closest-to-400 face is selected.
    pub preferred_weight: Option<u16>,
    /// Parsed OpenType feature list (`(tag, value)` pairs). Forwarded into
    /// every `rustybuzz::shape()` call used by the terminal pipeline.
    pub features: Vec<([u8; 4], u32)>,
}

impl TextShaper {
    /// Create a new TextShaper that discovers system fonts via fontdb.
    ///
    /// Finds the primary font matching `family_name` and preloads its data
    /// for shaping. Falls back to the first monospaced font if no match.
    pub fn new(family_name: &str) -> Self {
        Self::with_options(family_name, &ShapingOptions::default())
    }

    /// Create a new TextShaper with explicit `ShapingOptions` (preferred
    /// weight, OpenType features). The simpler `new()` keeps callers that
    /// don't need feature/weight overrides unchanged.
    pub fn with_options(family_name: &str, opts: &ShapingOptions) -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let primary_font_id = find_primary_font(&db, family_name, opts.preferred_weight);
        let emoji_font_id = find_emoji_font(&db);
        let cjk_font_id = find_cjk_font(&db, primary_font_id);

        // Placeholder resolver — replaced after fonts are loaded below.
        let placeholder_resolver: Arc<dyn FontResolver> =
            Arc::new(CmapResolver::new((&[], 0), None, None));

        #[cfg(not(target_os = "macos"))]
        let rb_features: Vec<rustybuzz::Feature> = opts
            .features
            .iter()
            .map(|(tag, value)| {
                rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(tag), *value, ..)
            })
            .collect();

        let mut shaper = TextShaper {
            db,
            fonts: HashMap::new(),
            primary_font_id,
            emoji_font_id,
            cjk_font_id,
            #[cfg(not(target_os = "macos"))]
            rb_features,
            #[cfg(not(target_os = "macos"))]
            primary_face: None,
            #[cfg(not(target_os = "macos"))]
            cjk_face: None,
            #[cfg(not(target_os = "macos"))]
            emoji_face: None,
            #[cfg(target_os = "macos")]
            primary_ct_font: None,
            #[cfg(target_os = "macos")]
            cjk_ct_font: None,
            #[cfg(target_os = "macos")]
            emoji_ct_font: None,
            resolver: placeholder_resolver,
            #[cfg(windows)]
            dwrite_resolver: None,
            char_shape_cache: RefCell::new(HashMap::new()),
            grapheme_shape_cache: RefCell::new(HashMap::new()),
            ligature_cache: RefCell::new(HashMap::new()),
        };

        // Preload primary font data for shaping
        if let Some(fid) = primary_font_id {
            shaper.load_font(fid);
        }
        // Preload emoji font data for fallback shaping
        if let Some(eid) = emoji_font_id
            && Some(eid) != primary_font_id
        {
            shaper.load_font(eid);
        }
        // Preload CJK font data for fallback shaping
        if let Some(cid) = cjk_font_id
            && Some(cid) != primary_font_id
            && Some(cid) != emoji_font_id
        {
            shaper.load_font(cid);
        }

        // ── Build long-lived cached faces (platform-specific) ──

        #[cfg(not(target_os = "macos"))]
        {
            shaper.primary_face = primary_font_id
                .and_then(|fid| shaper.fonts.get(&fid))
                .and_then(|fd| CachedFace::new(Arc::clone(&fd.data), fd.face_index));
            shaper.cjk_face = cjk_font_id
                .and_then(|fid| shaper.fonts.get(&fid))
                .and_then(|fd| CachedFace::new(Arc::clone(&fd.data), fd.face_index));
            shaper.emoji_face = emoji_font_id
                .and_then(|fid| shaper.fonts.get(&fid))
                .and_then(|fd| CachedFace::new(Arc::clone(&fd.data), fd.face_index));
        }

        #[cfg(target_os = "macos")]
        {
            // Create CTFont objects from font file paths for shaping.
            // Size doesn't matter for shaping (only glyph IDs), use 12.0.
            shaper.primary_ct_font = primary_font_id
                .and_then(|fid| shaper.font_path(fid))
                .and_then(|(path, index)| {
                    log::info!(
                        "CoreText discovery: loading primary CTFont from '{}' face_index={}",
                        path,
                        index
                    );
                    let ct = load_ct_font_for_shaping(&path, index);
                    if let Some(ref f) = ct {
                        log::info!(
                            "CoreText discovery: primary CTFont ps_name='{}' family='{}'",
                            f.postscript_name(),
                            f.family_name(),
                        );
                    } else {
                        log::warn!("CoreText discovery: failed to load primary CTFont");
                    }
                    ct
                });
            shaper.cjk_ct_font =
                cjk_font_id
                    .and_then(|fid| shaper.font_path(fid))
                    .and_then(|(path, index)| {
                        log::info!(
                            "CoreText discovery: loading CJK CTFont from '{}' face_index={}",
                            path,
                            index
                        );
                        let ct = load_ct_font_for_shaping(&path, index);
                        if let Some(ref f) = ct {
                            log::info!(
                                "CoreText discovery: CJK CTFont ps_name='{}' family='{}'",
                                f.postscript_name(),
                                f.family_name(),
                            );
                        } else {
                            log::warn!("CoreText discovery: failed to load CJK CTFont");
                        }
                        ct
                    });
            shaper.emoji_ct_font = emoji_font_id
                .and_then(|fid| shaper.font_path(fid))
                .and_then(|(path, index)| {
                    log::info!(
                        "CoreText discovery: loading emoji CTFont from '{}' face_index={}",
                        path,
                        index
                    );
                    let ct = load_ct_font_for_shaping(&path, index);
                    if let Some(ref f) = ct {
                        log::info!(
                            "CoreText discovery: emoji CTFont ps_name='{}' family='{}'",
                            f.postscript_name(),
                            f.family_name(),
                        );
                    } else {
                        log::warn!("CoreText discovery: failed to load emoji CTFont");
                    }
                    ct
                });
        }

        // Build font resolver from loaded font data
        let primary_fd = primary_font_id.and_then(|fid| shaper.fonts.get(&fid));
        let cjk_fd = cjk_font_id.and_then(|fid| shaper.fonts.get(&fid));
        let emoji_fd = emoji_font_id.and_then(|fid| shaper.fonts.get(&fid));

        if let Some(pfd) = primary_fd {
            let resolver = CmapResolver::new(
                (&pfd.data, pfd.face_index),
                cjk_fd.map(|fd| (fd.data.as_slice(), fd.face_index)),
                emoji_fd.map(|fd| (fd.data.as_slice(), fd.face_index)),
            );
            #[cfg(windows)]
            {
                let dw = Arc::new(font_resolver::DWriteResolver::new(resolver));
                shaper.resolver = Arc::clone(&dw) as Arc<dyn FontResolver>;
                shaper.dwrite_resolver = Some(dw);
            }
            #[cfg(not(windows))]
            {
                shaper.resolver = Arc::new(resolver);
            }
        }

        shaper
    }

    pub fn primary_font_id(&self) -> Option<fontdb::ID> {
        self.primary_font_id
    }

    pub fn emoji_font_id(&self) -> Option<fontdb::ID> {
        self.emoji_font_id
    }

    pub fn cjk_font_id(&self) -> Option<fontdb::ID> {
        self.cjk_font_id
    }

    /// Shared font resolver — can be cloned (Arc) into GlyphCache.
    pub fn font_resolver(&self) -> Arc<dyn FontResolver> {
        Arc::clone(&self.resolver)
    }

    /// DWrite resolver for system font face access (Windows only).
    #[cfg(windows)]
    pub fn dwrite_resolver(&self) -> Option<Arc<font_resolver::DWriteResolver>> {
        self.dwrite_resolver.clone()
    }

    /// Return the file path and face index of the primary font, for FreeType loading.
    pub fn primary_font_path(&self) -> Option<(String, u32)> {
        self.font_path(self.primary_font_id?)
    }

    /// Return the file path and face index of the emoji font, for FreeType loading.
    pub fn emoji_font_path(&self) -> Option<(String, u32)> {
        self.font_path(self.emoji_font_id?)
    }

    /// Return the file path and face index of the CJK font, for FreeType loading.
    pub fn cjk_font_path(&self) -> Option<(String, u32)> {
        self.font_path(self.cjk_font_id?)
    }

    fn font_path(&self, fid: fontdb::ID) -> Option<(String, u32)> {
        let face = self.db.face(fid)?;
        match &face.source {
            fontdb::Source::File(path) => Some((path.to_string_lossy().to_string(), face.index)),
            _ => None,
        }
    }

    /// Load font data for a given font ID. No-op if already loaded.
    pub fn load_font(&mut self, font_id: fontdb::ID) {
        if self.fonts.contains_key(&font_id) {
            return;
        }
        let Some(face_info) = self.db.face(font_id) else {
            return;
        };
        let face_index = face_info.index;
        let data = match &face_info.source {
            fontdb::Source::File(path) => std::fs::read(path).ok(),
            fontdb::Source::Binary(arc) => {
                let slice: &[u8] = (*arc).as_ref().as_ref();
                Some(slice.to_vec())
            }
            fontdb::Source::SharedFile(_, arc) => {
                let slice: &[u8] = (*arc).as_ref().as_ref();
                Some(slice.to_vec())
            }
        };

        if let Some(data) = data {
            log::info!(
                "loaded font data for shaping: {} bytes, face_index={}",
                data.len(),
                face_index
            );
            self.fonts.insert(
                font_id,
                FontData {
                    data: Arc::new(data),
                    face_index,
                },
            );
        }
    }

    /// Check if a font is loaded for shaping.
    pub fn has_font(&self, font_id: fontdb::ID) -> bool {
        self.fonts.contains_key(&font_id)
    }

    /// Return a shared reference to the raw font data for `id`. Used by the
    /// UI shaper to avoid re-reading font files from disk.
    pub fn font_data_arc(&self, id: fontdb::ID) -> Option<(Arc<Vec<u8>>, u32)> {
        let fd = self.fonts.get(&id)?;
        Some((Arc::clone(&fd.data), fd.face_index))
    }

    /// Create a reusable Face from cached font data (non-macOS only).
    #[cfg(not(target_os = "macos"))]
    pub fn create_face(&self, font_id: fontdb::ID) -> Option<rustybuzz::Face<'_>> {
        let font_data = self.fonts.get(&font_id)?;
        rustybuzz::Face::from_slice(&font_data.data, font_data.face_index)
    }

    // ─── face_set() ───────────────────────────────────────────────

    /// Return a [`FaceSet`] referencing the long-lived cached faces.
    /// Zero-cost: no font parsing, just borrows from `self`.
    #[cfg(not(target_os = "macos"))]
    pub fn face_set(&self) -> Option<FaceSet<'_>> {
        let cached = self.primary_face.as_ref()?;
        Some(FaceSet {
            primary: cached.as_face(),
            primary_id: self.primary_font_id?,
            cjk: self.cjk_face.as_ref().map(|f| f.as_face()),
            cjk_id: self.cjk_font_id,
            emoji: self.emoji_face.as_ref().map(|f| f.as_face()),
            emoji_id: self.emoji_font_id,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn face_set(&self) -> Option<FaceSet<'_>> {
        Some(FaceSet {
            primary: self.primary_ct_font.as_ref()?,
            primary_id: self.primary_font_id?,
            cjk: self.cjk_ct_font.as_ref(),
            cjk_id: self.cjk_font_id,
            emoji: self.emoji_ct_font.as_ref(),
            emoji_id: self.emoji_font_id,
        })
    }

    // ─── detect_ligatures_with_face ───────────────────────────────

    /// Detect ligatures using a pre-created face (avoids Face re-creation per call).
    /// Results are cached by (text, font_id) to avoid re-shaping identical runs.
    #[cfg(not(target_os = "macos"))]
    pub fn detect_ligatures_with_face(
        &self,
        text: &str,
        face: &rustybuzz::Face,
        font_id: fontdb::ID,
    ) -> Vec<Ligature> {
        let cache_key = (text.to_string(), font_id);
        if let Some(cached) = self.ligature_cache.borrow().get(&cache_key) {
            return cached.clone();
        }

        let result = self.detect_ligatures_uncached(text, face, font_id);
        self.ligature_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    #[cfg(target_os = "macos")]
    pub fn detect_ligatures_with_face(
        &self,
        text: &str,
        face: &core_text::font::CTFont,
        font_id: fontdb::ID,
    ) -> Vec<Ligature> {
        let cache_key = (text.to_string(), font_id);
        if let Some(cached) = self.ligature_cache.borrow().get(&cache_key) {
            return cached.clone();
        }

        let result = crate::shaper_coretext::ct_detect_ligatures(face, text, font_id);
        self.ligature_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    // ─── shape_run_with_face ──────────────────────────────────────

    /// Shape a whole run of text and return one `ShapedGlyph` per HarfBuzz
    /// cluster. This is the right primitive for terminal rows: it surfaces
    /// both real cluster-merging ligatures (`fi`, Arabic joining) AND
    /// `calt`-style contextual substitutions where a font replaces e.g. `==`
    /// with `equal_equal.liga` + `LIG` glyphs at separate clusters.
    #[cfg(not(target_os = "macos"))]
    pub fn shape_run_with_face(
        &self,
        text: &str,
        face: &rustybuzz::Face,
        font_id: fontdb::ID,
    ) -> Vec<ShapedGlyph> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        let output = rustybuzz::shape(face, &self.rb_features, buffer);
        let infos = output.glyph_infos();
        if infos.is_empty() {
            return Vec::new();
        }
        let char_byte_starts: Vec<usize> = text.char_indices().map(|(bi, _)| bi).collect();
        let char_count = char_byte_starts.len();
        let byte_to_char = |byte_off: usize| -> usize {
            char_byte_starts
                .binary_search(&byte_off)
                .unwrap_or_else(|x| x)
        };

        let mut out = Vec::with_capacity(infos.len());
        // Filter `.notdef` (glyph_id == 0) — those mean the font lacked the
        // codepoint and the caller should fall back to per-char shaping with
        // CJK / emoji faces. Collapse multi-glyph clusters to their first
        // glyph: the renderer can only attach one glyph per column, and
        // for terminal use cases multi-glyph clusters at a single column
        // (rare outside complex scripts) are best handled by the grapheme
        // path which uses `shape_grapheme_with_fallback`.
        let mut last_cluster: Option<u32> = None;
        for (i, info) in infos.iter().enumerate() {
            if info.glyph_id == 0 {
                continue;
            }
            if last_cluster == Some(info.cluster) {
                continue;
            }
            last_cluster = Some(info.cluster);
            let start = byte_to_char(info.cluster as usize);
            // Find the next cluster boundary (skipping same-cluster glyphs).
            let mut next_idx = i + 1;
            while next_idx < infos.len() && infos[next_idx].cluster == info.cluster {
                next_idx += 1;
            }
            let end = if next_idx < infos.len() {
                byte_to_char(infos[next_idx].cluster as usize).max(start)
            } else {
                char_count
            };
            out.push(ShapedGlyph {
                start_col: start,
                char_count: end.saturating_sub(start).max(1),
                glyph_id: info.glyph_id,
                font_id,
            });
        }
        out
    }

    /// macOS variant: shape the run via CoreText.
    #[cfg(target_os = "macos")]
    pub fn shape_run_with_face(
        &self,
        text: &str,
        font: &core_text::font::CTFont,
        font_id: fontdb::ID,
    ) -> Vec<ShapedGlyph> {
        crate::shaper_coretext::ct_shape_run(font, text, font_id)
    }

    // ─── detect_ligatures_uncached (non-macOS) ────────────────────

    #[cfg(not(target_os = "macos"))]
    fn detect_ligatures_uncached(
        &self,
        text: &str,
        face: &rustybuzz::Face,
        font_id: fontdb::ID,
    ) -> Vec<Ligature> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);

        let output = rustybuzz::shape(face, &self.rb_features, buffer);
        let infos = output.glyph_infos();

        if infos.is_empty() {
            return Vec::new();
        }

        let char_byte_starts: Vec<usize> = text.char_indices().map(|(bi, _)| bi).collect();
        let char_count = char_byte_starts.len();
        let byte_to_char = |byte_off: usize| -> usize {
            char_byte_starts
                .binary_search(&byte_off)
                .unwrap_or_else(|x| x)
        };
        let mut ligatures = Vec::new();

        for i in 0..infos.len() {
            let cluster_start = byte_to_char(infos[i].cluster as usize);
            let cluster_end = if i + 1 < infos.len() {
                byte_to_char(infos[i + 1].cluster as usize)
            } else {
                char_count
            };

            let span = cluster_end.saturating_sub(cluster_start);
            if span > 1 && infos[i].glyph_id != 0 {
                ligatures.push(Ligature {
                    start_col: cluster_start,
                    char_count: span,
                    glyph_id: infos[i].glyph_id,
                    font_id,
                });
            }
        }

        ligatures
    }

    // ─── shape_grapheme_with_fallback ─────────────────────────────

    /// Shape a grapheme cluster, trying the primary font first, then CJK, then emoji.
    /// Results are cached. Uses pre-created [`FaceSet`] to avoid per-call font parsing.
    pub fn shape_grapheme_with_fallback(
        &self,
        cluster: &str,
        faces: &FaceSet<'_>,
    ) -> Option<(u32, fontdb::ID)> {
        if let Some(cached) = self.grapheme_shape_cache.borrow().get(cluster) {
            return *cached;
        }
        let result = self.try_shape_with_face_set(cluster, faces, Self::shape_grapheme_with_face);
        self.grapheme_shape_cache
            .borrow_mut()
            .insert(cluster.to_string(), result);
        result
    }

    // ─── shape_char_with_fallback ─────────────────────────────────

    /// Shape a single character with fallback through primary → CJK → emoji fonts.
    /// Results are cached. Uses pre-created [`FaceSet`] to avoid per-call font parsing.
    pub fn shape_char_with_fallback(
        &self,
        ch: char,
        faces: &FaceSet<'_>,
    ) -> Option<(u32, fontdb::ID)> {
        if let Some(cached) = self.char_shape_cache.borrow().get(&ch) {
            return *cached;
        }
        let s = String::from(ch);
        let result = self.try_shape_with_face_set(&s, faces, Self::shape_single_char);
        self.char_shape_cache.borrow_mut().insert(ch, result);
        result
    }

    // ─── shape_single_char (platform-specific) ────────────────────

    #[cfg(not(target_os = "macos"))]
    fn shape_single_char(&self, s: &str, face: &rustybuzz::Face) -> Option<u32> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(s);
        let output = rustybuzz::shape(face, &self.rb_features, buffer);
        output
            .glyph_infos()
            .iter()
            .find(|gi| gi.glyph_id != 0)
            .map(|gi| gi.glyph_id)
    }

    #[cfg(target_os = "macos")]
    fn shape_single_char(&self, s: &str, font: &core_text::font::CTFont) -> Option<u32> {
        crate::shaper_coretext::ct_shape_char(font, s.chars().next()?)
    }

    // ─── try_shape_with_face_set (platform-specific face type) ────

    #[cfg(not(target_os = "macos"))]
    fn try_shape_with_face_set(
        &self,
        text: &str,
        faces: &FaceSet<'_>,
        shape: fn(&Self, &str, &rustybuzz::Face) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        self.try_shape_with_face_set_inner(text, faces, shape)
    }

    #[cfg(target_os = "macos")]
    fn try_shape_with_face_set(
        &self,
        text: &str,
        faces: &FaceSet<'_>,
        shape: fn(&Self, &str, &core_text::font::CTFont) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        self.try_shape_with_face_set_inner(text, faces, shape)
    }

    // ─── Shared fallback logic (generic over face type) ───────────

    #[cfg(not(target_os = "macos"))]
    fn try_shape_with_face_set_inner(
        &self,
        text: &str,
        faces: &FaceSet<'_>,
        shape: fn(&Self, &str, &rustybuzz::Face) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        let first_char = text.chars().next()?;
        let preferred = self.resolver.resolve_char(first_char);

        if let Some(result) = self.shape_with_resolved(text, preferred, faces, shape) {
            return Some(result);
        }

        let fallback_order = [
            ResolvedFont::Primary,
            ResolvedFont::Cjk,
            ResolvedFont::Emoji,
        ];
        for &font in &fallback_order {
            if font == preferred {
                continue;
            }
            if let Some(result) = self.shape_with_resolved(text, font, faces, shape) {
                return Some(result);
            }
        }
        None
    }

    #[cfg(target_os = "macos")]
    fn try_shape_with_face_set_inner(
        &self,
        text: &str,
        faces: &FaceSet<'_>,
        shape: fn(&Self, &str, &core_text::font::CTFont) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        let first_char = text.chars().next()?;
        let preferred = self.resolver.resolve_char(first_char);

        if let Some(result) = self.shape_with_resolved(text, preferred, faces, shape) {
            return Some(result);
        }

        let fallback_order = [
            ResolvedFont::Primary,
            ResolvedFont::Cjk,
            ResolvedFont::Emoji,
        ];
        for &font in &fallback_order {
            if font == preferred {
                continue;
            }
            if let Some(result) = self.shape_with_resolved(text, font, faces, shape) {
                return Some(result);
            }
        }
        None
    }

    // ─── shape_with_resolved (platform-specific) ──────────────────

    #[cfg(not(target_os = "macos"))]
    fn shape_with_resolved(
        &self,
        text: &str,
        font: ResolvedFont,
        faces: &FaceSet<'_>,
        shape: fn(&Self, &str, &rustybuzz::Face) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        let (face, font_id) = match font {
            ResolvedFont::Primary => (Some(faces.primary), Some(faces.primary_id)),
            ResolvedFont::Cjk => (faces.cjk, faces.cjk_id),
            ResolvedFont::Emoji => (faces.emoji, faces.emoji_id),
        };
        let face = face?;
        let font_id = font_id?;
        let glyph_id = shape(self, text, face)?;
        Some((glyph_id, font_id))
    }

    #[cfg(target_os = "macos")]
    fn shape_with_resolved(
        &self,
        text: &str,
        font: ResolvedFont,
        faces: &FaceSet<'_>,
        shape: fn(&Self, &str, &core_text::font::CTFont) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        let (face, font_id) = match font {
            ResolvedFont::Primary => (Some(faces.primary), Some(faces.primary_id)),
            ResolvedFont::Cjk => (faces.cjk, faces.cjk_id),
            ResolvedFont::Emoji => (faces.emoji, faces.emoji_id),
        };
        let face = face?;
        let font_id = font_id?;
        let glyph_id = shape(self, text, face)?;
        Some((glyph_id, font_id))
    }

    // ─── shape_grapheme_with_face (platform-specific) ─────────────

    /// Shape a grapheme cluster using a pre-created face.
    ///
    /// Returns `None` if the font doesn't actually combine the cluster into
    /// fewer glyphs than input characters (i.e., no GSUB substitution happened).
    #[cfg(not(target_os = "macos"))]
    pub fn shape_grapheme_with_face(&self, cluster: &str, face: &rustybuzz::Face) -> Option<u32> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(cluster);

        let output = rustybuzz::shape(face, &self.rb_features, buffer);
        let infos = output.glyph_infos();

        // If shaping produced as many glyphs as input chars, the font didn't
        // actually combine them — it just mapped each char individually.
        let input_chars = cluster.chars().count();
        if input_chars > 1 && infos.len() >= input_chars {
            return None;
        }

        infos
            .iter()
            .find(|gi| gi.glyph_id != 0)
            .map(|gi| gi.glyph_id)
    }

    #[cfg(target_os = "macos")]
    pub fn shape_grapheme_with_face(
        &self,
        cluster: &str,
        font: &core_text::font::CTFont,
    ) -> Option<u32> {
        crate::shaper_coretext::ct_shape_grapheme(font, cluster)
    }

    // ─── detect_ligatures (convenience, non-cached face) ──────────

    /// Detect ligatures in a line of text for the given font.
    /// Returns ligatures where multiple input characters map to a single glyph.
    #[cfg(not(target_os = "macos"))]
    pub fn detect_ligatures(&self, text: &str, font_id: fontdb::ID) -> Vec<Ligature> {
        // Check cache first to avoid create_face overhead
        let cache_key = (text.to_string(), font_id);
        if let Some(cached) = self.ligature_cache.borrow().get(&cache_key) {
            return cached.clone();
        }
        let Some(face) = self.create_face(font_id) else {
            return Vec::new();
        };
        let result = self.detect_ligatures_uncached(text, &face, font_id);
        self.ligature_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    #[cfg(target_os = "macos")]
    pub fn detect_ligatures(&self, text: &str, font_id: fontdb::ID) -> Vec<Ligature> {
        let cache_key = (text.to_string(), font_id);
        if let Some(cached) = self.ligature_cache.borrow().get(&cache_key) {
            return cached.clone();
        }
        // Try to find a CTFont for this font_id
        let ct_font = if Some(font_id) == self.primary_font_id {
            self.primary_ct_font.as_ref()
        } else if Some(font_id) == self.cjk_font_id {
            self.cjk_ct_font.as_ref()
        } else if Some(font_id) == self.emoji_font_id {
            self.emoji_ct_font.as_ref()
        } else {
            None
        };
        let Some(ct_font) = ct_font else {
            return Vec::new();
        };
        let result = crate::shaper_coretext::ct_detect_ligatures(ct_font, text, font_id);
        self.ligature_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    // ─── shape_grapheme (convenience, non-cached face) ────────────

    /// Shape a grapheme cluster (multi-codepoint string) and return the primary glyph ID.
    /// Returns None if the font doesn't have a glyph for this cluster.
    #[cfg(not(target_os = "macos"))]
    pub fn shape_grapheme(&self, cluster: &str, font_id: fontdb::ID) -> Option<u32> {
        let face = self.create_face(font_id)?;
        self.shape_grapheme_with_face(cluster, &face)
    }

    #[cfg(target_os = "macos")]
    pub fn shape_grapheme(&self, cluster: &str, font_id: fontdb::ID) -> Option<u32> {
        let ct_font = if Some(font_id) == self.primary_font_id {
            self.primary_ct_font.as_ref()
        } else if Some(font_id) == self.cjk_font_id {
            self.cjk_ct_font.as_ref()
        } else if Some(font_id) == self.emoji_font_id {
            self.emoji_ct_font.as_ref()
        } else {
            None
        };
        let ct_font = ct_font?;
        self.shape_grapheme_with_face(cluster, ct_font)
    }
}

// ─── CoreText font loading helper (macOS only) ───────────────────

/// Load a CTFont from a file path for shaping purposes.
/// Size is irrelevant for shaping (only glyph IDs matter), so we use 12.0.
#[cfg(target_os = "macos")]
fn load_ct_font_for_shaping(path: &str, index: u32) -> Option<core_text::font::CTFont> {
    load_ct_font_from_path(path, index, 12.0)
}

/// Load a CTFont from a file path + face index at the given size.
/// Handles TTC collections by using CTFontManagerCreateFontDescriptorsFromData
/// to enumerate all faces and select the correct one by index.
/// Shared by both shaper and rasterizer font loading.
#[cfg(target_os = "macos")]
pub(crate) fn load_ct_font_from_path(
    path: &str,
    index: u32,
    size: f64,
) -> Option<core_text::font::CTFont> {
    use core_foundation::base::TCFType;

    let data = std::fs::read(path).ok()?;

    if index == 0 {
        // Fast path: face 0 can be loaded directly via CGFont
        let provider =
            core_graphics::data_provider::CGDataProvider::from_buffer(std::sync::Arc::new(data));
        let cg_font = core_graphics::font::CGFont::from_data_provider(provider).ok()?;
        return Some(core_text::font::new_from_CGFont(&cg_font, size));
    }

    // TTC with index > 0: enumerate all faces via CTFontManagerCreateFontDescriptorsFromData
    let cf_data = core_foundation::data::CFData::from_buffer(&data);

    // CTFontManagerCreateFontDescriptorsFromData returns a CFArray of CTFontDescriptors
    unsafe extern "C" {
        fn CTFontManagerCreateFontDescriptorsFromData(
            data: core_foundation::data::CFDataRef,
        ) -> core_foundation::array::CFArrayRef;
    }

    let descriptors_ref =
        unsafe { CTFontManagerCreateFontDescriptorsFromData(cf_data.as_concrete_TypeRef()) };
    if descriptors_ref.is_null() {
        log::warn!(
            "CoreText: CTFontManagerCreateFontDescriptorsFromData returned null for {}",
            path
        );
        // Fallback: load face 0
        let provider =
            core_graphics::data_provider::CGDataProvider::from_buffer(std::sync::Arc::new(data));
        let cg_font = core_graphics::font::CGFont::from_data_provider(provider).ok()?;
        return Some(core_text::font::new_from_CGFont(&cg_font, size));
    }

    let descriptors: core_foundation::array::CFArray<core_text::font_descriptor::CTFontDescriptor> =
        unsafe { core_foundation::array::CFArray::wrap_under_create_rule(descriptors_ref) };

    let count = descriptors.len();
    if (index as isize) < count {
        let desc = descriptors.get(index as isize).unwrap();
        let ct_font = core_text::font::new_from_descriptor(&desc, size);
        log::info!(
            "CoreText: loaded face[{}] from TTC '{}' -> ps='{}' family='{}'",
            index,
            path,
            ct_font.postscript_name(),
            ct_font.family_name(),
        );
        Some(ct_font)
    } else {
        log::warn!(
            "CoreText: face index {} out of range ({} faces in {}), loading face 0",
            index,
            count,
            path,
        );
        let provider =
            core_graphics::data_provider::CGDataProvider::from_buffer(std::sync::Arc::new(data));
        let cg_font = core_graphics::font::CGFont::from_data_provider(provider).ok()?;
        Some(core_text::font::new_from_CGFont(&cg_font, size))
    }
}

// ─── Font discovery helpers (platform-independent) ────────────────

/// Find the primary font ID for the given family name.
///
/// When several faces share the same family (e.g. FiraCode Nerd Font has
/// Light/Regular/Medium/Bold all listed under "FiraCode Nerd Font"), pick the
/// face whose weight is closest to `preferred_weight` (defaults to 400 /
/// Regular) so we don't accidentally land on Bold just because fontdb
/// enumerated it first.
fn find_primary_font(
    db: &fontdb::Database,
    family_name: &str,
    preferred_weight: Option<u16>,
) -> Option<fontdb::ID> {
    let family_lower = family_name.to_ascii_lowercase();
    let target = preferred_weight.unwrap_or(400) as i32;

    let weight_dist = |w: fontdb::Weight| -> u16 { (w.0 as i32 - target).unsigned_abs() as u16 };

    // Exact match: pick best weight among same-family faces.
    let mut best: Option<(fontdb::ID, u16, &str, bool)> = None;
    for face in db.faces() {
        if face.style != fontdb::Style::Normal {
            continue;
        }
        for family in &face.families {
            if family.0.eq_ignore_ascii_case(family_name) {
                let dist = weight_dist(face.weight);
                if best.map(|(_, d, _, _)| dist < d).unwrap_or(true) {
                    best = Some((face.id, dist, family.0.as_str(), face.monospaced));
                }
            }
        }
    }
    if let Some((id, _, name, monospaced)) = best {
        log::info!("primary font: {} (monospaced={})", name, monospaced);
        return Some(id);
    }

    // Substring match: same — best weight wins.
    let mut best: Option<(fontdb::ID, u16, String, bool)> = None;
    for face in db.faces() {
        if face.style != fontdb::Style::Normal {
            continue;
        }
        for family in &face.families {
            if family.0.to_ascii_lowercase().contains(&family_lower) {
                let dist = weight_dist(face.weight);
                if best.as_ref().map(|(_, d, _, _)| dist < *d).unwrap_or(true) {
                    best = Some((face.id, dist, family.0.clone(), face.monospaced));
                }
            }
        }
    }
    if let Some((id, _, name, monospaced)) = best {
        log::info!(
            "primary font (substring): {} (monospaced={})",
            name,
            monospaced
        );
        return Some(id);
    }

    // First monospaced font
    for face in db.faces() {
        if face.monospaced && face.style == fontdb::Style::Normal {
            let name = face.families.first().map(|f| f.0.as_str()).unwrap_or("?");
            log::info!("primary font (monospace fallback): {}", name);
            return Some(face.id);
        }
    }

    log::warn!("no suitable font found for '{}'", family_name);
    None
}

/// Find a CJK font from system fonts for fallback rendering.
/// Skips the primary font since it's already tried first.
/// Prefers Normal style and Regular (400) weight.
///
/// On macOS, uses CTFontCreateForString to let the system pick a locale-aware
/// CJK font (e.g., PingFang SC for Chinese, Hiragino Sans for Japanese).
/// Falls back to the hardcoded list if the system API doesn't work.
fn find_cjk_font(db: &fontdb::Database, primary: Option<fontdb::ID>) -> Option<fontdb::ID> {
    // On macOS: ask the system which font handles CJK ideographs.
    // CTFontCreateForString respects system locale preferences.
    #[cfg(target_os = "macos")]
    {
        if let Some(id) = find_cjk_font_via_coretext(db, primary) {
            return Some(id);
        }
        log::info!("CJK: CTFontCreateForString fallback failed, trying hardcoded list");
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(id) = find_cjk_font_via_fontconfig(db, primary) {
            return Some(id);
        }
        if let Some(id) = find_cjk_monospace_font(db, primary) {
            return Some(id);
        }
        log::info!("CJK: fontconfig/monospace fallback failed, trying hardcoded list");
    }

    let known = [
        "Noto Sans CJK SC",
        "Noto Sans CJK TC",
        "Noto Sans CJK JP",
        "Noto Sans CJK KR",
        "Noto Sans CJK",
        "Source Han Sans SC",
        "Source Han Sans",
        "WenQuanYi Micro Hei Mono",
        "WenQuanYi Micro Hei",
        "WenQuanYi Zen Hei Mono",
        "WenQuanYi Zen Hei",
        "Microsoft YaHei",
        "SimHei",
        "PingFang SC",
        "PingFang TC",
        "PingFang HK",
        "Hiragino Sans",
        "Apple SD Gothic Neo",
    ];
    // First pass: exact family name, prefer regular weight
    for name in &known {
        let mut best: Option<fontdb::ID> = None;
        let mut best_weight_dist = u16::MAX;
        for face in db.faces() {
            if Some(face.id) == primary || face.style != fontdb::Style::Normal {
                continue;
            }
            for fam in &face.families {
                if fam.0.eq_ignore_ascii_case(name) {
                    let dist = (face.weight.0 as i32 - 400).unsigned_abs() as u16;
                    if dist < best_weight_dist {
                        best = Some(face.id);
                        best_weight_dist = dist;
                    }
                }
            }
        }
        if let Some(id) = best {
            let w = db.face(id).map(|f| f.weight.0).unwrap_or(0);
            log::info!("CJK font: {} (weight={})", name, w);
            return Some(id);
        }
    }
    // Fallback: any font with CJK-related keywords, prefer regular weight
    let keywords = [
        "cjk",
        "han",
        "wenquanyi",
        "yahei",
        "simhei",
        "hiragino",
        "pingfang",
    ];
    let mut best: Option<(fontdb::ID, u16)> = None;
    for face in db.faces() {
        if Some(face.id) == primary || face.style != fontdb::Style::Normal {
            continue;
        }
        for fam in &face.families {
            let lower = fam.0.to_ascii_lowercase();
            if keywords.iter().any(|kw| lower.contains(kw)) {
                let dist = (face.weight.0 as i32 - 400).unsigned_abs() as u16;
                if best.is_none_or(|(_, d)| dist < d) {
                    log::info!("CJK font candidate: {} (weight={})", fam.0, face.weight.0);
                    best = Some((face.id, dist));
                }
            }
        }
    }
    if let Some((id, _)) = best {
        return Some(id);
    }
    log::info!("no CJK font found on system");
    None
}

#[cfg(target_os = "linux")]
fn find_cjk_font_via_fontconfig(
    db: &fontdb::Database,
    primary: Option<fontdb::ID>,
) -> Option<fontdb::ID> {
    for query in ["monospace:charset=6c34", "monospace:charset=4e2d"] {
        let output = std::process::Command::new("fc-match")
            .args(["-f", "%{file}\n%{index}\n"])
            .arg(query)
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        let mut lines = stdout.lines();
        let path = lines.next()?.trim();
        if path.is_empty() {
            continue;
        }
        let index = lines
            .next()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);

        if let Some(id) = find_fontdb_face_by_path_index(db, primary, path, Some(index)) {
            log_cjk_font_selection(db, id, "fontconfig monospace");
            return Some(id);
        }

        // Some fontconfig builds report collection indexes differently than
        // fontdb. If the exact face index doesn't map, accept another CJK-
        // capable face from the same file rather than ignoring fontconfig's
        // chosen family entirely.
        if let Some(id) = find_fontdb_face_by_path_index(db, primary, path, None) {
            log_cjk_font_selection(db, id, "fontconfig monospace file");
            return Some(id);
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn find_fontdb_face_by_path_index(
    db: &fontdb::Database,
    primary: Option<fontdb::ID>,
    path: &str,
    index: Option<u32>,
) -> Option<fontdb::ID> {
    let target_path = std::path::Path::new(path);
    let target_canonical = std::fs::canonicalize(target_path).ok();
    let mut best: Option<(fontdb::ID, u16)> = None;

    for face in db.faces() {
        if Some(face.id) == primary || face.style != fontdb::Style::Normal {
            continue;
        }
        if index.is_some_and(|idx| face.index != idx) {
            continue;
        }
        if !font_source_matches_path(&face.source, target_path, target_canonical.as_deref()) {
            continue;
        }
        if !font_face_supports_char(face, '水') && !font_face_supports_char(face, '中') {
            continue;
        }

        let dist = (face.weight.0 as i32 - 400).unsigned_abs() as u16;
        if best.is_none_or(|(_, best_dist)| dist < best_dist) {
            best = Some((face.id, dist));
        }
    }

    best.map(|(id, _)| id)
}

#[cfg(target_os = "linux")]
fn font_source_matches_path(
    source: &fontdb::Source,
    target: &std::path::Path,
    target_canonical: Option<&std::path::Path>,
) -> bool {
    let source_path = match source {
        fontdb::Source::File(path) => path,
        fontdb::Source::SharedFile(path, _) => path,
        fontdb::Source::Binary(_) => return false,
    };

    source_path == target
        || target_canonical.is_some_and(|canonical| {
            source_path == canonical
                || std::fs::canonicalize(source_path)
                    .ok()
                    .as_deref()
                    .is_some_and(|source_canonical| source_canonical == canonical)
        })
}

#[cfg(target_os = "linux")]
fn find_cjk_monospace_font(
    db: &fontdb::Database,
    primary: Option<fontdb::ID>,
) -> Option<fontdb::ID> {
    let mut best: Option<(fontdb::ID, u16)> = None;
    for face in db.faces() {
        if Some(face.id) == primary
            || face.style != fontdb::Style::Normal
            || !face.monospaced
            || (!font_face_supports_char(face, '水') && !font_face_supports_char(face, '中'))
        {
            continue;
        }

        let dist = (face.weight.0 as i32 - 400).unsigned_abs() as u16;
        if best.is_none_or(|(_, best_dist)| dist < best_dist) {
            best = Some((face.id, dist));
        }
    }

    if let Some((id, _)) = best {
        log_cjk_font_selection(db, id, "fontdb monospace");
        Some(id)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn font_face_supports_char(face: &fontdb::FaceInfo, ch: char) -> bool {
    let face_index = face.index;
    let data = match &face.source {
        fontdb::Source::File(path) => std::fs::read(path).ok(),
        fontdb::Source::Binary(arc) => Some(arc.as_ref().as_ref().to_vec()),
        fontdb::Source::SharedFile(_, arc) => Some(arc.as_ref().as_ref().to_vec()),
    };
    let Some(data) = data else {
        return false;
    };
    let Ok(parsed) = ttf_parser::Face::parse(&data, face_index) else {
        return false;
    };
    parsed.glyph_index(ch).is_some()
}

#[cfg(target_os = "linux")]
fn log_cjk_font_selection(db: &fontdb::Database, id: fontdb::ID, source: &str) {
    let Some(face) = db.face(id) else { return };
    let name = face.families.first().map(|f| f.0.as_str()).unwrap_or("?");
    log::info!(
        "CJK font ({}): {} (weight={}, monospaced={})",
        source,
        name,
        face.weight.0,
        face.monospaced,
    );
}

/// Use CTFontCreateForString to let macOS pick the locale-appropriate CJK font.
/// Sends a CJK test character ("水" U+6C34) to the system, gets back the font
/// macOS would use, then matches it against fontdb by family name.
#[cfg(target_os = "macos")]
fn find_cjk_font_via_coretext(
    db: &fontdb::Database,
    primary: Option<fontdb::ID>,
) -> Option<fontdb::ID> {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    // extern for CTFontCreateForString (not exposed by core-text crate)
    unsafe extern "C" {
        fn CTFontCreateForString(
            current_font: core_text::font::CTFontRef,
            string: core_foundation::string::CFStringRef,
            range: core_foundation::base::CFRange,
        ) -> core_text::font::CTFontRef;
    }

    // Create a base font (system default) and ask CoreText which font handles "水"
    let base_font = core_text::font::new_from_name("Menlo", 12.0).ok()?;
    let test_str = CFString::new("水");
    let range = core_foundation::base::CFRange::init(0, 1);

    let ct_font = unsafe {
        let raw = CTFontCreateForString(
            base_font.as_concrete_TypeRef(),
            test_str.as_concrete_TypeRef(),
            range,
        );
        if raw.is_null() {
            return None;
        }
        core_text::font::CTFont::wrap_under_create_rule(raw)
    };

    let family = ct_font.family_name();
    let ps_name = ct_font.postscript_name();
    log::info!(
        "CJK: CTFontCreateForString selected '{}' (ps='{}') for U+6C34 '水'",
        family,
        ps_name,
    );

    // Match the family name against fontdb
    for face in db.faces() {
        if Some(face.id) == primary || face.style != fontdb::Style::Normal {
            continue;
        }
        for fam in &face.families {
            if fam.0.eq_ignore_ascii_case(&family) {
                let dist = (face.weight.0 as i32 - 400).unsigned_abs() as u16;
                if dist < 100 {
                    log::info!(
                        "CJK: matched fontdb face '{}' (weight={}) for system-selected '{}'",
                        fam.0,
                        face.weight.0,
                        family,
                    );
                    return Some(face.id);
                }
            }
        }
    }

    // If exact family match failed, try PostScript name substring
    for face in db.faces() {
        if Some(face.id) == primary || face.style != fontdb::Style::Normal {
            continue;
        }
        for fam in &face.families {
            if ps_name.contains(&fam.0) || fam.0.contains(&family) {
                log::info!(
                    "CJK: fuzzy matched fontdb face '{}' for system-selected '{}'",
                    fam.0,
                    family,
                );
                return Some(face.id);
            }
        }
    }

    log::warn!(
        "CJK: CTFontCreateForString selected '{}' but no fontdb match found",
        family,
    );
    None
}

/// Find a color emoji font from system fonts.
fn find_emoji_font(db: &fontdb::Database) -> Option<fontdb::ID> {
    // Prioritized list of known emoji font families
    let known = [
        "Noto Color Emoji",
        "Apple Color Emoji",
        "Segoe UI Emoji",
        "Twemoji",
        "Twitter Color Emoji",
        "EmojiOne Color",
        "JoyPixels",
    ];
    for name in &known {
        for face in db.faces() {
            for fam in &face.families {
                if fam.0.eq_ignore_ascii_case(name) {
                    log::info!("emoji font: {}", fam.0);
                    return Some(face.id);
                }
            }
        }
    }
    // Fallback: any font with "emoji" in the family name
    for face in db.faces() {
        for fam in &face.families {
            if fam.0.to_ascii_lowercase().contains("emoji") {
                log::info!("emoji font (fallback match): {}", fam.0);
                return Some(face.id);
            }
        }
    }
    log::info!("no emoji font found on system");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaper_new_empty() {
        let shaper = TextShaper::new("NonexistentFontFamily12345");
        // May or may not find a font depending on system, but shouldn't panic
        let _ = shaper.primary_font_id();
    }

    #[test]
    fn detect_ligatures_no_font() {
        let shaper = TextShaper::new("NonexistentFontFamily12345");
        let ligs = shaper.detect_ligatures("hello", fontdb::ID::dummy());
        assert!(ligs.is_empty());
    }

    #[test]
    fn shape_grapheme_no_font() {
        let shaper = TextShaper::new("NonexistentFontFamily12345");
        assert!(shaper.shape_grapheme("a", fontdb::ID::dummy()).is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cjk_font_prefers_fontconfig_monospace_match() {
        let Ok(output) = std::process::Command::new("fc-match")
            .args(["-f", "%{file}\n%{index}\n", "monospace:charset=6c34"])
            .output()
        else {
            return;
        };
        if !output.status.success() {
            return;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines();
        let Some(path) = lines.next().map(str::trim).filter(|s| !s.is_empty()) else {
            return;
        };
        let index = lines
            .next()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);

        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let expected = find_fontdb_face_by_path_index(&db, None, path, Some(index))
            .or_else(|| find_fontdb_face_by_path_index(&db, None, path, None));
        let Some(expected) = expected else {
            return;
        };

        assert_eq!(find_cjk_font(&db, None), Some(expected));
    }
}
