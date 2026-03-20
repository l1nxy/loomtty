use anyhow::Result;
use ciri_config::config::RenderConfig;
use glyphon::FontSystem;
use std::sync::Arc;
use wgpu;
use winit::window::Window;

use crate::rect::RectRenderer;

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub surface_config: wgpu::SurfaceConfiguration,
    /// When true, `surface.configure()` is deferred until the next frame.
    /// This coalesces rapid resize events into a single swapchain rebuild.
    surface_dirty: bool,
    /// Font system for glyph discovery and rasterization.
    pub font_system: FontSystem,
    pub rects: RectRenderer,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, render_config: &RenderConfig) -> Result<Self> {
        let size = window.inner_size();

        let backend = match render_config.backend.as_str() {
            "vulkan" => wgpu::Backends::VULKAN,
            "gl" | "gles" => wgpu::Backends::GL,
            "metal" => wgpu::Backends::METAL,
            "dx12" => wgpu::Backends::DX12,
            _ => wgpu::Backends::all(),
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: backend,
            ..Default::default()
        });

        let surface = instance.create_surface(window)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("no suitable GPU adapter found"))?;

        log::info!("GPU adapter: {:?} ({:?})", adapter.get_info().name, adapter.get_info().backend);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ciri"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    ..Default::default()
                },
                None,
            )
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let desired_mode = match render_config.present_mode.as_str() {
            "mailbox" => wgpu::PresentMode::Mailbox,
            "immediate" => wgpu::PresentMode::Immediate,
            _ => wgpu::PresentMode::Fifo,
        };
        let present_mode = if surface_caps.present_modes.contains(&desired_mode) {
            desired_mode
        } else {
            log::warn!(
                "present mode {:?} not supported, falling back to Fifo",
                desired_mode
            );
            wgpu::PresentMode::Fifo
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: render_config.frame_latency,
        };
        surface.configure(&device, &surface_config);

        let rects = RectRenderer::new(&device, surface_format, render_config.max_rectangles);

        Ok(Renderer {
            device,
            queue,
            surface,
            surface_config,
            surface_dirty: false,
            font_system: FontSystem::new(),
            rects,
        })
    }

    /// Record a new surface size. The actual `surface.configure()` is deferred
    /// to [`ensure_surface`] so that rapid resize events coalesce into one
    /// swapchain rebuild per frame.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface_dirty = true;
        }
    }

    /// Apply any pending `surface.configure()`. Call once at the start of each
    /// frame, before `get_current_texture()`.
    pub fn ensure_surface(&mut self) {
        if self.surface_dirty {
            self.surface.configure(&self.device, &self.surface_config);
            self.surface_dirty = false;
        }
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }
}
