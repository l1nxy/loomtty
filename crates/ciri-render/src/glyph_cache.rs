use ciri_config::config::RenderConfig;
use glyphon::fontdb;
use glyphon::FontSystem;
use std::collections::HashMap;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use wgpu;

/// UV coordinates of a glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct GlyphEntry {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub width: u16,
    pub height: u16,
    pub bearing_x: i16,
    pub bearing_y: i16,
}

impl GlyphEntry {
    pub const EMPTY: Self = GlyphEntry {
        u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0,
        width: 0, height: 0, bearing_x: 0, bearing_y: 0,
    };
}

/// Simple shelf-based atlas packer.
struct ShelfPacker {
    shelf_y: u32,
    shelf_height: u32,
    cursor_x: u32,
    size: u32,
}

impl ShelfPacker {
    fn new(size: u32) -> Self {
        ShelfPacker {
            shelf_y: 0,
            shelf_height: 0,
            cursor_x: 0,
            size,
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.size || h > self.size {
            return None;
        }

        // Check if it fits on the current shelf
        if self.cursor_x + w > self.size {
            // Start a new shelf
            self.shelf_y += self.shelf_height;
            self.shelf_height = 0;
            self.cursor_x = 0;
        }

        if self.shelf_y + h > self.size {
            return None; // Atlas full
        }

        let x = self.cursor_x;
        let y = self.shelf_y;
        self.cursor_x += w;
        self.shelf_height = self.shelf_height.max(h);
        Some((x, y))
    }
}

/// Glyph atlas for fast terminal text rendering.
/// Bypasses cosmic-text's font matching — rasterizes directly with swash.
pub struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub texture_view: wgpu::TextureView,
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
    pub instance_buffer: wgpu::Buffer,
    pub max_instances: usize,

    atlas_size: u32,
    cache: HashMap<char, GlyphEntry>,
    packer: ShelfPacker,
    scale_context: ScaleContext,
    /// Primary font + all system fonts for fallback.
    font_ids: Vec<fontdb::ID>,
    font_size: f32,
    pub cell_width: f32,
    pub cell_height: f32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
pub struct GlyphInstance {
    pub pos: [f32; 2],       // NDC position (top-left of glyph quad)
    pub size: [f32; 2],      // NDC size
    pub uv_pos: [f32; 2],    // atlas UV top-left
    pub uv_size: [f32; 2],   // atlas UV size
    pub color: [f32; 4],     // RGBA color
}

impl GlyphAtlas {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        font_system: &mut FontSystem,
        font_size_pt: f32,
        dpi_scale: f64,
        family_name: &str,
        render_config: &RenderConfig,
    ) -> Self {
        let atlas_size = render_config.atlas_size;
        // Convert point size to pixels: pt * dpi_scale * (96/72)
        let font_size = font_size_pt * dpi_scale as f32 * (96.0 / 72.0);

        // Find font by family name, fallback to first monospace
        let mut font_ids = Vec::new();
        {
            let db = font_system.db();
            let mut primary = None;
            let mut first_mono = None;
            let family_lower = family_name.to_ascii_lowercase();
            for face in db.faces() {
                if first_mono.is_none() && face.monospaced {
                    first_mono = Some(face.id);
                }
                // Match by family name (case-insensitive, also partial match for Nerd Font variants)
                for family in &face.families {
                    if family.0.eq_ignore_ascii_case(family_name)
                        || (family_name != "monospace"
                            && family.0.to_ascii_lowercase().contains(&family_lower))
                    {
                        primary = Some(face.id);
                    }
                }
            }
            // Primary font
            if let Some(id) = primary.or(first_mono) {
                font_ids.push(id);
                if let Some(face) = db.face(id) {
                    let name = face.families.first().map(|f| f.0.as_str()).unwrap_or("?");
                    log::info!("primary font: {name} (monospaced={})", face.monospaced);
                }
            }
            // Add all other fonts as fallbacks (for CJK etc.)
            for face in db.faces() {
                if !font_ids.contains(&face.id) {
                    font_ids.push(face.id);
                }
            }
            log::info!("font fallback chain: {} fonts total", font_ids.len());
        };

        // Determine cell metrics from the primary font
        let primary_id = font_ids.first().copied();
        let (cell_width, cell_height) = if let Some(fid) = primary_id {
            if let Some(font) = font_system.get_font(fid) {
                let swash_font = font.as_swash();
                let metrics = swash_font.metrics(&[]);
                let scale = font_size / metrics.units_per_em as f32;
                let ascent = metrics.ascent * scale;
                let descent = metrics.descent * scale;
                let height = (ascent + descent).ceil();
                // For cell width, measure 'M'
                let charmap = swash_font.charmap();
                let glyph_id = charmap.map('M');
                let glyph_metrics = swash_font.glyph_metrics(&[]);
                let advance = glyph_metrics.advance_width(glyph_id) * scale;
                (advance.ceil(), height.max(font_size * 1.2))
            } else {
                (font_size * 0.6, font_size * 1.2)
            }
        } else {
            (font_size * 0.6, font_size * 1.2)
        };

        // Create atlas texture (R8Unorm for alpha mask)
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyph_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyph_bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(GLYPH_SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glyph_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute { offset: 24, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute { offset: 32, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let max_instances = render_config.max_glyph_instances;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph_instances"),
            size: (max_instances * std::mem::size_of::<GlyphInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        GlyphAtlas {
            texture,
            texture_view,
            bind_group,
            bind_group_layout,
            pipeline,
            instance_buffer,
            max_instances,
            atlas_size,
            cache: HashMap::new(),
            packer: ShelfPacker::new(atlas_size),
            scale_context: ScaleContext::new(),
            font_ids,
            font_size,
            cell_width,
            cell_height,
        }
    }

    /// Ensure a character is in the atlas. Returns its GlyphEntry.
    pub fn ensure_char(
        &mut self,
        ch: char,
        font_system: &mut FontSystem,
        queue: &wgpu::Queue,
    ) -> Option<GlyphEntry> {
        if let Some(entry) = self.cache.get(&ch) {
            return Some(*entry);
        }

        // Space and control chars: no glyph needed
        if ch == ' ' || ch == '\0' || ch.is_control() {
            let entry = GlyphEntry::EMPTY;
            self.cache.insert(ch, entry);
            return Some(entry);
        }

        if self.font_ids.is_empty() {
            return None;
        }

        // Two-phase fallback: find which font has the glyph
        let mut resolved_font_id = None;
        let mut resolved_glyph_id = 0u16;
        for &fid in &self.font_ids {
            if let Some(font) = font_system.get_font(fid) {
                let swash_font = font.as_swash();
                let gid = swash_font.charmap().map(ch);
                if gid != 0 {
                    resolved_font_id = Some(fid);
                    resolved_glyph_id = gid;
                    break;
                }
            }
        }

        let Some(font_id) = resolved_font_id else {
            // No font has this glyph
            let entry = GlyphEntry::EMPTY;
            self.cache.insert(ch, entry);
            return Some(entry);
        };

        let font = font_system.get_font(font_id)?;
        let swash_font = font.as_swash();

        // Rasterize with swash
        let mut scaler = self.scale_context
            .builder(swash_font)
            .size(self.font_size)
            .hint(true)
            .build();

        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, resolved_glyph_id)?;

        let w = image.placement.width;
        let h = image.placement.height;

        if w == 0 || h == 0 {
            let entry = GlyphEntry::EMPTY;
            self.cache.insert(ch, entry);
            return Some(entry);
        }

        // Pack into atlas
        let (ax, ay) = self.packer.allocate(w, h)?;

        // Upload to texture
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &image.data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        let atlas_size = self.atlas_size;
        let entry = GlyphEntry {
            u0: ax as f32 / atlas_size as f32,
            v0: ay as f32 / atlas_size as f32,
            u1: (ax + w) as f32 / atlas_size as f32,
            v1: (ay + h) as f32 / atlas_size as f32,
            width: w as u16,
            height: h as u16,
            bearing_x: image.placement.left as i16,
            bearing_y: image.placement.top as i16,
        };
        self.cache.insert(ch, entry);
        Some(entry)
    }

    /// Render glyph instances.
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        instances: &[GlyphInstance],
    ) {
        if instances.is_empty() {
            return;
        }

        let count = instances.len().min(self.max_instances);
        let data = bytemuck::cast_slice(&instances[..count]);
        queue.write_buffer(&self.instance_buffer, 0, data);

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..data.len() as u64));
        pass.draw(0..4, 0..count as u32);
    }

    pub fn grid_size(&self, viewport_w: f32, viewport_h: f32) -> (u16, u16) {
        let cols = (viewport_w / self.cell_width).floor() as u16;
        let rows = (viewport_h / self.cell_height).floor() as u16;
        (cols.max(1), rows.max(1))
    }
}

const GLYPH_SHADER: &str = r#"
struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv_pos: vec2<f32>,
    @location(3) uv_size: vec2<f32>,
    @location(4) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    // Triangle strip: 0=TL, 1=TR, 2=BL, 3=BR
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);

    var out: VsOut;
    out.uv = inst.uv_pos + vec2<f32>(x, y) * inst.uv_size;
    out.color = inst.color;

    // Convert pixel position to NDC
    // pos is in pixels, we need to get viewport size somehow
    // We'll pass pre-computed NDC positions from the CPU
    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    out.position = vec4<f32>(px.x, px.y, 0.0, 1.0);
    return out;
}

@group(0) @binding(0) var atlas_tex: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_tex, atlas_sampler, in.uv).r;
    let r = srgb_to_linear(in.color.r);
    let g = srgb_to_linear(in.color.g);
    let b = srgb_to_linear(in.color.b);
    return vec4<f32>(r, g, b, in.color.a * alpha);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_packer_basic() {
        let mut p = ShelfPacker::new(100);
        let a = p.allocate(10, 10);
        assert_eq!(a, Some((0, 0)));
        let b = p.allocate(10, 10);
        assert_eq!(b, Some((10, 0)));
    }

    #[test]
    fn shelf_packer_wrap() {
        let mut p = ShelfPacker::new(100);
        // Fill first row
        for _ in 0..10 {
            assert!(p.allocate(10, 20).is_some());
        }
        // Next allocation wraps to shelf y=20
        let a = p.allocate(10, 15);
        assert_eq!(a, Some((0, 20)));
    }

    #[test]
    fn shelf_packer_full() {
        let mut p = ShelfPacker::new(20);
        assert!(p.allocate(20, 20).is_some());
        assert!(p.allocate(1, 1).is_none()); // full
    }

    #[test]
    fn shelf_packer_oversized() {
        let mut p = ShelfPacker::new(10);
        assert!(p.allocate(11, 5).is_none());
        assert!(p.allocate(5, 11).is_none());
    }
}
