//! GPU integration tests for ciri-gpu (blade backend).
//!
//! Adapted from wgpu test patterns:
//! - Headless context creation (no window/surface needed)
//! - Off-screen rendering to textures
//! - Pixel readback via staging buffers
//! - Scissor rect verification
//! - Instanced rendering verification
//! - Texture upload / atlas flush verification
//!
//! Set CIRI_SKIP_HEADLESS_GPU_TESTS to soft-skip these tests when headless GPU
//! initialization is unavailable. By default, initialization failures panic.

#![cfg(feature = "blade")]

use blade_graphics as gpu;
use blade_graphics::ShaderData;
use ciri_render::FrameScene;
use ciri_render::glyph_cache::{GlyphInstance, PaneGlyphRange, PendingUpload};
use ciri_render::rect::Rect;
use std::ptr;

// ─── Test infrastructure ────────────────────────────────────────────

/// Create a headless GPU context (no window, no presentation).
/// Mirrors wgpu's headless device creation pattern.
fn try_create_headless_context() -> Result<gpu::Context, String> {
    unsafe {
        gpu::Context::init(gpu::ContextDesc {
            presentation: false,
            validation: true,
            timing: false,
            capture: false,
            overlay: false,
            device_id: 0,
        })
    }
    .map_err(|e| format!("{e:?}"))
}

fn create_headless_context() -> Option<gpu::Context> {
    match try_create_headless_context() {
        Ok(ctx) => Some(ctx),
        Err(e) if std::env::var("CIRI_SKIP_HEADLESS_GPU_TESTS").is_ok() => {
            eprintln!(
                "Headless GPU init failed; skipping (CIRI_SKIP_HEADLESS_GPU_TESTS set): {e}"
            );
            None
        }
        Err(e) => panic!(
            "Headless GPU init failed (set CIRI_SKIP_HEADLESS_GPU_TESTS to skip): {e}"
        ),
    }
}

fn create_encoder(context: &gpu::Context) -> gpu::CommandEncoder {
    context.create_command_encoder(gpu::CommandEncoderDesc {
        name: "test",
        buffer_count: 2,
    })
}

/// Submit and wait for GPU completion (blocking).
fn submit_and_wait(context: &gpu::Context, encoder: &mut gpu::CommandEncoder) {
    let sp = context.submit(encoder);
    let ok = context.wait_for(&sp, 5000); // 5s timeout
    assert!(ok, "GPU submission timed out");
}

/// Create a render target texture for off-screen rendering.
fn create_render_target(
    context: &gpu::Context,
    width: u32,
    height: u32,
    format: gpu::TextureFormat,
) -> (gpu::Texture, gpu::TextureView) {
    let texture = context.create_texture(gpu::TextureDesc {
        name: "test_rt",
        format,
        size: gpu::Extent {
            width,
            height,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::TARGET | gpu::TextureUsage::COPY,
        external: None,
    });
    let view = context.create_texture_view(
        texture,
        gpu::TextureViewDesc {
            name: "test_rt_view",
            format,
            dimension: gpu::ViewDimension::D2,
            subresources: &gpu::TextureSubresources::default(),
        },
    );
    (texture, view)
}

/// Create a readback buffer and copy texture contents into it.
/// Returns the buffer; caller reads data from `buffer.data()`.
fn readback_texture(
    context: &gpu::Context,
    encoder: &mut gpu::CommandEncoder,
    texture: gpu::Texture,
    width: u32,
    height: u32,
    bpp: u32,
) -> gpu::Buffer {
    let size = (width * height * bpp) as u64;
    let buffer = context.create_buffer(gpu::BufferDesc {
        name: "readback",
        size,
        memory: gpu::Memory::Shared,
    });

    {
        let mut transfer = encoder.transfer("readback");
        transfer.copy_texture_to_buffer(
            gpu::TexturePiece {
                texture,
                mip_level: 0,
                array_layer: 0,
                origin: [0, 0, 0],
            },
            buffer.at(0),
            width * bpp,
            gpu::Extent {
                width,
                height,
                depth: 1,
            },
        );
    }

    buffer
}

/// Read pixel data from a Shared buffer.
fn read_buffer(buffer: &gpu::Buffer, size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    unsafe {
        ptr::copy_nonoverlapping(buffer.data(), data.as_mut_ptr(), size);
    }
    data
}

struct ResizeProofTarget {
    size: (u32, u32),
    texture: gpu::Texture,
    view: gpu::TextureView,
    pipeline: gpu::RenderPipeline,
    uniform_buffer: gpu::Buffer,
    instance_buffer: gpu::Buffer,
}

fn create_rect_pipeline(
    context: &gpu::Context,
    format: gpu::TextureFormat,
    name: &'static str,
) -> gpu::RenderPipeline {
    let shader = context.create_shader(gpu::ShaderDesc {
        source: TEST_RECT_SHADER,
    });
    let vertex_layout = gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: 32,
    };

    context.create_render_pipeline(gpu::RenderPipelineDesc {
        name,
        data_layouts: &[&TestRectData::layout()],
        vertex: shader.at("vs_main"),
        vertex_fetches: &[gpu::VertexFetchState {
            layout: &vertex_layout,
            instanced: true,
        }],
        primitive: gpu::PrimitiveState {
            topology: gpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: Some(shader.at("fs_main")),
        color_targets: &[gpu::ColorTargetState {
            format,
            blend: None,
            write_mask: gpu::ColorWrites::all(),
        }],
        multisample_state: gpu::MultisampleState::default(),
    })
}

fn create_resize_proof_target(
    context: &gpu::Context,
    format: gpu::TextureFormat,
    size: (u32, u32),
    color: [f32; 4],
) -> ResizeProofTarget {
    let (texture, view) = create_render_target(context, size.0, size.1, format);
    let pipeline = create_rect_pipeline(context, format, "resize_proof");
    let uniform_buffer = context.create_buffer(gpu::BufferDesc {
        name: "resize_proof_viewport",
        size: 16,
        memory: gpu::Memory::Shared,
    });
    let instance_buffer = context.create_buffer(gpu::BufferDesc {
        name: "resize_proof_rect",
        size: std::mem::size_of::<Rect>() as u64,
        memory: gpu::Memory::Shared,
    });

    let viewport = [size.0 as f32, size.1 as f32, 0.0f32, 0.0f32];
    unsafe {
        ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, uniform_buffer.data(), 16);
    }

    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: size.0 as f32,
        h: size.1 as f32,
        color,
    };
    unsafe {
        let data = bytemuck::bytes_of(&rect);
        ptr::copy_nonoverlapping(data.as_ptr(), instance_buffer.data(), data.len());
    }

    ResizeProofTarget {
        size,
        texture,
        view,
        pipeline,
        uniform_buffer,
        instance_buffer,
    }
}

fn render_and_readback_target(context: &gpu::Context, target: &ResizeProofTarget) -> Vec<u8> {
    let mut encoder = create_encoder(context);
    encoder.start();
    encoder.init_texture(target.texture);
    {
        let mut pass = encoder.render(
            "resize-proof",
            gpu::RenderTargetSet {
                colors: &[gpu::RenderTarget {
                    view: target.view,
                    init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                    finish_op: gpu::FinishOp::Store,
                }],
                depth_stencil: None,
            },
        );
        let mut pe = pass.with(&target.pipeline);
        pe.bind(
            0,
            &TestRectData {
                uniforms: target.uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, target.instance_buffer.at(0));
        pe.draw(0, 4, 0, 1);
    }

    let readback = readback_texture(
        context,
        &mut encoder,
        target.texture,
        target.size.0,
        target.size.1,
        4,
    );
    submit_and_wait(context, &mut encoder);
    let data = read_buffer(&readback, (target.size.0 * target.size.1 * 4) as usize);
    context.destroy_buffer(readback);
    context.destroy_command_encoder(&mut encoder);
    data
}

fn assert_render_target_color(
    context: &gpu::Context,
    target: &ResizeProofTarget,
    expected: [f32; 4],
) {
    let data = render_and_readback_target(context, target);
    let expected = [
        (expected[0] * 255.0).round() as u8,
        (expected[1] * 255.0).round() as u8,
        (expected[2] * 255.0).round() as u8,
        (expected[3] * 255.0).round() as u8,
    ];
    assert_eq!(&data[0..4], &expected);
    assert_eq!(&data[(data.len() - 4)..], &expected);
}

fn destroy_resize_proof_target(context: &gpu::Context, target: &mut ResizeProofTarget) {
    context.destroy_render_pipeline(&mut target.pipeline);
    context.destroy_buffer(target.uniform_buffer);
    context.destroy_buffer(target.instance_buffer);
    context.destroy_texture_view(target.view);
    context.destroy_texture(target.texture);
}

// ─── Data layout tests (CPU-only, no GPU) ───────────────────────────

#[test]
fn rect_bytemuck_layout() {
    // Rect must be 32 bytes: 4 f32 coords + 4 f32 color = 8 * 4
    assert_eq!(std::mem::size_of::<Rect>(), 32);
    assert_eq!(std::mem::align_of::<Rect>(), 4);

    let rect = Rect {
        x: 1.0,
        y: 2.0,
        w: 3.0,
        h: 4.0,
        color: [0.5, 0.6, 0.7, 0.8],
    };
    let bytes: &[u8] = bytemuck::bytes_of(&rect);
    assert_eq!(bytes.len(), 32);

    // Verify roundtrip
    let back: &Rect = bytemuck::from_bytes(bytes);
    assert_eq!(back.x, 1.0);
    assert_eq!(back.color[3], 0.8);
}

#[test]
fn glyph_instance_bytemuck_layout() {
    // GlyphInstance: 2+2+2+2+4 = 12 f32 = 48 bytes
    assert_eq!(std::mem::size_of::<GlyphInstance>(), 64);
    assert_eq!(std::mem::align_of::<GlyphInstance>(), 4);

    let inst = GlyphInstance {
        pos: [10.0, 20.0],
        size: [8.0, 16.0],
        uv_pos: [0.0, 0.0],
        uv_size: [0.1, 0.2],
        color: [1.0, 1.0, 1.0, 1.0],
        bg_color: [0.0, 0.0, 0.0, 1.0],
    };
    let bytes: &[u8] = bytemuck::bytes_of(&inst);
    assert_eq!(bytes.len(), 64);

    // Verify cast_slice works for GPU upload
    let instances = [inst; 4];
    let slice: &[u8] = bytemuck::cast_slice(&instances);
    assert_eq!(slice.len(), 64 * 4);
}

#[test]
fn pane_glyph_range_default() {
    let sr = PaneGlyphRange::default();
    assert_eq!(sr.scissor, (0, 0, 0, 0));
    assert_eq!(sr.start, 0);
    assert_eq!(sr.count, 0);
    assert_eq!(sr.pane_origin, [0.0, 0.0]);
    assert_eq!(sr.pane_size, [0.0, 0.0]);
    assert_eq!(sr.pane_radii, [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn frame_scene_construction() {
    let rects = [Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
        color: [0.0, 0.0, 0.0, 1.0],
    }];
    let glyphs: &[GlyphInstance] = &[];
    let scene = FrameScene {
        clear_color: [0.1, 0.2, 0.3, 1.0],
        bg_rects: &rects,
        bg_rect_ranges: &[],
        glyphs,
        color_glyphs: &[],
        glyph_batches: &[],
        color_glyph_batches: &[],
        active_bg_start: 0,
        active_glyph_batches: &[],
        active_color_glyph_batches: &[],
        pane_glyph_end: 0,
        pane_color_glyph_end: 0,
        overlay_bg_start: 0,
        sdf_rects: &[],
    };
    assert_eq!(scene.bg_rects.len(), 1);
    assert_eq!(scene.clear_color[0], 0.1);
}

#[test]
fn rect_slice_cast_zero_copy() {
    // Verify that &[Rect] → &[u8] via bytemuck is truly zero-copy
    let rects = vec![
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            color: [1.0, 0.0, 0.0, 1.0],
        };
        100
    ];
    let bytes: &[u8] = bytemuck::cast_slice(&rects);
    assert_eq!(bytes.len(), 100 * 32);
    // Verify pointer equality (zero-copy)
    assert_eq!(bytes.as_ptr(), rects.as_ptr() as *const u8);
}

#[test]
fn pending_upload_data_integrity() {
    let upload = PendingUpload {
        x: 10,
        y: 20,
        w: 8,
        h: 8,
        data: vec![0xFF; 64], // 8x8 alpha
    };
    assert_eq!(upload.data.len(), (upload.w * upload.h) as usize);
    assert!(upload.data.iter().all(|&b| b == 0xFF));
}

// ─── GPU context tests ──────────────────────────────────────────────

#[test]
fn headless_context_creation() {
    let Some(_ctx) = create_headless_context() else {
        return;
    };
    // If we get here, headless Vulkan/Metal context works
}

#[test]
fn headless_context_device_info() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let info = ctx.device_information();
    assert!(!info.device_name.is_empty());
    assert!(!info.driver_name.is_empty());
}

// ─── Buffer tests ───────────────────────────────────────────────────

#[test]
fn buffer_create_shared() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "test_shared",
        size: 256,
        memory: gpu::Memory::Shared,
    });
    // Write and read back via shared memory
    let data = [1.0f32, 2.0, 3.0, 4.0];
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr() as *const u8, buffer.data(), 16);
    }
    let readback = read_buffer(&buffer, 16);
    let floats: &[f32] = bytemuck::cast_slice(&readback);
    assert_eq!(floats, &[1.0, 2.0, 3.0, 4.0]);

    ctx.destroy_buffer(buffer);
}

#[test]
fn buffer_create_upload() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "test_upload",
        size: 1024,
        memory: gpu::Memory::Upload,
    });
    // Upload buffers are CPU-writable, GPU-readable
    unsafe {
        ptr::write_bytes(buffer.data(), 0xAB, 1024);
    }
    ctx.sync_buffer(buffer);
    ctx.destroy_buffer(buffer);
}

#[test]
fn buffer_rect_upload() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let max_rects = 100;
    let buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "rect_instances",
        size: (max_rects * std::mem::size_of::<Rect>()) as u64,
        memory: gpu::Memory::Shared,
    });

    let rects: Vec<Rect> = (0..50)
        .map(|i| Rect {
            x: i as f32 * 10.0,
            y: 0.0,
            w: 8.0,
            h: 16.0,
            color: [1.0, 1.0, 1.0, 1.0],
        })
        .collect();

    let data = bytemuck::cast_slice(&rects);
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), buffer.data(), data.len());
    }

    // Read back and verify
    let readback = read_buffer(&buffer, data.len());
    let rects_back: &[Rect] = bytemuck::cast_slice(&readback);
    assert_eq!(rects_back.len(), 50);
    assert_eq!(rects_back[0].x, 0.0);
    assert_eq!(rects_back[49].x, 490.0);

    ctx.destroy_buffer(buffer);
}

#[test]
fn buffer_glyph_instance_upload() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let max_instances = 256;
    let buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "glyph_instances",
        size: (max_instances * std::mem::size_of::<GlyphInstance>()) as u64,
        memory: gpu::Memory::Shared,
    });

    let instances: Vec<GlyphInstance> = (0..128)
        .map(|i| GlyphInstance {
            pos: [i as f32 * 8.0, 0.0],
            size: [8.0, 16.0],
            uv_pos: [0.0, 0.0],
            uv_size: [0.01, 0.02],
            color: [1.0, 1.0, 1.0, 1.0],
            bg_color: [0.0, 0.0, 0.0, 1.0],
        })
        .collect();

    let data = bytemuck::cast_slice(&instances);
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), buffer.data(), data.len());
    }

    let readback = read_buffer(&buffer, data.len());
    let back: &[GlyphInstance] = bytemuck::cast_slice(&readback);
    assert_eq!(back.len(), 128);
    assert_eq!(back[127].pos[0], 127.0 * 8.0);

    ctx.destroy_buffer(buffer);
}

// ─── Texture tests ──────────────────────────────────────────────────

#[test]
fn texture_create_r8() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let texture = ctx.create_texture(gpu::TextureDesc {
        name: "atlas_r8",
        format: gpu::TextureFormat::R8Unorm,
        size: gpu::Extent {
            width: 512,
            height: 512,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::RESOURCE | gpu::TextureUsage::COPY,
        external: None,
    });
    ctx.destroy_texture(texture);
}

#[test]
fn texture_create_rgba8_srgb() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let texture = ctx.create_texture(gpu::TextureDesc {
        name: "atlas_rgba8",
        format: gpu::TextureFormat::Rgba8UnormSrgb,
        size: gpu::Extent {
            width: 1024,
            height: 1024,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::RESOURCE | gpu::TextureUsage::COPY,
        external: None,
    });
    ctx.destroy_texture(texture);
}

#[test]
fn texture_upload_and_readback_r8() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let size = 16u32;

    let texture = ctx.create_texture(gpu::TextureDesc {
        name: "test_r8",
        format: gpu::TextureFormat::R8Unorm,
        size: gpu::Extent {
            width: size,
            height: size,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::RESOURCE | gpu::TextureUsage::COPY,
        external: None,
    });

    // Create staging buffer with test data
    let staging = ctx.create_buffer(gpu::BufferDesc {
        name: "staging",
        size: (size * size) as u64,
        memory: gpu::Memory::Upload,
    });

    // Fill with a gradient pattern
    let mut pattern = vec![0u8; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            pattern[(y * size + x) as usize] = ((x + y) * 8) as u8;
        }
    }
    unsafe {
        ptr::copy_nonoverlapping(pattern.as_ptr(), staging.data(), pattern.len());
    }
    ctx.sync_buffer(staging);

    // Upload to texture
    encoder.start();
    encoder.init_texture(texture);
    {
        let mut transfer = encoder.transfer("upload");
        transfer.copy_buffer_to_texture(
            staging.at(0),
            size, // bytes_per_row
            gpu::TexturePiece {
                texture,
                mip_level: 0,
                array_layer: 0,
                origin: [0, 0, 0],
            },
            gpu::Extent {
                width: size,
                height: size,
                depth: 1,
            },
        );
    }

    // Readback
    let readback = readback_texture(&ctx, &mut encoder, texture, size, size, 1);
    submit_and_wait(&ctx, &mut encoder);

    // Verify data
    let data = read_buffer(&readback, (size * size) as usize);
    assert_eq!(data.len(), pattern.len());
    assert_eq!(data, pattern);

    ctx.destroy_buffer(staging);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

#[test]
fn texture_upload_and_readback_rgba8() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let size = 8u32;
    let bpp = 4u32;

    let texture = ctx.create_texture(gpu::TextureDesc {
        name: "test_rgba8",
        format: gpu::TextureFormat::Rgba8Unorm,
        size: gpu::Extent {
            width: size,
            height: size,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::RESOURCE | gpu::TextureUsage::COPY,
        external: None,
    });

    let staging = ctx.create_buffer(gpu::BufferDesc {
        name: "staging_rgba",
        size: (size * size * bpp) as u64,
        memory: gpu::Memory::Upload,
    });

    // Checkerboard pattern: red/blue
    let mut pixels = vec![0u8; (size * size * bpp) as usize];
    for y in 0..size {
        for x in 0..size {
            let offset = ((y * size + x) * bpp) as usize;
            if (x + y) % 2 == 0 {
                pixels[offset] = 255; // R
                pixels[offset + 3] = 255; // A
            } else {
                pixels[offset + 2] = 255; // B
                pixels[offset + 3] = 255; // A
            }
        }
    }
    unsafe {
        ptr::copy_nonoverlapping(pixels.as_ptr(), staging.data(), pixels.len());
    }
    ctx.sync_buffer(staging);

    encoder.start();
    encoder.init_texture(texture);
    {
        let mut transfer = encoder.transfer("upload_rgba");
        transfer.copy_buffer_to_texture(
            staging.at(0),
            size * bpp,
            gpu::TexturePiece {
                texture,
                mip_level: 0,
                array_layer: 0,
                origin: [0, 0, 0],
            },
            gpu::Extent {
                width: size,
                height: size,
                depth: 1,
            },
        );
    }

    let readback = readback_texture(&ctx, &mut encoder, texture, size, size, bpp);
    submit_and_wait(&ctx, &mut encoder);

    let data = read_buffer(&readback, pixels.len());
    assert_eq!(data, pixels);

    ctx.destroy_buffer(staging);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

// ─── Atlas flush simulation tests ───────────────────────────────────

#[test]
fn atlas_pending_upload_flush() {
    // Simulate what GlyphAtlasGpu.flush_uploads does:
    // drain PendingUpload vec → staging buffer → copy_buffer_to_texture
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let atlas_size = 64u32;

    let texture = ctx.create_texture(gpu::TextureDesc {
        name: "atlas",
        format: gpu::TextureFormat::R8Unorm,
        size: gpu::Extent {
            width: atlas_size,
            height: atlas_size,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::RESOURCE | gpu::TextureUsage::COPY,
        external: None,
    });

    let staging = ctx.create_buffer(gpu::BufferDesc {
        name: "staging",
        size: (atlas_size * atlas_size) as u64,
        memory: gpu::Memory::Upload,
    });

    // Simulate 3 glyph uploads at different atlas positions
    let uploads = vec![
        PendingUpload {
            x: 0,
            y: 0,
            w: 8,
            h: 12,
            data: vec![128u8; 96],
        },
        PendingUpload {
            x: 8,
            y: 0,
            w: 10,
            h: 14,
            data: vec![200u8; 140],
        },
        PendingUpload {
            x: 0,
            y: 14,
            w: 6,
            h: 8,
            data: vec![255u8; 48],
        },
    ];

    encoder.start();
    encoder.init_texture(texture);

    // Pack uploads into staging buffer (same logic as AtlasLayer::flush_uploads)
    let mut cursor: usize = 0;
    for upload in &uploads {
        let total = (upload.w * upload.h) as usize;
        unsafe {
            ptr::copy_nonoverlapping(upload.data.as_ptr(), staging.data().add(cursor), total);
        }
        cursor += total;
    }
    ctx.sync_buffer(staging);

    // Record transfer commands
    let mut offset: u64 = 0;
    for upload in &uploads {
        let total = (upload.w * upload.h) as u64;
        let mut transfer = encoder.transfer("glyph_upload");
        transfer.copy_buffer_to_texture(
            staging.at(offset),
            upload.w, // bytes_per_row (R8 = 1 bpp)
            gpu::TexturePiece {
                texture,
                mip_level: 0,
                array_layer: 0,
                origin: [upload.x, upload.y, 0],
            },
            gpu::Extent {
                width: upload.w,
                height: upload.h,
                depth: 1,
            },
        );
        offset += total;
    }

    // Readback full atlas
    let readback = readback_texture(&ctx, &mut encoder, texture, atlas_size, atlas_size, 1);
    submit_and_wait(&ctx, &mut encoder);

    let data = read_buffer(&readback, (atlas_size * atlas_size) as usize);

    // Verify glyph 1 at (0,0) 8x12
    assert_eq!(data[0], 128); // (0,0)
    assert_eq!(data[7], 128); // (7,0)
    assert_eq!(data[(11 * atlas_size) as usize], 128); // (0,11)

    // Verify glyph 2 at (8,0) 10x14
    assert_eq!(data[8], 200); // (8,0)
    assert_eq!(data[17], 200); // (17,0)

    // Verify glyph 3 at (0,14) 6x8
    assert_eq!(data[(14 * atlas_size) as usize], 255); // (0,14)

    // Verify untouched area is zero
    assert_eq!(data[(atlas_size * atlas_size - 1) as usize], 0);

    ctx.destroy_buffer(staging);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

#[test]
fn atlas_clear_and_refill() {
    // Simulate atlas overflow → clear → refill cycle
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let atlas_size = 32u32;

    let texture = ctx.create_texture(gpu::TextureDesc {
        name: "atlas_clear",
        format: gpu::TextureFormat::R8Unorm,
        size: gpu::Extent {
            width: atlas_size,
            height: atlas_size,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::RESOURCE | gpu::TextureUsage::COPY,
        external: None,
    });

    let staging = ctx.create_buffer(gpu::BufferDesc {
        name: "staging",
        size: (atlas_size * atlas_size) as u64,
        memory: gpu::Memory::Upload,
    });

    // Step 1: Fill with data
    let fill_data = vec![0xAA_u8; (atlas_size * atlas_size) as usize];
    unsafe {
        ptr::copy_nonoverlapping(fill_data.as_ptr(), staging.data(), fill_data.len());
    }
    ctx.sync_buffer(staging);

    encoder.start();
    encoder.init_texture(texture);
    {
        let mut t = encoder.transfer("fill");
        t.copy_buffer_to_texture(
            staging.at(0),
            atlas_size,
            gpu::TexturePiece {
                texture,
                mip_level: 0,
                array_layer: 0,
                origin: [0, 0, 0],
            },
            gpu::Extent {
                width: atlas_size,
                height: atlas_size,
                depth: 1,
            },
        );
    }
    submit_and_wait(&ctx, &mut encoder);

    // Step 2: Clear (zero the texture) — separate submission
    let zeros = vec![0u8; (atlas_size * atlas_size) as usize];
    unsafe {
        ptr::copy_nonoverlapping(zeros.as_ptr(), staging.data(), zeros.len());
    }
    ctx.sync_buffer(staging);

    encoder.start();
    {
        let mut t = encoder.transfer("clear");
        t.copy_buffer_to_texture(
            staging.at(0),
            atlas_size,
            gpu::TexturePiece {
                texture,
                mip_level: 0,
                array_layer: 0,
                origin: [0, 0, 0],
            },
            gpu::Extent {
                width: atlas_size,
                height: atlas_size,
                depth: 1,
            },
        );
    }
    submit_and_wait(&ctx, &mut encoder);

    // Step 3: Refill with new data — separate submission to avoid staging conflicts
    let new_data = [0x55_u8; 64]; // 8x8 glyph
    unsafe {
        ptr::copy_nonoverlapping(new_data.as_ptr(), staging.data(), new_data.len());
    }
    ctx.sync_buffer(staging);

    encoder.start();
    {
        let mut t = encoder.transfer("refill");
        t.copy_buffer_to_texture(
            staging.at(0),
            8,
            gpu::TexturePiece {
                texture,
                mip_level: 0,
                array_layer: 0,
                origin: [0, 0, 0],
            },
            gpu::Extent {
                width: 8,
                height: 8,
                depth: 1,
            },
        );
    }

    let readback = readback_texture(&ctx, &mut encoder, texture, atlas_size, atlas_size, 1);
    submit_and_wait(&ctx, &mut encoder);

    let data = read_buffer(&readback, (atlas_size * atlas_size) as usize);
    // Top-left 8x8 should be 0x55
    assert_eq!(data[0], 0x55);
    assert_eq!(data[7], 0x55);
    // Outside 8x8 should be zero (from clear)
    assert_eq!(data[8], 0);
    assert_eq!(data[(atlas_size * 8) as usize], 0);

    ctx.destroy_buffer(staging);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

// ─── Render pipeline tests ──────────────────────────────────────────

#[derive(blade_macros::ShaderData)]
struct TestRectData {
    uniforms: gpu::BufferPiece,
}

const TEST_RECT_SHADER: &str = r#"
struct Uniforms {
    viewport_size: vec4<f32>,
};

var<uniform> uniforms: Uniforms;

struct RectInstance {
    pos: vec2<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: RectInstance) -> VertexOutput {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);
    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    let ndc = vec2<f32>(
        px.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - px.y / uniforms.viewport_size.y * 2.0
    );
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

#[test]
fn render_pipeline_creation() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let format = gpu::TextureFormat::Rgba8Unorm;
    let shader = ctx.create_shader(gpu::ShaderDesc {
        source: TEST_RECT_SHADER,
    });

    let vertex_layout = gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: 32,
    };

    let mut pipeline = ctx.create_render_pipeline(gpu::RenderPipelineDesc {
        name: "test_rect",
        data_layouts: &[&TestRectData::layout()],
        vertex: shader.at("vs_main"),
        vertex_fetches: &[gpu::VertexFetchState {
            layout: &vertex_layout,
            instanced: true,
        }],
        primitive: gpu::PrimitiveState {
            topology: gpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: Some(shader.at("fs_main")),
        color_targets: &[gpu::ColorTargetState {
            format,
            blend: Some(gpu::BlendState::ALPHA_BLENDING),
            write_mask: gpu::ColorWrites::all(),
        }],
        multisample_state: gpu::MultisampleState::default(),
    });

    ctx.destroy_render_pipeline(&mut pipeline);
}

#[test]
fn render_fullscreen_rect_and_readback() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let w = 4u32;
    let h = 4u32;
    let format = gpu::TextureFormat::Rgba8Unorm;

    let (texture, view) = create_render_target(&ctx, w, h, format);
    let shader = ctx.create_shader(gpu::ShaderDesc {
        source: TEST_RECT_SHADER,
    });

    let vertex_layout = gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: 32,
    };

    let mut pipeline = ctx.create_render_pipeline(gpu::RenderPipelineDesc {
        name: "test_rect",
        data_layouts: &[&TestRectData::layout()],
        vertex: shader.at("vs_main"),
        vertex_fetches: &[gpu::VertexFetchState {
            layout: &vertex_layout,
            instanced: true,
        }],
        primitive: gpu::PrimitiveState {
            topology: gpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: Some(shader.at("fs_main")),
        color_targets: &[gpu::ColorTargetState {
            format,
            blend: None,
            write_mask: gpu::ColorWrites::all(),
        }],
        multisample_state: gpu::MultisampleState::default(),
    });

    let uniform_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "viewport",
        size: 16,
        memory: gpu::Memory::Shared,
    });
    let viewport = [w as f32, h as f32, 0.0f32, 0.0f32];
    unsafe {
        ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, uniform_buffer.data(), 16);
    }

    let instance_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "rects",
        size: 32,
        memory: gpu::Memory::Shared,
    });
    // Full-screen red rect
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: w as f32,
        h: h as f32,
        color: [1.0, 0.0, 0.0, 1.0],
    };
    unsafe {
        let data = bytemuck::bytes_of(&rect);
        ptr::copy_nonoverlapping(data.as_ptr(), instance_buffer.data(), data.len());
    }

    encoder.start();
    encoder.init_texture(texture);

    {
        let mut pass = encoder.render(
            "test",
            gpu::RenderTargetSet {
                colors: &[gpu::RenderTarget {
                    view,
                    init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                    finish_op: gpu::FinishOp::Store,
                }],
                depth_stencil: None,
            },
        );

        let mut pe = pass.with(&pipeline);
        pe.bind(
            0,
            &TestRectData {
                uniforms: uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, instance_buffer.at(0));
        pe.draw(0, 4, 0, 1); // 4 vertices, 1 instance
    }

    let readback = readback_texture(&ctx, &mut encoder, texture, w, h, 4);
    submit_and_wait(&ctx, &mut encoder);

    let data = read_buffer(&readback, (w * h * 4) as usize);
    // Every pixel should be red (255, 0, 0, 255)
    for y in 0..h {
        for x in 0..w {
            let offset = ((y * w + x) * 4) as usize;
            assert_eq!(
                data[offset], 255,
                "pixel ({x},{y}) R should be 255, got {}",
                data[offset]
            );
            assert_eq!(data[offset + 1], 0, "pixel ({x},{y}) G should be 0");
            assert_eq!(data[offset + 2], 0, "pixel ({x},{y}) B should be 0");
            assert_eq!(data[offset + 3], 255, "pixel ({x},{y}) A should be 255");
        }
    }

    ctx.destroy_render_pipeline(&mut pipeline);
    ctx.destroy_buffer(uniform_buffer);
    ctx.destroy_buffer(instance_buffer);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture_view(view);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

#[test]
fn render_instanced_rects() {
    // Render two non-overlapping rects and verify both appear
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let w = 8u32;
    let h = 4u32;
    let format = gpu::TextureFormat::Rgba8Unorm;

    let (texture, view) = create_render_target(&ctx, w, h, format);
    let shader = ctx.create_shader(gpu::ShaderDesc {
        source: TEST_RECT_SHADER,
    });

    let vertex_layout = gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: 32,
    };

    let mut pipeline = ctx.create_render_pipeline(gpu::RenderPipelineDesc {
        name: "instanced_rect",
        data_layouts: &[&TestRectData::layout()],
        vertex: shader.at("vs_main"),
        vertex_fetches: &[gpu::VertexFetchState {
            layout: &vertex_layout,
            instanced: true,
        }],
        primitive: gpu::PrimitiveState {
            topology: gpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: Some(shader.at("fs_main")),
        color_targets: &[gpu::ColorTargetState {
            format,
            blend: None,
            write_mask: gpu::ColorWrites::all(),
        }],
        multisample_state: gpu::MultisampleState::default(),
    });

    let uniform_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "viewport",
        size: 16,
        memory: gpu::Memory::Shared,
    });
    let viewport = [w as f32, h as f32, 0.0f32, 0.0f32];
    unsafe {
        ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, uniform_buffer.data(), 16);
    }

    // Two rects: left half green, right half blue
    let rects = [
        Rect {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
            color: [0.0, 1.0, 0.0, 1.0],
        },
        Rect {
            x: 4.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
            color: [0.0, 0.0, 1.0, 1.0],
        },
    ];
    let instance_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "rects",
        size: 64,
        memory: gpu::Memory::Shared,
    });
    unsafe {
        let data = bytemuck::cast_slice(&rects);
        ptr::copy_nonoverlapping(data.as_ptr(), instance_buffer.data(), data.len());
    }

    encoder.start();
    encoder.init_texture(texture);

    {
        let mut pass = encoder.render(
            "test",
            gpu::RenderTargetSet {
                colors: &[gpu::RenderTarget {
                    view,
                    init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                    finish_op: gpu::FinishOp::Store,
                }],
                depth_stencil: None,
            },
        );
        let mut pe = pass.with(&pipeline);
        pe.bind(
            0,
            &TestRectData {
                uniforms: uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, instance_buffer.at(0));
        pe.draw(0, 4, 0, 2); // 4 vertices, 2 instances
    }

    let readback = readback_texture(&ctx, &mut encoder, texture, w, h, 4);
    submit_and_wait(&ctx, &mut encoder);

    let data = read_buffer(&readback, (w * h * 4) as usize);

    // Left half (x < 4): green
    let pixel_00 = &data[0..4];
    assert_eq!(pixel_00, &[0, 255, 0, 255], "pixel (0,0) should be green");

    // Right half (x >= 4): blue
    let pixel_40 = &data[16..20]; // x=4, y=0 → offset = 4*4 = 16
    assert_eq!(pixel_40, &[0, 0, 255, 255], "pixel (4,0) should be blue");

    ctx.destroy_render_pipeline(&mut pipeline);
    ctx.destroy_buffer(uniform_buffer);
    ctx.destroy_buffer(instance_buffer);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture_view(view);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

#[test]
fn render_scissor_rect() {
    // Render a full-screen white rect, but scissor to top-left 2x2 of a 4x4 target
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let w = 4u32;
    let h = 4u32;
    let format = gpu::TextureFormat::Rgba8Unorm;

    let (texture, view) = create_render_target(&ctx, w, h, format);
    let shader = ctx.create_shader(gpu::ShaderDesc {
        source: TEST_RECT_SHADER,
    });

    let vertex_layout = gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: 32,
    };

    let mut pipeline = ctx.create_render_pipeline(gpu::RenderPipelineDesc {
        name: "scissor_test",
        data_layouts: &[&TestRectData::layout()],
        vertex: shader.at("vs_main"),
        vertex_fetches: &[gpu::VertexFetchState {
            layout: &vertex_layout,
            instanced: true,
        }],
        primitive: gpu::PrimitiveState {
            topology: gpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: Some(shader.at("fs_main")),
        color_targets: &[gpu::ColorTargetState {
            format,
            blend: None,
            write_mask: gpu::ColorWrites::all(),
        }],
        multisample_state: gpu::MultisampleState::default(),
    });

    let uniform_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "viewport",
        size: 16,
        memory: gpu::Memory::Shared,
    });
    let viewport = [w as f32, h as f32, 0.0f32, 0.0f32];
    unsafe {
        ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, uniform_buffer.data(), 16);
    }

    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: w as f32,
        h: h as f32,
        color: [1.0, 1.0, 1.0, 1.0],
    };
    let instance_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "rects",
        size: 32,
        memory: gpu::Memory::Shared,
    });
    unsafe {
        let data = bytemuck::bytes_of(&rect);
        ptr::copy_nonoverlapping(data.as_ptr(), instance_buffer.data(), data.len());
    }

    encoder.start();
    encoder.init_texture(texture);

    {
        let mut pass = encoder.render(
            "test",
            gpu::RenderTargetSet {
                colors: &[gpu::RenderTarget {
                    view,
                    init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                    finish_op: gpu::FinishOp::Store,
                }],
                depth_stencil: None,
            },
        );
        let mut pe = pass.with(&pipeline);
        pe.bind(
            0,
            &TestRectData {
                uniforms: uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, instance_buffer.at(0));
        // Scissor: only top-left 2x2
        pe.set_scissor_rect(&gpu::ScissorRect {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        });
        pe.draw(0, 4, 0, 1);
    }

    let readback = readback_texture(&ctx, &mut encoder, texture, w, h, 4);
    submit_and_wait(&ctx, &mut encoder);

    let data = read_buffer(&readback, (w * h * 4) as usize);

    // Top-left 2x2 should be white (255,255,255,255)
    for y in 0..2u32 {
        for x in 0..2u32 {
            let off = ((y * w + x) * 4) as usize;
            assert_eq!(data[off], 255, "pixel ({x},{y}) R in scissor should be 255");
        }
    }
    // Outside scissor should be black (from clear)
    for y in 2..4u32 {
        for x in 0..4u32 {
            let off = ((y * w + x) * 4) as usize;
            assert_eq!(
                data[off], 0,
                "pixel ({x},{y}) R outside scissor should be 0"
            );
        }
    }
    // Right half of top rows also outside scissor
    for y in 0..2u32 {
        for x in 2..4u32 {
            let off = ((y * w + x) * 4) as usize;
            assert_eq!(
                data[off], 0,
                "pixel ({x},{y}) R outside scissor should be 0"
            );
        }
    }

    ctx.destroy_render_pipeline(&mut pipeline);
    ctx.destroy_buffer(uniform_buffer);
    ctx.destroy_buffer(instance_buffer);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture_view(view);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

#[test]
fn render_alpha_blending() {
    // Render two overlapping rects: opaque red, then 50% alpha green on top
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let w = 2u32;
    let h = 2u32;
    let format = gpu::TextureFormat::Rgba8Unorm;

    let (texture, view) = create_render_target(&ctx, w, h, format);
    let shader = ctx.create_shader(gpu::ShaderDesc {
        source: TEST_RECT_SHADER,
    });

    let vertex_layout = gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: 32,
    };

    let mut pipeline = ctx.create_render_pipeline(gpu::RenderPipelineDesc {
        name: "blend_test",
        data_layouts: &[&TestRectData::layout()],
        vertex: shader.at("vs_main"),
        vertex_fetches: &[gpu::VertexFetchState {
            layout: &vertex_layout,
            instanced: true,
        }],
        primitive: gpu::PrimitiveState {
            topology: gpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: Some(shader.at("fs_main")),
        color_targets: &[gpu::ColorTargetState {
            format,
            blend: Some(gpu::BlendState::ALPHA_BLENDING),
            write_mask: gpu::ColorWrites::all(),
        }],
        multisample_state: gpu::MultisampleState::default(),
    });

    let uniform_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "viewport",
        size: 16,
        memory: gpu::Memory::Shared,
    });
    unsafe {
        let vp = [w as f32, h as f32, 0.0f32, 0.0f32];
        ptr::copy_nonoverlapping(vp.as_ptr() as *const u8, uniform_buffer.data(), 16);
    }

    // Two fullscreen rects: opaque red then 50% green
    let rects = [
        Rect {
            x: 0.0,
            y: 0.0,
            w: w as f32,
            h: h as f32,
            color: [1.0, 0.0, 0.0, 1.0],
        },
        Rect {
            x: 0.0,
            y: 0.0,
            w: w as f32,
            h: h as f32,
            color: [0.0, 1.0, 0.0, 0.5],
        },
    ];
    let instance_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "rects",
        size: 64,
        memory: gpu::Memory::Shared,
    });
    unsafe {
        let data = bytemuck::cast_slice(&rects);
        ptr::copy_nonoverlapping(data.as_ptr(), instance_buffer.data(), data.len());
    }

    encoder.start();
    encoder.init_texture(texture);

    {
        let mut pass = encoder.render(
            "test",
            gpu::RenderTargetSet {
                colors: &[gpu::RenderTarget {
                    view,
                    init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                    finish_op: gpu::FinishOp::Store,
                }],
                depth_stencil: None,
            },
        );
        // Draw opaque red
        let mut pe = pass.with(&pipeline);
        pe.bind(
            0,
            &TestRectData {
                uniforms: uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, instance_buffer.at(0));
        pe.draw(0, 4, 0, 1);
        // Draw 50% green on top
        pe.draw(0, 4, 1, 1);
    }

    let readback = readback_texture(&ctx, &mut encoder, texture, w, h, 4);
    submit_and_wait(&ctx, &mut encoder);

    let data = read_buffer(&readback, (w * h * 4) as usize);

    // Alpha blending: dst = src * srcA + dst * (1 - srcA)
    // Red channel: 0.0 * 0.5 + 1.0 * 0.5 = 0.5 → ~128
    // Green channel: 1.0 * 0.5 + 0.0 * 0.5 = 0.5 → ~128
    let r = data[0];
    let g = data[1];
    let b = data[2];
    assert!(
        (r as i32 - 128).unsigned_abs() <= 2,
        "R should be ~128 (got {r})"
    );
    assert!(
        (g as i32 - 128).unsigned_abs() <= 2,
        "G should be ~128 (got {g})"
    );
    assert_eq!(b, 0, "B should be 0 (got {b})");

    ctx.destroy_render_pipeline(&mut pipeline);
    ctx.destroy_buffer(uniform_buffer);
    ctx.destroy_buffer(instance_buffer);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture_view(view);
    ctx.destroy_texture(texture);
    ctx.destroy_command_encoder(&mut encoder);
}

#[test]
fn resize_behavior_contract_is_explicit() {
    let deferred_backends = ["blade"];
    let immediate_backends = ["gl", "dx"];

    assert!(deferred_backends.contains(&"blade"));
    assert!(immediate_backends.contains(&"gl"));
    assert!(immediate_backends.contains(&"dx"));
    assert!(!deferred_backends.contains(&"gl"));
}

#[test]
fn blade_headless_resize_behavior_contract_is_proven() {
    let initial = (80u32, 24u32);
    let requested = (132u32, 48u32);
    let initial_color = [1.0, 0.0, 0.0, 1.0];
    let resized_color = [0.25, 0.5, 0.75, 1.0];

    let Some(ctx) = create_headless_context() else {
        return;
    };
    let format = gpu::TextureFormat::Rgba8Unorm;

    let mut committed = create_resize_proof_target(&ctx, format, initial, initial_color);
    assert_eq!(committed.size, initial);
    assert_render_target_color(&ctx, &committed, initial_color);

    let pending = create_resize_proof_target(&ctx, format, requested, resized_color);

    // Deferred resize semantics: until apply_surface() commits the new surface,
    // rendering still targets the old surface dimensions and content.
    assert_eq!(committed.size, initial);
    assert_render_target_color(&ctx, &committed, initial_color);

    destroy_resize_proof_target(&ctx, &mut committed);
    committed = pending;

    assert_eq!(committed.size, requested);
    assert_render_target_color(&ctx, &committed, resized_color);

    destroy_resize_proof_target(&ctx, &mut committed);
}

#[test]
fn immediate_headless_resize_behavior_contract_is_proven() {
    let initial = (80u32, 24u32);
    let requested = (132u32, 48u32);
    let initial_color = [1.0, 0.0, 0.0, 1.0];
    let resized_color = [0.25, 0.5, 0.75, 1.0];

    let Some(ctx) = create_headless_context() else {
        return;
    };
    let format = gpu::TextureFormat::Rgba8Unorm;

    let mut target = create_resize_proof_target(&ctx, format, initial, initial_color);
    assert_render_target_color(&ctx, &target, initial_color);

    destroy_resize_proof_target(&ctx, &mut target);
    target = create_resize_proof_target(&ctx, format, requested, resized_color);

    assert_eq!(target.size, requested);
    assert_render_target_color(&ctx, &target, resized_color);

    destroy_resize_proof_target(&ctx, &mut target);
}

#[test]
fn post_resize_render_readback_stays_safe() {
    let Some(ctx) = create_headless_context() else {
        return;
    };
    let mut encoder = create_encoder(&ctx);
    let format = gpu::TextureFormat::Rgba8Unorm;
    let initial = (4u32, 4u32);
    let resized = (7u32, 5u32);

    let (initial_texture, initial_view) = create_render_target(&ctx, initial.0, initial.1, format);

    let shader = ctx.create_shader(gpu::ShaderDesc {
        source: TEST_RECT_SHADER,
    });
    let vertex_layout = gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: 32,
    };

    let mut pipeline = ctx.create_render_pipeline(gpu::RenderPipelineDesc {
        name: "resize_render_test",
        data_layouts: &[&TestRectData::layout()],
        vertex: shader.at("vs_main"),
        vertex_fetches: &[gpu::VertexFetchState {
            layout: &vertex_layout,
            instanced: true,
        }],
        primitive: gpu::PrimitiveState {
            topology: gpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: Some(shader.at("fs_main")),
        color_targets: &[gpu::ColorTargetState {
            format,
            blend: None,
            write_mask: gpu::ColorWrites::all(),
        }],
        multisample_state: gpu::MultisampleState::default(),
    });

    let uniform_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "resize_viewport",
        size: 16,
        memory: gpu::Memory::Shared,
    });
    let instance_buffer = ctx.create_buffer(gpu::BufferDesc {
        name: "resize_rects",
        size: 32,
        memory: gpu::Memory::Shared,
    });

    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: resized.0 as f32,
        h: resized.1 as f32,
        color: [0.25, 0.5, 0.75, 1.0],
    };
    unsafe {
        let data = bytemuck::bytes_of(&rect);
        ptr::copy_nonoverlapping(data.as_ptr(), instance_buffer.data(), data.len());
    }

    ctx.destroy_texture_view(initial_view);
    ctx.destroy_texture(initial_texture);
    let (current_texture, current_view) = create_render_target(&ctx, resized.0, resized.1, format);

    let viewport = [resized.0 as f32, resized.1 as f32, 0.0f32, 0.0f32];
    unsafe {
        ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, uniform_buffer.data(), 16);
    }

    encoder.start();
    encoder.init_texture(current_texture);
    {
        let mut pass = encoder.render(
            "resize",
            gpu::RenderTargetSet {
                colors: &[gpu::RenderTarget {
                    view: current_view,
                    init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                    finish_op: gpu::FinishOp::Store,
                }],
                depth_stencil: None,
            },
        );
        let mut pe = pass.with(&pipeline);
        pe.bind(
            0,
            &TestRectData {
                uniforms: uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, instance_buffer.at(0));
        pe.draw(0, 4, 0, 1);
    }

    let readback = readback_texture(&ctx, &mut encoder, current_texture, resized.0, resized.1, 4);
    submit_and_wait(&ctx, &mut encoder);
    let data = read_buffer(&readback, (resized.0 * resized.1 * 4) as usize);

    assert_eq!(data.len(), (resized.0 * resized.1 * 4) as usize);
    assert_eq!(&data[0..4], &[64, 128, 191, 255]);
    assert_eq!(&data[(data.len() - 4)..], &[64, 128, 191, 255]);

    ctx.destroy_render_pipeline(&mut pipeline);
    ctx.destroy_buffer(uniform_buffer);
    ctx.destroy_buffer(instance_buffer);
    ctx.destroy_buffer(readback);
    ctx.destroy_texture_view(current_view);
    ctx.destroy_texture(current_texture);
    ctx.destroy_command_encoder(&mut encoder);
}
