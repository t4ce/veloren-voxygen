//! Executable conformance proof, using the production mesh and fixed draw node.
//! Select lavapipe explicitly for the host reference; this is not TRUEOS proof.
#![allow(dead_code)]
extern crate alloc;
extern crate self as common_base;
#[macro_export]
macro_rules! span {
    ($($tokens:tt)*) => {};
}

mod direct_rgba8;
#[path = "../../../src/render/minimal_sky.rs"]
mod minimal_sky;
#[path = "../../../src/render/skybox_feature.rs"]
mod skybox_feature;

// Only the surrounding game types are shimmed; mesh generation and pipeline
// source are the original production files, including their winding order.
mod render {
    pub trait Vertex: Copy + Clone {
        const STRIDE: wgpu::BufferAddress;
        const QUADS_INDEX: Option<wgpu::IndexFormat>;
    }
    pub struct GlobalsLayouts {
        pub globals: wgpu::BindGroupLayout,
        pub shadow_textures: wgpu::BindGroupLayout,
    }
    pub struct AaMode;
    impl AaMode {
        pub fn samples(&self) -> u32 {
            1
        }
    }
    mod mesh {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/render/mesh.rs"
        ));
    }
    pub use mesh::{Mesh, Quad};
    pub mod pipelines {
        pub mod skybox {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../src/render/pipelines/skybox.rs"
            ));
        }
    }
}

fn texture(device: &wgpu::Device, w: u32, h: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("proof output"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    bpp: u32,
) -> Vec<u8> {
    let size = texture.size();
    let pitch = (size.width * bpp).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("proof readback"),
        size: u64::from(pitch * size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap();
    let packed = bytes
        .chunks(pitch as usize)
        .flat_map(|row| row[..(size.width * bpp) as usize].iter().copied())
        .collect();
    drop(bytes);
    buffer.unmap();
    packed
}

fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("Vulkan reference adapter");
    println!("Skybox reference adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("fixed skybox proof: no optional device features"),
        ..Default::default()
    }))
    .unwrap();
    let mesh = render::pipelines::skybox::create_mesh();
    assert_eq!(mesh.vertices().len(), 36);
    let feature = skybox_feature::SkyboxFeature::new(
        &device,
        bytemuck::cast_slice(mesh.vertices()),
        32,
        32,
        wgpu::TextureFormat::Rgba8Unorm,
    )
    .unwrap();
    let output = texture(&device, 64, 64, wgpu::TextureFormat::Rgba8Unorm);
    for (name, sun_z, expected) in [
        ("day", -1.0, [0.14, 0.39, 0.75]),
        ("day-transition", -0.5, [0.96, 0.295, 0.45]),
        ("dusk", 0.0, [1.78, 0.2, 0.15]),
        ("night-transition", 0.5, [0.8905, 0.1015, 0.080625]),
        ("night", 1.0, [0.001, 0.003, 0.01125]),
    ] {
        let mut globals = [0.0f32; 128];
        // Original column-major all_mat at byte 128. A view along +Z puts
        // the real cube around the eye; the shader forces reverse depth to zero.
        for i in 0..4 {
            globals[32 + i * 5] = 1.0;
        }
        globals[70] = sun_z; // original sun_dir.z: byte 280
        feature
            .upload_globals(&queue, bytemuck::cast_slice(&globals))
            .unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        feature.encode(&mut encoder, &output.create_view(&Default::default()));
        queue.submit([encoder.finish()]);
        let hdr = readback(&device, &queue, feature.hdr_color(), 8);
        let display = readback(&device, &queue, &output, 4);
        let direct = direct_rgba8::render(
            &device,
            &queue,
            bytemuck::cast_slice(mesh.vertices()),
            bytemuck::cast_slice(&globals),
        );
        let max_difference = display
            .iter()
            .zip(&direct)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            max_difference <= 1,
            "{name}: direct RGBA8 differs by {max_difference} bytes"
        );
        assert!(direct.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let clear = minimal_sky::rgba8(sun_z).to_le_bytes();
        assert!(
            direct
                .chunks_exact(4)
                .all(|pixel| pixel.iter().zip(clear).all(|(a, b)| a.abs_diff(b) <= 1)),
            "native minimal sky colour differs from baked shader"
        );
        println!(
            "PASS {name}: direct RGBA8 original shaders, no material/depth/postprocess; max difference {max_difference}/255"
        );
        for pixel in hdr.chunks_exact(8) {
            for c in 0..3 {
                let actual =
                    half::f16::from_bits(u16::from_le_bytes([pixel[c * 2], pixel[c * 2 + 1]]))
                        .to_f32();
                assert!(
                    (actual - expected[c]).abs() < 0.002,
                    "{name} HDR channel {c}: {actual} != {}",
                    expected[c]
                );
            }
            // Original skybox blend preserves the cleared scene alpha.
            assert_eq!(&pixel[6..8], &[0, 0]);
        }
        for pixel in display.chunks_exact(4) {
            for c in 0..3 {
                let target = (expected[c].clamp(0.0, 1.0) * 255.0).round() as i16;
                assert!(
                    (i16::from(pixel[c]) - target).abs() <= 1,
                    "{name} display channel {c}: {} != {target}",
                    pixel[c]
                );
            }
            assert_eq!(
                pixel[3], 255,
                "display must be opaque before UI4 publication"
            );
        }
        println!(
            "PASS {name}: original cube + baked shaders; HDR preserved; 32x32 scene -> 64x64 opaque display"
        );
    }
}
