//! GPU glyph atlas — rasterizes and caches terminal text glyphs for instanced rendering.
//!
//! Two atlas layers:
//! - **Alpha** (`R8Unorm`): monochrome text glyphs, tinted by instance color in the shader
//! - **Color** (`Rgba8UnormSrgb`): color emoji, sampled directly
//!
//! Both share the same bind group layout and vertex shader; only the fragment
//! stage differs (sRGB conversion for text vs. direct sampling for emoji).

use ciri_config::config::RenderConfig;
use glyphon::fontdb;
use glyphon::FontSystem;
use std::collections::HashMap;
use swash::scale::{image::Content, Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use wgpu;

use crate::shaper::TextShaper;

// ─── Font style ──────────────────────────────────────────────────────

/// Font style for glyph cache lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    /// Select font style from bold/italic flags.
    pub fn from_bold_italic(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (true, true) => Self::BoldItalic,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (false, false) => Self::Regular,
        }
    }
}

// ─── Glyph entry ─────────────────────────────────────────────────────

/// UV coordinates and metrics of a cached glyph in the atlas.
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
    /// True if this glyph was rasterized as RGBA color (emoji).
    pub is_color: bool,
}

impl GlyphEntry {
    pub const EMPTY: Self = GlyphEntry {
        u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0,
        width: 0, height: 0, bearing_x: 0, bearing_y: 0,
        is_color: false,
    };
}

// ─── Shelf-based atlas packer ────────────────────────────────────────

/// Simple shelf-based 2D rectangle packer for atlas allocation.
/// Allocates left-to-right, top-to-bottom in horizontal shelves.
struct ShelfPacker {
    shelf_y: u32,       // y-origin of the current shelf
    shelf_height: u32,  // tallest glyph on the current shelf
    cursor_x: u32,      // next free x on the current shelf
    size: u32,          // atlas dimension (square)
}

impl ShelfPacker {
    fn new(size: u32) -> Self {
        ShelfPacker { shelf_y: 0, shelf_height: 0, cursor_x: 0, size }
    }

    /// Try to allocate a `w×h` region. Returns `(x, y)` origin or `None` if full.
    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.size || h > self.size {
            return None;
        }
        // Wrap to next shelf if current row is too narrow
        if self.cursor_x + w > self.size {
            self.shelf_y += self.shelf_height;
            self.shelf_height = 0;
            self.cursor_x = 0;
        }
        if self.shelf_y + h > self.size {
            return None; // atlas full
        }
        let (x, y) = (self.cursor_x, self.shelf_y);
        self.cursor_x += w;
        self.shelf_height = self.shelf_height.max(h);
        Some((x, y))
    }
}

// ─── Atlas layer ─────────────────────────────────────────────────────

/// A single GPU texture atlas layer with its own pipeline, bind group, and packer.
/// Both the alpha (text) and color (emoji) atlases are represented as an `AtlasLayer`.
struct AtlasLayer {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    instance_buffer: wgpu::Buffer,
    packer: ShelfPacker,
    /// Bytes per pixel (1 for R8Unorm, 4 for Rgba8UnormSrgb).
    bpp: u32,
}

impl AtlasLayer {
    /// Create a new atlas layer.
    fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        bind_group_layout: &wgpu::BindGroupLayout,
        atlas_size: u32,
        max_instances: usize,
        tex_format: wgpu::TextureFormat,
        filter: wgpu::FilterMode,
        fragment_src: &str,
        blend: wgpu::BlendState,
        label: &str,
    ) -> Self {
        // Texture
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: atlas_size, height: atlas_size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: tex_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: None,
            mag_filter: filter,
            min_filter: filter,
            ..Default::default()
        });

        // Bind group (texture + sampler)
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        // Compose full shader: shared vertex stage + layer-specific fragment stage
        let full_shader = format!("{VERTEX_SHADER}\n{fragment_src}");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(full_shader.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[glyph_instance_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend),
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

        // Instance buffer
        let buf_size = (max_instances * std::mem::size_of::<GlyphInstance>()) as u64;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: buf_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bpp = match tex_format {
            wgpu::TextureFormat::R8Unorm => 1,
            _ => 4, // Rgba8UnormSrgb and others
        };

        AtlasLayer {
            texture, bind_group, pipeline, instance_buffer,
            packer: ShelfPacker::new(atlas_size),
            bpp,
        }
    }

    /// Upload glyph pixel data at `(x, y)` in the atlas texture.
    fn upload(&self, queue: &wgpu::Queue, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * self.bpp),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Upload and render glyph instances.
    fn render(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        instances: &[GlyphInstance],
        max_instances: usize,
    ) {
        if instances.is_empty() {
            return;
        }
        let count = instances.len().min(max_instances);
        let data = bytemuck::cast_slice(&instances[..count]);
        queue.write_buffer(&self.instance_buffer, 0, data);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..data.len() as u64));
        pass.draw(0..4, 0..count as u32);
    }

    /// Zero out the texture and reset the packer.
    fn clear(&mut self, queue: &wgpu::Queue, atlas_size: u32) {
        self.packer = ShelfPacker::new(atlas_size);
        let zeros = vec![0u8; (atlas_size * atlas_size * self.bpp) as usize];
        self.upload(queue, 0, 0, atlas_size, atlas_size, &zeros);
    }
}

// ─── Glyph atlas ─────────────────────────────────────────────────────

/// Glyph atlas for fast terminal text rendering.
/// Manages two atlas layers (alpha text + color emoji), a glyph cache,
/// and font fallback chains for bold/italic variants.
pub struct GlyphAtlas {
    /// Monochrome text glyphs (R8Unorm, tinted by instance color).
    alpha: AtlasLayer,
    /// Color emoji (Rgba8UnormSrgb, sampled directly).
    color: AtlasLayer,

    max_instances: usize,
    atlas_size: u32,
    /// Cache: (char, style) → atlas entry.
    cache: HashMap<(char, FontStyle), GlyphEntry>,
    /// Cache: (glyph_id, font_id, style) → atlas entry for shaped glyphs.
    glyph_id_cache: HashMap<(u32, fontdb::ID, FontStyle), GlyphEntry>,
    scale_context: ScaleContext,
    /// Font fallback chains per style (Regular, Bold, Italic, BoldItalic).
    font_chains: HashMap<FontStyle, Vec<fontdb::ID>>,
    font_size: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    /// Font ascent in pixels (distance from baseline to top of cell).
    pub ascent: f32,
    /// Text shaper for ligature detection and grapheme cluster support.
    pub shaper: TextShaper,
    /// Primary font ID (first in the regular chain) for shaping.
    primary_font_id: Option<fontdb::ID>,
}

/// Per-instance data for instanced glyph rendering.
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
        let max_instances = render_config.max_glyph_instances;

        // Convert point size to pixels: pt × (96 × scale) / 72
        // Matches ghostty/alacritty font sizing convention.
        let font_size = font_size_pt * (96.0 * dpi_scale as f32) / 72.0;

        // ── Font setup ──
        let base_chain = build_fallback_chain(font_system, family_name);
        let mut font_chains = HashMap::new();
        font_chains.insert(FontStyle::Regular, base_chain.clone());
        font_chains.insert(FontStyle::Bold, build_style_chain(font_system, &base_chain, FontStyle::Bold));
        font_chains.insert(FontStyle::Italic, build_style_chain(font_system, &base_chain, FontStyle::Italic));
        font_chains.insert(FontStyle::BoldItalic, build_style_chain(font_system, &base_chain, FontStyle::BoldItalic));

        // ── Cell metrics from primary font ──
        let (cell_width, cell_height, ascent) = compute_cell_metrics(
            font_system, base_chain.first().copied(), font_size,
        );

        // ── Shared bind group layout (texture + sampler) ──
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

        // ── Atlas layers ──
        let alpha = AtlasLayer::new(
            device, format, &bind_group_layout, atlas_size, max_instances,
            wgpu::TextureFormat::R8Unorm,
            wgpu::FilterMode::Nearest,
            ALPHA_FRAGMENT,
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
            "glyph_atlas",
        );

        let color = AtlasLayer::new(
            device, format, &bind_group_layout, atlas_size, max_instances,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::FilterMode::Linear,
            COLOR_FRAGMENT,
            wgpu::BlendState::ALPHA_BLENDING,
            "color_emoji_atlas",
        );

        let primary_font_id = base_chain.first().copied();
        let mut shaper = TextShaper::new();
        if let Some(fid) = primary_font_id {
            shaper.load_font(fid, font_system);
        }

        GlyphAtlas {
            alpha, color, max_instances, atlas_size,
            cache: HashMap::new(),
            glyph_id_cache: HashMap::new(),
            scale_context: ScaleContext::new(),
            font_chains,
            font_size,
            cell_width,
            cell_height,
            ascent,
            shaper,
            primary_font_id,
        }
    }

    /// Ensure a glyph for `ch` with the given `style` is in the atlas.
    /// Returns the cached entry, rasterizing and uploading if needed.
    pub fn ensure_styled_char(
        &mut self,
        ch: char,
        style: FontStyle,
        font_system: &mut FontSystem,
        queue: &wgpu::Queue,
    ) -> Option<GlyphEntry> {
        let key = (ch, style);
        if let Some(entry) = self.cache.get(&key) {
            return Some(*entry);
        }

        // Space and control chars: no glyph needed
        if ch == ' ' || ch == '\0' || ch.is_control() {
            self.cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        // Find which font in the fallback chain has this glyph
        let font_ids = self.font_chains.get(&style)
            .or_else(|| self.font_chains.get(&FontStyle::Regular))?;
        let (font_id, glyph_id) = resolve_glyph(font_system, font_ids, ch)?;

        // Rasterize the glyph
        let image = rasterize_glyph(
            &mut self.scale_context, font_system, font_id, glyph_id,
            self.font_size, style,
        )?;

        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 {
            self.cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        // Upload to the appropriate atlas layer
        let is_color = matches!(image.content, Content::Color);
        let entry = if is_color {
            let (ax, ay) = self.color.packer.allocate(w, h)?;
            self.color.upload(queue, ax, ay, w, h, &image.data);
            make_glyph_entry(ax, ay, w, h, &image, self.atlas_size, true)
        } else {
            // Convert pixel data to single-channel alpha
            let alpha_data = to_alpha(&image.data, w, h);
            let (ax, ay) = self.alpha.packer.allocate(w, h)?;
            self.alpha.upload(queue, ax, ay, w, h, &alpha_data);
            make_glyph_entry(ax, ay, w, h, &image, self.atlas_size, false)
        };

        self.cache.insert(key, entry);
        Some(entry)
    }

    /// Ensure a glyph by its ID (from text shaping) is in the atlas.
    pub fn ensure_glyph_id(
        &mut self,
        glyph_id: u32,
        font_id: fontdb::ID,
        style: FontStyle,
        font_system: &mut FontSystem,
        queue: &wgpu::Queue,
    ) -> Option<GlyphEntry> {
        let key = (glyph_id, font_id, style);
        if let Some(entry) = self.glyph_id_cache.get(&key) {
            return Some(*entry);
        }
        if glyph_id == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let image = rasterize_glyph(
            &mut self.scale_context, font_system, font_id, glyph_id as u16,
            self.font_size, style,
        )?;

        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let is_color = matches!(image.content, Content::Color);
        let entry = if is_color {
            let (ax, ay) = self.color.packer.allocate(w, h)?;
            self.color.upload(queue, ax, ay, w, h, &image.data);
            make_glyph_entry(ax, ay, w, h, &image, self.atlas_size, true)
        } else {
            let alpha_data = to_alpha(&image.data, w, h);
            let (ax, ay) = self.alpha.packer.allocate(w, h)?;
            self.alpha.upload(queue, ax, ay, w, h, &alpha_data);
            make_glyph_entry(ax, ay, w, h, &image, self.atlas_size, false)
        };

        self.glyph_id_cache.insert(key, entry);
        Some(entry)
    }

    /// Get the primary font ID (first in the regular fallback chain).
    pub fn primary_font_id(&self) -> Option<fontdb::ID> {
        self.primary_font_id
    }

    /// Ensure a regular-style character is in the atlas.
    pub fn ensure_char(
        &mut self,
        ch: char,
        font_system: &mut FontSystem,
        queue: &wgpu::Queue,
    ) -> Option<GlyphEntry> {
        self.ensure_styled_char(ch, FontStyle::Regular, font_system, queue)
    }

    /// Render text glyph instances (alpha atlas).
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        instances: &[GlyphInstance],
    ) {
        self.alpha.render(queue, pass, instances, self.max_instances);
    }

    /// Render color emoji instances (RGBA atlas).
    pub fn render_color(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        instances: &[GlyphInstance],
    ) {
        self.color.render(queue, pass, instances, self.max_instances);
    }

    /// Clear the glyph cache and reset both atlas packers.
    /// Call on font family/size change (e.g. config hot-reload).
    pub fn clear_cache(&mut self, queue: &wgpu::Queue) {
        self.cache.clear();
        self.glyph_id_cache.clear();
        self.alpha.clear(queue, self.atlas_size);
        self.color.clear(queue, self.atlas_size);
        log::info!("glyph cache cleared (atlas {}×{})", self.atlas_size, self.atlas_size);
    }

    /// Compute grid dimensions (cols × rows) for the given viewport size.
    pub fn grid_size(&self, viewport_w: f32, viewport_h: f32) -> (u16, u16) {
        let cols = (viewport_w / self.cell_width).floor() as u16;
        let rows = (viewport_h / self.cell_height).floor() as u16;
        (cols.max(1), rows.max(1))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Compute cell width, height, and ascent from the primary font.
fn compute_cell_metrics(
    font_system: &mut FontSystem,
    primary_id: Option<fontdb::ID>,
    font_size: f32,
) -> (f32, f32, f32) {
    let fallback = (font_size * 0.6, font_size * 1.2, font_size * 1.2 * 0.8);

    let Some(fid) = primary_id else { return fallback };
    let Some(font) = font_system.get_font(fid) else { return fallback };

    let swash_font = font.as_swash();
    let metrics = swash_font.metrics(&[]);
    let scale = font_size / metrics.units_per_em as f32;
    let ascent = (metrics.ascent * scale).ceil();
    let descent = (metrics.descent * scale).ceil();
    let height = (ascent + descent).ceil();

    // Cell width = advance width of 'M'
    let glyph_id = swash_font.charmap().map('M');
    let advance = swash_font.glyph_metrics(&[]).advance_width(glyph_id) * scale;
    let cw = advance.ceil();
    let ch = height.max(font_size * 1.2);
    // Clamp ascent to cell_height to prevent out-of-bounds glyph positions
    let safe_ascent = ascent.min(ch);

    log::info!("font metrics: ascent={ascent:.1} descent={descent:.1} height={height:.1} cw={cw:.1} ch={ch:.1}");
    (cw, ch, safe_ascent)
}

/// Find which font in the fallback chain contains `ch`, returning (font_id, glyph_id).
fn resolve_glyph(
    font_system: &mut FontSystem,
    font_ids: &[fontdb::ID],
    ch: char,
) -> Option<(fontdb::ID, u16)> {
    for &fid in font_ids {
        if let Some(font) = font_system.get_font(fid) {
            let gid = font.as_swash().charmap().map(ch);
            if gid != 0 {
                return Some((fid, gid));
            }
        }
    }
    None
}

/// Rasterize a glyph with optional synthetic bold/italic.
/// Tries color bitmap first (emoji), then falls back to alpha outline.
fn rasterize_glyph(
    scale_ctx: &mut ScaleContext,
    font_system: &mut FontSystem,
    font_id: fontdb::ID,
    glyph_id: u16,
    font_size: f32,
    style: FontStyle,
) -> Option<swash::scale::image::Image> {
    let font = font_system.get_font(font_id)?;
    let swash_font = font.as_swash();

    // Check if we need synthetic bold/italic (no native variant available)
    let need_synth_bold = matches!(style, FontStyle::Bold | FontStyle::BoldItalic) && {
        let db = font_system.db();
        db.face(font_id).is_some_and(|f| f.weight.0 < 600)
    };
    let need_synth_italic = matches!(style, FontStyle::Italic | FontStyle::BoldItalic) && {
        let db = font_system.db();
        db.face(font_id).is_some_and(|f| f.style == fontdb::Style::Normal)
    };

    let mut scaler = scale_ctx.builder(swash_font).size(font_size).hint(true).build();

    // Try color bitmap (emoji) first
    let color_image = {
        let mut r = Render::new(&[
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::ColorOutline(0),
        ]);
        r.format(Format::Subpixel).offset(swash::zeno::Vector::new(0.0, 0.0));
        r.render(&mut scaler, glyph_id)
    };

    color_image.or_else(|| {
        // Fall back to alpha outline with synthetic transformations
        let italic_transform = need_synth_italic.then_some(swash::zeno::Transform {
            xx: 1.0, yx: 0.0,
            xy: 0.2125, yy: 1.0, // tan(12°) ≈ 0.2125 oblique skew
            x: 0.0, y: 0.0,
        });
        let embolden = if need_synth_bold { 0.02 * font_size } else { 0.0 };

        let mut r = Render::new(&[Source::Outline]);
        r.format(Format::Alpha)
         .offset(swash::zeno::Vector::new(0.0, 0.0))
         .transform(italic_transform)
         .embolden(embolden);
        r.render(&mut scaler, glyph_id)
    })
}

/// Convert rasterized pixel data to single-channel alpha.
fn to_alpha(data: &[u8], w: u32, h: u32) -> Vec<u8> {
    let expected_alpha = (w * h) as usize;
    let expected_rgba = (w * h * 4) as usize;

    if data.len() == expected_alpha {
        data.to_vec()
    } else if data.len() == expected_rgba {
        // RGBA → extract alpha channel
        data.iter().skip(3).step_by(4).copied().collect()
    } else {
        // Subpixel (3 bytes per pixel) → average RGB to single alpha
        data.chunks(3).map(|rgb| {
            ((rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3) as u8
        }).collect()
    }
}

/// Build a `GlyphEntry` from atlas coordinates and image placement.
fn make_glyph_entry(
    ax: u32, ay: u32, w: u32, h: u32,
    image: &swash::scale::image::Image,
    atlas_size: u32,
    is_color: bool,
) -> GlyphEntry {
    let s = atlas_size as f32;
    GlyphEntry {
        u0: ax as f32 / s,
        v0: ay as f32 / s,
        u1: (ax + w) as f32 / s,
        v1: (ay + h) as f32 / s,
        width: w as u16,
        height: h as u16,
        bearing_x: image.placement.left as i16,
        bearing_y: image.placement.top as i16,
        is_color,
    }
}

// ─── Vertex layout ───────────────────────────────────────────────────

fn glyph_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GlyphInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute { offset: 0,  shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 8,  shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 24, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 32, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
        ],
    }
}

// ─── WGSL shaders ────────────────────────────────────────────────────
//
// Vertex stage is shared; only the fragment stage differs per layer.
// They are concatenated at pipeline creation time.

/// Shared vertex shader: maps instanced glyph quads from NDC positions.
const VERTEX_SHADER: &str = r#"
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
    // pos/size are pre-computed NDC from the CPU
    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    out.position = vec4<f32>(px.x, px.y, 0.0, 1.0);
    return out;
}
"#;

/// Alpha text fragment: sample R8 alpha, tint with instance color, sRGB→linear.
const ALPHA_FRAGMENT: &str = r#"
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

/// Color emoji fragment: sample RGBA directly, modulate by instance color for dim/fade.
const COLOR_FRAGMENT: &str = r#"
@group(0) @binding(0) var atlas_tex: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_tex, atlas_sampler, in.uv);
    // Instance color RGB carries the dim factor; alpha carries opacity.
    return vec4<f32>(texel.rgb * in.color.rgb, texel.a * in.color.a);
}
"#;

// ─── Font fallback chains ────────────────────────────────────────────

/// Build font fallback chain using fontconfig (Unix) or simple scan (Windows).
/// Returns fontdb IDs in priority order: primary font first, then fallbacks.
fn build_fallback_chain(font_system: &mut FontSystem, family_name: &str) -> Vec<fontdb::ID> {
    let mut font_ids = Vec::new();

    #[cfg(unix)]
    {
        use fontconfig::{Fontconfig, Pattern};
        use std::ffi::CString;

        if let Some(fc) = Fontconfig::new() {
            // Step 1: Find primary font by family name match in fontdb
            let db = font_system.db();
            let family_lower = family_name.to_ascii_lowercase();
            for face in db.faces() {
                for family in &face.families {
                    if family.0.eq_ignore_ascii_case(family_name)
                        || family.0.to_ascii_lowercase().contains(&family_lower)
                    {
                        if !font_ids.contains(&face.id) {
                            font_ids.push(face.id);
                        }
                        break;
                    }
                }
            }

            // Step 2: Use fontconfig FcFontSort for locale-aware fallback ordering
            let mut pat = Pattern::new(&fc);
            if let Ok(fam) = CString::new(family_name) {
                pat.add_string(c"family", &fam);
            }
            // sort_fonts() internally does config_substitute + default_substitute.
            // Do NOT call them manually — double-substitute corrupts the pattern.
            let sorted = pat.sort_fonts(false);

            // Build path→ID lookup for fast matching against fontconfig results
            let db = font_system.db();
            let mut path_to_ids: HashMap<(String, u32), fontdb::ID> = HashMap::new();
            for face in db.faces() {
                if let fontdb::Source::File(ref path) = face.source {
                    path_to_ids.insert((path.to_string_lossy().to_string(), face.index), face.id);
                }
            }

            for fc_font in sorted.iter() {
                let Some(fc_path_raw) = fc_font.filename() else { continue };
                let fc_path = fc_path_raw.replace("\\", "");
                let fc_index = fc_font.face_index().unwrap_or(0) as u32;
                if let Some(&id) = path_to_ids.get(&(fc_path, fc_index))
                    && !font_ids.contains(&id) {
                        font_ids.push(id);
                    }
            }

            // Add remaining fontdb fonts not covered by fontconfig
            for face in db.faces() {
                if !font_ids.contains(&face.id) {
                    font_ids.push(face.id);
                }
            }
        }
    }

    // Fallback: no fontconfig or Windows — scan fontdb directly
    if font_ids.is_empty() {
        let db = font_system.db();
        let mut primary = None;
        let mut first_mono = None;
        let family_lower = family_name.to_ascii_lowercase();
        for face in db.faces() {
            if first_mono.is_none() && face.monospaced {
                first_mono = Some(face.id);
            }
            for family in &face.families {
                if family.0.eq_ignore_ascii_case(family_name)
                    || (family_name != "monospace"
                        && family.0.to_ascii_lowercase().contains(&family_lower))
                {
                    primary = Some(face.id);
                }
            }
        }
        if let Some(id) = primary.or(first_mono) {
            font_ids.push(id);
        }
        for face in db.faces() {
            if !font_ids.contains(&face.id) {
                font_ids.push(face.id);
            }
        }
    }

    if let Some(&first) = font_ids.first() {
        let db = font_system.db();
        if let Some(face) = db.face(first) {
            let name = face.families.first().map(|f| f.0.as_str()).unwrap_or("?");
            log::info!("primary font: {name} (monospaced={})", face.monospaced);
        }
    }
    log::info!("font fallback chain: {} fonts total", font_ids.len());
    font_ids
}

/// Build a style-specific fallback chain by filtering for fonts matching the style.
/// Falls back to the regular chain if no style-specific variant exists.
fn build_style_chain(
    font_system: &mut FontSystem,
    base_chain: &[fontdb::ID],
    style: FontStyle,
) -> Vec<fontdb::ID> {
    if matches!(style, FontStyle::Regular) {
        return base_chain.to_vec();
    }

    let want_bold = matches!(style, FontStyle::Bold | FontStyle::BoldItalic);
    let want_italic = matches!(style, FontStyle::Italic | FontStyle::BoldItalic);

    let db = font_system.db();
    let primary_family = base_chain.first()
        .and_then(|id| db.face(*id))
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone())
        .unwrap_or_default();

    // Find fonts in the same family with matching weight/style
    let mut style_ids: Vec<fontdb::ID> = base_chain.iter()
        .filter(|&&fid| {
            db.face(fid).is_some_and(|face| {
                let is_bold = face.weight.0 >= 600;
                let is_italic = face.style != fontdb::Style::Normal;
                let family_match = face.families.iter().any(|f| f.0 == primary_family);
                family_match && is_bold == want_bold && is_italic == want_italic
            })
        })
        .copied()
        .collect();

    if !style_ids.is_empty() {
        // Style-specific fonts first, then rest of chain as fallback
        for &fid in base_chain {
            if !style_ids.contains(&fid) {
                style_ids.push(fid);
            }
        }
        let name = db.face(style_ids[0])
            .and_then(|f| f.families.first())
            .map(|f| f.0.as_str())
            .unwrap_or("?");
        log::info!("font style {:?}: using {name}", style);
        return style_ids;
    }

    log::info!("font style {:?}: no variant found, using regular", style);
    base_chain.to_vec()
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_packer_basic() {
        let mut p = ShelfPacker::new(100);
        assert_eq!(p.allocate(10, 10), Some((0, 0)));
        assert_eq!(p.allocate(10, 10), Some((10, 0)));
    }

    #[test]
    fn shelf_packer_wrap() {
        let mut p = ShelfPacker::new(100);
        for _ in 0..10 {
            assert!(p.allocate(10, 20).is_some());
        }
        // Next allocation wraps to shelf y=20
        assert_eq!(p.allocate(10, 15), Some((0, 20)));
    }

    #[test]
    fn shelf_packer_full() {
        let mut p = ShelfPacker::new(20);
        assert!(p.allocate(20, 20).is_some());
        assert!(p.allocate(1, 1).is_none());
    }

    #[test]
    fn shelf_packer_oversized() {
        let mut p = ShelfPacker::new(10);
        assert!(p.allocate(11, 5).is_none());
        assert!(p.allocate(5, 11).is_none());
    }

    #[test]
    fn font_style_hash_distinct() {
        let mut map = HashMap::new();
        map.insert(('A', FontStyle::Regular), 1);
        map.insert(('A', FontStyle::Bold), 2);
        map.insert(('A', FontStyle::Italic), 3);
        assert_eq!(map.len(), 3);
        assert_eq!(map[&('A', FontStyle::Bold)], 2);
    }

    #[test]
    fn font_style_from_bold_italic() {
        assert_eq!(FontStyle::from_bold_italic(false, false), FontStyle::Regular);
        assert_eq!(FontStyle::from_bold_italic(true, false), FontStyle::Bold);
        assert_eq!(FontStyle::from_bold_italic(false, true), FontStyle::Italic);
        assert_eq!(FontStyle::from_bold_italic(true, true), FontStyle::BoldItalic);
    }

    #[test]
    fn to_alpha_passthrough() {
        let data = vec![100, 200, 50, 255];
        assert_eq!(to_alpha(&data, 2, 2), data); // 4 bytes = 2×2×1 → already alpha
    }

    #[test]
    fn to_alpha_from_rgba() {
        // 1×1 RGBA pixel: R=10, G=20, B=30, A=128
        let data = vec![10, 20, 30, 128];
        assert_eq!(to_alpha(&data, 1, 1), vec![128]); // extracts alpha channel
    }
}
