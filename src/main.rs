#![deny(unsafe_code)]
#![recursion_limit = "2048"]

extern crate alloc;
#[cfg(target_os = "windows")]
#[global_allocator]
#[cfg(not(feature = "headless"))]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Allow profiling allocations with Tracy

#[cfg(not(feature = "headless"))]
use i18n::{self, LocalizationHandle};
#[cfg(not(feature = "headless"))]
use veloren_voxygen::{
    GlobalState,
    audio::AudioFrontend,
    cli, panic_handler,
    profile::Profile,
    run,
    scene::terrain::SpriteRenderContext,
    settings::{AudioOutput, Settings, get_fps},
    window::Window,
};

#[cfg(not(feature = "headless"))]
use chrono::Utc;
#[cfg(not(feature = "headless"))]
use common::clock::Clock;
#[cfg(not(feature = "headless"))]
use std::panic;
#[cfg(not(feature = "headless"))]
use std::path::PathBuf;
#[cfg(not(feature = "headless"))]
use tracing::{info, warn};

#[cfg(not(feature = "headless"))]
use wgpu::{Backends, Instance};

#[cfg(not(feature = "headless"))]
fn main() {
    // Declared first and dropped last: host teardown must wait for owned
    // presenter workers and the Tokio runtime to finish normal destruction.
    #[cfg(target_os = "trueos")]
    let _vm_shutdown = trueos::shutdown::ShutdownGuard::register()
        .expect("Failed to register cooperative Voxy shutdown");

    // Process CLI arguments
    use clap::Parser;
    let args = cli::Args::parse();

    if let Some(command) = args.command {
        match command {
            cli::Commands::ListWgpuBackends => {
                #[cfg(target_os = "windows")]
                let backends = &["opengl", "dx12", "vulkan"];
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                let backends = &["opengl", "vulkan"];
                #[cfg(target_os = "macos")]
                let backends = &["metal"];

                for backend in backends {
                    println!("{backend}");
                }
                return;
            }
            cli::Commands::ListWgpuDevices => {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                let instance = Instance::new(
                    wgpu::InstanceDescriptor::new_without_display_handle().with_env(),
                );
                let adapters = runtime.block_on(instance.enumerate_adapters(Backends::default()));
                for adapter in adapters {
                    println!("{}", adapter.get_info().name);
                }
                return;
            }
        }
    }

    let userdata_dir = common_base::userdata_dir();

    // Determine where Voxygen's logs should go
    // Choose a path to store the logs by the following order:
    //  - The VOXYGEN_LOGS environment variable
    //  - The <userdata>/voxygen/logs
    let logs_dir = std::env::var_os("VOXYGEN_LOGS")
        .map(PathBuf::from)
        .unwrap_or_else(|| userdata_dir.join("voxygen").join("logs"));

    // Init logging and hold the guards.
    let now = Utc::now();
    let log_filename = format!("{}_voxygen.log", now.format("%Y-%m-%d"));
    // Bring-up diagnostics must not commit the growing log file each batch.
    #[cfg(target_os = "trueos")]
    let _guards = common_frontend::init_stdout(None);
    #[cfg(not(target_os = "trueos"))]
    let _guards = common_frontend::init_stdout(Some((&logs_dir, &log_filename)));

    #[cfg(feature = "picasso-assets")]
    common::assets::initialize_picasso_assets();

    // Re-run userdata selection so any warnings will be logged
    common_base::userdata_dir();

    info!("Using userdata dir at: {}", userdata_dir.display());

    // Determine Voxygen's config directory either by env var or placed in veloren's
    // userdata folder
    let config_dir = std::env::var_os("VOXYGEN_CONFIG")
        .map(PathBuf::from)
        .and_then(|path| {
            if path.exists() {
                Some(path)
            } else {
                warn!(?path, "VOXYGEN_CONFIG points to invalid path.");
                None
            }
        })
        .unwrap_or_else(|| userdata_dir.join("voxygen"));
    info!("Using config dir at: {}", config_dir.display());

    // Load the settings
    let mut settings = Settings::load(&config_dir);
    #[cfg(target_os = "trueos")]
    {
        settings.apply_trueos_bringup_profile()
            .expect("Invalid embedded trueos-bringup-profile.json");
        info!("Applied TRUEOS bring-up graphics profile; settings remain adjustable this session");
    }
    // Start windowed even when the previous session saved fullscreen or maximized.
    settings.graphics.window.size = [1920, 1080];
    settings.graphics.window.maximised = false;
    settings.graphics.fullscreen.enabled = false;
    settings.display_warnings();

    panic_handler::set_panic_hook(log_filename, logs_dir);

    // Setup tokio runtime
    use alloc::sync::Arc;
    use common::consts::MIN_RECOMMENDED_TOKIO_THREADS;
    use core::{sync::atomic::AtomicUsize, sync::atomic::Ordering};
    use tokio::runtime::Builder;

    let cores = veloren_voxygen::CPU_COUNT;
    let tokio_runtime = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .worker_threads((cores / 4).max(MIN_RECOMMENDED_TOKIO_THREADS))
            .thread_name_fn(|| {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-voxygen-{}", id)
            })
            .build()
            .unwrap(),
    );

    #[cfg(target_os = "trueos")]
    veloren_voxygen::selection_progress::start(&tokio_runtime);

    // Initialise watcher for animation hot-reloading

    // Setup audio
    let mut audio = match settings.audio.output {
        AudioOutput::Off => AudioFrontend::no_audio(),
        AudioOutput::Automatic => AudioFrontend::new(
            settings.audio.num_sfx_channels,
            settings.audio.num_ui_channels,
            settings.audio.subtitles,
            // settings.audio.combat_music_enabled,
            false, // We're disabling combat music for now
            settings.audio.buffer_size.samples,
            settings.audio.sample_rate,
        ),
    };

    audio.set_master_volume(settings.audio.master_volume.get_checked());
    audio.set_music_volume(settings.audio.music_volume.get_checked());
    audio.set_sfx_volume(settings.audio.sfx_volume.get_checked());
    audio.set_instrument_volume(settings.audio.instrument_volume.get_checked());
    audio.set_ambience_volume(settings.audio.ambience_volume.get_checked());
    audio.set_music_spacing(settings.audio.music_spacing);

    // Load the profile.
    let profile = Profile::load(&config_dir);

    let mut i18n =
        LocalizationHandle::load(&settings.language.selected_language).unwrap_or_else(|error| {
            let selected_language = &settings.language.selected_language;
            warn!(
                ?error,
                ?selected_language,
                "Impossible to load language: change to the default language (English) instead.",
            );
            i18n::REFERENCE_LANG.clone_into(&mut settings.language.selected_language);
            LocalizationHandle::load_expect(&settings.language.selected_language)
        });
    i18n.set_english_fallback(settings.language.use_english_fallback);

    // Keep the original play-state stack. TRUEOS first publishes its UI4 menu;
    // scene-device initialization starts only after successful login.
    {
        // Create window
        use veloren_voxygen::{error::Error, render::RenderError};
        let event_loop = veloren_voxygen::window::EventLoop::new().unwrap();
        run::run(event_loop, move |event_loop| {
        let mut window = match Window::new(&settings, &tokio_runtime, event_loop) {
            Ok(ok) => ok,
            // Custom panic message when a graphics backend could not be found
            Err(Error::RenderError(RenderError::CouldNotFindAdapter)) => {
                #[cfg(target_os = "windows")]
                const POTENTIAL_FIX: &str =
                    " Updating the graphics drivers on this system may resolve this issue.";
                #[cfg(target_os = "macos")]
                const POTENTIAL_FIX: &str = "";
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                const POTENTIAL_FIX: &str =
                    " Installing or updating vulkan drivers may resolve this issue.";

                panic!(
                    "Failed to select a rendering backend! No compatible backends were found. We \
                     currently support vulkan, metal, dx12, and opengl.{} If the issue persists, \
                     please include the operating system and GPU details in your bug report to help \
                     us identify the cause.",
                    POTENTIAL_FIX
                );
            },
            Err(error) => panic!("Failed to create window!: {:?}", error),
        };

        let clipboard = veloren_voxygen::ui::ice::Clipboard::connect(window.window());

        let lazy_init = SpriteRenderContext::prepare(window.preparation_texture_limit());

        let global_state = GlobalState {
            userdata_dir,
            config_dir,
            audio,
            profile,
            window,
            tokio_runtime,
            portal_credentials: None,

            lazy_init,
            clock: Clock::new(core::time::Duration::from_secs_f64(
                1.0 / get_fps(settings.graphics.max_fps) as f64,
            )),
            settings,
            info_message: None,
            i18n,
            clipboard,
            clear_shadows_next_frame: false,
            args: args.clone(),
        };

        global_state
    })
    .unwrap();
    #[cfg(target_os = "trueos")]
    info!("voxy: shutdown stage=event-loop-returned");
    }
}

#[cfg(feature = "headless")]
fn main() {
    #[cfg(target_os = "trueos")]
    let _vm_shutdown = trueos::shutdown::ShutdownGuard::register()
        .expect("Failed to register cooperative Voxy shutdown");
    if let Err(error) = veloren_voxygen::headless::run() {
        #[cfg(target_os = "trueos")]
        let _ = trueos::logl::log_record(
            trueos::logl::level::ERROR,
            "apps::voxygen",
            format_args!("Headless client: {error}"),
        );
        #[cfg(not(target_os = "trueos"))]
        eprintln!("Headless client: {error}");
        #[cfg(not(target_os = "trueos"))]
        std::process::exit(1);
    }
}
