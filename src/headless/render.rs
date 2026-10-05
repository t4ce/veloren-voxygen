//! Small, independent GPU path for the real lean game client.
//! One pipeline, one uniform binding, one depth target, two vertex buffers.
//!
//! Ubuntu: `cargo run --features headless` uses this renderer directly.
//! The existing client supplies networking, simulation and gameplay input.
//! One pipeline/pass draws terrain, entity box proxies and text without sampled
//! textures, lights, optional GPU features or postprocessing. Terrain is bounded
//! to +/-40 blocks XY and +/-32 blocks Z, with at most 600,000 vertices. Meshing
//! runs off-thread at most twice per second. Character aliases and controls stay
//! the same as the existing headless client. Missing world data stays empty.
use super::gpu::Gpu;
use crate::client::Client;
use std::sync::Arc;
use winit::{event_loop::OwnedDisplayHandle, window::Window};

type Error = Box<dyn std::error::Error>;

pub(crate) struct Renderer {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
}

impl Renderer {
    pub(crate) fn new(
        window: Arc<Window>,
        display: OwnedDisplayHandle,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<Self, Error> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display)),
        );
        let surface = instance.create_surface(Arc::clone(&window))?;
        let adapter = runtime.block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            runtime.block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("minimal wgpu device"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            }))?;
        tracing::info!(adapter = ?adapter.get_info(),
            "Minimal wgpu: no optional device features, 1 pipeline, 1 uniform, no sampled textures");
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("No compatible surface configuration")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        let gpu = Gpu::new(
            device.clone(),
            queue.clone(),
            config.format,
            config.width,
            config.height,
        );
        Ok(Self {
            instance,
            window,
            surface,
            device,
            queue,
            config,
            gpu,
        })
    }

    pub(crate) fn draw(
        &mut self,
        client: Option<&Client>,
        yaw: f32,
        pitch: f32,
        status: &str,
    ) -> Result<(), Error> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        if (size.width, size.height) != (self.config.width, self.config.height) {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(Arc::clone(&self.window))?;
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("Surface validation failed".into());
            }
        };
        let view = frame.texture.create_view(&Default::default());
        self.gpu
            .draw(&view, size.width, size.height, client, yaw, pitch, status);
        self.window.pre_present_notify();
        self.queue.present(frame);
        Ok(())
    }
}
