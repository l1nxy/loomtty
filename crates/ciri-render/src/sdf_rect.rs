//! GPU instance for signed-distance-field-rendered chrome rectangles.
//!
//! Layout matches the shader input struct directly so upload is a single
//! `bytemuck::cast_slice`. Field order is chosen so every `vec4<f32>` in
//! WGSL sits on a 16-byte boundary (positions 16, 32, 48, 80) without any
//! padding holes on the Rust side — both requirements are necessary for
//! `bytemuck::Pod` and the matching blade vertex layout.
//!
//! Units: `pos` / `size` / `border_width` / `shadow_blur` / `shadow_offset`
//! are in screen (logical) pixels. `color`, `border_color`, and
//! `shadow_color` are sRGB-encoded RGBA with straight alpha — the same
//! convention `ciri-ui`'s `ResolvedTheme` stores, matching the existing
//! rect pipeline (no gamma decode). Pre-multiplication happens inside
//! the SDF shader, not at upload time.
//!
//! Zero-valued visuals are cheap and well-defined: `border_width = 0.0`
//! disables the border (regardless of `border_color`); `shadow_blur = 0.0`
//! disables the shadow.

/// SDF chrome rect: rounded corners + optional border + optional shadow.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
pub struct SdfRect {
    /// Top-left in screen-space (logical) pixels.
    pub pos: [f32; 2],
    /// Width, height in logical pixels.
    pub size: [f32; 2],
    /// Fill color (sRGB RGBA, straight alpha; shader pre-multiplies).
    pub color: [f32; 4],
    /// Per-corner radii: `[tl, tr, br, bl]` in logical pixels.
    pub radii: [f32; 4],
    /// Border color (sRGB RGBA); ignored when `border_width == 0`.
    pub border_color: [f32; 4],
    /// Border width in logical pixels; `0` disables the border.
    pub border_width: f32,
    /// Gaussian-ish shadow blur radius in logical pixels; `0` disables.
    pub shadow_blur: f32,
    /// Shadow offset `(x, y)` in logical pixels.
    pub shadow_offset: [f32; 2],
    /// Shadow color (sRGB RGBA); ignored when `shadow_blur == 0`.
    pub shadow_color: [f32; 4],
}

impl SdfRect {
    /// Layout-time constant: total bytes per instance. Backend vertex
    /// layouts reference specific offsets into this struct.
    pub const SIZE: usize = std::mem::size_of::<Self>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_matches_shader_struct() {
        // Eight 16-byte slots: pos+size (1), color (1), radii (1),
        // border_color (1), border_width+shadow_blur+shadow_offset (1),
        // shadow_color (1) → 6 × 16 = 96 bytes.
        assert_eq!(SdfRect::SIZE, 96);
    }

    #[test]
    fn vec4_fields_are_16_aligned() {
        // Matches the WGSL attribute offsets in the blade SDF shader.
        let offset_of = |field_ptr: *const f32, base: *const SdfRect| {
            (field_ptr as usize) - (base as usize)
        };
        let s = SdfRect::default();
        let base = &s as *const SdfRect;
        assert_eq!(offset_of(s.color.as_ptr(), base), 16);
        assert_eq!(offset_of(s.radii.as_ptr(), base), 32);
        assert_eq!(offset_of(s.border_color.as_ptr(), base), 48);
        assert_eq!(offset_of(s.shadow_color.as_ptr(), base), 80);
    }

    #[test]
    fn is_bytemuck_pod() {
        // Will fail to compile if Pod isn't actually derivable —
        // guards against accidental padding on future edits.
        fn assert_pod<T: bytemuck::Pod>() {}
        assert_pod::<SdfRect>();
    }
}
