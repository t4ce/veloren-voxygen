//! wgpu render transport into a brokered UI4 frame.
use super::gpu::Gpu;
use super::scene::FrameInfo;
use crate::client::Client;
use std::fmt;
use trueos::ui4_scene::{Damage, Error as UiError, Frame};

const SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64: u64 = 0xF84D_E655_632E_F102;
const _: () = assert!(
    super::shader::fnv1a64(include_bytes!("render_textured.wgsl"))
        == SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64
);

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
    pending_publish: Option<FrameInfo>,
    awaiting_presentation: Option<(u64, FrameInfo)>,
    proof: super::PresentationProof,
    first_published: bool,
    first_presented: bool,
}
impl Renderer {
    pub(super) fn new(width: u32, height: u32) -> Result<Self, Error> {
        let package = trueos_wgpu::ShaderPackage::new(
            include_str!("render_textured.wgsl"),
            SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64,
        );
        let context = trueos_wgpu::Context::open_with_package(package)
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let gpu = Gpu::new(
            context.device().clone(),
            context.queue().clone(),
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
        );
        context.wait().map_err(|e| Error::Gpu(e.to_string()))?;
        Ok(Self {
            context,
            gpu,
            pending_publish: None,
            awaiting_presentation: None,
            proof: super::PresentationProof::default(),
            first_published: false,
            first_presented: false,
        })
    }

    pub(super) fn publish(
        &mut self,
        frame: &mut Frame,
        width: u32,
        height: u32,
        world_joined: bool,
    ) -> Result<(), Error> {
        self.observe_presentation(frame, width, height, world_joined)?;
        if let Some(info) = self.pending_publish {
            let serial = frame.publish_tracked(Damage::full(width, height))?;
            self.pending_publish = None;
            self.awaiting_presentation = Some((serial, info));
            if !self.first_published {
                self.first_published = true;
                super::connection_progress(format_args!(
                    "Voxygen headless: first GPU frame published window={} extent={}x{} boundary=producer-retired+ui4-publish",
                    frame.window_id(),
                    width,
                    height,
                ));
            }
        }
        self.observe_presentation(frame, width, height, world_joined)
    }

    fn observe_presentation(
        &mut self,
        frame: &mut Frame,
        width: u32,
        height: u32,
        world_joined: bool,
    ) -> Result<(), Error> {
        let Some((serial, info)) = self.awaiting_presentation else {
            return Ok(());
        };
        // One outstanding publication keeps every counted frame individually
        // observable: no faster producer can replace it before SURFLIVE.
        if !frame.was_presented(serial)? {
            return Err(UiError::Busy.into());
        }
        self.awaiting_presentation = None;
        if !self.first_presented {
            self.first_presented = true;
            super::connection_progress(format_args!(
                "Voxygen headless: first frame presented window={} serial={} extent={}x{} boundary=physical-SURFLIVE",
                frame.window_id(),
                serial,
                width,
                height,
            ));
        }
        if self.proof.observe(serial, world_joined, info)
            && (matches!(self.proof.consecutive, 1 | 10 | 20 | 30)
                || self.proof.consecutive.is_multiple_of(128))
        {
            super::connection_progress(format_args!(
                "Voxygen terrain frame proof: consecutive={} window={} serial={} revision={} terrain_vertices={} overlay_vertices={} position={:?} boundary=GPU-retired+physical-SURFLIVE",
                self.proof.consecutive,
                frame.window_id(),
                serial,
                info.terrain_revision,
                info.terrain_vertices,
                info.overlay_vertices,
                info.position,
            ));
        }
        Ok(())
    }

    pub(super) fn terrain_presented(&self) -> bool {
        self.proof.consecutive > 0
    }

    pub(super) fn draw(
        &mut self,
        frame: &mut Frame,
        width: u32,
        height: u32,
        client: Option<&Client>,
        yaw: f32,
        pitch: f32,
        world_joined: bool,
    ) -> Result<(), Error> {
        self.publish(frame, width, height, world_joined)?;
        frame.begin_gpu_frame()?;
        let texture = self
            .context
            .acquire_frame(frame.window_id())
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let view = texture.create_view(&Default::default());
        let info = self.gpu.draw(&view, width, height, client, yaw, pitch);
        self.context.wait().map_err(|e| Error::Gpu(e.to_string()))?;
        self.pending_publish = Some(info);
        self.publish(frame, width, height, world_joined)
    }
}
