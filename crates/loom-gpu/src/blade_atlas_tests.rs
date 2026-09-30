//! Real GPU readback regressions for atlas upload, growth and clear ordering.

use super::*;

#[test]
fn atlas_upload_growth_and_clear_preserve_pixels() {
    let Ok(context) = (unsafe {
        gpu::Context::init(gpu::ContextDesc {
            presentation: false,
            validation: true,
            ..Default::default()
        })
    }) else {
        eprintln!("atlas readback: no GPU available, skipping");
        return;
    };
    let mut atlas = GlyphAtlasGpu::new(&context, gpu::TextureFormat::Rgba8Unorm, 512, 32768, 0);
    let mut encoder = context.create_command_encoder(gpu::CommandEncoderDesc {
        name: "atlas_readback_test",
        buffer_count: 2,
    });
    encoder.start();
    atlas.init_textures(&mut encoder);
    let sync = context.submit(&mut encoder);
    assert!(context.wait_for(&sync, 5000));

    for layer in [&mut atlas.alpha, &mut atlas.color] {
        let bpp = layer.bpp as usize;
        let readback = context.create_buffer(gpu::BufferDesc {
            name: "atlas_readback",
            size: (512 * 512 * bpp) as u64,
            memory: gpu::Memory::Shared,
        });
        assert!(
            layer.staging_buffer.is_none(),
            "unused layers need no upload buffer"
        );
        // Enough uniforms for the actual batches, independent of 32768 glyphs.
        assert!(layer.uniform_capacity < 32768);
        layer.ensure_uniform_capacity(&context, 513);
        layer.write_uniform(512, 512.0, 512.0, [0.0; 2], [512.0; 2], [0.0; 4]);

        // First read the initialized texture. Then a small upload, a payload
        // requiring staging growth, and a clear + two odd-sized uploads in
        // one submission. Every byte outside the uploaded glyphs must be zero.
        for step in 0..4 {
            let mut expected = vec![0u8; 512 * 512 * bpp];
            let mut uploads = match step {
                0 => vec![],
                1 => vec![PendingUpload {
                    x: 4,
                    y: 5,
                    w: 3,
                    h: 1,
                    data: vec![71; 3 * bpp],
                }],
                2 => vec![PendingUpload {
                    x: 0,
                    y: 0,
                    w: 512,
                    h: 512,
                    data: vec![119; 512 * 512 * bpp],
                }],
                _ => vec![
                    PendingUpload {
                        x: 10,
                        y: 20,
                        w: 3,
                        h: 1,
                        data: vec![173; 3 * bpp],
                    },
                    PendingUpload {
                        x: 30,
                        y: 40,
                        w: 5,
                        h: 3,
                        data: vec![231; 15 * bpp],
                    },
                ],
            };
            for upload in &uploads {
                for row in 0..upload.h as usize {
                    let start = ((upload.y as usize + row) * 512 + upload.x as usize) * bpp;
                    let len = upload.w as usize * bpp;
                    expected[start..start + len]
                        .copy_from_slice(&upload.data[row * len..(row + 1) * len]);
                }
            }
            encoder.start();
            layer.flush_uploads(&context, &mut encoder, &mut uploads, step == 3);
            assert!(uploads.is_empty(), "all glyphs must land in the same frame");
            {
                let mut transfer = encoder.transfer("atlas_readback");
                transfer.copy_texture_to_buffer(
                    gpu::TexturePiece {
                        texture: layer.texture,
                        mip_level: 0,
                        array_layer: 0,
                        origin: [0; 3],
                    },
                    readback.at(0),
                    512 * layer.bpp,
                    gpu::Extent {
                        width: 512,
                        height: 512,
                        depth: 1,
                    },
                );
            }
            let sync = context.submit(&mut encoder);
            assert!(context.wait_for(&sync, 5000));
            // SAFETY: the submission writing readback has completed and the
            // slice stays within this CPU-visible buffer's allocation.
            let pixels = unsafe { std::slice::from_raw_parts(readback.data(), expected.len()) };
            assert!(
                pixels == expected,
                "atlas pixels differ: bpp={bpp}, step={step}"
            );
        }
        context.destroy_buffer(readback);
    }
    context.destroy_command_encoder(&mut encoder);
    atlas.destroy(&context);
}
