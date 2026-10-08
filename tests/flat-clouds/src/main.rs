//! Host GPU proof of the shipped transparent Flat pair, not native admission.
use wgpu::util::DeviceExt;
const W: u32 = 64;
fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    w: u32,
    h: u32,
    bytes: &[u8],
) -> wgpu::Texture {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    queue.write_texture(
        t.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: None,
        },
        t.size(),
    );
    t
}
fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    println!("Flat cloud host reference: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::util::make_spirv(include_bytes!(
            "../../../shaderbin/clouds-vert.flat-cloud-layer.spv"
        )),
    });
    let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::util::make_spirv(include_bytes!(
            "../../../shaderbin/cloud-flat-layer-frag.flat-cloud-layer.spv"
        )),
    });
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let tex = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let sampler = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            uniform(0),
            tex(1),
            sampler(2),
            tex(5),
            sampler(6),
            tex(12),
            sampler(13),
            uniform(14),
            uniform(15),
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
        }),
        multiview_mask: None,
        cache: None,
    });
    let terrain_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production terrain palette shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../../../src/headless/render_textured.wgsl").into(),
        ),
    });
    let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terrain over clouds"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &terrain_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 32,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
            })],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &terrain_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
        }),
        multiview_mask: None,
        cache: None,
    });
    let terrain_camera = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &[0; 80],
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let terrain_vertices: [[f32; 8]; 3] = [
        [-1., -1., 0., 0., 0.5, 0.5, 0., 0.],
        [3., -1., 0., 0., 0.5, 0.5, 0., 0.],
        [-1., 3., 0., 0., 0.5, 0.5, 0., 0.],
    ];
    let terrain_vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&terrain_vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let terrain_atlas = texture(&device, &queue, 1, 1, &[255, 0, 0, 255]);
    let terrain_atlas_view = terrain_atlas.create_view(&Default::default());
    let terrain_sampler = device.create_sampler(&Default::default());
    let terrain_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &terrain_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: terrain_camera.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&terrain_atlas_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&terrain_sampler),
            },
        ],
    });
    let mut nz = 1u32;
    let noise: Vec<u8> = (0..64 * 64)
        .flat_map(|_| {
            nz = nz.wrapping_mul(1664525).wrapping_add(1013904223);
            [((nz >> 24) & 255) as u8, 0, 0, 255]
        })
        .collect();
    let noise = texture(&device, &queue, 64, 64, &noise);
    let altitude = texture(&device, &queue, 64, 64, &vec![0; 64 * 64 * 4]);
    let noise_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let world_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let render = |coverage: u8,
                  time: f32,
                  ray_z: f32,
                  camera_z: f32,
                  altitude_top: f32,
                  gamma: f32,
                  spatial: bool,
                  terrain: bool| {
        let mut globals = [0f32; 128];
        globals[48..52].copy_from_slice(&[0., 0., camera_z, 0.]); // cam_pos
        globals[60..64].copy_from_slice(&[256., 100., 100., altitude_top]); // world altitude
        globals[64] = time;
        globals[68..72].copy_from_slice(&[0., 0., -1., 0.]); // midday sun
        globals[72..76].copy_from_slice(&[0., 0., 1., 0.]);
        globals[100] = gamma; // gamma_exposure: clouds must ignore it
        globals[112] = 0.1; // ambiance
        // A synthetic inverse camera transforms the clip plane to rays whose
        // vertical sign is controlled, without duplicating cloud calculations.
        let inverse = [
            2000.,
            0.,
            0.,
            0.,
            0.,
            2000.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            camera_z + ray_z * 1000.,
            1.,
        ];
        let rain = [0u8; 208];
        let buf = |bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };
        let globals = buf(bytemuck::cast_slice(&globals));
        let inverse = buf(bytemuck::cast_slice(&inverse));
        let rain = buf(&rain);
        let weather = if spatial {
            texture(&device, &queue, 2, 1, &[0, 0, 0, 255, coverage, 0, 0, 255])
        } else {
            texture(&device, &queue, 1, 1, &[coverage, 0, 0, 255])
        };
        let noise_view = noise.create_view(&Default::default());
        let alt_view = altitude.create_view(&Default::default());
        let weather_view = weather.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&noise_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&noise_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&alt_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&world_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: wgpu::BindingResource::TextureView(&weather_view),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: wgpu::BindingResource::Sampler(&world_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: rain.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 15,
                    resource: inverse.as_entire_binding(),
                },
            ],
        });
        let out = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: W,
                height: W,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = out.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(W * W * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
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
            pass.draw(0..3, 0..1);
        }
        if terrain {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain retains cloud color"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&terrain_pipeline);
            pass.set_bind_group(0, &terrain_bind, &[]);
            pass.set_vertex_buffer(0, terrain_vertices.slice(..));
            pass.set_scissor_rect(0, W / 2, W, W / 2);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            out.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(W * 4),
                    rows_per_image: None,
                },
            },
            out.size(),
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range().unwrap().to_vec();
        readback.unmap();
        bytes
    };
    let clear = render(0, 0., 1., 0., 1000., 1., false, false);
    assert!(
        clear.iter().all(|v| *v == 0),
        "clear weather must reveal the display backdrop"
    );
    let low = render(1, 0., 1., 0., 1000., 1., false, false);
    let high = render(4, 0., 1., 0., 1000., 1., false, false);
    let alpha = |image: &[u8]| image.chunks_exact(4).map(|p| u64::from(p[3])).sum::<u64>();
    assert!(
        alpha(&low) > 0 && alpha(&high) > alpha(&low),
        "weather must control coverage"
    );
    assert!(
        high.chunks_exact(4)
            .all(|p| p[..3].iter().all(|v| *v <= p[3])),
        "premultiplied RGBA8"
    );
    assert_eq!(
        high,
        render(4, 0., 1., 0., 1000., 4., false, false),
        "gamma must stay outside the cloud pass"
    );
    assert_ne!(
        high,
        render(4, 50., 1., 0., 1000., 1., false, false),
        "noise must animate with game time"
    );
    assert!(
        render(4, 0., -1., 0., 1000., 1., false, false)
            .iter()
            .all(|v| *v == 0),
        "plane behind camera"
    );
    assert!(
        alpha(&render(4, 0., 1., 0., 2000., 1., false, false)) < alpha(&high),
        "world bounds must move the plane"
    );
    assert!(
        render(4, 0., 1., 3000., 1000., 1., false, false)
            .iter()
            .all(|v| *v == 0),
        "camera above cloud plane looking up"
    );
    assert!(
        alpha(&render(4, 0., -1., 3000., 1000., 1., false, false)) > 0,
        "camera above plane looking down"
    );
    let spatial = render(4, 0., 1., 0., 1000., 1., true, false);
    let left: u64 = spatial
        .chunks_exact((W * 4) as usize)
        .map(|row| alpha(&row[..(W * 2) as usize]))
        .sum();
    let right: u64 = spatial
        .chunks_exact((W * 4) as usize)
        .map(|row| alpha(&row[(W * 2) as usize..]))
        .sum();
    assert!(
        left < right,
        "cloud placement must follow spatial world weather"
    );
    let composed = render(4, 0., 1., 0., 1000., 1., false, true);
    let halfway = (W * W * 2) as usize;
    assert_eq!(
        &composed[..halfway],
        &high[..halfway],
        "uncovered clouds retained"
    );
    assert!(
        composed[halfway..]
            .chunks_exact(4)
            .all(|p| p == [255, 0, 0, 255]),
        "opaque terrain must cover the cloud layer"
    );
    println!(
        "PASS transparent Flat: weather coverage, altitude, camera, time, premultiplied output, no shader gamma"
    );
}
