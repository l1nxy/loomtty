//! DirectWrite-based font resolver for Windows.
//!
//! Uses `IDWriteFontFallback::MapCharacters()` for system-native font
//! matching, identical to how Windows Terminal resolves fonts.  This
//! automatically finds system fonts for Braille, symbols, CJK, emoji,
//! and any other script — no manual font lists needed.

use super::{FontResolver, ResolvedFont};
use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Graphics::DirectWrite::*;
use windows::core::*;

/// Font resolver using DirectWrite's system font fallback.
///
/// Falls back to [`CmapResolver`](super::CmapResolver) for characters
/// that `MapCharacters` can't resolve (shouldn't happen in practice).
pub struct DWriteResolver {
    fallback: Option<IDWriteFontFallback>,
    base_family: Vec<u16>,
    base_collection: Option<IDWriteFontCollection>,
    /// Known font faces for classifying MapCharacters results.
    known_faces: RefCell<KnownFaces>,
    /// Per-character cache: resolved font class.
    cache: RefCell<HashMap<char, ResolvedFont>>,
    /// Per-character cache: DWrite font face from MapCharacters (for system fallback).
    face_cache: RefCell<HashMap<char, IDWriteFontFace>>,
    /// Fallback for characters MapCharacters can't handle.
    inner: super::CmapResolver,
}

struct KnownFaces {
    primary: Option<IDWriteFontFace>,
    cjk: Option<IDWriteFontFace>,
    emoji: Option<IDWriteFontFace>,
}

// SAFETY: DWrite COM objects are thread-safe (free-threaded) when created
// with DWRITE_FACTORY_TYPE_SHARED. The RefCell<HashMap> is only accessed
// from the main thread (same as all rendering).
unsafe impl Send for DWriteResolver {}
unsafe impl Sync for DWriteResolver {}

impl DWriteResolver {
    /// Create a DWrite resolver.
    pub fn new(inner: super::CmapResolver) -> Self {
        let (fallback, base_family, base_collection) = Self::init_dwrite().unwrap_or_else(|| {
            log::warn!("DWrite font fallback init failed, using CmapResolver only");
            (None, Vec::new(), None)
        });

        DWriteResolver {
            fallback,
            base_family,
            base_collection,
            known_faces: RefCell::new(KnownFaces {
                primary: None,
                cjk: None,
                emoji: None,
            }),
            cache: RefCell::new(HashMap::new()),
            face_cache: RefCell::new(HashMap::new()),
            inner,
        }
    }

    /// Set the known DWrite font faces for classifying MapCharacters results.
    /// Called by GlyphCache after DWrite rasterizer is initialized.
    pub fn set_known_faces(
        &self,
        primary: Option<IDWriteFontFace>,
        cjk: Option<IDWriteFontFace>,
        emoji: Option<IDWriteFontFace>,
    ) {
        let mut faces = self.known_faces.borrow_mut();
        faces.primary = primary;
        faces.cjk = cjk;
        faces.emoji = emoji;
        self.cache.borrow_mut().clear();
        self.face_cache.borrow_mut().clear();
    }

    /// Get the system fallback font face for a character (if MapCharacters
    /// found one that's not primary/CJK/emoji). Used by GlyphCache as a
    /// last-resort rasterization face.
    pub fn get_system_face(&self, ch: char) -> Option<IDWriteFontFace> {
        self.face_cache.borrow().get(&ch).cloned()
    }

    fn init_dwrite() -> Option<(
        Option<IDWriteFontFallback>,
        Vec<u16>,
        Option<IDWriteFontCollection>,
    )> {
        unsafe {
            let factory: IDWriteFactory2 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let fallback = factory.GetSystemFontFallback().ok();

            let mut collection = None;
            factory
                .GetSystemFontCollection(&mut collection, false)
                .ok()?;
            let collection = collection?;

            // Get first family name as base for MapCharacters
            let family = collection.GetFontFamily(0).ok()?;
            let names = family.GetFamilyNames().ok()?;
            let len = names.GetStringLength(0).ok()? as usize;
            let mut buf = vec![0u16; len + 1];
            names.GetString(0, &mut buf).ok()?;

            Some((fallback, buf, Some(collection)))
        }
    }

    fn resolve_via_dwrite(&self, ch: char) -> Option<ResolvedFont> {
        let fallback = self.fallback.as_ref()?;
        let collection = self.base_collection.as_ref()?;

        let text: Vec<u16> = ch.encode_utf16(&mut [0u16; 2]).to_vec();
        let source = SimpleTextSource::new(&text);

        let mut mapped_length = 0u32;
        let mut mapped_font = None;
        let mut scale = 0.0f32;

        unsafe {
            fallback
                .MapCharacters(
                    &source,
                    0,
                    text.len() as u32,
                    collection,
                    PCWSTR(self.base_family.as_ptr()),
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    &mut mapped_length,
                    &mut mapped_font,
                    &mut scale,
                )
                .ok()?;
        }

        let mapped_font = mapped_font?;
        let mapped_face: IDWriteFontFace = unsafe { mapped_font.CreateFontFace().ok()? };

        // Classify the returned font against known faces
        let faces = self.known_faces.borrow();
        if is_same_font(&mapped_face, &faces.primary) {
            return Some(ResolvedFont::Primary);
        }
        if is_same_font(&mapped_face, &faces.cjk) {
            return Some(ResolvedFont::Cjk);
        }
        if is_same_font(&mapped_face, &faces.emoji) {
            return Some(ResolvedFont::Emoji);
        }
        drop(faces);

        // Unknown system font — cache the face for GlyphCache to use
        let is_color = mapped_face
            .cast::<IDWriteFontFace2>()
            .ok()
            .is_some_and(|f2| unsafe { f2.IsColorFont().as_bool() });

        self.face_cache.borrow_mut().insert(ch, mapped_face);

        Some(if is_color {
            ResolvedFont::Emoji
        } else {
            ResolvedFont::Primary
        })
    }
}

impl FontResolver for DWriteResolver {
    fn resolve_char(&self, ch: char) -> ResolvedFont {
        if let Some(&cached) = self.cache.borrow().get(&ch) {
            return cached;
        }

        let result = self
            .resolve_via_dwrite(ch)
            .unwrap_or_else(|| self.inner.resolve_char(ch));

        self.cache.borrow_mut().insert(ch, result);
        result
    }
}

fn is_same_font(a: &IDWriteFontFace, b: &Option<IDWriteFontFace>) -> bool {
    let Some(b) = b else { return false };
    let a_files = get_font_files(a);
    let b_files = get_font_files(b);
    !a_files.is_empty() && !b_files.is_empty() && a_files[0] == b_files[0]
}

/// Get file reference keys for font face comparison.
fn get_font_files(face: &IDWriteFontFace) -> Vec<Vec<u8>> {
    unsafe {
        let mut count = 0u32;
        if face.GetFiles(&mut count, None).is_err() || count == 0 {
            return Vec::new();
        }
        let mut files: Vec<Option<IDWriteFontFile>> = vec![None; count as usize];
        if face.GetFiles(&mut count, Some(files.as_mut_ptr())).is_err() {
            return Vec::new();
        }
        files
            .into_iter()
            .filter_map(|f| {
                let f = f?;
                let mut key_ptr: *const std::ffi::c_void = std::ptr::null();
                let mut key_size = 0u32;
                f.GetReferenceKey(&mut key_ptr as *mut _ as *mut _, &mut key_size)
                    .ok()?;
                if key_ptr.is_null() || key_size == 0 {
                    return None;
                }
                let slice = std::slice::from_raw_parts(key_ptr as *const u8, key_size as usize);
                Some(slice.to_vec())
            })
            .collect()
    }
}

// ─── IDWriteTextAnalysisSource COM implementation ──────────────────

/// Minimal `IDWriteTextAnalysisSource` implementation for `MapCharacters`.
#[implement(IDWriteTextAnalysisSource)]
struct SimpleTextSource {
    text: Vec<u16>,
}

impl SimpleTextSource {
    fn new(text: &[u16]) -> IDWriteTextAnalysisSource {
        let source = SimpleTextSource {
            text: text.to_vec(),
        };
        source.into()
    }
}

impl IDWriteTextAnalysisSource_Impl for SimpleTextSource_Impl {
    fn GetTextAtPosition(
        &self,
        position: u32,
        text: *mut *mut u16,
        text_length: *mut u32,
    ) -> Result<()> {
        let pos = position as usize;
        if pos >= self.text.len() {
            unsafe {
                *text = std::ptr::null_mut();
                *text_length = 0;
            }
        } else {
            unsafe {
                *text = self.text.as_ptr().add(pos) as *mut u16;
                *text_length = (self.text.len() - pos) as u32;
            }
        }
        Ok(())
    }

    fn GetTextBeforePosition(
        &self,
        position: u32,
        text: *mut *mut u16,
        text_length: *mut u32,
    ) -> Result<()> {
        let pos = position as usize;
        if pos == 0 || pos > self.text.len() {
            unsafe {
                *text = std::ptr::null_mut();
                *text_length = 0;
            }
        } else {
            unsafe {
                *text = self.text.as_ptr() as *mut u16;
                *text_length = pos as u32;
            }
        }
        Ok(())
    }

    fn GetParagraphReadingDirection(&self) -> DWRITE_READING_DIRECTION {
        DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
    }

    fn GetLocaleName(
        &self,
        _position: u32,
        text_length: *mut u32,
        locale: *mut *mut u16,
    ) -> Result<()> {
        static EMPTY_LOCALE: [u16; 1] = [0];
        unsafe {
            *locale = EMPTY_LOCALE.as_ptr() as *mut u16;
            *text_length = self.text.len() as u32;
        }
        Ok(())
    }

    fn GetNumberSubstitution(
        &self,
        _position: u32,
        text_length: *mut u32,
        substitution: *mut Option<IDWriteNumberSubstitution>,
    ) -> Result<()> {
        unsafe {
            *text_length = self.text.len() as u32;
            *substitution = None;
        }
        Ok(())
    }
}
