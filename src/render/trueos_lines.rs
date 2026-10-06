//! The first native scene producer: projected terrain edges on the existing UI4 window.
//!
//! Positions are clip-space XYZ supplied by the scene bridge. Keeping this
//! small executor separate from extraction lets terrain uploads and camera
//! math evolve without changing the GPU lease and retirement contract.

use trueos::{
    ui4_scene::{Damage, Error as Ui4Error},
    ui4_solara_text::SceneTarget,
    vgpu::{self, Buffer, Device, Queue, QueueClass, RenderPipeline, ShaderModule},
};

const MAX_LINE_VERTICES: usize = 65_536;
const VERTEX_BYTES: usize = MAX_LINE_VERTICES * 12;
const INDEX_BYTES: usize = MAX_LINE_VERTICES * 4;

pub(super) struct LinePresenter {
    target: SceneTarget,
    device: Device,
    queue: Queue,
    shader: ShaderModule,
    pipeline: RenderPipeline,
    vertices: Buffer,
    indices: Buffer,
    vertex_bytes: Vec<u8>,
    width: u32,
    height: u32,
    pending_publish: bool,
    poisoned: bool,
}

impl LinePresenter {
    pub(super) fn new(window: u32, width: u32, height: u32) -> Result<Self, String> {
        let target = SceneTarget::for_window(window, width, height)
            .map_err(ui_error)?
            .background()
            .map_err(ui_error)?;
        let device = Device::open(
            vgpu::Capabilities::BUFFER
                .union(vgpu::Capabilities::QUEUE)
                .union(vgpu::Capabilities::TIMELINE)
                .union(vgpu::Capabilities::RENDER)
                .union(vgpu::Capabilities::PRESENT),
        )
        .map_err(|e| gpu_error("open", e))?;
        let initialized = (|| {
            let queue = device
                .create_queue(QueueClass::Render)
                .map_err(|e| gpu_error("queue", e))?;
            let shader = device
                .create_shader_module(vgpu::SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)
                .map_err(|e| gpu_error("shader", e))?;
            let pipeline = device
                .create_render_pipeline(shader, 12, 0)
                .map_err(|e| gpu_error("pipeline", e))?;
            let vertices = device
                .create_buffer(
                    VERTEX_BYTES,
                    vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_VERTEX,
                )
                .map_err(|e| gpu_error("vertex buffer", e))?;
            let indices = device
                .create_buffer(
                    INDEX_BYTES,
                    vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_INDEX,
                )
                .map_err(|e| gpu_error("index buffer", e))?;
            let mut index_bytes = Vec::with_capacity(INDEX_BYTES);
            for index in 0..MAX_LINE_VERTICES as u32 {
                index_bytes.extend_from_slice(&index.to_le_bytes());
            }
            let count = device
                .write_buffer(indices, 0, &index_bytes)
                .map_err(|e| gpu_error("index upload", e))?;
            if count != INDEX_BYTES {
                return Err(format!(
                    "TRUEOS line index upload short: {count}/{INDEX_BYTES}"
                ));
            }
            Ok((queue, shader, pipeline, vertices, indices))
        })();
        // A partially constructed device is intentionally closed by the
        // kernel's owner teardown if construction fails before all handles exist.
        let (queue, shader, pipeline, vertices, indices) = match initialized {
            Ok(handles) => handles,
            Err(error) => {
                let _ = device.close();
                return Err(error);
            }
        };
        Ok(Self {
            target,
            device,
            queue,
            shader,
            pipeline,
            vertices,
            indices,
            vertex_bytes: Vec::with_capacity(VERTEX_BYTES),
            width,
            height,
            pending_publish: false,
            poisoned: false,
        })
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.target.set_extent(width, height).map_err(ui_error)?;
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Returns false when UI4 has back pressure. Retry on the next tick.
    pub(super) fn draw(&mut self, lines: &[[f32; 3]]) -> Result<bool, String> {
        if self.poisoned {
            return Err("TRUEOS line queue needs recreation after uncertain GPU retirement".into());
        }
        // A successful GPU submit leaves UI4 with one frame to publish. Busy
        // must be retried on that same frame before obtaining another lease.
        if self.pending_publish {
            if !self.publish()? {
                return Ok(false);
            }
        }
        if lines.len() > MAX_LINE_VERTICES || lines.len() % 2 != 0 {
            return Err(format!(
                "TRUEOS line frame requires an even count <= {MAX_LINE_VERTICES}"
            ));
        }
        if lines.iter().flatten().any(|v| !v.is_finite()) {
            return Err("TRUEOS line frame contains a non-finite clip coordinate".into());
        }
        if !lines.is_empty() {
            self.vertex_bytes.clear();
            for vertex in lines {
                for component in vertex {
                    self.vertex_bytes
                        .extend_from_slice(&component.to_le_bytes());
                }
            }
            let count = self
                .device
                .write_buffer(self.vertices, 0, &self.vertex_bytes)
                .map_err(|e| gpu_error("vertex upload", e))?;
            if count != self.vertex_bytes.len() {
                return Err(format!(
                    "TRUEOS line vertex upload short: {count}/{}",
                    self.vertex_bytes.len()
                ));
            }
        }
        match self.target.begin_gpu_frame() {
            Ok(()) => {}
            Err(Ui4Error::Busy) => return Ok(false),
            Err(error) => return Err(ui_error(error)),
        }
        // Once UI4 grants this lease, a failed import or submission has an
        // uncertain completion. A fresh presenter is required after failure.
        self.poisoned = true;
        let surface = self
            .device
            .acquire_ui4_surface(self.target.render_target())
            .map_err(|e| gpu_error("surface acquire", e))?;
        let point = if lines.is_empty() {
            self.device
                .submit_ui4_clear(self.queue, surface, 0xff10_1820)
                .map_err(|e| gpu_error("empty scene clear", e))?
        } else {
            let mut batch = vgpu::IndexedDrawBatchV2 {
                draw_count: 1,
                clear_rgba8_srgb: 0xff10_1820,
                ..Default::default()
            };
            batch.draws[0] = vgpu::IndexedBatchDrawV2 {
                index_count: lines.len() as u32,
                rgba8_srgb: 0xffff_ffff,
                topology: vgpu::PRIMITIVE_TOPOLOGY_LINE_LIST,
                ..Default::default()
            };
            self.device
                .submit_ui4_indexed_batch_v2(
                    self.queue,
                    surface,
                    self.pipeline,
                    self.vertices,
                    self.indices,
                    batch,
                )
                .map_err(|e| gpu_error("line submit", e))?
        };
        self.device
            .wait(self.queue, point.value)
            .map_err(|e| gpu_error("line retirement", e))?;
        self.poisoned = false;
        self.pending_publish = true;
        self.publish()
    }

    fn publish(&mut self) -> Result<bool, String> {
        match self.target.publish(Damage::full(self.width, self.height)) {
            Ok(()) => {
                self.pending_publish = false;
                Ok(true)
            }
            Err(Ui4Error::Busy) => Ok(false),
            Err(error) => Err(ui_error(error)),
        }
    }
}

impl Drop for LinePresenter {
    fn drop(&mut self) {
        // Device teardown owns any still-live buffers on uncertain completion.
        if !self.poisoned {
            let _ = self.device.destroy_buffer(self.indices);
            let _ = self.device.destroy_buffer(self.vertices);
            let _ = self.device.destroy_render_pipeline(self.pipeline);
            let _ = self.device.destroy_shader_module(self.shader);
            let _ = self.device.destroy_queue(self.queue);
        }
        let _ = self.device.close();
    }
}

fn gpu_error(operation: &str, code: i32) -> String {
    format!("TRUEOS line {operation} failed ({code})")
}

fn ui_error(error: Ui4Error) -> String {
    format!("TRUEOS line UI4 failed ({error:?})")
}
