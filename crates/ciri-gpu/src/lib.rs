//! GPU backend crate for ciri terminal renderer.
//!
//! Provides compile-time backend selection:
//! - **blade** (default): Vulkan on Linux, Metal on macOS
//! - **gl**: OpenGL 3.3+ / EGL — smooth Wayland resize via implicit eglSwapBuffers
//! - **dx**: Direct3D 11 — native Windows backend

#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "blade")]
mod blade;
#[cfg(feature = "blade")]
pub use self::blade::{GlyphAtlasGpu, Renderer};

#[cfg(feature = "gl")]
mod gl;
#[cfg(feature = "gl")]
pub use self::gl::{GlyphAtlasGpu, Renderer};

#[cfg(all(feature = "dx", windows))]
mod dx;
#[cfg(all(feature = "dx", windows))]
pub use self::dx::{GlyphAtlasGpu, Renderer};
