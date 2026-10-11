//! Retained terrain: opaque vertex color for the fixed capture, palette for previews.
//! One retained terrain draw follows the flat clouds on the paired background.
use crate::terrain_preview::{ATLAS_SIZE, Vertex};
use std::sync::Arc;

pub(crate) struct Geometry {
    pub vertices: Arc<[Vertex]>,
    // Empty atlas selects the admitted opaque-color program.
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
    retired_step: u8,
    figure_draws: u32,
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
                retired_step: 0,
                figure_draws: 0,
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
        // Ready meshes arrive independently. Their position in this frame's
        // list is not an identity: inserting one must not rebuild its neighbors.
        for (index, frame) in frames.iter().enumerate() {
            let slot = self.figures.iter().position(|cache| cache.as_ref()
                .is_some_and(|figure| figure.has_geometry(&frame.geometry)))
                .unwrap_or_else(|| { self.figures.push(None); self.figures.len() - 1 });
            let cache = &mut self.figures[slot];
            self.figure_draws = self.figure_draws.saturating_add(1);
            let trace = self.figure_draws <= 1024;
            if trace { tracing::info!(draw = self.figure_draws, index, vertices = frame.geometry.vertices.len(), "world-figure: draw entered"); }
            self.unknown_completion = true;
            super::minimal_sky::NativeSky::draw_figure_on(self.device, self.queue, cache, target, 0, frame, true, true)?;
            self.unknown_completion = false;
            if trace { tracing::info!(draw = self.figure_draws, index, "world-figure: draw retired"); }
        }
        let mut slot = 0;
        while slot < self.figures.len() {
            if self.figures[slot].as_ref().is_some_and(|figure|
                frames.iter().any(|frame| figure.has_geometry(&frame.geometry))) {
                slot += 1;
            } else if let Some(figure) = self.figures.swap_remove(slot) {
                tracing::info!("world-figure: stale mesh release entered");
                figure.destroy(self.device);
                tracing::info!("world-figure: stale mesh release complete");
            }
        }
        Ok(())
    }

    /// CPU producers are asynchronous; GPU ownership stays on this thread.
    /// One allocation or a bounded upload slice per frame fills the back buffer.
    pub fn draw_buffered(&mut self, target: u32, frame: &Frame, load_color: bool) -> Result<(), i32> {
        if let Some(mesh) = &self.retired_mesh {
            // Reclaim at most one retired object per frame; no GPU ABI worker.
            match self.retired_step {
                0 => { let _ = self.device.destroy_buffer(mesh.vertices); },
                1 => { let _ = self.device.destroy_buffer(mesh.indices); },
                2 => { if let Some(atlas) = mesh.atlas { let _ = self.device.destroy_buffer(atlas); } },
                3 => { let _ = self.device.destroy_render_pipeline(mesh.pipeline); },
                _ => { let _ = self.device.destroy_shader_module(mesh.shader); },
            }
            self.retired_step += 1;
            if self.retired_step == 5 { self.retired_mesh = None; self.retired_step = 0; }
        }
        if self.pending_upload.is_none() && self.retired_mesh.is_none()
            && std::time::Instant::now() >= self.next_upload
            && self.mesh.as_ref().is_none_or(|mesh| !Arc::ptr_eq(&mesh.geometry, &frame.geometry)) {
            self.pending_upload = Some(Upload::new(frame.geometry.clone(), frame.metrics));
        }
        if let Some(upload) = self.pending_upload.as_mut() {
            match upload.step(self.device) {
                Ok(Some(mesh)) => {
                    self.buffered_upload_bytes = upload.written as u64;
                    self.resident_metrics = Some(upload.metrics);
                    self.retired_mesh = self.mesh.replace(mesh);
                    self.retired_step = 0;
                    self.pending_upload = None;
                }
                Ok(None) => {},
                Err(code) => {
                    if let Some(upload) = self.pending_upload.take() { upload.destroy(self.device); }
                    self.next_upload = std::time::Instant::now() + std::time::Duration::from_secs(1);
                    tracing::warn!(code, "Terrain back-buffer preparation failed; retaining front buffer");
                },
            }
        }
        let Some(mesh) = self.mesh.as_ref() else { return Err(trueos::vgpu::ERR_BUSY); };
        let mut metrics = self.resident_metrics.unwrap_or(frame.metrics);
        metrics.width = frame.metrics.width; metrics.height = frame.metrics.height;
        metrics.loaded_chunks = frame.metrics.loaded_chunks;
        let resident = Frame { geometry: mesh.geometry.clone(), camera: frame.camera,
            metrics, prepared_at: frame.prepared_at };
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
                    sampled_texture: mesh.atlas.map_or(0, |atlas| atlas.raw()),
                    texture_width: if mesh.atlas.is_some() { ATLAS_SIZE } else { 0 },
                    texture_height: if mesh.atlas.is_some() { ATLAS_SIZE } else { 0 },
                    texture_pitch: if mesh.atlas.is_some() { ATLAS_SIZE * 4 } else { 0 },
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
        if let Some(upload) = self.pending_upload.take() { upload.destroy(self.device); }
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
const UPLOAD_SLICE: usize = 256 * 1024;
#[cfg(target_os = "trueos")]
struct Upload {
    geometry: Arc<Geometry>,
    metrics: super::terrain_layers::Metrics,
    shader: Option<trueos::vgpu::ShaderModule>,
    pipeline: Option<trueos::vgpu::RenderPipeline>,
    buffers: [Option<trueos::vgpu::Buffer>; 3],
    stage: u8,
    offset: usize,
    written: usize,
}
#[cfg(target_os = "trueos")]
impl Upload {
    fn new(geometry: Arc<Geometry>, metrics: super::terrain_layers::Metrics) -> Self {
        Self { geometry, metrics, shader: None, pipeline: None, buffers: [None;3], stage: 0, offset: 0, written: 0 }
    }
    fn step(&mut self, device: trueos::vgpu::Device) -> Result<Option<Mesh>, i32> {
        use trueos::vgpu::*;
        if self.stage == 0 && (self.geometry.vertices.is_empty()
            || self.geometry.vertices.len() % 3 != 0 || self.geometry.vertices.len() > if self.geometry.atlas.is_empty() {((u32::MAX as usize - 96) / 32) / 3 * 3} else {crate::terrain_preview::MAX_VERTICES}
            || !self.geometry.atlas.is_empty() && self.geometry.atlas.len() != (ATLAS_SIZE * ATLAS_SIZE) as usize) { return Err(ERR_UNSUPPORTED); }
        if self.offset == 0 { tracing::info!(stage = self.stage, "terrain-upload: stage entered; owner=presentation"); }
        match self.stage {
            0 => self.shader = Some(device.create_shader_module(if self.geometry.atlas.is_empty() {SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64} else {SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64})?),
            1 => self.pipeline = Some(device.create_render_pipeline(self.shader.unwrap(),32,0)?),
            2 => self.buffers[0] = Some(device.create_buffer(80+self.geometry.vertices.len()*32, BUFFER_USAGE_MAP_WRITE|BUFFER_USAGE_VERTEX)?),
            3 => self.buffers[1] = Some(device.create_buffer(self.geometry.vertices.len()*4, BUFFER_USAGE_MAP_WRITE|BUFFER_USAGE_INDEX)?),
            4 | 7 if self.geometry.atlas.is_empty() => {},
            4 => self.buffers[2] = Some(device.create_buffer(self.geometry.atlas.len()*4, BUFFER_USAGE_MAP_WRITE)?),
            5..=7 => {
                let slot = (self.stage-5) as usize;
                let index_bytes;
                let bytes: &[u8] = match slot {
                    0 => bytemuck::cast_slice(self.geometry.vertices.as_ref()),
                    1 => {
                        let first = self.offset/4;
                        let count = (self.geometry.vertices.len()-first).min(UPLOAD_SLICE/4);
                        index_bytes = (first..first+count).map(|v| v as u32).collect::<Vec<_>>();
                        bytemuck::cast_slice(index_bytes.as_slice())
                    }
                    _ => bytemuck::cast_slice(self.geometry.atlas.as_ref()),
                };
                let total = match slot { 0 => self.geometry.vertices.len()*32, 1 => self.geometry.vertices.len()*4, _ => self.geometry.atlas.len()*4 };
                let part = if slot == 1 { bytes } else { &bytes[self.offset..(self.offset+UPLOAD_SLICE).min(total)] };
                let destination = self.offset + if slot == 0 {80} else {0};
                if device.write_buffer(self.buffers[slot].unwrap(),destination,part)? != part.len() { return Err(ERR_IO); }
                self.offset += part.len(); self.written += part.len();
                if self.offset < total { return Ok(None); }
            }
            _ => unreachable!(),
        }
        tracing::info!(stage = self.stage, "terrain-upload: stage complete");
        self.stage += 1; self.offset = 0;
        if self.stage == 8 {
            Ok(Some(Mesh { geometry: self.geometry.clone(), shader: self.shader.take().unwrap(),
                pipeline: self.pipeline.take().unwrap(), vertices: self.buffers[0].take().unwrap(),
                indices: self.buffers[1].take().unwrap(), atlas: self.buffers[2].take() }))
        } else { Ok(None) }
    }
    fn destroy(mut self, device: trueos::vgpu::Device) {
        for buffer in self.buffers.iter_mut().filter_map(Option::take) { let _ = device.destroy_buffer(buffer); }
        if let Some(pipeline) = self.pipeline.take() { let _ = device.destroy_render_pipeline(pipeline); }
        if let Some(shader) = self.shader.take() { let _ = device.destroy_shader_module(shader); }
    }
}
#[cfg(target_os = "trueos")]
struct Mesh {
    geometry: Arc<Geometry>,
    shader: trueos::vgpu::ShaderModule,
    pipeline: trueos::vgpu::RenderPipeline,
    vertices: trueos::vgpu::Buffer,
    indices: trueos::vgpu::Buffer,
    atlas: Option<trueos::vgpu::Buffer>,
}
#[cfg(target_os = "trueos")]
impl Mesh {
    fn new(device: trueos::vgpu::Device, geometry: Arc<Geometry>) -> Result<Self, i32> {
        use trueos::vgpu::*;
        if geometry.vertices.is_empty()
            || geometry.vertices.len() % 3 != 0
            || geometry.vertices.len() > if geometry.atlas.is_empty() {((u32::MAX as usize - 96) / 32) / 3 * 3} else {crate::terrain_preview::MAX_VERTICES}
            || (!geometry.atlas.is_empty() && geometry.atlas.len() != (ATLAS_SIZE * ATLAS_SIZE) as usize)
        {
            return Err(ERR_UNSUPPORTED);
        }
        let shader = device.create_shader_module(if geometry.atlas.is_empty() {SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64} else {SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64})?;
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
            let atlas = if geometry.atlas.is_empty() { None } else {
                Some(device.create_buffer(geometry.atlas.len() * 4, BUFFER_USAGE_MAP_WRITE)?)
            };
            buffers.extend(atlas);
            let sequential: Vec<u32> = (0..geometry.vertices.len() as u32).collect();
            for (buffer, offset, bytes) in [
                (
                    Some(vertices),
                    80,
                    bytemuck::cast_slice(geometry.vertices.as_ref()),
                ),
                (Some(indices), 0, bytemuck::cast_slice(sequential.as_slice())),
                (atlas, 0, bytemuck::cast_slice(geometry.atlas.as_ref())),
            ] {
                let Some(buffer) = buffer else { continue; };
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
        if let Some(atlas) = self.atlas { let _ = device.destroy_buffer(atlas); }
        let _ = device.destroy_render_pipeline(self.pipeline);
        let _ = device.destroy_shader_module(self.shader);
    }
}
