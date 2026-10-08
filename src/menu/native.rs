//! Shipped iced menus on a paired UI4 scene/UI window. Independent render
//! workers keep GPU admission and retirement outside Winit input dispatch.
use super::connection_screen::ConnectionScreen;
use super::main::{
    DetailedInitializationStage,
    client_init::{ClientInit, Msg},
    ui::{Event, MainMenuUi},
};
use crate::client::addr::ConnectionArgs;
use crate::ui::ice::renderer::{activity::micros, presenter::LayeredPresenter};
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
use trueos::ui4_winit::SceneTarget;
use vek::Vec2;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::ModifiersState,
    platform::trueos::{ActiveEventLoopExtTrueOS, WindowExtTrueOS},
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
        portal_credentials: None,
        client: None,
        connection_screen: ConnectionScreen::default(),
        last_tick: Instant::now(),
        error: Arc::clone(&failure),
    };
    // run_app consumes its handler; errors inside callbacks are reported before exit.
    EventLoop::new()
        .map_err(|e| e.to_string())?
        .run_app(app)
        .map_err(|e| e.to_string())?;
    tracing::info!("Native menu event loop returned; Winit released its window");
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
    portal_credentials: Option<Arc<crate::server_portal::PortalCredentials>>,
    client: Option<ConnectedClient>,
    connection_screen: ConnectionScreen,
    last_tick: Instant,
    error: Arc<std::sync::Mutex<Option<String>>>,
}

struct ConnectedClient {
    client: Box<crate::client::Client>,
    last_tick: Instant,
    characters_pending: bool,
}

/// Client::drop waits for network disconnect. Keep that teardown off the
/// Winit input thread, including a completed client cancelled during the hold.
fn retire_client(runtime: &tokio::runtime::Runtime, client: &mut Option<ConnectedClient>) {
    if let Some(client) = client.take() {
        runtime.spawn_blocking(move || drop(client));
    }
}

struct State {
    // Join render workers before dropping the owning Winit window.
    presenter: LayeredPresenter,
    window: Arc<dyn Window>,
    ui: MainMenuUi,
    clipboard: Clipboard,
    modifiers: ModifiersState,
    input_logged: u8,
    revision: u64,
    activity: LoopActivity,
    activity_since: Instant,
}

/// Window events are counted without retaining key/text contents.
#[derive(Default, Debug)]
struct LoopActivity {
    ticks: u64,
    pointer_moves: u64,
    pointer_buttons: u64,
    keys: u64,
    wheels: u64,
    resize_events: u64,
    redraw_requests: u64,
    other_window_events: u64,
    maintain_call_us: u64,
    max_maintain_call_us: u64,
}
impl State {
    fn report_activity(&mut self, force: bool) {
        let elapsed = self.activity_since.elapsed();
        if !force && elapsed < Duration::from_secs(5) {
            return;
        }
        let screen = self.ui.native_activity_screen();
        let (ui, preparation) = self.ui.take_native_activity();
        let (scene, foreground) = self.presenter.take_activity();
        let window = std::mem::take(&mut self.activity);
        tracing::info!(target:"voxy_ui_activity", interval_ms=elapsed.as_millis() as u64,
            screen, revision=self.revision, ?window, ?ui, ?preparation, "Native menu activity");
        tracing::info!(target:"voxy_ui_activity", producer="scene", ?scene, "Native producer activity");
        tracing::info!(target:"voxy_ui_activity", producer="foreground", ?foreground, "Native producer activity");
        self.activity_since = Instant::now();
    }
}

impl App {
    fn fail(&mut self, event_loop: &dyn ActiveEventLoop, error: String) {
        if let Some(state) = self.state.as_mut() {
            state.report_activity(true);
        }
        tracing::error!(%error, "Native iced menu stopped");
        *self.error.lock().unwrap() = Some(error);
        event_loop.exit();
    }
    fn tick(&mut self, event_loop: &dyn ActiveEventLoop) -> Result<(), String> {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let state = self.state.as_mut().unwrap();
        state.presenter.check()?;
        state.activity.ticks += 1;
        {
            let viewport = state.window.trueos_content_viewport();
            state.ui.set_native_origin(Vec2::new(
                viewport.position.x as f32,
                viewport.position.y as f32,
            ));
            let size = state.window.surface_size();
            let size = Vec2::new(size.width, size.height);
            if size.x == 0 || size.y == 0 {
                return Ok(());
            }
            if let Some(init) = &self.init {
                while let Some(stage) = init.stage_update() {
                    tracing::info!(?stage, "Native login initialization stage");
                    state
                        .ui
                        .update_stage(DetailedInitializationStage::Client(stage));
                }
                match init.poll() {
                    Some(Msg::IsAuthTrusted(server)) => {
                        // Compatibility with an initializer that still emits
                        // the old request: resolve it without showing a dialog.
                        init.auth_trust(server, true);
                    }
                    Some(Msg::Done(result)) => {
                        self.init = None;
                        match result {
                            Ok(mut client) => {
                                // ClientInit has completed handshake, login,
                                // initial data loading and StartingClient here.
                                crate::ecs::init(client.state_mut().ecs_mut());
                                let characters_pending = !client.are_plugins_missing();
                                if characters_pending {
                                    client.load_character_list();
                                }
                                self.client = Some(ConnectedClient {
                                    client: Box::new(client),
                                    last_tick: Instant::now(),
                                    characters_pending,
                                });
                                tracing::info!(
                                    "Native login complete; client ready at character selection"
                                );
                                self.connection_screen.complete("Login complete.\nThe character screen is not ready to render yet.".into());
                            }
                            Err(error) => {
                                tracing::warn!(?error, "Native multiplayer connection failed");
                                self.connection_screen.complete(
                                    super::main::get_client_init_msg_error(error, &self.i18n),
                                );
                            }
                        }
                    }
                    None => {}
                }
            }
            // Follow the character-selection client's normal network upkeep
            // without starting a renderer or selecting an in-game character.
            let session_error = self.client.as_mut().and_then(|session| {
                let elapsed = now.saturating_duration_since(session.last_tick);
                if elapsed < Duration::from_millis(33) {
                    return None;
                }
                session.last_tick = now;
                let result = session
                    .client
                    .tick(common::comp::ControllerInputs::default(), elapsed);
                session.client.cleanup();
                if session.characters_pending && !session.client.character_list().loading {
                    tracing::info!(
                        characters = session.client.character_list().characters.len(),
                        "Native character list received"
                    );
                    session.characters_pending = false;
                }
                match result {
                    Ok(events)
                        if events
                            .iter()
                            .any(|event| matches!(event, crate::client::Event::Disconnect)) =>
                    {
                        Some("The server disconnected.\nPlease log in again.".into())
                    }
                    Ok(_) => None,
                    Err(error) => {
                        tracing::warn!(?error, "Native character-selection connection failed");
                        Some(super::main::get_client_msg_error(
                            error,
                            None,
                            &self.i18n.read(),
                        ))
                    }
                }
            });
            if let Some(error) = session_error {
                retire_client(&self.runtime, &mut self.client);
                if self.connection_screen.is_active() {
                    self.connection_screen.complete(error);
                } else {
                    state.ui.show_info(error);
                }
            }
            // maintain_native prepares the current screen before applying its
            // messages. A Login click's plan still belongs to the login screen.
            let rendered_connecting = state.ui.native_activity_screen() == "connecting";
            let maintain_started = Instant::now();
            let (events, plan) = state.ui.maintain_native(
                &self.settings,
                &self.runtime,
                &mut state.clipboard,
                Vec2::new(viewport.size.width, viewport.size.height),
                dt,
            )?;
            let elapsed = micros(maintain_started.elapsed());
            state.activity.maintain_call_us += elapsed;
            state.activity.max_maintain_call_us = state.activity.max_maintain_call_us.max(elapsed);
            for event in events {
                match event {
                    Event::Quit => {
                        state.report_activity(true);
                        tracing::info!("Native menu exit requested by Quit control");
                        event_loop.exit();
                    }
                    Event::CancelLoginAttempt => {
                        if let Some(mut init) = self.init.take() {
                            init.cancel();
                            // A completion already queued in its receiver may
                            // own a Client too; destroy it on the worker.
                            self.runtime.spawn_blocking(move || drop(init));
                        }
                        // A fast successful login may already be maintained
                        // while its visible transition waits for the minimum.
                        retire_client(&self.runtime, &mut self.client);
                        self.connection_screen.cancel();
                        state.ui.cancel_connection();
                        tracing::info!("Native connection cancelled; minimum appearance bypassed");
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
                        retire_client(&self.runtime, &mut self.client);
                        self.connection_screen.begin();
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
                        self.portal_credentials =
                            Some(Arc::new(crate::server_portal::PortalCredentials::new(
                                username.clone(),
                                password.clone(),
                            )));
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
            if let Some(mut plan) = plan {
                plan.place_in_viewport(
                    viewport.position.x,
                    viewport.position.y,
                    viewport.size.width,
                    viewport.size.height,
                );
                state.revision += 1;
                state.presenter.submit(state.revision, size, plan);
                if rendered_connecting {
                    self.connection_screen.submitted(state.revision);
                }
            }
            // Start the minimum only after both render workers publish the
            // loading screen; upload/queue time does not count as appearance.
            if self
                .connection_screen
                .published(state.presenter.published_revision(), Instant::now())
            {
                tracing::info!(
                    minimum_ms = 3000,
                    "Native connection screen published; appearance timer started"
                );
            }
            // Handle input first so an explicit Cancel wins even on the tick
            // where a pending success or failure becomes eligible to display.
            if let Some(message) = self.connection_screen.take_ready(Instant::now()) {
                state.ui.show_info(message);
                tracing::info!("Native connection outcome shown after minimum appearance");
            }
        }
        state.report_activity(false);
        Ok(())
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
                    .create_layered_window(
                        WindowAttributes::default()
                            .with_title("Voxygen")
                            .with_surface_size(winit::dpi::PhysicalSize::new(
                                self.settings.graphics.window.size[0],
                                self.settings.graphics.window.size[1],
                            )),
                        60,
                    )
                    .map_err(|e| e.to_string())?,
            );
            window
                .trueos_set_resize_aspect_ratio(Some(winit::dpi::PhysicalSize::new(
                    self.settings.graphics.window.size[0],
                    self.settings.graphics.window.size[1],
                )))
                .map_err(|error| error.to_string())?;
            let size = window.surface_size();
            let size = Vec2::new(size.width, size.height);
            let target = SceneTarget::for_window(window.trueos_window_id(), size.x, size.y)
                .map_err(|e| format!("window target: {e:?}"))?;
            let background = target
                .background()
                .map_err(|e| format!("scene layer: {e:?}"))?;
            let presenter = LayeredPresenter::new(target, background)?;
            tracing::info!(
                "Native menu uses independent scene/UI producers; foreground BCS0, display-plane alpha"
            );
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
                presenter,
                ui,
                clipboard,
                modifiers: ModifiersState::empty(),
                input_logged: 0,
                revision: 0,
                activity: LoopActivity::default(),
                activity_since: Instant::now(),
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
        match &event {
            WindowEvent::PointerMoved { .. } => state.activity.pointer_moves += 1,
            WindowEvent::PointerButton { .. } => state.activity.pointer_buttons += 1,
            WindowEvent::KeyboardInput { .. } => state.activity.keys += 1,
            WindowEvent::MouseWheel { .. } => state.activity.wheels += 1,
            WindowEvent::SurfaceResized { .. } | WindowEvent::ScaleFactorChanged { .. } => {
                state.activity.resize_events += 1
            }
            WindowEvent::RedrawRequested => state.activity.redraw_requests += 1,
            _ => state.activity.other_window_events += 1,
        }
        if matches!(event, WindowEvent::CloseRequested) {
            state.ui.request_quit();
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
        let (input_bit, input_kind) = match &event {
            WindowEvent::PointerButton { .. } => (1, "pointer button"),
            WindowEvent::KeyboardInput { .. } => (2, "keyboard"),
            _ => (0, ""),
        };
        if input_bit != 0 && state.input_logged & input_bit == 0 {
            state.input_logged |= input_bit;
            tracing::info!(input_kind, "Native menu received routed Winit input");
        }
        if let Some(event) = window_event(&event, state.window.scale_factor(), state.modifiers) {
            state.ui.handle_ui_event(event);
        }
    }
    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if matches!(trueos::shutdown::requested(), Ok(true)) {
            tracing::info!("voxy: cooperative stop requested; exiting native menu");
            event_loop.exit();
            return;
        }
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
