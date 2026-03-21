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
