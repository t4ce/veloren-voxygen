//! Fixed BareMinimum figure draw, using the shipped original SPIR-V pair.
//! Takes the game's packed quad mesh, atlas and uniform bytes without CPU
//! skinning or lighting approximations. Native execution admission is separate.
use super::skybox_feature::{COLOR_FORMAT, DEPTH_FORMAT, MATERIAL_FORMAT};

pub const LOCALS_BYTES: usize = 144;
pub const BONE_BYTES: usize = 128;
pub const MAX_BONES: usize = 16;

pub struct FigureFeature {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    empty_bind: wgpu::BindGroup,
    atlas_bind: wgpu::BindGroup,
    locals_bind: wgpu::BindGroup,
    locals: wgpu::Buffer,
    bones: wgpu::Buffer,
}
impl FigureFeature {
    /// Packed Uint32x2 vertices are the original TerrainVertex::new_figure
    /// representation, in groups of four. Atlas texels retain their encoded
    /// colour/light attributes; this does not accept ordinary image colours.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        globals_layout: &wgpu::BindGroupLayout,
        vertices: &[[u32; 2]],
        atlas_size: [u32; 2],
        atlas_pixels: &[[u8; 4]],
    ) -> Result<Self, &'static str> {
        Self::with_color_format(
            device,
            queue,
            globals_layout,
            vertices,
            atlas_size,
            atlas_pixels,
            COLOR_FORMAT,
        )
    }

    pub fn new_scanout(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        globals_layout: &wgpu::BindGroupLayout,
        vertices: &[[u32; 2]],
        atlas_size: [u32; 2],
        atlas_pixels: &[[u8; 4]],
    ) -> Result<Self, &'static str> {
        Self::with_color_format(
            device,
            queue,
            globals_layout,
            vertices,
            atlas_size,
            atlas_pixels,
            wgpu::TextureFormat::Rgba8Unorm,
        )
    }

    fn with_color_format(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        globals_layout: &wgpu::BindGroupLayout,
        vertices: &[[u32; 2]],
        atlas_size: [u32; 2],
        atlas_pixels: &[[u8; 4]],
        color_format: wgpu::TextureFormat,
    ) -> Result<Self, &'static str> {
        let [width, height] = atlas_size;
        if vertices.is_empty() || vertices.len() % 4 != 0 {
            return Err("figure requires packed four-vertex quads");
        }
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) != atlas_pixels.len() as u64
        {
            return Err("figure requires a complete nonzero encoded atlas");
        }
        let vertex_count = u32::try_from(vertices.len()).map_err(|_| "figure vertex count")?;
        let index_count = (vertex_count / 4)
            .checked_mul(6)
            .ok_or("figure index count")?;
        let indices: Vec<u32> = (0..vertex_count)
            .step_by(4)
            .flat_map(|i| [i, i + 1, i + 2, i + 2, i + 1, i + 3])
            .collect();
        let vertices = initialized_buffer(
            device,
            "original packed figure quads",
            bytemuck::cast_slice(vertices),
            wgpu::BufferUsages::VERTEX,
        )?;
        let indices = initialized_buffer(
            device,
            "figure quad indices",
            bytemuck::cast_slice(&indices),
            wgpu::BufferUsages::INDEX,
        )?;
        let empty_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("figure unused shadow group"),
            entries: &[],
        });
        let empty_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &empty_layout,
            entries: &[],
        });
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("figure encoded colour/light atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("original figure atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            atlas.as_image_copy(),
            bytemuck::cast_slice(atlas_pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width.checked_mul(4).ok_or("figure atlas pitch")?),
                rows_per_image: Some(height),
            },
            atlas.size(),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("figure atlas"),
            layout: &atlas_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &atlas.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let locals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("original figure locals and bones"),
            entries: &[
                uniform_entry(0, LOCALS_BYTES),
                uniform_entry(1, BONE_BYTES * MAX_BONES),
            ],
        });
        let locals = uniform(device, "original figure locals", LOCALS_BYTES);
        let bones = uniform(device, "original figure bones", BONE_BYTES * MAX_BONES);
        let locals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("figure locals and bones"),
            layout: &locals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: locals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: bones.as_entire_binding(),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fixed bring-up figure layout"),
            bind_group_layouts: &[
                Some(globals_layout),
                Some(&empty_layout),
                Some(&atlas_layout),
                Some(&locals_layout),
            ],
            immediate_size: 0,
        });
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("original bring-up figure VS"),
            source: wgpu::util::make_spirv(include_bytes!(
                "../../shaderbin/figure-vert.trueos-bringup.spv"
            )),
        });
        let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("original bring-up figure PS"),
            source: wgpu::util::make_spirv(include_bytes!(
                "../../shaderbin/figure-frag.trueos-bringup.spv"
            )),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fixed bring-up figure"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32],
                })],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: Some("main"),
                targets: &[Some(color_format.into()), Some(MATERIAL_FORMAT.into())],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            pipeline,
            vertices,
            indices,
            index_count,
            empty_bind,
            atlas_bind,
            locals_bind,
            locals,
            bones,
        })
    }
    /// Upload full original layouts. Every vertex bone index is 0..15, so a
    /// complete palette is required even when the fixture uses only one bone.
    pub fn upload_pose(
        &self,
        queue: &wgpu::Queue,
        locals: &[u8],
        bones: &[u8],
    ) -> Result<(), &'static str> {
        if locals.len() != LOCALS_BYTES || bones.len() != MAX_BONES * BONE_BYTES {
            return Err("figure requires original 144-byte locals and 16 complete bone records");
        }
        queue.write_buffer(&self.locals, 0, locals);
        queue.write_buffer(&self.bones, 0, bones);
        Ok(())
    }
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, globals: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, &self.empty_bind, &[]);
        pass.set_bind_group(2, &self.atlas_bind, &[]);
        pass.set_bind_group(3, &self.locals_bind, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}
fn uniform_entry(binding: u32, bytes: usize) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: std::num::NonZeroU64::new(bytes as u64),
        },
        count: None,
    }
}
fn uniform(device: &wgpu::Device, label: &str, bytes: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
fn initialized_buffer(
    device: &wgpu::Device,
    label: &str,
    bytes: &[u8],
    usage: wgpu::BufferUsages,
) -> Result<wgpu::Buffer, &'static str> {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.len() as u64,
        usage,
        mapped_at_creation: true,
    });
    buffer
        .slice(..)
        .get_mapped_range_mut()
        .map_err(|_| "figure buffer mapping")?
        .copy_from_slice(bytes);
    buffer.unmap();
    Ok(buffer)
}
