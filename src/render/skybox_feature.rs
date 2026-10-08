//! Fixed selection scene: shipped bring-up skybox, optional figure, and display copy.
//! Keeps the original HDR/material/depth targets without creating game pipelines.
//! Device admission and UI4 leases belong to the platform, not this draw node.

use std::num::NonZeroU64;

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const MATERIAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Uint;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const GLOBALS_CAPACITY: u64 = 512;

pub struct SkyboxFeature {
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    globals_layout: wgpu::BindGroupLayout,
    vertices: wgpu::Buffer,
    vertex_count: u32,
    skybox: wgpu::RenderPipeline,
    display: wgpu::RenderPipeline,
    display_bind: wgpu::BindGroup,
    empty_bind: wgpu::BindGroup,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    material: wgpu::TextureView,
    depth: wgpu::TextureView,
}

impl SkyboxFeature {
    /// `vertices` is the original create_skybox_mesh() Float32x3 triangle list.
    /// Width/height are the internal scene extent; output extent may be larger.
    pub fn new(
        device: &wgpu::Device,
        vertices: &[u8],
        width: u32,
        height: u32,
        output_format: wgpu::TextureFormat,
    ) -> Result<Self, &'static str> {
        if width == 0 || height == 0 || vertices.is_empty() || vertices.len() % 36 != 0 {
            return Err("skybox requires a nonzero extent and Float32x3 triangles");
        }
        let vertex_count = u32::try_from(vertices.len() / 12).map_err(|_| "skybox vertex count")?;
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skybox Globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(GLOBALS_CAPACITY),
                },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skybox original Globals"),
            size: GLOBALS_CAPACITY,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skybox Globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("original skybox mesh"),
            size: vertices.len() as u64,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        vertex_buffer
            .slice(..)
            .get_mapped_range_mut()
            .map_err(|_| "skybox vertex mapping")?
            .copy_from_slice(vertices);
        vertex_buffer.unmap();
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fixed skybox layout"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let vs = spirv(
            device,
            "original bring-up skybox VS",
            include_bytes!("../../shaderbin/skybox-vert.trueos-bringup.spv"),
        );
        let fs = spirv(
            device,
            "original bring-up skybox PS",
            include_bytes!("../../shaderbin/skybox-frag.trueos-bringup.spv"),
        );
        let skybox = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fixed bring-up skybox"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x3,
                    }],
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
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent::REPLACE,
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::Zero,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(MATERIAL_FORMAT.into()),
                ],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let color = target(device, "skybox HDR colour", width, height, COLOR_FORMAT);
        let color_view = color.create_view(&Default::default());
        let material = target(device, "skybox material", width, height, MATERIAL_FORMAT)
            .create_view(&Default::default());
        let depth = target(device, "skybox reverse depth", width, height, DEPTH_FORMAT)
            .create_view(&Default::default());
        // BareMinimum clouds and postprocess both copy RGB and make alpha one.
        // Feed the HDR colour directly to the original minimal postprocess;
        // eliminating the redundant clouds copy preserves this fixed profile.
        let empty_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[],
        });
        let empty_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &empty_layout,
            entries: &[],
        });
        let display_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("minimal display copy"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let display_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("minimal display copy"),
            layout: &display_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("minimal display copy"),
            bind_group_layouts: &[Some(&empty_layout), Some(&display_layout)],
            immediate_size: 0,
        });
        let vs = spirv(
            device,
            "original postprocess VS",
            include_bytes!("../../shaderbin/postprocess-vert.trueos-bringup.spv"),
        );
        let fs = spirv(
            device,
            "original minimal display PS",
            include_bytes!("../../shaderbin/postprocess-frag.trueos-bringup.display.spv"),
        );
        let display = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("minimal HDR-to-UI4 display copy"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: Some("main"),
                targets: &[Some(output_format.into())],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            globals,
            globals_bind,
            globals_layout,
            vertices: vertex_buffer,
            vertex_count,
            skybox,
            display,
            display_bind,
            empty_bind,
            color,
            color_view,
            material,
            depth,
        })
    }

    pub fn upload_globals(&self, queue: &wgpu::Queue, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.len() < 288 || bytes.len() > GLOBALS_CAPACITY as usize || bytes.len() % 16 != 0 {
            return Err("skybox Globals must retain its original aligned uniform layout");
        }
        queue.write_buffer(&self.globals, 0, bytes);
        Ok(())
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        self.encode_with_figure(encoder, output, None);
    }

    pub fn globals_layout(&self) -> &wgpu::BindGroupLayout {
        &self.globals_layout
    }

    /// Figure and sky share HDR/material/reverse-depth attachments; display
    /// samples their combined color directly, without the redundant clouds copy.
    pub fn encode_with_figure(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        figure: Option<&super::figure_feature::FigureFeature>,
    ) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("minimal selection HDR scene"),
                color_attachments: &[
                    Some(attachment(&self.color_view)),
                    Some(attachment(&self.material)),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(figure) = figure {
                figure.draw(&mut pass, &self.globals_bind);
            }
            pass.set_pipeline(&self.skybox);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..self.vertex_count, 0..1);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("minimal selection opaque display copy"),
            color_attachments: &[Some(attachment(output))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.display);
        pass.set_bind_group(0, &self.empty_bind, &[]);
        pass.set_bind_group(1, &self.display_bind, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn hdr_color(&self) -> &wgpu::Texture {
        &self.color
    }
}

fn spirv(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::util::make_spirv(bytes),
    })
}

fn target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
    }
}
