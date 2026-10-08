//! Connected-world control path for the admitted sky and figure renderer.
use crate::{
    Direction, GlobalState, PlayState, PlayStateResult,
    client::{Client, Event as ClientEvent},
    game_input::GameInput,
    hud::PersistedHudState,
    key_state::KeyState,
    render::{Drawer, GlobalsBindGroup, figure_preview::Preview},
    scene::camera::{Camera, CameraMode},
    settings::Settings,
    window::Event,
};
use common::{comp, event::UpdateCharacterMetadata, util::Dir};
use specs::WorldExt;
use std::{cell::RefCell, rc::Rc};
use vek::*;

pub struct SessionState {
    client: Rc<RefCell<Client>>,
    camera: Camera,
    keys: KeyState,
    preview: Preview,
    frames: u64,
}
impl SessionState {
    pub fn new(
        global: &mut GlobalState,
        _: UpdateCharacterMetadata,
        client: Rc<RefCell<Client>>,
        _: Rc<RefCell<PersistedHudState>>,
    ) -> Self {
        let size = global.window.window().surface_size();
        let mut camera = Camera::new(
            size.width as f32 / size.height.max(1) as f32,
            CameraMode::ThirdPerson,
        );
        camera.set_fov_deg(global.settings.graphics.fov);
        client
            .borrow_mut()
            .set_lod_distance(global.settings.graphics.lod_distance);
        client
            .borrow_mut()
            .request_player_physics(global.settings.networking.player_physics_behavior);
        client.borrow_mut().request_lossy_terrain_compression(
            global.settings.networking.lossy_terrain_compression,
        );
        tracing::info!(
            "Native world session entered; renderer=sky+figure camera=voxy controls=client-controller"
        );
        Self {
            client,
            camera,
            keys: Default::default(),
            preview: Default::default(),
            frames: 0,
        }
    }
}
impl PlayState for SessionState {
    fn enter(&mut self, global: &mut GlobalState, _: Direction) {
        global.window.grab_cursor(true);
        global.window.begin_scene_display();
        global.window.prepare_scene_display();
    }
    fn tick(&mut self, global: &mut GlobalState, events: Vec<Event>) -> PlayStateResult {
        for event in events {
            match event {
                Event::Close => return PlayStateResult::Shutdown,
                Event::Focused(false) => self.keys = KeyState::default(),
                Event::Focused(true) => global.window.grab_cursor(true),
                Event::Resize(size) => self
                    .camera
                    .set_aspect_ratio(size.x as f32 / size.y.max(1) as f32),
                Event::CursorPan(delta) => self
                    .camera
                    .rotate_by(Vec3::new(delta.x, delta.y, 0.0) * 0.005),
                Event::Zoom(delta) => self.camera.zoom_by(delta, None),
                Event::InputUpdate(input, pressed) => {
                    let action = match input {
                        GameInput::MoveForward => {
                            self.keys.up = pressed;
                            None
                        }
                        GameInput::MoveBack => {
                            self.keys.down = pressed;
                            None
                        }
                        GameInput::MoveLeft => {
                            self.keys.left = pressed;
                            None
                        }
                        GameInput::MoveRight => {
                            self.keys.right = pressed;
                            None
                        }
                        GameInput::SwimUp => {
                            self.keys.swim_up = pressed;
                            None
                        }
                        GameInput::SwimDown => {
                            self.keys.swim_down = pressed;
                            None
                        }
                        GameInput::Jump => Some(comp::InputKind::Jump),
                        GameInput::WallJump => Some(comp::InputKind::WallJump),
                        GameInput::Primary => Some(comp::InputKind::Primary),
                        GameInput::Secondary => Some(comp::InputKind::Secondary),
                        GameInput::Block => Some(comp::InputKind::Block),
                        GameInput::Roll => Some(comp::InputKind::Roll),
                        GameInput::Fly => Some(comp::InputKind::Fly),
                        GameInput::CycleCamera if pressed => {
                            self.camera.next_mode(false, false);
                            None
                        }
                        _ => None,
                    };
                    if let Some(action) = action {
                        self.client
                            .borrow_mut()
                            .handle_input(action, pressed, None, None);
                    }
                }
                _ => {}
            }
        }
        self.frames += 1;
        let observe = self.frames <= 3 || self.frames % 128 == 0;
        if observe {
            tracing::info!(frame = self.frames, "Native world phase=client-tick");
        }
        let axis = self.keys.dir_vec();
        let inputs = comp::ControllerInputs {
            move_dir: self.camera.right_xy() * axis.x + self.camera.forward_xy() * axis.y,
            move_z: self.keys.swim_up as u8 as f32 - self.keys.swim_down as u8 as f32,
            look_dir: Dir::from_unnormalized(self.camera.forward()).unwrap_or_default(),
            ..Default::default()
        };
        match self
            .client
            .borrow_mut()
            .tick(inputs, global.clock.game_dt())
        {
            Ok(events) => {
                if events
                    .iter()
                    .any(|event| matches!(event, ClientEvent::Disconnect))
                {
                    global.info_message = Some("Server disconnected".into());
                    return PlayStateResult::Pop;
                }
            }
            Err(error) => {
                tracing::error!(?error, "Native world client tick failed");
                global.info_message = Some(format!("World connection failed: {error:?}"));
                return PlayStateResult::Pop;
            }
        }
        if observe {
            tracing::info!(frame = self.frames, "Native world phase=camera");
        }
        let mut client = self.client.borrow_mut();
        let position = client
            .current::<comp::Pos>()
            .map_or(Vec3::zero(), |pos| pos.0);
        self.camera.set_focus_pos(position + Vec3::unit_z() * 1.5);
        self.camera.update(
            client.state().get_time(),
            global.clock.real_dt().as_secs_f32(),
            true,
        );
        self.camera.compute_dependents(&client.state().terrain());
        if observe {
            tracing::info!(
                frame = self.frames,
                ?position,
                "Native world phase=presentation"
            );
        }
        if global.window.prepare_scene_display() {
            let size = global.window.window().surface_size();
            let sun_z = client
                .state()
                .ecs()
                .read_resource::<common::resources::TimeOfDay>()
                .get_sun_dir()
                .z;
            let frame = match client.current::<comp::Body>() {
                Some(comp::Body::Humanoid(body))
                    if self.camera.get_mode() != CameraMode::FirstPerson =>
                {
                    let inventories = client.state().ecs().read_storage::<comp::Inventory>();
                    self.preview.frame(
                        body,
                        inventories.get(client.entity()),
                        Vec2::new(size.width, size.height),
                        client.state().get_time() as f32,
                        sun_z,
                    )
                }
                _ => Ok(None),
            };
            match frame {
                Ok(mut frame) => {
                    if let Some(frame) = frame.as_mut().and_then(std::sync::Arc::get_mut) {
                        let deps = self.camera.dependents();
                        frame.state[128..192].copy_from_slice(bytemuck::bytes_of(
                            &(deps.proj_mat * deps.view_mat).into_col_arrays(),
                        ));
                        let focus = self.camera.get_focus_pos();
                        frame.state[208..220].copy_from_slice(bytemuck::bytes_of(
                            &focus.map(f32::trunc).into_array(),
                        ));
                        frame.state[224..236].copy_from_slice(bytemuck::bytes_of(
                            &focus.map(f32::fract).into_array(),
                        ));
                        frame.state[640..652]
                            .copy_from_slice(bytemuck::bytes_of(&position.into_array()));
                        let ori = client.current::<comp::Ori>().unwrap_or_default();
                        let rotation = Mat4::from(ori.to_quat())
                            * Mat4::rotation_z(core::f32::consts::FRAC_PI_2);
                        for bytes in frame.state[672..].chunks_exact_mut(128) {
                            let mut bone: anim::FigureBoneData =
                                bytemuck::pod_read_unaligned(bytes);
                            bone.0 = (rotation * Mat4::from_col_arrays(bone.0)).into_col_arrays();
                            bone.1 = (rotation * Mat4::from_col_arrays(bone.1)).into_col_arrays();
                            bytes.copy_from_slice(bytemuck::bytes_of(&bone));
                        }
                    }
                    if let Err(error) = global.window.present_character_scene(sun_z, frame) {
                        tracing::error!(%error, "Native world presentation failed");
                    }
                }
                Err(error) => tracing::error!(%error, "Native world avatar preparation failed"),
            }
        }
        client.cleanup();
        if observe {
            tracing::info!(frame = self.frames, "Native world phase=complete");
        }
        PlayStateResult::Continue
    }
    fn name(&self) -> &'static str {
        "Native World"
    }
    fn capped_fps(&self) -> bool {
        true
    }
    fn uses_native_ui(&self) -> bool {
        true
    }
    fn globals_bind_group(&self) -> &GlobalsBindGroup {
        unreachable!("native world publishes through UI4")
    }
    fn render(&self, _: &mut Drawer<'_>, _: &Settings) {}
}
