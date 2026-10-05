//! wgpu render transport into a brokered UI4 frame.
use super::gpu::Gpu;
use crate::client::Client;
use std::fmt;
use trueos::ui4_scene::{Damage, Error as UiError, Frame};

#[derive(Debug)]
pub(super) enum Error {
    Ui(UiError),
    Gpu(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui(error) => write!(f, "UI4 render: {error:?}"),
            Self::Gpu(error) => write!(f, "TRUEOS wgpu: {error}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<UiError> for Error {
    fn from(error: UiError) -> Self {
        Self::Ui(error)
    }
}

pub(super) struct Renderer {
    context: trueos_wgpu::Context,
    gpu: Gpu,
    pending_publish: bool,
}
impl Renderer {
    pub(super) fn new(width: u32, height: u32) -> Result<Self, Error> {
        let package = trueos_wgpu::ShaderPackage::new(
            include_str!("render.wgsl"),
            super::shader::PACKAGE_FNV1A64,
        );
        let context = trueos_wgpu::Context::open_with_package(package)
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let gpu = Gpu::new(
            context.device().clone(),
            context.queue().clone(),
            wgpu::TextureFormat::Bgra8Unorm,
            width,
            height,
        );
        context.wait().map_err(|e| Error::Gpu(e.to_string()))?;
        Ok(Self {
            context,
            gpu,
            pending_publish: false,
        })
    }

    pub(super) fn publish(
        &mut self,
        frame: &mut Frame,
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        if self.pending_publish {
            frame.publish(Damage::full(width, height))?;
            self.pending_publish = false;
        }
        Ok(())
    }

    pub(super) fn draw(
        &mut self,
        frame: &mut Frame,
        width: u32,
        height: u32,
        client: Option<&Client>,
        yaw: f32,
        pitch: f32,
        status: &str,
    ) -> Result<(), Error> {
        self.publish(frame, width, height)?;
        frame.begin_gpu_frame()?;
        let texture = self
            .context
            .acquire_frame(frame.window_id())
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let view = texture.create_view(&Default::default());
        self.gpu
            .draw(&view, width, height, client, yaw, pitch, status);
        self.context.wait().map_err(|e| Error::Gpu(e.to_string()))?;
        self.pending_publish = true;
        self.publish(frame, width, height)
    }
}
