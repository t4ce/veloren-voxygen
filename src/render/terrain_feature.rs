//! Near and whole-map terrain share the admitted palette shader and vertex layout.
//! One retained terrain draw follows the flat clouds on the paired background.
use crate::terrain_preview::{ATLAS_SIZE, Vertex};
use std::sync::Arc;

pub(crate) struct Geometry {
    pub vertices: Arc<[Vertex]>,
    pub atlas: Arc<[[u8; 4]]>,
}
pub(crate) struct Frame {
    pub geometry: Arc<Geometry>,
    pub camera: [[f32; 4]; 5],
    pub metrics: super::terrain_layers::Metrics,
    pub prepared_at: std::time::Instant,
}

pub(crate) fn camera(
    eye: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
    fov: f32,
    aspect: f32,
) -> [[f32; 4]; 5] {
    let vector = |v: [f32; 3]| [v[0], v[1], v[2], 0.0];
    [
        vector(eye),
        vector(right),
        vector(up),
        vector(forward),
        [1.0 / (fov * 0.5).tan(), aspect, 0.1, 256.0],
    ]
}

/// Voxy stores its camera in coordinates relative to the truncated focus.
/// The terrain shader needs an absolute eye and the inverse-view basis.
pub(crate) fn camera_from_view(
    inverse_view: vek::Mat4<f32>,
    focus: vek::Vec3<f32>,
    fov: f32,
    aspect: f32,
) -> [[f32; 4]; 5] {
    let vector = |axis| vek::Vec3::from(inverse_view * axis).into_array();
    let eye = vek::Vec3::from(inverse_view * vek::Vec4::unit_w()) + focus.map(f32::trunc);
    camera(
        eye.into_array(),
        vector(vek::Vec4::unit_x()),
        vector(vek::Vec4::unit_y()),
        vector(-vek::Vec4::unit_z()),
        fov,
        aspect,
    )
}

#[cfg(target_os = "trueos")]
pub(crate) struct NativeTerrain {
    device: trueos::vgpu::Device,
    queue: trueos::vgpu::Queue,
    mesh: Option<Mesh>,
    pending_upload: Option<Upload>,
    retired_mesh: Option<Mesh>,
    resident_metrics: Option<super::terrain_layers::Metrics>,
    next_upload: std::time::Instant,
    buffered_upload_bytes: u64,
    figures: Vec<Option<super::minimal_sky::NativeFigure>>,
    unknown_completion: bool,
    heartbeat: super::terrain_heartbeat::Heartbeat,
}
#[cfg(target_os = "trueos")]
impl NativeTerrain {
    pub fn open() -> Result<Self, String> {
        use trueos::vgpu::*;
        let device = Device::open(
            Capabilities::BUFFER
                .union(Capabilities::QUEUE)
                .union(Capabilities::TIMELINE)
                .union(Capabilities::RENDER)
                .union(Capabilities::PRESENT),
        )
        .map_err(|error| format!("terrain device: {error}"))?;
        match device.create_queue(QueueClass::Render) {
            Ok(queue) => Ok(Self {
                device,
                queue,
                mesh: None,
                pending_upload: None,
                retired_mesh: None,
                resident_metrics: None,
                next_upload: std::time::Instant::now(),
                buffered_upload_bytes: 0,
                figures: Vec::new(),
                unknown_completion: false,
                heartbeat: super::terrain_heartbeat::Heartbeat::new(),
            }),
            Err(error) => {
                let _ = device.close();
                Err(format!("terrain queue: {error}"))
            }
        }
    }

    pub fn heartbeat(&mut self) { self.heartbeat.tick(); }
    pub fn busy(&mut self) { self.heartbeat.busy(); }
    pub fn published(&mut self, frame: &Frame) {
        self.heartbeat.published(self.resident_metrics.unwrap_or(frame.metrics), frame.prepared_at.elapsed().as_micros() as u64);
    }

    pub fn draw_entities(&mut self, target: u32, frames: &[Arc<super::figure_preview::Frame>]) -> Result<(), i32> {
        while self.figures.len() > frames.len() {
            if let Some(figure) = self.figures.pop().flatten() { figure.destroy(self.device); }
        }
        self.figures.resize_with(frames.len(), || None);
        for (cache, frame) in self.figures.iter_mut().zip(frames) {
            self.unknown_completion = true;
            super::minimal_sky::NativeSky::draw_figure_on(self.device, self.queue, cache, target, 0, frame, true, true)?;
            self.unknown_completion = false;
        }
        Ok(())
    }

    /// Never prepare a replacement on the presentation thread. A single worker
    /// fills a back buffer while the front buffer remains complete and drawable.
    pub fn draw_buffered(&mut self, target: u32, frame: &Frame, load_color: bool) -> Result<(), i32> {
        use std::sync::mpsc::TryRecvError;
        if let Some(upload) = &self.pending_upload {
            let result = match upload.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(trueos::vgpu::ERR_IO)),
            };
            if let Some(result) = result {
                let upload = self.pending_upload.take().unwrap();
                // Completion was sent after all buffer writes; joining occurs
                // only on shutdown, never on the steady-state draw path.
                match result {
                    Ok(mesh) => {
                        self.buffered_upload_bytes = (mesh.geometry.vertices.len() * 36 + mesh.geometry.atlas.len() * 4) as u64;
                        // The previous draw fence has retired. Hand reclamation
                        // to the next preparation worker rather than freeing a
                        // large buffer set on the presentation thread.
                        self.retired_mesh = self.mesh.replace(mesh);
                        self.resident_metrics = Some(upload.metrics);
                    }
                    Err(code) => {
                        self.next_upload = std::time::Instant::now() + std::time::Duration::from_secs(1);
                        tracing::warn!(code, "Terrain back-buffer preparation failed; retaining front buffer");
                    },
                }
            }
        }
        if self.pending_upload.is_none() && std::time::Instant::now() >= self.next_upload
            && self.mesh.as_ref().is_none_or(|mesh|
            !Arc::ptr_eq(&mesh.geometry, &frame.geometry)) {
            let geometry = frame.geometry.clone();
            let device = self.device;
            let retired = self.retired_mesh.take().map(|mesh| Retired { device, mesh: Some(mesh) });
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker = std::thread::Builder::new().name("terrain-upload".into()).spawn(move || {
                drop(retired);
                let result = Mesh::new(device, geometry);
                if let Err(error) = sender.send(result) {
                    if let Ok(mesh) = error.0 { mesh.destroy(device); }
                }
            });
            match worker {
                Ok(thread) => self.pending_upload = Some(Upload { receiver, thread, metrics: frame.metrics }),
                Err(error) => {
                    self.next_upload = std::time::Instant::now() + std::time::Duration::from_secs(1);
                    tracing::warn!(%error, "Terrain upload worker unavailable; retaining front buffer");
                },
            }
        }
        let Some(mesh) = self.mesh.as_ref() else { return Err(trueos::vgpu::ERR_BUSY); };
        let mut metrics = self.resident_metrics.unwrap_or(frame.metrics);
        metrics.width = frame.metrics.width; metrics.height = frame.metrics.height;
        metrics.loaded_chunks = frame.metrics.loaded_chunks;
        let resident = Frame { geometry: mesh.geometry.clone(), camera: frame.camera,
            metrics, prepared_at: frame.prepared_at };
        // Arc identity now matches the front buffer, so draw performs no mesh creation.
        self.draw(target, &resident, load_color)
    }

    /// Caller has leased the paired background and publishes after completion.
    pub fn draw(&mut self, target: u32, frame: &Frame, load_color: bool) -> Result<(), i32> {
        use trueos::vgpu::*;
        let started = std::time::Instant::now();
        let mut uploaded = 80u64 + std::mem::take(&mut self.buffered_upload_bytes);
        if self
            .mesh
            .as_ref()
            .is_none_or(|mesh| !Arc::ptr_eq(&mesh.geometry, &frame.geometry))
        {
            // Construct before replacing the retired mesh, so partial creation
            // failures clean up their own resources without losing the old one.
            let mesh = Mesh::new(self.device, frame.geometry.clone())?;
            uploaded += (frame.geometry.vertices.len() * 36 + frame.geometry.atlas.len() * 4) as u64;
            if let Some(previous) = self.mesh.replace(mesh) {
                previous.destroy(self.device);
            }
            tracing::info!(target: "voxy_scene_contract", vertices = frame.geometry.vertices.len(),
                "Native terrain palette and mesh ready; near terrain and world entities enabled");
        }
        let mesh = self.mesh.as_ref().unwrap();
        let state = bytemuck::cast_slice(&frame.camera);
        if self.device.write_buffer(mesh.vertices, 0, state)? != state.len() {
            return Err(ERR_IO);
        }
        self.heartbeat.uploaded(frame.metrics, uploaded);
        let surface = self.device.acquire_ui4_surface(target)?;
        let prepare_us = started.elapsed().as_micros() as u64;
        let submitted = std::time::Instant::now();
        self.unknown_completion = true;
        let point = self
            .device
            .submit_ui4_indexed(
                self.queue,
                surface,
                mesh.pipeline,
                mesh.vertices,
                mesh.indices,
                IndexedDraw {
                    vertex_offset: 80,
                    index_count: mesh.geometry.vertices.len() as u32,
                    // Uncovered pixels reveal the display-engine backdrop.
                    clear_rgba8_srgb: 0,
                    topology: PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                    sampled_texture: mesh.atlas.raw(),
                    texture_width: ATLAS_SIZE,
                    texture_height: ATLAS_SIZE,
                    texture_pitch: ATLAS_SIZE * 4,
                    texture_reserved: (if load_color {INDEXED_DRAW_LOAD_COLOR} else {0}) | INDEXED_DRAW_DRAWABLE_DEPTH
                        | INDEXED_DRAW_DEPTH_TEST
                        | INDEXED_DRAW_DEPTH_WRITE
                        | INDEXED_DRAW_CLEAR_DEPTH
                        | (3 << INDEXED_DRAW_DEPTH_COMPARE_SHIFT),
                    ..Default::default()
                },
            )
            .map_err(|error| if error == ERR_BUSY { ERR_IO } else { error })?;
        self.device.wait(self.queue, point.value)?;
        self.unknown_completion = false;
        self.heartbeat.retired(frame.metrics, prepare_us, submitted.elapsed().as_micros() as u64);
        Ok(())
    }
}
#[cfg(target_os = "trueos")]
impl Drop for NativeTerrain {
    fn drop(&mut self) {
        // An ambiguous submission may still reference these handles.
        if self.unknown_completion {
            return;
        }
        if let Some(upload) = self.pending_upload.take() {
            let _ = upload.thread.join();
            if let Ok(Ok(mesh)) = upload.receiver.try_recv() { mesh.destroy(self.device); }
        }
        if let Some(mesh) = self.retired_mesh.take() { mesh.destroy(self.device); }
        for figure in self.figures.drain(..).flatten() { figure.destroy(self.device); }
        if let Some(mesh) = self.mesh.take() {
            mesh.destroy(self.device);
        }
        let _ = self.device.destroy_queue(self.queue);
        let _ = self.device.close();
    }
}
#[cfg(target_os = "trueos")]
struct Retired {
    device: trueos::vgpu::Device,
    mesh: Option<Mesh>,
}
#[cfg(target_os = "trueos")]
impl Drop for Retired {
    fn drop(&mut self) {
        if let Some(mesh) = self.mesh.take() { mesh.destroy(self.device); }
    }
}
#[cfg(target_os = "trueos")]
struct Upload {
    receiver: std::sync::mpsc::Receiver<Result<Mesh, i32>>,
    thread: std::thread::JoinHandle<()>,
    metrics: super::terrain_layers::Metrics,
}
#[cfg(target_os = "trueos")]
struct Mesh {
    geometry: Arc<Geometry>,
    shader: trueos::vgpu::ShaderModule,
    pipeline: trueos::vgpu::RenderPipeline,
    vertices: trueos::vgpu::Buffer,
    indices: trueos::vgpu::Buffer,
    atlas: trueos::vgpu::Buffer,
}
#[cfg(target_os = "trueos")]
impl Mesh {
    fn new(device: trueos::vgpu::Device, geometry: Arc<Geometry>) -> Result<Self, i32> {
        use trueos::vgpu::*;
        if geometry.vertices.is_empty()
            || geometry.vertices.len() % 3 != 0
            || geometry.vertices.len() > crate::terrain_preview::MAX_VERTICES
            || geometry.atlas.len() != (ATLAS_SIZE * ATLAS_SIZE) as usize
        {
            return Err(ERR_UNSUPPORTED);
        }
        let shader = device.create_shader_module(SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64)?;
        let mut buffers = Vec::new();
        let mut created_pipeline = None;
        let result = (|| {
            let pipeline = device.create_render_pipeline(shader, 32, 0)?;
            created_pipeline = Some(pipeline);
            let vertices = device.create_buffer(
                80 + geometry.vertices.len() * 32,
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX,
            )?;
            buffers.push(vertices);
            let indices = device.create_buffer(
                geometry.vertices.len() * 4,
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX,
            )?;
            buffers.push(indices);
            let atlas = device.create_buffer(geometry.atlas.len() * 4, BUFFER_USAGE_MAP_WRITE)?;
            buffers.push(atlas);
            let sequential: Vec<u32> = (0..geometry.vertices.len() as u32).collect();
            for (buffer, offset, bytes) in [
                (
                    vertices,
                    80,
                    bytemuck::cast_slice(geometry.vertices.as_ref()),
                ),
                (indices, 0, bytemuck::cast_slice(sequential.as_slice())),
                (atlas, 0, bytemuck::cast_slice(geometry.atlas.as_ref())),
            ] {
                if device.write_buffer(buffer, offset, bytes)? != bytes.len() {
                    return Err(ERR_IO);
                }
            }
            Ok(Self {
                geometry,
                shader,
                pipeline,
                vertices,
                indices,
                atlas,
            })
        })();
        if result.is_err() {
            for buffer in buffers {
                let _ = device.destroy_buffer(buffer);
            }
            if let Some(pipeline) = created_pipeline {
                let _ = device.destroy_render_pipeline(pipeline);
            }
            let _ = device.destroy_shader_module(shader);
        }
        result
    }
    fn destroy(self, device: trueos::vgpu::Device) {
        let _ = device.destroy_buffer(self.vertices);
        let _ = device.destroy_buffer(self.indices);
        let _ = device.destroy_buffer(self.atlas);
        let _ = device.destroy_render_pipeline(self.pipeline);
        let _ = device.destroy_shader_module(self.shader);
    }
}
