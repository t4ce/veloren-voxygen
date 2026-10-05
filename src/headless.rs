//! Lean game client with geometry on the native wgpu surface and console status.
use crate::client::{Client, ClientType, Event, addr::ConnectionArgs};
use clap::Parser;
use common::{
    ViewDistances,
    character::CharacterId,
    comp::{ControllerInputs, InputKind},
    util::Dir,
};
use std::{
    collections::{BTreeSet, HashSet},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tokio::runtime::Runtime;
use tracing::warn;
use vek::{Vec2, Vec3};
mod gpu;
#[cfg(not(target_os = "trueos"))]
mod render;
#[cfg(target_os = "trueos")]
mod render_trueos;
mod scene;
#[cfg(any(target_os = "trueos", test))]
pub mod shader;
#[cfg(target_os = "trueos")]
mod ui4;
#[cfg(target_os = "trueos")]
use ui4::KeyCode;
#[cfg(not(target_os = "trueos"))]
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, ElementState, Ime, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

const MENU_DELAY: Duration = Duration::from_secs(3);

const FRAME: Duration = Duration::from_nanos(16_666_667);

fn connection_progress(message: std::fmt::Arguments<'_>) {
    #[cfg(target_os = "trueos")]
    // IMPORTANT survives the GPU diagnostic profile's area filters.
    let _ = trueos::logl::log_record(trueos::logl::level::IMPORTANT, "apps::voxygen", message);
    #[cfg(not(target_os = "trueos"))]
    eprintln!("{message}");
}

#[derive(Parser)]
#[command(
    about = "Voxygen headless: live geometry and console status. Type your password in the window and press Enter."
)]
struct Args {
    #[cfg_attr(
        target_os = "trueos",
        arg(long, default_value = "192.168.178.111:14004")
    )]
    #[cfg_attr(
        not(target_os = "trueos"),
        arg(long, default_value = "localhost:14004")
    )]
    server: String,
    #[arg(long, default_value = "t4ce")]
    username: String,
    /// Existing character ID; otherwise create/reuse <username>-headless.
    #[arg(long)]
    character: Option<i64>,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=65))]
    view_distance: u32,
    /// Authentication endpoint allowed to receive the login credentials.
    #[arg(long, default_value = "https://auth.veloren.net")]
    auth_server: String,
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(target_os = "trueos"))]
    let args = Args::parse();
    #[cfg(target_os = "trueos")]
    let args = Args::parse_from(["voxygen-headless"]);
    let _logs = common_frontend::init_stdout(None);
    #[cfg(feature = "picasso-assets")]
    common::assets::initialize_picasso_assets();
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?,
    );
    let app = App::new(args, runtime);
    connection_progress(format_args!(
        "Voxygen headless: geometry only; WASD / mouse / Space; click attack / F wield / R respawn / 1-5 abilities / Esc release"
    ));
    #[cfg(not(target_os = "trueos"))]
    {
        let event_loop = EventLoop::new()?;
        let mut app = app;
        event_loop.run_app(&mut app)?;
        if let Some(error) = app.render_error {
            return Err(error.into());
        }
        Ok(())
    }
    #[cfg(target_os = "trueos")]
    ui4::run(app)
}

struct App {
    args: Args,
    runtime: Arc<Runtime>,
    #[cfg(not(target_os = "trueos"))]
    renderer: Option<render::Renderer>,
    #[cfg(not(target_os = "trueos"))]
    render_error: Option<String>,
    #[cfg(not(target_os = "trueos"))]
    window: Option<Arc<Window>>,
    #[cfg(target_os = "trueos")]
    window: Option<trueos::ui4_scene::Frame>,
    password: String,
    pending: Option<mpsc::Receiver<Result<Client, String>>>,
    login_progress: Option<mpsc::Receiver<String>>,
    client: Option<Client>,
    input: Input,
    captured: bool,
    character_requested: bool,
    character_created: bool,
    world_joined: bool,
    menu_due: Option<Instant>,
    #[cfg(not(target_os = "trueos"))]
    next_tick: Instant,
    last_tick: Instant,
    #[cfg(not(target_os = "trueos"))]
    ime_active: bool,
}

#[derive(Default)]
struct Input {
    keys: HashSet<KeyCode>,
    actions: BTreeSet<InputKind>,
    yaw: f32,
    pitch: f32,
}

impl Input {
    fn controller(&self) -> ControllerInputs {
        let axis = |positive, negative| {
            i32::from(self.keys.contains(&positive)) as f32
                - i32::from(self.keys.contains(&negative)) as f32
        };
        let forward = Vec2::new(self.yaw.sin(), self.yaw.cos());
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let movement = forward * axis(KeyCode::KeyW, KeyCode::KeyS)
            + right * axis(KeyCode::KeyD, KeyCode::KeyA);
        ControllerInputs {
            move_dir: movement / movement.magnitude().max(1.0),
            move_z: axis(KeyCode::Space, KeyCode::ControlLeft),
            look_dir: Dir::from_unnormalized(Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.yaw.cos() * self.pitch.cos(),
                self.pitch.sin(),
            ))
            .expect("finite mouse orientation"),
            strafing: true,
            ..ControllerInputs::default()
        }
    }

    fn mouse(&mut self, dx: f64, dy: f64) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        self.yaw = (self.yaw + dx.clamp(-10_000.0, 10_000.0) as f32 * 0.003)
            .rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - dy.clamp(-10_000.0, 10_000.0) as f32 * 0.003).clamp(-1.55, 1.55);
    }
}

impl App {
    fn new(args: Args, runtime: Arc<Runtime>) -> Self {
        Self {
            args,
            runtime,
            #[cfg(not(target_os = "trueos"))]
            renderer: None,
            #[cfg(not(target_os = "trueos"))]
            render_error: None,
            window: None,
            password: String::new(),
            pending: None,
            login_progress: None,
            client: None,
            input: Input::default(),
            captured: false,
            character_requested: false,
            character_created: false,
            world_joined: false,
            menu_due: None,
            #[cfg(not(target_os = "trueos"))]
            next_tick: Instant::now(),
            last_tick: Instant::now(),
            #[cfg(not(target_os = "trueos"))]
            ime_active: false,
        }
    }

    fn prompt(&mut self, message: &str) {
        connection_progress(format_args!(
            "Voxygen {} @ {}: {}",
            self.args.username, self.args.server, message
        ));
        #[cfg(not(target_os = "trueos"))]
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "Voxygen headless — {} @ {} — {message}",
                self.args.username, self.args.server
            ));
        }
    }

    fn capture(&mut self, capture: bool) {
        #[cfg(target_os = "trueos")]
        {
            self.captured = self
                .window
                .as_mut()
                .is_some_and(|frame| frame.set_center_snapped_mouse(capture).is_ok() && capture);
        }
        #[cfg(not(target_os = "trueos"))]
        if let Some(window) = &self.window {
            self.captured = if capture {
                window
                    .set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
                    .is_ok()
            } else {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
                false
            };
            window.set_cursor_visible(!self.captured);
            window.set_ime_allowed(self.client.is_none());
        }
        if !self.captured {
            if let Some(client) = &mut self.client {
                for action in std::mem::take(&mut self.input.actions) {
                    client.handle_input(action, false, None, None);
                }
            }
            self.input.keys.clear();
        }
    }

    fn login(&mut self) {
        if self.pending.is_some() || self.client.is_some() {
            return;
        }
        let password = std::mem::take(&mut self.password);
        let username = self.args.username.clone();
        let server = self.args.server.clone();
        let auth_server = self.args.auth_server.trim_end_matches('/').to_owned();
        let runtime = Arc::clone(&self.runtime);
        let (sender, receiver) = mpsc::channel();
        let (progress_sender, progress_receiver) = mpsc::channel();
        self.login_progress = Some(progress_receiver);
        self.pending = Some(receiver);
        self.prompt("Connecting…");
        std::thread::spawn(move || {
            let report = |message: String| {
                connection_progress(format_args!("{message}"));
                let _ = progress_sender.send(message);
            };
            report(format!("Login worker started: {username} @ {server}"));
            let result = runtime.block_on(async {
                report("Login future polling; timeout=60s".to_owned());
                tokio::time::timeout(
                    Duration::from_secs(60),
                    Client::new(
                        ConnectionArgs::Tcp {
                            hostname: server,
                            prefer_ipv6: false,
                        },
                        Arc::clone(&runtime),
                        &mut None,
                        &username,
                        &password,
                        None,
                        |endpoint| endpoint.trim_end_matches('/') == auth_server,
                        &|stage| report(format!("Connection stage: {stage:?}")),
                        |_| {},
                        std::path::PathBuf::new(),
                        ClientType::Game,
                    ),
                )
                .await
            });
            let result = match result {
                Ok(Ok(client)) => Ok(client),
                Ok(Err(error)) => Err(format!("{error:?}")),
                Err(_) => Err("Connection timed out".to_owned()),
            };
            match &result {
                Ok(_) => connection_progress(format_args!(
                    "Voxygen login complete; requesting characters next"
                )),
                Err(error) => connection_progress(format_args!("Voxygen login failed: {error}")),
            }
            let _ = sender.send(result);
        });
    }

    fn action(&mut self, action: InputKind, pressed: bool) {
        let changed = if pressed {
            self.input.actions.insert(action)
        } else {
            self.input.actions.remove(&action)
        };
        if changed && let Some(client) = &mut self.client {
            client.handle_input(action, pressed, None, None);
        }
    }

    fn key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        if key == KeyCode::Escape && pressed {
            self.capture(false);
            self.prompt("Mouse released — click to capture");
            return;
        }
        if !self.captured {
            return;
        }
        if pressed {
            self.input.keys.insert(key);
        } else {
            self.input.keys.remove(&key);
        }
        let action = match key {
            KeyCode::Space => Some(InputKind::Jump),
            KeyCode::ShiftLeft => Some(InputKind::Roll),
            KeyCode::ControlLeft => Some(InputKind::Block),
            KeyCode::Digit1 => Some(InputKind::Ability(0)),
            KeyCode::Digit2 => Some(InputKind::Ability(1)),
            KeyCode::Digit3 => Some(InputKind::Ability(2)),
            KeyCode::Digit4 => Some(InputKind::Ability(3)),
            KeyCode::Digit5 => Some(InputKind::Ability(4)),
            _ => None,
        };
        if let Some(action) = action {
            self.action(action, pressed);
        }
        if pressed
            && !repeat
            && let Some(client) = &mut self.client
        {
            match key {
                KeyCode::KeyF => client.toggle_wield(),
                KeyCode::KeyR => {
                    client.respawn();
                }
                KeyCode::KeyX => client.toggle_sit(),
                KeyCode::KeyC => client.toggle_sneak(),
                _ => {}
            }
        }
    }

    fn tick(&mut self) {
        while let Some(message) = self.login_progress.as_ref().and_then(|p| p.try_recv().ok()) {
            self.prompt(&message);
        }
        let connected = self
            .pending
            .as_ref()
            .and_then(|pending| match pending.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Login worker stopped".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = connected {
            self.pending = None;
            self.login_progress = None;
            match result {
                Ok(mut client) => {
                    client.load_character_list();
                    self.client = Some(client);
                    self.character_requested = false;
                    self.character_created = false;
                    self.world_joined = false;
                    self.menu_due = Some(Instant::now() + MENU_DELAY);
                    self.last_tick = Instant::now();
                    self.prompt("Loading characters…");
                }
                Err(error) => {
                    warn!(%error, "Login failed");
                    self.prompt(&format!("{error} — type password and press Enter to retry"));
                }
            }
        }
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .min(Duration::from_millis(100));
        self.last_tick = now;
        let inputs = self.input.controller();
        let events = match self.client.as_mut().map(|client| client.tick(inputs, dt)) {
            Some(Ok(events)) => events,
            Some(Err(error)) => {
                connection_progress(format_args!("Headless client tick failed: {error:?}"));
                self.capture(false);
                self.client = None;
                self.character_requested = false;
                self.world_joined = false;
                self.prompt("Disconnected — type password and press Enter to retry");
                return;
            }
            None => return,
        };
        for event in events {
            match event {
                Event::Disconnect => {
                    self.capture(false);
                    self.client = None;
                    self.character_requested = false;
                    self.world_joined = false;
                    self.prompt("Disconnected — type password and press Enter to retry");
                    return;
                }
                Event::CharacterError(error) => {
                    warn!(%error, "Character selection failed");
                    self.capture(false);
                    self.client = None;
                    self.character_requested = false;
                    self.world_joined = false;
                    self.prompt(&format!("{error} — type password and Enter to retry"));
                    return;
                }
                Event::CharacterJoined(_) => {
                    self.world_joined = true;
                    self.capture(true);
                    self.prompt("World entry confirmed by server — WASD / mouse / Space to jump");
                    if let Some(position) = self.client.as_ref().and_then(Client::position) {
                        connection_progress(format_args!(
                            "Voxygen joined at position {position:?}"
                        ));
                    }
                }
                _ => {}
            }
        }
        if !self.character_requested {
            let client = self.client.as_mut().expect("connected client");
            let list = client.character_list();
            if list.loading || self.menu_due.is_some_and(|due| now < due) {
                return;
            }
            let alias = headless_character_alias(&self.args.username);
            let character = list
                .characters
                .iter()
                .find(|item| match self.args.character {
                    Some(id) => item.character.id == Some(CharacterId(id)),
                    None => item.character.alias == alias,
                })
                .and_then(|item| item.character.id);
            match character {
                Some(id) => {
                    client.request_character(
                        id,
                        ViewDistances {
                            terrain: self.args.view_distance,
                            entity: self.args.view_distance,
                        },
                    );
                    self.character_requested = true;
                    self.prompt("Entering world — waiting for server confirmation…");
                }
                None if self.args.character.is_none() && !self.character_created => {
                    client.create_character(
                        alias.clone(),
                        Some("common.items.weapons.sword.starter".to_owned()),
                        None,
                        common::comp::Body::Humanoid(common::comp::humanoid::Body::random()),
                        false,
                        None,
                    );
                    self.character_created = true;
                    self.menu_due = Some(now + MENU_DELAY);
                    self.prompt(&format!(
                        "Creating {alias} — will enter after server confirmation"
                    ));
                }
                None => {
                    self.client = None;
                    self.prompt("Character not found after selection/creation — type password and Enter to retry");
                }
            }
        }
    }
}

#[cfg(not(target_os = "trueos"))]
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match event_loop.create_window(
                Window::default_attributes()
                    .with_title("Voxygen headless — password input")
                    .with_inner_size(winit::dpi::PhysicalSize::new(1280, 720)),
            ) {
                Ok(window) => {
                    if let Some(monitor) = window.current_monitor() {
                        let size = monitor.size();
                        let (x, y, width, height) = scene::placement(size.width, size.height);
                        let origin = monitor.position();
                        let _ =
                            window.request_inner_size(winit::dpi::PhysicalSize::new(width, height));
                        window.set_outer_position(winit::dpi::PhysicalPosition::new(
                            origin.x + x,
                            origin.y + y,
                        ));
                    }
                    let window = Arc::new(window);
                    match render::Renderer::new(
                        Arc::clone(&window),
                        event_loop.owned_display_handle(),
                        &self.runtime,
                    ) {
                        Ok(renderer) => self.renderer = Some(renderer),
                        Err(error) => {
                            warn!(%error, "Could not initialize minimal wgpu renderer");
                            self.render_error = Some(error.to_string());
                            event_loop.exit();
                            return;
                        }
                    }
                    window.set_ime_allowed(true);
                    window.focus_window();
                    window.request_redraw();
                    self.window = Some(window);
                    self.prompt("Type password and press Enter (input is hidden)");
                }
                Err(error) => {
                    warn!(%error, "Could not create input window");
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|window| window.id() != id) {
            return;
        }
        match event {
            WindowEvent::RedrawRequested => {
                let renderer = self.renderer.as_mut().expect("minimal renderer");
                match renderer.draw(self.client.as_ref(), self.input.yaw, self.input.pitch) {
                    Ok(()) => {}
                    Err(error) => {
                        warn!(%error, "Minimal wgpu frame failed");
                        self.render_error = Some(error.to_string());
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::Resized(_) => {
                self.window.as_ref().expect("input window").request_redraw();
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(false) => self.capture(false),
            WindowEvent::Ime(Ime::Preedit(text, _)) => self.ime_active = !text.is_empty(),
            WindowEvent::Ime(Ime::Disabled) => self.ime_active = false,
            WindowEvent::Ime(Ime::Commit(text))
                if self.client.is_none() && self.pending.is_none() =>
            {
                self.ime_active = false;
                self.password
                    .extend(text.chars().filter(|c| !c.is_control()));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let PhysicalKey::Code(key) = event.physical_key else {
                    return;
                };
                if self.client.is_none() && self.pending.is_none() {
                    if !pressed {
                        return;
                    }
                    if edit_password(
                        &mut self.password,
                        key,
                        event.text.as_deref(),
                        self.ime_active,
                    ) {
                        self.login();
                    }
                } else {
                    self.key(key, pressed, event.repeat);
                }
            }
            WindowEvent::MouseInput { state, button, .. } if self.world_joined => {
                let pressed = state == ElementState::Pressed;
                if !self.captured && pressed {
                    self.capture(true);
                    return;
                }
                if !self.captured {
                    return;
                }
                let action = match button {
                    MouseButton::Left => Some(InputKind::Primary),
                    MouseButton::Right => Some(InputKind::Secondary),
                    MouseButton::Middle => Some(InputKind::Block),
                    _ => None,
                };
                if let Some(action) = action {
                    self.action(action, pressed);
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: winit::event::DeviceId, event: DeviceEvent) {
        if self.captured
            && let DeviceEvent::MouseMotion { delta } = event
        {
            self.input.mouse(delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_tick {
            self.tick();
            self.next_tick = Instant::now() + FRAME;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}

fn headless_character_alias(username: &str) -> String {
    let suffix = "-headless";
    let prefix: String = username
        .chars()
        .take(common::character::MAX_NAME_LENGTH - suffix.len())
        .collect();
    format!("{prefix}{suffix}")
}

/// Returns true when Enter submits the hidden password buffer.
fn edit_password(password: &mut String, key: KeyCode, text: Option<&str>, composing: bool) -> bool {
    if composing {
        return false;
    }
    match key {
        KeyCode::Enter | KeyCode::NumpadEnter => return true,
        KeyCode::Backspace => {
            password.pop();
        }
        KeyCode::Escape => password.clear(),
        _ if !composing => {
            if let Some(text) = text {
                password.extend(text.chars().filter(|c| !c.is_control()));
            }
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_character_alias_is_stable_and_fits_the_server_name_limit() {
        assert_eq!(headless_character_alias("t4ce"), "t4ce-headless");
        let long_name = "é".repeat(common::character::MAX_NAME_LENGTH * 2);
        let alias = headless_character_alias(&long_name);
        assert!(common::character::verify_character_name(&alias));
        assert_eq!(alias.chars().count(), common::character::MAX_NAME_LENGTH);
        assert!(alias.ends_with("-headless"));
    }

    #[test]
    fn password_unicode_editing_and_enter_submission() {
        let mut password = String::new();
        assert!(!edit_password(
            &mut password,
            KeyCode::KeyA,
            Some("aé\n"),
            false
        ));
        assert_eq!(password, "aé");
        edit_password(&mut password, KeyCode::Backspace, None, false);
        assert_eq!(password, "a");
        edit_password(&mut password, KeyCode::KeyB, Some("b"), true);
        assert_eq!(password, "a");
        assert!(!edit_password(&mut password, KeyCode::Enter, None, true));
        assert!(edit_password(
            &mut password,
            KeyCode::Enter,
            Some("\r"),
            false
        ));
        assert_eq!(password, "a");
        edit_password(&mut password, KeyCode::Escape, None, false);
        assert!(password.is_empty());
    }

    #[test]
    fn enter_starts_login_and_connection_failure_allows_retry() {
        let args = Args::parse_from(["headless", "--server", "127.0.0.1:0"]);
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap(),
        );
        let mut app = App::new(args, runtime);
        edit_password(
            &mut app.password,
            KeyCode::KeyT,
            Some("test-password"),
            false,
        );
        assert!(edit_password(
            &mut app.password,
            KeyCode::Enter,
            None,
            false
        ));
        app.login();
        assert!(app.password.is_empty());
        assert!(app.pending.is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.pending.is_some() && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            app.pending.is_none(),
            "failed connection must finish asynchronously"
        );
        assert!(app.client.is_none());
        assert!(!edit_password(
            &mut app.password,
            KeyCode::KeyT,
            Some("retry"),
            false
        ));
        assert_eq!(app.password, "retry");
    }

    #[test]
    fn diagonal_movement_is_normalized_and_opposing_keys_cancel() {
        let mut input = Input::default();
        input.keys.extend([KeyCode::KeyW, KeyCode::KeyD]);
        let controller = input.controller();
        assert!((controller.move_dir.magnitude() - 1.0).abs() < 0.0001);
        assert!(controller.move_dir.x > 0.0 && controller.move_dir.y > 0.0);
        input.keys.extend([KeyCode::KeyS, KeyCode::KeyA]);
        assert_eq!(input.controller().move_dir, Vec2::zero());
    }

    #[test]
    fn mouse_look_turns_movement_and_rejects_nonfinite_motion() {
        let mut input = Input::default();
        input.keys.insert(KeyCode::KeyW);
        input.mouse(std::f64::consts::FRAC_PI_2 / 0.003, 0.0);
        assert!(input.controller().move_dir.x > 0.999);
        assert!(input.controller().move_dir.y.abs() < 0.001);
        let before = input.controller().look_dir;
        input.mouse(f64::NAN, f64::INFINITY);
        assert_eq!(input.controller().look_dir, before);
        input.mouse(0.0, 1e9);
        assert!(input.controller().look_dir.is_valid());
    }
}
