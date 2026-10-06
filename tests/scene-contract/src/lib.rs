#[path = "../../../src/render/trueos_presentation.rs"]
mod trueos_presentation;
#[path = "../../../src/ui/ice/renderer/handoff.rs"]
mod handoff;
#[path = "../../../src/render/trueos_host.rs"]
mod trueos_host;
#[path = "../../../src/render/trueos_scene.rs"]
mod trueos_scene;

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        num::NonZeroU32,
        task::{Context, Poll, Waker},
    };
    fn ready<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("host negotiation must be immediate"),
        }
    }
    #[test]
    fn negotiates_host_extraction_without_timer_or_arbitrary_gpu_claims() {
        let instance = super::trueos_scene::instance();
        let adapter =
            ready(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
        assert_eq!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        let metadata = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm);
        assert!(metadata.allowed_usages.contains(wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING));
        assert!(metadata.flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE));
        assert!(adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float).allowed_usages.is_empty());

        assert!(
            !adapter
                .features()
                .intersects(wgpu::Features::TIMESTAMP_QUERY)
        );
        assert!(
            ready(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::TIMESTAMP_QUERY,
                ..Default::default()
            }))
            .is_err()
        );
        let (device, queue) =
            ready(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        use wgpu::util::DeviceExt;
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("retained terrain contract"),
            contents: &[1u8; 16],
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        });
        queue.write_buffer(&buffer, 4, &[2; 4]);
        assert_eq!(buffer.size(), 16);
    }
    #[test]
    fn owns_a_single_surface_acquisition_and_releases_discarded_frames() {
        let instance = super::trueos_scene::instance();
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: None,
            raw_window_handle: wgpu::rwh::TrueosWindowHandle::new(NonZeroU32::new(7).unwrap())
                .into(),
        };
        let surface = unsafe { instance.create_surface_unsafe(target) }.unwrap();
        let adapter = ready(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .unwrap();
        let (device, _) =
            ready(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                width: 640,
                height: 360,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
                color_space: wgpu::SurfaceColorSpace::Auto,
            },
        );
        let wgpu::CurrentSurfaceTexture::Success(frame) = surface.get_current_texture() else {
            panic!("first terrain frame unavailable")
        };
        assert_eq!(frame.texture.format(), wgpu::TextureFormat::Rgba8Unorm);
        assert!(matches!(
            surface.get_current_texture(),
            wgpu::CurrentSurfaceTexture::Lost
        ));
        drop(frame);
        assert!(matches!(
            surface.get_current_texture(),
            wgpu::CurrentSurfaceTexture::Success(_)
        ));
    }
}
