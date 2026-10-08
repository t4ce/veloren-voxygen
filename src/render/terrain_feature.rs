//! Nearby terrain uses the exact palette shader and vertex layout of voxy-terrain.
//! The paired background is cleared to the existing sky color in the same draw.
use crate::terrain_preview::{ATLAS_SIZE, Vertex};
use std::sync::Arc;

pub(crate) struct Geometry {
    pub vertices: Arc<[Vertex]>,
    pub atlas: Arc<[[u8; 4]]>,
}
pub(crate) struct Frame {
    pub geometry: Arc<Geometry>,
    pub camera: [[f32; 4]; 5],
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
    unknown_completion: bool,
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
                unknown_completion: false,
            }),
            Err(error) => {
                let _ = device.close();
                Err(format!("terrain queue: {error}"))
            }
        }
    }

    /// Caller has leased the paired background and publishes after completion.
    pub fn draw(&mut self, target: u32, frame: &Frame, load_color: bool) -> Result<(), i32> {
        use trueos::vgpu::*;
        if self
            .mesh
            .as_ref()
            .is_none_or(|mesh| !Arc::ptr_eq(&mesh.geometry, &frame.geometry))
        {
            // Construct before replacing the retired mesh, so partial creation
            // failures clean up their own resources without losing the old one.
            let mesh = Mesh::new(self.device, frame.geometry.clone())?;
            if let Some(previous) = self.mesh.replace(mesh) {
                previous.destroy(self.device);
            }
            tracing::info!(target: "voxy_scene_contract", vertices = frame.geometry.vertices.len(),
                "Native terrain palette and mesh ready; avatar and entity proxies guarded");
        }
        let mesh = self.mesh.as_ref().unwrap();
        let state = bytemuck::cast_slice(&frame.camera);
        if self.device.write_buffer(mesh.vertices, 0, state)? != state.len() {
            return Err(ERR_IO);
        }
        let surface = self.device.acquire_ui4_surface(target)?;
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
        if let Some(mesh) = self.mesh.take() {
            mesh.destroy(self.device);
        }
        let _ = self.device.destroy_queue(self.queue);
        let _ = self.device.close();
    }
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
