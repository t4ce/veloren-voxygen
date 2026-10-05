//! The same wgpu pipeline and render pass on every target.
use super::scene::{ATLAS_SIZE, FrameInfo, MAX_VERTICES, Scene, Vertex};
use crate::client::Client;

pub(super) struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    camera: wgpu::Buffer,
    camera_bind: wgpu::BindGroup,
    atlas: wgpu::Texture,
    depth: wgpu::TextureView,
    size: (u32, u32),
    terrain_buffer: wgpu::Buffer,
    terrain_len: u32,
    overlay_buffer: wgpu::Buffer,
    scene: Scene,
    mesh_revision: u64,
}

impl Gpu {
    pub(super) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("minimal geometry"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "render_textured.wgsl"
            ))),
        });
        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("minimal camera"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("minimal geometry"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Voxygen voxel-color atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas.create_view(&Default::default());
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Voxygen voxel-color nearest sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let camera_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("minimal camera"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });
        let depth = depth_target(&device, width, height);
        let terrain_buffer = vertex_buffer(&device, MAX_VERTICES, "minimal terrain");
        let overlay_buffer = vertex_buffer(&device, 180_000, "minimal entity proxies");
        Self {
            device,
            queue,
            pipeline,
            camera,
            camera_bind,
            atlas,
            depth,
            size: (width, height),
            terrain_buffer,
            terrain_len: 0,
            overlay_buffer,
            scene: Scene::new(),
            // Also initialize the proxy palette before a clear-only first frame.
            mesh_revision: u64::MAX,
        }
    }

    pub(super) fn draw(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        client: Option<&Client>,
        yaw: f32,
        pitch: f32,
    ) -> FrameInfo {
        if self.size != (width, height) {
            self.depth = depth_target(&self.device, width, height);
            self.size = (width, height);
        }
        let prepared = self.scene.prepare(client, yaw, pitch, width, height);
        if prepared.revision != self.mesh_revision {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(prepared.atlas),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS_SIZE * 4),
                    rows_per_image: Some(ATLAS_SIZE),
                },
                wgpu::Extent3d {
                    width: ATLAS_SIZE,
                    height: ATLAS_SIZE,
                    depth_or_array_layers: 1,
                },
            );
            self.terrain_len = prepared.terrain.len() as u32;
            self.mesh_revision = prepared.revision;
            if !prepared.terrain.is_empty() {
                self.queue.write_buffer(
                    &self.terrain_buffer,
                    0,
                    bytemuck::cast_slice(prepared.terrain),
                );
            }
        }
        self.queue
            .write_buffer(&self.camera, 0, bytemuck::cast_slice(&prepared.camera));
        self.queue.write_buffer(
            &self.overlay_buffer,
            0,
            bytemuck::cast_slice(&prepared.overlay),
        );
        let overlay_len = prepared.overlay.len() as u32;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("minimal frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("minimal geometry"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.025,
                            g: 0.035,
                            b: 0.05,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind, &[]);
            if self.terrain_len > 0 {
                pass.set_vertex_buffer(0, self.terrain_buffer.slice(..));
                pass.draw(0..self.terrain_len, 0..1);
            }
            if overlay_len > 0 {
                pass.set_vertex_buffer(0, self.overlay_buffer.slice(..));
                pass.draw(0..overlay_len, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        FrameInfo {
            terrain_revision: self.mesh_revision,
            terrain_vertices: self.terrain_len,
            overlay_vertices: overlay_len,
            position: client.and_then(Client::position),
        }
    }
}

fn vertex_buffer(device: &wgpu::Device, count: usize, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (count * size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn depth_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("minimal depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
