//! Shipped iced menus on TRUEOS: microfont composition followed by an opaque
//! retained-sprite copy. No world renderer or WGPU Instance is created here.
use super::main::{
    DetailedInitializationStage,
    client_init::{ClientInit, Msg},
    ui::{Event, MainMenuUi},
};
use crate::client::addr::ConnectionArgs;
use crate::{
    cli,
    settings::Settings,
    ui::ice::{Clipboard, window_event},
};
use i18n::LocalizationHandle;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use trueos::ui4_solara_text::{Damage, Error as UiError, SceneTarget, SpriteCorner, SpriteQuad};
use vek::Vec2;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::ModifiersState,
    platform::trueos::WindowExtTrueOS,
    window::{Window, WindowAttributes, WindowId},
};

pub fn run(
    settings: Settings,
    i18n: LocalizationHandle,
    runtime: Arc<tokio::runtime::Runtime>,
    config_dir: PathBuf,
    args: cli::Args,
) -> Result<(), String> {
    let failure = Arc::new(std::sync::Mutex::new(None));
    let app = App {
        settings,
        i18n,
        runtime,
        config_dir,
        args,
        state: None,
        init: None,
        last_tick: Instant::now(),
        error: Arc::clone(&failure),
    };
    // run_app consumes its handler; errors inside callbacks are reported before exit.
    EventLoop::new()
        .map_err(|e| e.to_string())?
        .run_app(app)
        .map_err(|e| e.to_string())?;
    match failure.lock().unwrap().take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct App {
    settings: Settings,
    i18n: LocalizationHandle,
    runtime: Arc<tokio::runtime::Runtime>,
    config_dir: PathBuf,
    args: cli::Args,
    state: Option<State>,
    init: Option<ClientInit>,
    last_tick: Instant,
    error: Arc<std::sync::Mutex<Option<String>>>,
}
struct State {
    window: Arc<dyn Window>,
    target: SceneTarget,
    ui: MainMenuUi,
    clipboard: Clipboard,
    modifiers: ModifiersState,
    size: Vec2<u32>,
    last_pixels: Vec<u8>,
    serial: Option<u64>,
    pending: Option<Pending>,
    first_presented: bool,
}
struct Pending {
    pixels: Vec<u8>,
    size: Vec2<u32>,
    phase: u8,
}

impl App {
    fn fail(&mut self, event_loop: &dyn ActiveEventLoop, error: String) {
        tracing::error!(%error, "Native iced menu stopped");
        *self.error.lock().unwrap() = Some(error);
        event_loop.exit();
    }
    fn tick(&mut self, event_loop: &dyn ActiveEventLoop) -> Result<(), String> {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let state = self.state.as_mut().unwrap();
        // Never overwrite a retained source while its exact publication is pending.
        if let Some(serial) = state.serial {
            if !state
                .target
                .was_presented(serial)
                .map_err(|e| format!("presentation: {e:?}"))?
            {
                return Ok(());
            }
            state.serial = None;
            if !state.first_presented {
                state.first_presented = true;
                tracing::info!(serial, "Native iced opaque frame reached SURFLIVE");
            }
        }
        if state.pending.is_none() {
            let size = state.window.surface_size();
            let size = Vec2::new(size.width, size.height);
            if size.x == 0 || size.y == 0 {
                return Ok(());
            }
            if let Some(init) = &self.init {
                while let Some(stage) = init.stage_update() {
                    state
                        .ui
                        .update_stage(DetailedInitializationStage::Client(stage));
                }
                match init.poll() {
                    Some(Msg::IsAuthTrusted(server)) => state.ui.auth_trust_prompt(server),
                    Some(Msg::Done(result)) => {
                        self.init = None;
                        match result {
                            // Character preview/world resources belong to the next renderer step.
                            Ok(_client) => state.ui.show_info("Connected. Native character preview renderer is not available yet.".into()),
                            Err(error) => state.ui.show_info(format!("Connection failed: {error:?}")),
                        }
                    }
                    None => {}
                }
            }
            let (events, pixels) = state.ui.maintain_native(
                &self.settings,
                &self.runtime,
                &mut state.clipboard,
                size,
                dt,
            )?;
            for event in events {
                match event {
                    Event::Quit => event_loop.exit(),
                    Event::CancelLoginAttempt => {
                        self.init = None;
                        state.ui.cancel_connection();
                    }
                    Event::LoginAttempt {
                        username,
                        password,
                        server_address,
                    } => {
                        if let Err(error) = common::comp::Player::alias_validate(&username) {
                            state.ui.show_info(
                                match error {
                                    common::comp::AliasError::ForbiddenCharacters => {
                                        "Username contains forbidden characters."
                                    }
                                    common::comp::AliasError::TooLong => "Username is too long.",
                                }
                                .into(),
                            );
                            continue;
                        }
                        let net = &mut self.settings.networking;
                        net.username.clone_from(&username);
                        net.default_server.clone_from(&server_address);
                        if !server_address.is_empty() && !net.servers.contains(&server_address) {
                            net.servers.push(server_address.clone());
                        }
                        let connection = if net.use_srv {
                            ConnectionArgs::Srv {
                                hostname: server_address,
                                prefer_ipv6: false,
                                validate_tls: net.validate_tls,
                                use_quic: net.use_quic,
                            }
                        } else if net.use_quic {
                            ConnectionArgs::Quic {
                                hostname: server_address,
                                prefer_ipv6: false,
                                validate_tls: net.validate_tls,
                            }
                        } else {
                            ConnectionArgs::Tcp {
                                hostname: server_address,
                                prefer_ipv6: false,
                            }
                        };
                        self.settings.save_to_file_warn(&self.config_dir);
                        self.init = Some(ClientInit::new(
                            connection,
                            username,
                            password,
                            Arc::clone(&self.runtime),
                            self.settings
                                .language
                                .send_to_server
                                .then(|| self.settings.language.selected_language.clone()),
                            &self.config_dir,
                            self.args.client_type.0,
                        ));
                    }
                    Event::AuthServerTrust(server, trust) => {
                        if let Some(init) = &self.init {
                            init.auth_trust(server, trust);
                        }
                    }
                    Event::ChangeLanguage(language) => {
                        self.settings.language.selected_language = language.language_identifier;
                        self.i18n = LocalizationHandle::load_expect(
                            &self.settings.language.selected_language,
                        );
                        self.i18n
                            .set_english_fallback(self.settings.language.use_english_fallback);
                        state.ui.update_language(self.i18n, &self.settings);
                        self.settings.save_to_file_warn(&self.config_dir);
                    }
                    Event::DeleteServer { server_index } => {
                        if server_index < self.settings.networking.servers.len() {
                            self.settings.networking.servers.remove(server_index);
                            self.settings.save_to_file_warn(&self.config_dir);
                        }
                    }
                }
            }
            if pixels == state.last_pixels && size == state.size {
                return Ok(());
            }
            if size != state.size {
                state.target =
                    SceneTarget::for_window(state.window.trueos_window_id(), size.x, size.y)
                        .map_err(|e| format!("resize target: {e:?}"))?;
                state.size = size;
            }
            state.pending = Some(Pending {
                pixels,
                size,
                phase: 0,
            });
        }
        // Keep the same write lease and transaction on Busy; retry only its current phase.
        let pending = state.pending.as_mut().unwrap();
        loop {
            let result = match pending.phase {
                0 => state.target.upload_sprite_rgba8(
                    1,
                    pending.size.x,
                    pending.size.y,
                    &pending.pixels,
                ),
                1 => state.target.begin_gpu_frame(),
                2 => state.target.draw_sprite_quads(&[full_quad(pending.size)]),
                _ => match state
                    .target
                    .publish_tracked(Damage::full(pending.size.x, pending.size.y))
                {
                    Ok(serial) => {
                        state.serial = Some(serial);
                        state.last_pixels = state.pending.take().unwrap().pixels;
                        return Ok(());
                    }
                    Err(UiError::Busy) => return Ok(()),
                    Err(error) => return Err(format!("publish: {error:?}")),
                },
            };
            match result {
                Ok(()) => pending.phase += 1,
                Err(UiError::Busy) => return Ok(()),
                Err(error) => {
                    return Err(format!("native frame phase {}: {error:?}", pending.phase));
                }
            }
        }
    }
}
fn full_quad(size: Vec2<u32>) -> SpriteQuad {
    SpriteQuad {
        sprite_id: 1,
        c0: SpriteCorner {
            x: 0.,
            y: 0.,
            u: 0.,
            v: 0.,
        },
        c1: SpriteCorner {
            x: size.x as f32,
            y: 0.,
            u: 1.,
            v: 0.,
        },
        c2: SpriteCorner {
            x: size.x as f32,
            y: size.y as f32,
            u: 1.,
            v: 1.,
        },
        c3: SpriteCorner {
            x: 0.,
            y: size.y as f32,
            u: 0.,
            v: 1.,
        },
        color_rgba: u32::MAX,
        source_over: false,
    }
}
impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let result = (|| {
            let window: Arc<dyn Window> = Arc::from(
                event_loop
                    .create_window(
                        WindowAttributes::default()
                            .with_title("Voxygen")
                            .with_surface_size(winit::dpi::PhysicalSize::new(
                                self.settings.graphics.window.size[0],
                                self.settings.graphics.window.size[1],
                            )),
                    )
                    .map_err(|e| e.to_string())?,
            );
            let size = window.surface_size();
            let size = Vec2::new(size.width, size.height);
            let target = SceneTarget::for_window(window.trueos_window_id(), size.x, size.y)
                .map_err(|e| format!("window target: {e:?}"))?;
            let mut ui = MainMenuUi::new_native(
                &self.settings,
                self.i18n,
                self.args.server.clone(),
                size,
                window.scale_factor(),
            );
            if std::env::var("VOXYGEN_UI_PROOF").as_deref() == Ok("loading") {
                ui.show_loading_proof();
                tracing::info!(
                    "Showing shipped loading screen for native rendering proof; no connection started"
                );
            }
            let clipboard = Clipboard::connect(window.as_ref());
            Ok::<_, String>(State {
                window,
                target,
                ui,
                clipboard,
                size,
                modifiers: ModifiersState::empty(),
                last_pixels: Vec::new(),
                pending: None,
                serial: None,
                first_presented: false,
            })
        })();
        match result {
            Ok(state) => self.state = Some(state),
            Err(error) => self.fail(event_loop, error),
        }
    }
    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if state.window.id() != id {
            return;
        }
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
            return;
        }
        if let WindowEvent::ModifiersChanged(modifiers) = &event {
            state.modifiers = modifiers.state();
        }
        if let WindowEvent::ScaleFactorChanged { scale_factor, .. } = &event {
            state
                .ui
                .handle_event(crate::window::Event::ScaleFactorChanged(*scale_factor));
        }
        if let Some(event) = window_event(&event, state.window.scale_factor(), state.modifiers) {
            state.ui.handle_ui_event(event);
        }
    }
    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.state.is_some() {
            if let Err(error) = self.tick(event_loop) {
                self.fail(event_loop, error);
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}
