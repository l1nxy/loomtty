use wgpu;

#[repr(C)]
#[derive(Copy, Clone, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
struct RectVertex {
    position: [f32; 2],
    color: [f32; 4],
}

/// Renders colored rectangles (terminal cell backgrounds, cursor, etc.)
pub struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    max_rects: usize,
}

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
}

impl RectRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, max_rects: usize) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect_pipeline_layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<RectVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect_vertex_buffer"),
            size: (max_rects * 6 * std::mem::size_of::<RectVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        RectRenderer {
            pipeline,
            vertex_buffer,
            max_rects,
        }
    }

    /// Upload rectangles and render them.
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        rects: &[Rect],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if rects.is_empty() {
            return;
        }

        let count = rects.len().min(self.max_rects);
        let mut vertices = Vec::with_capacity(count * 6);

        for rect in &rects[..count] {
            // Convert pixel coordinates to NDC (-1..1)
            let x0 = rect.x / viewport_w * 2.0 - 1.0;
            let y0 = 1.0 - rect.y / viewport_h * 2.0;
            let x1 = (rect.x + rect.w) / viewport_w * 2.0 - 1.0;
            let y1 = 1.0 - (rect.y + rect.h) / viewport_h * 2.0;
            let c = rect.color;

            // Two triangles per rect
            vertices.push(RectVertex { position: [x0, y0], color: c });
            vertices.push(RectVertex { position: [x1, y0], color: c });
            vertices.push(RectVertex { position: [x0, y1], color: c });
            vertices.push(RectVertex { position: [x1, y0], color: c });
            vertices.push(RectVertex { position: [x1, y1], color: c });
            vertices.push(RectVertex { position: [x0, y1], color: c });
        }

        let data = bytemuck::cast_slice(&vertices);
        queue.write_buffer(&self.vertex_buffer, 0, data);

        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..data.len() as u64));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
}

const RECT_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// Convert a single sRGB channel to linear
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn srgb_color_to_linear(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b), c.a);
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return srgb_color_to_linear(in.color);
}
"#;
