//! Reference experiment: the unchanged bring-up shaders, one RGBA8 target.
//! No HDR texture, material texture, depth texture, or postprocess pass.
pub fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vertices: &[u8],
    globals: &[u8],
) -> Vec<u8> {
    use wgpu::util::DeviceExt;
    let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("direct sky original VS"),
        source: wgpu::util::make_spirv(include_bytes!(
            "../../../shaderbin/skybox-vert.trueos-bringup.spv"
        )),
    });
    let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("direct sky original PS"),
        source: wgpu::util::make_spirv(include_bytes!(
            "../../../shaderbin/skybox-frag.trueos-bringup.spv"
        )),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("direct opaque RGBA8 sky experiment"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            })],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            // Replace alpha as well as RGB: the shipped fragment shader writes one.
            targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
        }),
        multiview_mask: None,
        cache: None,
    });
    let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("direct sky Globals"),
        contents: globals,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: globals.as_entire_binding(),
        }],
    });
    let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("direct sky original mesh"),
        contents: vertices,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let output = super::texture(device, 64, 64, wgpu::TextureFormat::Rgba8Unorm);
    let view = output.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("direct sky"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_vertex_buffer(0, vertex.slice(..));
        pass.draw(0..(vertices.len() / 12) as u32, 0..1);
    }
    queue.submit([encoder.finish()]);
    super::readback(device, queue, &output, 4)
}
