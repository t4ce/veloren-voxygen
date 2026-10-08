//! Admitted Flat cloud plane. Exact f32 frame/weather values and original noise
//! pixels share one sampled image; the shader keeps the original cloud arithmetic.
use specs::WorldExt;
use std::sync::Arc;
pub(crate) struct Frame {
    pub pixels: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
}
pub(crate) fn from_client(
    client: &crate::client::Client,
    camera: [[f32; 4]; 5],
    ambiance: f32,
) -> Option<Frame> {
    use common::assets::AssetExt;
    static NOISE: std::sync::OnceLock<image::RgbaImage> = std::sync::OnceLock::new();
    let noise = NOISE.get_or_init(|| {
        common::assets::Image::load_expect("voxygen.texture.noise")
            .read()
            .0
            .to_rgba8()
    });
    let weather = client.state().weather_grid();
    let world = client.world_data();
    let time = client
        .state()
        .ecs()
        .read_resource::<common::resources::TimeOfDay>();
    let sun = time.get_sun_dir();
    // Until the client has a valid world/camera, keep the scene transparent.
    if world.chunk_size().x == 0 || world.chunk_size().y == 0
        || !camera.iter().flatten().all(|v| v.is_finite())
        || camera[4][0] <= 0.0 || camera[4][1] <= 0.0
        || !time.0.is_finite() || !ambiance.is_finite()
        || !world.min_chunk_alt().is_finite() || !world.max_chunk_alt().is_finite()
    {
        return None;
    }
    let width = noise.width().max(weather.size().x).max(32);
    let height = 1 + noise.height() + weather.size().y.max(1);
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let mut params = [0f32; 32];
    for i in 0..4 {
        params[i * 4..i * 4 + 4].copy_from_slice(&camera[i]);
    }
    params[15] = camera[4][0];
    params[16] = camera[4][1];
    params[17] = (time.0 % 86400.0) as f32;
    params[18] = (time.0 / 86400.0 % 1000.0) as f32;
    params[19..22].copy_from_slice(&sun.into_array());
    params[22] = world.min_chunk_alt();
    params[23] = world.max_chunk_alt();
    params[24] = f32::from(world.chunk_size().x);
    params[25] = f32::from(world.chunk_size().y);
    params[26] = client.weather_at_player().rain;
    params[27] = ambiance;
    params[28] = noise.width() as f32;
    params[29] = noise.height() as f32;
    params[30] = weather.size().x.max(1) as f32;
    params[31] = weather.size().y.max(1) as f32;
    for (i, p) in params.iter().enumerate() {
        pixels[i * 4..i * 4 + 4].copy_from_slice(&p.to_le_bytes());
    }
    for y in 0..noise.height() {
        let dst = (y as usize + 1) * width as usize * 4;
        let src = y as usize * noise.width() as usize * 4;
        pixels[dst..dst + noise.width() as usize * 4]
            .copy_from_slice(&noise.as_raw()[src..src + noise.width() as usize * 4]);
    }
    for (pos, cell) in weather.iter() {
        let index = ((1 + noise.height() + pos.y as u32) * width + pos.x as u32) as usize * 4;
        pixels[index..index + 4].copy_from_slice(&cell.cloud.to_le_bytes());
    }
    Some(Frame {
        pixels: pixels.into(),
        width,
        height,
    })
}

pub(crate) struct NativeClouds {
    device: trueos::vgpu::Device,
    queue: trueos::vgpu::Queue,
    shader: trueos::vgpu::ShaderModule,
    pipeline: trueos::vgpu::RenderPipeline,
    vertices: trueos::vgpu::Buffer,
    indices: trueos::vgpu::Buffer,
    pixels: Option<(trueos::vgpu::Buffer, usize)>,
    unknown_completion: bool,
}
impl NativeClouds {
    pub fn open() -> Result<Self, String> {
        use trueos::vgpu::*;
        let device = Device::open(
            Capabilities::BUFFER
                .union(Capabilities::QUEUE)
                .union(Capabilities::TIMELINE)
                .union(Capabilities::RENDER)
                .union(Capabilities::PRESENT),
        )
        .map_err(|e| format!("cloud device: {e}"))?;
        // Assemble progressively; each failure retires only resources already created.
        let queue = device.create_queue(QueueClass::Render).map_err(|e| {
            let _ = device.close();
            format!("cloud queue: {e}")
        })?;
        let result = (|| {
            let shader = device.create_shader_module(SHADER_PACKAGE_VOXY_FLAT_CLOUD_FNV1A64)?;
            let pipeline = match device.create_render_pipeline(shader, 20, 0) {
                Ok(p) => p,
                Err(e) => {
                    let _ = device.destroy_shader_module(shader);
                    return Err(e);
                }
            };
            let vertices =
                match device.create_buffer(60, BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = device.destroy_render_pipeline(pipeline);
                        let _ = device.destroy_shader_module(shader);
                        return Err(e);
                    }
                };
            let indices =
                match device.create_buffer(12, BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = device.destroy_buffer(vertices);
                        let _ = device.destroy_render_pipeline(pipeline);
                        let _ = device.destroy_shader_module(shader);
                        return Err(e);
                    }
                };
            let verts: [[f32; 5]; 3] = [
                [-1., -1., 0., 0., 1.],
                [3., -1., 0., 2., 1.],
                [-1., 3., 0., 0., -1.],
            ];
            let upload = (|| {
                if device.write_buffer(vertices, 0, bytemuck::cast_slice(&verts))? != 60
                    || device.write_buffer(indices, 0, bytemuck::cast_slice(&[0u32, 1, 2]))? != 12
                {
                    return Err(ERR_IO);
                }
                Ok(())
            })();
            if let Err(error) = upload {
                let _ = device.destroy_buffer(indices);
                let _ = device.destroy_buffer(vertices);
                let _ = device.destroy_render_pipeline(pipeline);
                let _ = device.destroy_shader_module(shader);
                return Err(error);
            }
            Ok(Self {
                device,
                queue,
                shader,
                pipeline,
                vertices,
                indices,
                pixels: None,
                unknown_completion: false,
            })
        })();
        match result {
            Ok(r) => Ok(r),
            Err(e) => {
                let _ = device.destroy_queue(queue);
                let _ = device.close();
                Err(format!("cloud pipeline: {e}"))
            }
        }
    }
    pub fn draw(&mut self, target: u32, frame: &Frame) -> Result<(), i32> {
        use trueos::vgpu::*;
        if self
            .pixels
            .as_ref()
            .is_none_or(|(_, size)| *size != frame.pixels.len())
        {
            let buffer = self
                .device
                .create_buffer(frame.pixels.len(), BUFFER_USAGE_MAP_WRITE)?;
            if let Some((old, _)) = self.pixels.replace((buffer, frame.pixels.len())) {
                self.device.destroy_buffer(old)?;
            }
        }
        let pixels = self.pixels.as_ref().unwrap().0;
        if self.device.write_buffer(pixels, 0, &frame.pixels)? != frame.pixels.len() {
            return Err(ERR_IO);
        }
        let surface = self.device.acquire_ui4_surface(target)?;
        self.unknown_completion = true;
        let point = self
            .device
            .submit_ui4_indexed(
                self.queue,
                surface,
                self.pipeline,
                self.vertices,
                self.indices,
                IndexedDraw {
                    index_count: 3,
                    clear_rgba8_srgb: 0,
                    topology: PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                    sampled_texture: pixels.raw(),
                    texture_width: frame.width,
                    texture_height: frame.height,
                    texture_pitch: frame.width * 4,
                    sampler_flags: 3,
                    ..Default::default()
                },
            )
            .map_err(|e| if e == ERR_BUSY { ERR_IO } else { e })?;
        self.device.wait(self.queue, point.value)?;
        self.unknown_completion = false;
        Ok(())
    }
}
impl Drop for NativeClouds {
    fn drop(&mut self) {
        if self.unknown_completion {
            return;
        }
        if let Some((p, _)) = self.pixels.take() {
            let _ = self.device.destroy_buffer(p);
        }
        let _ = self.device.destroy_buffer(self.indices);
        let _ = self.device.destroy_buffer(self.vertices);
        let _ = self.device.destroy_render_pipeline(self.pipeline);
        let _ = self.device.destroy_shader_module(self.shader);
        let _ = self.device.destroy_queue(self.queue);
        let _ = self.device.close();
    }
}
