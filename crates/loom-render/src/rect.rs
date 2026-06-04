/// A colored rectangle for terminal cell backgrounds, borders, cursor, etc.
/// Layout matches the GPU instance format directly for zero-copy upload.
#[repr(C)]
#[derive(Copy, Clone, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
}

/// A contiguous rect draw range sharing pane corner clipping uniforms.
#[derive(Copy, Clone, Default, Debug)]
pub struct PaneRectRange {
    pub start: u32,
    pub count: u32,
    pub pane_origin: [f32; 2],
    pub pane_size: [f32; 2],
    /// CSS order: top-left, top-right, bottom-right, bottom-left.
    pub pane_radii: [f32; 4],
}
