use ciri_config::config::RenderConfig;
use glyphon::fontdb;
use glyphon::FontSystem;
use std::collections::HashMap;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use wgpu;

/// Font style for glyph cache lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// Build font fallback chain using fontconfig (Unix) or simple scan (Windows).
/// Returns fontdb IDs in priority order: primary font first, then fallbacks
/// in the order fontconfig recommends.
fn build_fallback_chain(font_system: &mut FontSystem, family_name: &str) -> Vec<fontdb::ID> {
    let mut font_ids = Vec::new();

    #[cfg(unix)]
    {
        // Use fontconfig's FcFontSort for correct locale-aware fallback ordering.
        use fontconfig::{Fontconfig, Pattern};
        use std::ffi::CString;

        if let Some(fc) = Fontconfig::new() {
            // Step 1: Find primary font by exact family match in fontdb
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

            // Step 2: Use fontconfig FcFontSort for fallback ordering
            let mut pat = Pattern::new(&fc);
            if let Ok(fam) = CString::new(family_name) {
                pat.add_string(c"family", &fam);
            }
            // sort_fonts() internally calls config_substitute + default_substitute.
            // Do NOT call them manually — double-substitute corrupts the pattern.
            let sorted = pat.sort_fonts(false);

            // Build a path→fontdb::ID lookup for fast matching
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

            // Add any remaining fontdb fonts not covered by fontconfig
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

    // Log the primary font
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

/// Build a style-specific fallback chain by filtering fonts that match the requested style.
/// Falls back to the regular chain if no style-specific fonts are found.
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
    // Find the primary font family name
    let primary_family = base_chain.first()
        .and_then(|id| db.face(*id))
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone())
        .unwrap_or_default();

    let mut style_ids = Vec::new();

    // First pass: find fonts in the same family with matching style
    for &fid in base_chain {
        if let Some(face) = db.face(fid) {
            let is_bold = face.weight.0 >= 600; // SemiBold or heavier
            let is_italic = face.style != fontdb::Style::Normal;
            let family_match = face.families.iter().any(|f| f.0 == primary_family);

            if family_match && is_bold == want_bold && is_italic == want_italic {
                style_ids.push(fid);
            }
        }
    }

    // If we found style-specific fonts, put them first, then the rest of the chain as fallback
    if !style_ids.is_empty() {
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

    // No style-specific fonts found — fall back to base chain
    log::info!("font style {:?}: no variant found, using regular", style);
    base_chain.to_vec()
}

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
    /// true if this glyph was rasterized as RGBA color (emoji).
    pub is_color: bool,
}

impl GlyphEntry {
    pub const EMPTY: Self = GlyphEntry {
        u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0,
        width: 0, height: 0, bearing_x: 0, bearing_y: 0,
        is_color: false,
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
/// Supports bold/italic font variants and color emoji.
pub struct GlyphAtlas {
    // Alpha atlas (R8Unorm) for regular text
    pub texture: wgpu::Texture,
    pub texture_view: wgpu::TextureView,
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
    pub instance_buffer: wgpu::Buffer,
    pub max_instances: usize,

    // Color emoji atlas (Rgba8Unorm)
    color_texture: wgpu::Texture,
    #[allow(dead_code)]
    color_texture_view: wgpu::TextureView,
    color_bind_group: wgpu::BindGroup,
    color_pipeline: wgpu::RenderPipeline,
    color_packer: ShelfPacker,

    atlas_size: u32,
    /// Cache key: (char, style) → GlyphEntry
    cache: HashMap<(char, FontStyle), GlyphEntry>,
    packer: ShelfPacker,
    scale_context: ScaleContext,
    /// Font chains per style: Regular, Bold, Italic, BoldItalic
    font_chains: HashMap<FontStyle, Vec<fontdb::ID>>,
    font_size: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    /// Font ascent in pixels (distance from baseline to top of cell).
    pub ascent: f32,
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
        // Convert point size to pixels: pt * dpi / 72
        // where dpi = 96 * scale_factor (winit's dpi_scale).
        // This matches ghostty/alacritty's font sizing.
        let font_size = font_size_pt * (96.0 * dpi_scale as f32) / 72.0;

        let base_chain = build_fallback_chain(font_system, family_name);

        // Build style-specific chains
        let mut font_chains = HashMap::new();
        font_chains.insert(FontStyle::Regular, base_chain.clone());
        font_chains.insert(FontStyle::Bold, build_style_chain(font_system, &base_chain, FontStyle::Bold));
        font_chains.insert(FontStyle::Italic, build_style_chain(font_system, &base_chain, FontStyle::Italic));
        font_chains.insert(FontStyle::BoldItalic, build_style_chain(font_system, &base_chain, FontStyle::BoldItalic));

        // Determine cell metrics from the primary font.
        let primary_id = base_chain.first().copied();
        let (cell_width, cell_height, ascent_px) = if let Some(fid) = primary_id {
            if let Some(font) = font_system.get_font(fid) {
                let swash_font = font.as_swash();
                let metrics = swash_font.metrics(&[]);
                let scale = font_size / metrics.units_per_em as f32;
                let ascent = (metrics.ascent * scale).ceil();
                let descent = (metrics.descent * scale).ceil();
                let height = (ascent + descent).ceil();
                // For cell width, measure 'M'
                let charmap = swash_font.charmap();
                let glyph_id = charmap.map('M');
                let glyph_metrics = swash_font.glyph_metrics(&[]);
                let advance = glyph_metrics.advance_width(glyph_id) * scale;
                let cw = advance.ceil();
                let ch = height.max(font_size * 1.2);
                // Clamp ascent to cell_height to prevent glyph positions going out of bounds
                let safe_ascent = ascent.min(ch);
                log::info!("font metrics: ascent={ascent:.1} descent={descent:.1} height={height:.1} cw={cw:.1} ch={ch:.1}");
                (cw, ch, safe_ascent)
            } else {
                (font_size * 0.6, font_size * 1.2, font_size * 1.2 * 0.8)
            }
        } else {
            (font_size * 0.6, font_size * 1.2, font_size * 1.2 * 0.8)
        };

        // Create alpha atlas texture (R8Unorm for text)
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
                buffers: &[glyph_instance_layout()],
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

        // Create color emoji atlas (Rgba8UnormSrgb)
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("color_emoji_atlas"),
            size: wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let color_texture_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let color_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("color_emoji_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let color_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("color_emoji_bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&color_sampler),
                },
            ],
        });

        // Color emoji pipeline (samples RGBA directly, ignores instance color)
        let color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("color_emoji_shader"),
            source: wgpu::ShaderSource::Wgsl(COLOR_EMOJI_SHADER.into()),
        });

        let color_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("color_emoji_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let color_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("color_emoji_pipeline"),
            layout: Some(&color_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &color_shader,
                entry_point: Some("vs_main"),
                buffers: &[glyph_instance_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &color_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
            color_texture,
            color_texture_view,
            color_bind_group,
            color_pipeline,
            color_packer: ShelfPacker::new(atlas_size),
            atlas_size,
            cache: HashMap::new(),
            packer: ShelfPacker::new(atlas_size),
            scale_context: ScaleContext::new(),
            font_chains,
            font_size,
            cell_width,
            cell_height,
            ascent: ascent_px,
        }
    }

    /// Ensure a character with a given style is in the atlas. Returns its GlyphEntry.
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
            let entry = GlyphEntry::EMPTY;
            self.cache.insert(key, entry);
            return Some(entry);
        }

        let font_ids = self.font_chains.get(&style)
            .or_else(|| self.font_chains.get(&FontStyle::Regular))?;

        if font_ids.is_empty() {
            return None;
        }

        // Two-phase fallback: find which font has the glyph
        let mut resolved_font_id = None;
        let mut resolved_glyph_id = 0u16;
        for &fid in font_ids {
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
            self.cache.insert(key, entry);
            return Some(entry);
        };

        let font = font_system.get_font(font_id)?;
        let swash_font = font.as_swash();

        // Build synthesis settings for bold/italic when we don't have a native variant
        let need_synth_bold = matches!(style, FontStyle::Bold | FontStyle::BoldItalic) && {
            let db = font_system.db();
            db.face(font_id).is_some_and(|f| f.weight.0 < 600)
        };
        let need_synth_italic = matches!(style, FontStyle::Italic | FontStyle::BoldItalic) && {
            let db = font_system.db();
            db.face(font_id).is_some_and(|f| f.style == fontdb::Style::Normal)
        };

        // Rasterize with swash
        let mut scaler = self.scale_context
            .builder(swash_font)
            .size(self.font_size)
            .hint(true)
            .build();

        // Build the render pipeline with synthetic transformations
        let italic_transform = if need_synth_italic {
            Some(swash::zeno::Transform {
                xx: 1.0, yx: 0.0,
                xy: 0.2125, yy: 1.0, // tan(12°) ≈ 0.2125 oblique skew
                x: 0.0, y: 0.0,
            })
        } else {
            None
        };

        let embolden_strength = if need_synth_bold { 0.02 * self.font_size } else { 0.0 };

        // Try color first, then alpha
        let image = {
            let mut r = Render::new(&[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ]);
            r.format(Format::Alpha)
             .offset(swash::zeno::Vector::new(0.0, 0.0))
             .transform(italic_transform)
             .embolden(embolden_strength);
            r.render(&mut scaler, resolved_glyph_id)
        }?;

        let w = image.placement.width;
        let h = image.placement.height;

        if w == 0 || h == 0 {
            let entry = GlyphEntry::EMPTY;
            self.cache.insert(key, entry);
            return Some(entry);
        }

        // Determine if this is a color glyph (4 bytes per pixel)
        let is_color = image.data.len() == (w * h * 4) as usize;

        let entry = if is_color {
            // Color emoji → RGBA atlas
            let (ax, ay) = self.color_packer.allocate(w, h)?;

            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.color_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &image.data,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );

            GlyphEntry {
                u0: ax as f32 / self.atlas_size as f32,
                v0: ay as f32 / self.atlas_size as f32,
                u1: (ax + w) as f32 / self.atlas_size as f32,
                v1: (ay + h) as f32 / self.atlas_size as f32,
                width: w as u16,
                height: h as u16,
                bearing_x: image.placement.left as i16,
                bearing_y: image.placement.top as i16,
                is_color: true,
            }
        } else {
            // Regular text glyph → R8 alpha atlas
            let alpha_data = if image.data.len() == (w * h) as usize {
                // Already alpha
                image.data
            } else if image.data.len() == (w * h * 4) as usize {
                // RGBA → take alpha channel
                image.data.iter().skip(3).step_by(4).copied().collect()
            } else {
                // Subpixel (3 bytes per pixel) → average to single alpha
                image.data.chunks(3).map(|rgb| {
                    ((rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3) as u8
                }).collect()
            };

            let (ax, ay) = self.packer.allocate(w, h)?;

            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &alpha_data,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );

            GlyphEntry {
                u0: ax as f32 / self.atlas_size as f32,
                v0: ay as f32 / self.atlas_size as f32,
                u1: (ax + w) as f32 / self.atlas_size as f32,
                v1: (ay + h) as f32 / self.atlas_size as f32,
                width: w as u16,
                height: h as u16,
                bearing_x: image.placement.left as i16,
                bearing_y: image.placement.top as i16,
                is_color: false,
            }
        };

        self.cache.insert(key, entry);
        Some(entry)
    }

    /// Backwards-compatible: ensure a regular-style character.
    pub fn ensure_char(
        &mut self,
        ch: char,
        font_system: &mut FontSystem,
        queue: &wgpu::Queue,
    ) -> Option<GlyphEntry> {
        self.ensure_styled_char(ch, FontStyle::Regular, font_system, queue)
    }

    /// Render glyph instances (alpha atlas text).
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

    /// Render color emoji instances (RGBA atlas).
    pub fn render_color(
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

        pass.set_pipeline(&self.color_pipeline);
        pass.set_bind_group(0, &self.color_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..data.len() as u64));
        pass.draw(0..4, 0..count as u32);
    }

    /// Clear the glyph cache and reset the atlas packer.
    /// Call this when the font family or size changes (e.g. config hot-reload).
    pub fn clear_cache(&mut self, queue: &wgpu::Queue) {
        self.cache.clear();
        self.packer = ShelfPacker::new(self.atlas_size);
        self.color_packer = ShelfPacker::new(self.atlas_size);
        // Clear alpha texture to zero
        let zeros = vec![0u8; (self.atlas_size * self.atlas_size) as usize];
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &zeros,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(self.atlas_size),
                rows_per_image: Some(self.atlas_size),
            },
            wgpu::Extent3d {
                width: self.atlas_size,
                height: self.atlas_size,
                depth_or_array_layers: 1,
            },
        );
        // Clear color texture to zero
        let color_zeros = vec![0u8; (self.atlas_size * self.atlas_size * 4) as usize];
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &color_zeros,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(self.atlas_size * 4),
                rows_per_image: Some(self.atlas_size),
            },
            wgpu::Extent3d {
                width: self.atlas_size,
                height: self.atlas_size,
                depth_or_array_layers: 1,
            },
        );
        log::info!("glyph cache cleared (atlas {}×{})", self.atlas_size, self.atlas_size);
    }

    pub fn grid_size(&self, viewport_w: f32, viewport_h: f32) -> (u16, u16) {
        let cols = (viewport_w / self.cell_width).floor() as u16;
        let rows = (viewport_h / self.cell_height).floor() as u16;
        (cols.max(1), rows.max(1))
    }
}

fn glyph_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GlyphInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 24, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { offset: 32, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
        ],
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

/// Shader for color emoji: samples RGBA directly from the color atlas.
const COLOR_EMOJI_SHADER: &str = r#"
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
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);

    var out: VsOut;
    out.uv = inst.uv_pos + vec2<f32>(x, y) * inst.uv_size;
    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    out.position = vec4<f32>(px.x, px.y, 0.0, 1.0);
    return out;
}

@group(0) @binding(0) var atlas_tex: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Color emoji: use texture color directly (already sRGB from Rgba8UnormSrgb)
    return textureSample(atlas_tex, atlas_sampler, in.uv);
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

    #[test]
    fn font_style_hash_distinct() {
        let mut map = HashMap::new();
        map.insert(('A', FontStyle::Regular), 1);
        map.insert(('A', FontStyle::Bold), 2);
        map.insert(('A', FontStyle::Italic), 3);
        assert_eq!(map.len(), 3);
        assert_eq!(map[&('A', FontStyle::Bold)], 2);
    }
}
