//! Scene bootstrap contract, independent of the admitted bring-up shader adapter.
use std::pin::Pin;
use wgpu::custom::*;
pub(super) fn instance() -> wgpu::Instance {
    tracing::info!(target: "voxy_scene_contract", "Creating TRUEOS scene instance; execution device pending");
    wgpu::Instance::from_custom(SceneInstance)
}
#[derive(Debug)]
struct SceneInstance;
impl InstanceInterface for SceneInstance {
    fn new(_desc: wgpu::InstanceDescriptor) -> Self {
        Self
    }
    #[expect(unsafe_code)]
    #[expect(unsafe_code)]
    unsafe fn create_surface(
        &self,
        target: wgpu::SurfaceTargetUnsafe,
    ) -> Result<DispatchSurface, wgpu::CreateSurfaceError> {
        match target {
            wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_window_handle: wgpu::rwh::RawWindowHandle::Trueos(handle),
                ..
            } => {
                tracing::info!(target: "voxy_scene_contract", window = handle.window.get(), "Scene surface bound to UI4 window");
                Ok(DispatchSurface::custom(SceneSurface {
                    window: handle.window.get(),
                }))
            }
            _ => Err(wgpu::CreateSurfaceError::from_message(
                "TRUEOS scene surface requires a TRUEOS window handle".into(),
            )),
        }
    }
    fn request_adapter(
        &self,
        options: &wgpu::RequestAdapterOptions<'_, '_>,
    ) -> Pin<Box<dyn RequestAdapterFuture>> {
        let window = options
            .compatible_surface
            .and_then(|surface| surface.as_custom::<SceneSurface>())
            .map(|surface| surface.window);
        tracing::info!(target: "voxy_scene_contract", ?window, fallback = options.force_fallback_adapter, preference = ?options.power_preference, "Scene adapter requested");
        if options.force_fallback_adapter
            || options.compatible_surface.is_some() && window.is_none()
        {
            let empty = wgpu::Backends::empty();
            return Box::pin(std::future::ready(Err(
                wgpu::RequestAdapterError::NotFound {
                    active_backends: empty,
                    requested_backends: empty,
                    supported_backends: empty,
                    no_fallback_backends: empty,
                    no_adapter_backends: empty,
                    incompatible_surface_backends: empty,
                },
            )));
        }
        Box::pin(std::future::ready(Ok(DispatchAdapter::custom(
            SceneAdapter,
        ))))
    }
    fn poll_all_devices(&self, _force_wait: bool) -> bool {
        true
    }
    fn wgsl_language_features(&self) -> wgpu::WgslLanguageFeatures {
        wgpu::WgslLanguageFeatures::empty()
    }
    fn enumerate_adapters(
        &self,
        _backends: wgpu::Backends,
    ) -> Pin<Box<dyn EnumerateAdapterFuture>> {
        Box::pin(std::future::ready(vec![DispatchAdapter::custom(
            SceneAdapter,
        )]))
    }
}
#[derive(Debug)]
struct SceneAdapter;
impl AdapterInterface for SceneAdapter {
    fn request_device(
        &self,
        desc: &wgpu::DeviceDescriptor<'_>,
    ) -> Pin<Box<dyn RequestDeviceFuture>> {
        let message = format!(
            "TRUEOS scene device negotiation: execution not implemented; requested_features={:?}; requested_limits={:?}; trace={:?}. No device or shader execution was admitted.",
            desc.required_features, desc.required_limits, desc.trace
        );
        tracing::error!(target: "voxy_scene_contract", "{message}");
        Box::pin(std::future::ready(Err(
            wgpu::RequestDeviceError::from_message(message),
        )))
    }
    fn is_surface_supported(&self, surface: &DispatchSurface) -> bool {
        surface.as_custom::<SceneSurface>().is_some()
    }
    fn features(&self) -> wgpu::Features {
        wgpu::Features::empty()
    }
    fn limits(&self) -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_texture_dimension_2d = 0;
        limits.max_buffer_size = 0;
        limits
    }
    fn downlevel_capabilities(&self) -> wgpu::DownlevelCapabilities {
        wgpu::DownlevelCapabilities {
            flags: wgpu::DownlevelFlags::empty(),
            ..Default::default()
        }
    }
    fn get_info(&self) -> wgpu::AdapterInfo {
        let mut info = wgpu::AdapterInfo::new(wgpu::DeviceType::Other, wgpu::Backend::Noop);
        info.name = "TRUEOS scene contract bootstrap (no execution device)".into();
        info.driver = "TRUEOS UI4 scene negotiation".into();
        info
    }
    fn get_texture_format_features(
        &self,
        _format: wgpu::TextureFormat,
    ) -> wgpu::TextureFormatFeatures {
        wgpu::TextureFormatFeatures {
            allowed_usages: wgpu::TextureUsages::empty(),
            flags: wgpu::TextureFormatFeatureFlags::empty(),
        }
    }
    fn get_presentation_timestamp(&self) -> wgpu::PresentationTimestamp {
        wgpu::PresentationTimestamp::INVALID_TIMESTAMP
    }
    fn cooperative_matrix_properties(&self) -> Vec<wgpu::CooperativeMatrixProperties> {
        Vec::new()
    }
}
#[derive(Debug)]
struct SceneSurface {
    window: u32,
}
impl SurfaceInterface for SceneSurface {
    fn get_capabilities(&self, _adapter: &DispatchAdapter) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities::default()
    }
    fn configure(&self, _device: &DispatchDevice, _config: &wgpu::SurfaceConfiguration) {
        panic!("TRUEOS scene surface cannot be configured without an execution device");
    }
    fn get_current_texture(
        &self,
        _desc: Option<wgpu::TextureDescriptor<'static>>,
    ) -> (
        Option<DispatchTexture>,
        wgpu::SurfaceStatus,
        DispatchSurfaceOutputDetail,
    ) {
        (
            None,
            wgpu::SurfaceStatus::Lost,
            DispatchSurfaceOutputDetail::custom(UnacquiredFrame),
        )
    }
}
#[derive(Debug)]
struct UnacquiredFrame;
impl SurfaceOutputDetailInterface for UnacquiredFrame {
    fn texture_discard(&self) {}
    fn texture_release(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
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
            Poll::Ready(result) => result,
            Poll::Pending => panic!("bootstrap negotiation must not block"),
        }
    }
    #[test]
    fn binds_window_selects_adapter_and_reports_device_contract_without_execution() {
        let instance = wgpu::Instance::from_custom(SceneInstance);
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: None,
            raw_window_handle: wgpu::rwh::TrueosWindowHandle::new(NonZeroU32::new(7).unwrap())
                .into(),
        };
        // The bootstrap only stores the handle; it does not access a window or GPU.
        #[expect(unsafe_code)]
        let surface = unsafe { instance.create_surface_unsafe(target) }.unwrap();
        assert_eq!(surface.as_custom::<SceneSurface>().unwrap().window, 7);
        let adapter = ready(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .unwrap();
        let adapter = adapter.as_custom::<SceneAdapter>().unwrap();
        assert!(adapter.is_surface_supported(&DispatchSurface::custom(SceneSurface { window: 7 })));
        assert!(adapter.features().is_empty());
        let request = wgpu::DeviceDescriptor {
            required_features: wgpu::Features::IMMEDIATES,
            required_limits: wgpu::Limits {
                max_immediate_size: 64,
                ..Default::default()
            },
            ..Default::default()
        };
        let error = ready(adapter.request_device(&request))
            .unwrap_err()
            .to_string();
        assert!(error.contains("IMMEDIATES"));
        assert!(error.contains("max_immediate_size: 64"));
        assert!(error.contains("execution not implemented"));
        let surface = surface.as_custom::<SceneSurface>().unwrap();
        let (texture, status, _) = surface.get_current_texture(None);
        assert!(texture.is_none());
        assert!(matches!(status, wgpu::SurfaceStatus::Lost));
    }
    #[test]
    fn rejects_foreign_window_handles_with_a_surface_error() {
        let instance = wgpu::Instance::from_custom(SceneInstance);
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: None,
            raw_window_handle: wgpu::rwh::XlibWindowHandle::new(1).into(),
        };
        #[expect(unsafe_code)]
        let error = unsafe { instance.create_surface_unsafe(target) }.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires a TRUEOS window handle")
        );
    }

    #[test]
    fn rejects_fallback_instead_of_returning_a_fake_software_device() {
        let options = wgpu::RequestAdapterOptions {
            force_fallback_adapter: true,
            ..Default::default()
        };
        assert!(ready(SceneInstance.request_adapter(&options)).is_err());
    }
}
