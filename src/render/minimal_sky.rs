//! Fixed BareMinimum + Flat-cloud sky. Matches include/sky.glsl's
//! get_sky_color(): there are no directional features in this profile.
pub fn rgba8(sun_z: f32) -> u32 {
    let night = sun_z.max(0.0);
    let day = (-sun_z).max(0.0);
    let dusk = [1.78, 0.2, 0.15];
    let dark = [0.001, 0.003, 0.01125];
    let light = [0.14, 0.39, 0.75];
    let rgb: [u8; 3] = core::array::from_fn(|i| {
        let twilight = dusk[i] * (1.0 - night) + dark[i] * night;
        let value = twilight * (1.0 - day) + light[i] * day;
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    });
    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255])
}

#[cfg(target_os = "trueos")]
pub struct NativeSky {
    device: trueos::vgpu::Device,
    queue: trueos::vgpu::Queue,
    figure: Option<NativeFigure>,
}
#[cfg(target_os = "trueos")]
impl NativeSky {
    pub fn open() -> Result<Self, String> {
        use trueos::vgpu::{Capabilities, Device, QueueClass};
        let device = Device::open(
            // vgpu::open requires BUFFER, QUEUE, and TIMELINE for every
            // tenant device, including this clear-only producer.
            Capabilities::BUFFER
                .union(Capabilities::QUEUE)
                .union(Capabilities::TIMELINE)
                .union(Capabilities::RENDER)
                .union(Capabilities::PRESENT),
        )
        .map_err(|e| format!("sky device open: {e}"))?;
        match device.create_queue(QueueClass::Render) {
            Ok(queue) => Ok(Self {
                device,
                queue,
                figure: None,
            }),
            Err(e) => {
                let _ = device.close();
                Err(format!("sky render queue: {e}"))
            }
        }
    }
    pub fn draw_figure(
        &mut self,
        target: u32,
        rgba: u32,
        frame: &super::figure_preview::Frame,
    ) -> Result<(), i32> {
        if self
            .figure
            .as_ref()
            .is_none_or(|figure| !std::sync::Arc::ptr_eq(&figure.geometry, &frame.geometry))
        {
            tracing::info!("Figure preview: native buffers and pipeline preparation entered");
            // The previous submission is retired before these buffers are replaced.
            if let Some(previous) = self.figure.take() {
                previous.destroy(self.device);
            }
            self.figure = Some(NativeFigure::new(self.device, frame.geometry.clone())?);
            tracing::info!("Figure preview: native buffers and pipeline ready");
        }
        let figure = self.figure.as_ref().unwrap();
        if self.device.write_buffer(figure.vertices, 0, &frame.state)? != frame.state.len() {
            return Err(trueos::vgpu::ERR_IO);
        }
        let surface = self.device.acquire_ui4_surface(target)?;
        use trueos::vgpu::*;
        let point = self
            .device
            .submit_ui4_indexed(
                self.queue,
                surface,
                figure.pipeline,
                figure.vertices,
                figure.indices,
                IndexedDraw {
                    vertex_offset: VOXY_FIGURE_STATE_BYTES as u64,
                    index_count: figure.geometry.indices.len() as u32,
                    clear_rgba8_srgb: rgba,
                    topology: PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                    sampled_texture: figure.atlas.raw(),
                    texture_width: figure.geometry.atlas_size[0],
                    texture_height: figure.geometry.atlas_size[1],
                    texture_pitch: figure.geometry.atlas_size[0] * 4,
                    texture_reserved: INDEXED_DRAW_DRAWABLE_DEPTH
                        | INDEXED_DRAW_DEPTH_TEST
                        | INDEXED_DRAW_DEPTH_WRITE
                        | INDEXED_DRAW_CLEAR_DEPTH
                        | (6 << INDEXED_DRAW_DEPTH_COMPARE_SHIFT),
                    ..Default::default()
                },
            )
            .map_err(|e| if e == ERR_BUSY { ERR_IO } else { e })?;
        self.device.wait(self.queue, point.value)
    }

    /// Caller owns the acquired UI4 lease and publishes after this returns.
    /// The vgpu clear consumes the import and stages its exact GPU release.
    pub fn draw(&self, target: u32, rgba: u32) -> Result<(), i32> {
        let surface = self.device.acquire_ui4_surface(target)?;
        // Import Busy retains the caller's write lease. Submission failure
        // drops the import and cancels it, so it must not be retried as if the
        // old write lease were still active. This worker owns its queue alone.
        let point = self
            .device
            .submit_ui4_clear(self.queue, surface, rgba)
            .map_err(|e| {
                if e == trueos::vgpu::ERR_BUSY {
                    trueos::vgpu::ERR_IO
                } else {
                    e
                }
            })?;
        self.device.wait(self.queue, point.value)
    }
}
#[cfg(target_os = "trueos")]
impl Drop for NativeSky {
    fn drop(&mut self) {
        if let Some(figure) = self.figure.take() {
            figure.destroy(self.device);
        }
        let _ = self.device.destroy_queue(self.queue);
        let _ = self.device.close();
    }
}

#[cfg(target_os = "trueos")]
struct NativeFigure {
    geometry: std::sync::Arc<super::figure_preview::Geometry>,
    shader: trueos::vgpu::ShaderModule,
    pipeline: trueos::vgpu::RenderPipeline,
    vertices: trueos::vgpu::Buffer,
    indices: trueos::vgpu::Buffer,
    atlas: trueos::vgpu::Buffer,
}
#[cfg(target_os = "trueos")]
impl NativeFigure {
    fn new(
        device: trueos::vgpu::Device,
        geometry: std::sync::Arc<super::figure_preview::Geometry>,
    ) -> Result<Self, i32> {
        use trueos::vgpu::*;
        let shader = device.create_shader_module(SHADER_PACKAGE_VOXY_FIGURE_FNV1A64)?;
        let mut buffers = Vec::new();
        let mut created_pipeline = None;
        let result = (|| {
            let pipeline = device.create_render_pipeline(shader, 8, 0)?;
            created_pipeline = Some(pipeline);
            let vertices = device.create_buffer(
                VOXY_FIGURE_STATE_BYTES + geometry.vertices.len() * 8,
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX,
            )?;
            buffers.push(vertices);
            let indices = device.create_buffer(
                geometry.indices.len() * 4,
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX,
            )?;
            buffers.push(indices);
            let atlas = device.create_buffer(geometry.atlas.len() * 4, BUFFER_USAGE_MAP_WRITE)?;
            buffers.push(atlas);
            for (buffer, offset, data) in [
                (
                    vertices,
                    VOXY_FIGURE_STATE_BYTES,
                    bytemuck::cast_slice(geometry.vertices.as_slice()),
                ),
                (
                    indices,
                    0,
                    bytemuck::cast_slice(geometry.indices.as_slice()),
                ),
                (atlas, 0, bytemuck::cast_slice(geometry.atlas.as_slice())),
            ] {
                if device.write_buffer(buffer, offset, data)? != data.len() {
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
