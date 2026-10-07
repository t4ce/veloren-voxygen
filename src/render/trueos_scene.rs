//! Full-client command bridge. The default scene is UI4's opaque black clear.
//! Native terrain geometry is opt-in; original graphics passes remain out-gated.
use super::trueos_host::{self, Host, HostTexture};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};
use wgpu::custom::*;

pub(super) fn instance() -> wgpu::Instance {
    tracing::info!(target: "voxy_scene_contract", native_lines = cfg!(feature = "trueos-native-lines"), "TRUEOS scene: UI4 black background by default; original graphics shaders out-gated");
    wgpu::Instance::from_custom(SceneInstance)
}

#[derive(Debug)]
struct SceneInstance;
impl InstanceInterface for SceneInstance {
    fn new(_: wgpu::InstanceDescriptor) -> Self {
        Self
    }
    #[expect(unsafe_code)]
    unsafe fn create_surface(
        &self,
        target: wgpu::SurfaceTargetUnsafe,
    ) -> Result<DispatchSurface, wgpu::CreateSurfaceError> {
        match target {
            wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_window_handle: wgpu::rwh::RawWindowHandle::Trueos(handle),
                ..
            } => Ok(DispatchSurface::custom(SceneSurface {
                state: Arc::new(Mutex::new(SurfaceState {
                    window: handle.window.get(),
                    width: 0,
                    height: 0,
                    acquired: false,
                    first_terrain: false,
                    first_surflive: false,
                    attempted_frames: 0,
                    last_source_log: None,
                    #[cfg(all(target_os = "trueos", feature = "trueos-native-lines"))]
                    presenter: None,
                })),
            })),
            _ => Err(wgpu::CreateSurfaceError::from_message(
                "TRUEOS terrain bridge requires a TRUEOS window".into(),
            )),
        }
    }
    fn request_adapter(
        &self,
        options: &wgpu::RequestAdapterOptions<'_, '_>,
    ) -> Pin<Box<dyn RequestAdapterFuture>> {
        if options.force_fallback_adapter
            || options
                .compatible_surface
                .is_some_and(|s| s.as_custom::<SceneSurface>().is_none())
        {
            return Box::pin(std::future::ready(Err(
                wgpu::RequestAdapterError::NotFound {
                    active_backends: wgpu::Backends::empty(),
                    requested_backends: wgpu::Backends::empty(),
                    supported_backends: wgpu::Backends::empty(),
                    no_fallback_backends: wgpu::Backends::empty(),
                    no_adapter_backends: wgpu::Backends::empty(),
                    incompatible_surface_backends: wgpu::Backends::empty(),
                },
            )));
        }
        Box::pin(std::future::ready(Ok(DispatchAdapter::custom(
            SceneAdapter,
        ))))
    }
    fn poll_all_devices(&self, _: bool) -> bool {
        true
    }
    fn wgsl_language_features(&self) -> wgpu::WgslLanguageFeatures {
        wgpu::WgslLanguageFeatures::empty()
    }
    fn enumerate_adapters(&self, _: wgpu::Backends) -> Pin<Box<dyn EnumerateAdapterFuture>> {
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
        if !trueos_host::features().contains(desc.required_features)
            || !desc.required_limits.check_limits(&trueos_host::limits())
        {
            return Box::pin(std::future::ready(Err(wgpu::RequestDeviceError::from_message(
                "TRUEOS terrain extraction bridge does not support the requested device contract".into()
            ))));
        }
        let (device, queue, _) = trueos_host::device();
        Box::pin(std::future::ready(Ok((device, queue))))
    }
    fn is_surface_supported(&self, s: &DispatchSurface) -> bool {
        s.as_custom::<SceneSurface>().is_some()
    }
    fn features(&self) -> wgpu::Features {
        trueos_host::features()
    }
    fn limits(&self) -> wgpu::Limits {
        trueos_host::limits()
    }
    fn downlevel_capabilities(&self) -> wgpu::DownlevelCapabilities {
        wgpu::DownlevelCapabilities::default()
    }
    fn get_info(&self) -> wgpu::AdapterInfo {
        trueos_host::info()
    }
    fn get_texture_format_features(
        &self,
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureFormatFeatures {
        // The only intermediate host descriptor admitted for the excluded
        // texture graph. This does not advertise native texture execution.
        if format == wgpu::TextureFormat::Rgba8Unorm {
            wgpu::TextureFormatFeatures {
                allowed_usages: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                flags: wgpu::TextureFormatFeatureFlags::FILTERABLE,
            }
        } else {
            wgpu::TextureFormatFeatures {
                allowed_usages: wgpu::TextureUsages::empty(),
                flags: wgpu::TextureFormatFeatureFlags::empty(),
            }
        }
    }
    fn get_presentation_timestamp(&self) -> wgpu::PresentationTimestamp {
        wgpu::PresentationTimestamp::INVALID_TIMESTAMP
    }
    fn cooperative_matrix_properties(&self) -> Vec<wgpu::CooperativeMatrixProperties> {
        Vec::new()
    }
}

struct SurfaceState {
    window: u32,
    width: u32,
    height: u32,
    acquired: bool,
    first_terrain: bool,
    first_surflive: bool,
    attempted_frames: u64,
    last_source_log: Option<std::time::Instant>,
    #[cfg(all(target_os = "trueos", feature = "trueos-native-lines"))]
    presenter: Option<super::trueos_lines::LinePresenter>,
}
impl std::fmt::Debug for SurfaceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerrainLineSurface")
            .field("window", &self.window)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}
#[derive(Debug)]
struct SceneSurface {
    state: Arc<Mutex<SurfaceState>>,
}
impl SurfaceInterface for SceneSurface {
    fn get_capabilities(&self, _: &DispatchAdapter) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: vec![wgpu::TextureFormat::Rgba8Unorm],
            present_modes: vec![wgpu::PresentMode::Fifo],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
            ..Default::default()
        }
    }
    fn configure(&self, _: &DispatchDevice, config: &wgpu::SurfaceConfiguration) {
        assert!(config.width > 0 && config.height > 0);
        assert_eq!(config.format, wgpu::TextureFormat::Rgba8Unorm);
        let mut state = self.state.lock().unwrap();
        assert!(!state.acquired, "Cannot configure acquired terrain frame");
        state.width = config.width;
        state.height = config.height;
        #[cfg(all(target_os = "trueos", feature = "trueos-native-lines"))]
        {
            if let Some(presenter) = &mut state.presenter {
                presenter
                    .resize(config.width, config.height)
                    .expect("Terrain line surface resize failed");
            } else {
                state.presenter = Some(
                    super::trueos_lines::LinePresenter::new(
                        state.window,
                        config.width,
                        config.height,
                    )
                    .expect("Native terrain line GPU admission failed"),
                );
            }
        }
    }
    fn get_current_texture(
        &self,
        _: Option<wgpu::TextureDescriptor<'static>>,
    ) -> (
        Option<DispatchTexture>,
        wgpu::SurfaceStatus,
        DispatchSurfaceOutputDetail,
    ) {
        let mut state = self.state.lock().unwrap();
        if state.width == 0 || state.acquired {
            return (
                None,
                wgpu::SurfaceStatus::Lost,
                DispatchSurfaceOutputDetail::custom(SceneOutput {
                    state: self.state.clone(),
                }),
            );
        }
        state.acquired = true;
        (
            Some(DispatchTexture::custom(HostTexture::frame(
                state.width,
                state.height,
            ))),
            wgpu::SurfaceStatus::Good,
            DispatchSurfaceOutputDetail::custom(SceneOutput {
                state: self.state.clone(),
            }),
        )
    }
}
#[derive(Debug)]
struct SceneOutput {
    state: Arc<Mutex<SurfaceState>>,
}
impl SurfaceOutputDetailInterface for SceneOutput {
    fn texture_discard(&self) {
        self.state.lock().unwrap().acquired = false;
    }
    fn texture_release(&self) {
        self.state.lock().unwrap().acquired = false;
    }
}
pub(super) fn present(detail: &DispatchSurfaceOutputDetail, host: &Arc<Host>) {
    let output = detail
        .as_custom::<SceneOutput>()
        .expect("Terrain bridge owns surface output");
    let mut state = output.state.lock().unwrap();
    assert!(state.acquired, "Terrain frame was not acquired");
    let frame = host.take_frame();
    let lines = frame.lines;
    state.attempted_frames += 1;
    #[cfg(all(target_os = "trueos", feature = "trueos-native-lines"))]
    {
        if state
            .last_source_log
            .is_none_or(|last| last.elapsed() >= std::time::Duration::from_secs(2))
        {
            state.last_source_log = Some(std::time::Instant::now());
            let _ = trueos::logl::log_record(
                trueos::logl::level::IMPORTANT,
                "apps::voxygen",
                format_args!(
                    "Voxygen terrain source: frame={} draws={} candidates={} near={} segments={} vertices={} truncated_quads={}",
                    state.attempted_frames,
                    frame.stats.terrain_draws,
                    frame.stats.candidate_quads,
                    frame.stats.near_quads,
                    frame.stats.emitted_segments,
                    lines.len(),
                    frame.stats.truncated_quads
                ),
            );
        }
        let report = state
            .presenter
            .as_mut()
            .expect("Terrain line surface configured")
            .draw(&lines)
            .unwrap_or_else(|error| panic!("Native terrain line execution failed: {error}"));
        if let Some(publication) = report.published {
            if !state.first_terrain && publication.frame.vertices > 0 {
                state.first_terrain = true;
                let _ = trueos::logl::log_record(
                    trueos::logl::level::IMPORTANT,
                    "apps::voxygen",
                    format_args!(
                        "Voxygen terrain lines: first native frame retired and published; window={} sequence={} serial={:?} vertices={} extent={}x{} boundary=GPU-retired+UI4-published",
                        state.window,
                        publication.frame.sequence,
                        publication.serial,
                        publication.frame.vertices,
                        publication.frame.extent[0],
                        publication.frame.extent[1]
                    ),
                );
            }
        }
        if let Some(publication) = report.surflive {
            if !state.first_surflive && publication.frame.vertices > 0 {
                state.first_surflive = true;
                let _ = trueos::logl::log_record(
                    trueos::logl::level::IMPORTANT,
                    "apps::voxygen",
                    format_args!(
                        "Voxygen terrain SURFLIVE proven: window={} sequence={} serial={} vertices={} extent={}x{} boundary=exact-background-publication+physical-SURFLIVE",
                        state.window,
                        publication.frame.sequence,
                        publication.serial.unwrap(),
                        publication.frame.vertices,
                        publication.frame.extent[0],
                        publication.frame.extent[1]
                    ),
                );
            }
        }
    }
    host.recycle_lines(lines);
    state.acquired = false;
}

static STARTUP_STAGE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
pub(crate) fn startup_label() -> &'static str {
    match STARTUP_STAGE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => "Starting graphics worker",
        1 => "Negotiating terrain bridge",
        2 => "Checking scene formats",
        3 if cfg!(feature = "trueos-native-lines") => "Admitting native line GPU",
        3 => "Using UI4 black scene; native geometry disabled",
        4 => "Loading scene descriptors",
        5 => "Preparing terrain pipeline handles",
        6 => "Finishing scene resources",
        7 => "Finishing graphics state",
        8 => "Scene renderer ready",
        _ => "Finishing scene resources",
    }
}
pub(super) fn mark_stage(stage: u8) {
    STARTUP_STAGE.store(stage, std::sync::atomic::Ordering::Relaxed);
    #[cfg(target_os = "trueos")]
    let _ = trueos::logl::log_record(
        trueos::logl::level::IMPORTANT,
        "apps::voxygen",
        format_args!("Voxygen scene startup: {}", startup_label()),
    );
}
pub(super) fn prepare_surface(
    surface: &wgpu::Surface<'_>,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let surface = surface
        .as_custom::<SceneSurface>()
        .ok_or("Foreign terrain surface")?;
    let mut state = surface.state.lock().unwrap();
    #[cfg(all(target_os = "trueos", feature = "trueos-native-lines"))]
    if state.presenter.is_none() {
        state.presenter = Some(super::trueos_lines::LinePresenter::new(
            state.window,
            width,
            height,
        )?);
    }
    Ok(())
}
