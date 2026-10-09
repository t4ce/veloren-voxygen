//! Connected-world control path for the admitted sky and terrain renderer.
#[path = "native_zoom.rs"]
mod zoom;
use crate::{
    Direction, GlobalState, PlayState, PlayStateResult,
    client::{Client, Event as ClientEvent},
    game_input::GameInput,
    hud::PersistedHudState,
    key_state::KeyState,
    render::{Drawer, GlobalsBindGroup},
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
    zoom: zoom::Zoom,
    terrain_height: Option<(f32, f32)>,
    keys: KeyState,
    terrain: crate::terrain_preview::Scene,
    composition: crate::render::terrain_composition::Composition,
    geometry: Option<(
        u64,
        std::sync::Arc<crate::render::terrain_feature::Geometry>,
    )>,
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
            "Native world session entered; renderer=flat-clouds+terrain avatar=guarded conrod=guarded camera=voxy controls=client-controller"
        );
        Self {
            client,
            camera,
            zoom: zoom::Zoom::new(10.0),
            terrain_height: None,
            keys: Default::default(),
            terrain: crate::terrain_preview::Scene::new(),
            composition: crate::render::terrain_composition::Composition::new(),
            geometry: None,
            frames: 0,
        }
    }
    fn refresh_zoom_limit(&mut self) {
        let size = common::terrain::TerrainGrid::chunk_size();
        self.zoom.set_limit(zoom::fit_distance(
            [size.x as f32, size.y as f32],
            self.terrain_height, self.camera.get_effective_fov(),
            self.camera.get_aspect_ratio(),
        ));
    }

}
impl PlayState for SessionState {
    fn enter(&mut self, global: &mut GlobalState, _: Direction) {
        global.window.grab_cursor(true);
        global.window.begin_scene_display();
        global.window.prepare_scene_display();
    }
    fn tick(&mut self, global: &mut GlobalState, events: Vec<Event>) -> PlayStateResult {
        let frame_started = std::time::Instant::now();
        self.refresh_zoom_limit();
        for event in events {
            match event {
                Event::Close => return PlayStateResult::Shutdown,
                Event::Focused(false) => self.keys = KeyState::default(),
                Event::Focused(true) => global.window.grab_cursor(true),
                Event::Resize(size) => {
                    self.camera.set_aspect_ratio(size.x as f32 / size.y.max(1) as f32);
                    self.refresh_zoom_limit();
                    // UI4 commits a paired resize only after both producers
                    // publish new backing frames. The guarded HUD must still
                    // publish its transparent foreground at the new extent.
                    global.window.begin_scene_display();
                }
                Event::CursorPan(delta) => self
                    .camera
                    .rotate_by(Vec3::new(delta.x, delta.y, 0.0) * 0.005),
                Event::Zoom(delta) => {
                    self.zoom.scroll(delta);
                }
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
                            self.zoom.toggle();
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
        let zoom_distance = self.zoom.advance(global.clock.real_dt().as_secs_f32());
        self.camera.set_distance_continuous(zoom_distance);
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
            // World position and its containing chunk must have arrived before
            // requesting a mesh; waiting frames retain just the sky.
            let synced = client
                .position()
                .filter(|position| {
                    position.into_array().iter().all(|value| value.is_finite())
                        && client.state().terrain().contains_key_real(
                            common::terrain::TerrainGrid::chunk_key(
                                position.map(|v| v.floor() as i32),
                            ),
                        )
                })
                .is_some();
            let prepared = self.terrain.prepare_terrain(
                synced.then_some(&*client),
                size.width.max(1),
                size.height.max(1),
            );
            if self.geometry.as_ref().is_none_or(|(revision, _)| *revision != prepared.revision) {
                self.terrain_height = prepared.terrain.iter().map(|v| v.position[2])
                    .fold(None, |range, z| Some(range.map_or((z, z),
                        |(low, high): (f32, f32)| (low.min(z), high.max(z)))));
                self.geometry = Some((prepared.revision, std::sync::Arc::new(
                    crate::render::terrain_feature::Geometry {
                        vertices: prepared.terrain.into(), atlas: prepared.atlas.into(),
                    })));
            }
            let coverage = prepared.coverage.and_then(|[min, end]|
                crate::render::terrain_layers::Coverage::new(min, end));
            let chunks = common::terrain::TerrainGrid::chunk_size();
            let metrics = crate::render::terrain_layers::Metrics {
                near_warm_us: prepared.warm_us,
                stream_run_us: client.terrain_stream_run_us(),
                received_at: prepared.received_at, width: size.width, height: size.height,
                warm_failures: prepared.worker_failures,
                near_chunks: coverage.map_or(0, |c| c.chunks(chunks.into_array())),
                loaded_chunks: client.state().terrain().iter().filter(|(key, _)|
                    client.state().terrain().contains_key_real(*key)).count() as u32,
                ..Default::default()
            };
            let near = self.geometry.as_ref().map(|(_, geometry)| geometry.clone());
            let composed = self.composition.prepare(&client, near, prepared.revision, coverage, metrics);
            let terrain = composed.and_then(|(geometry, metrics)| {
                if geometry.vertices.is_empty() { return None; }
                if client.position().is_none_or(|pos|
                    pos.into_array().iter().any(|value| !value.is_finite())) { return None; }
                let deps = self.camera.dependents();
                let mut camera = crate::render::terrain_feature::camera_from_view(
                    deps.view_mat_inv, self.camera.get_focus_pos(), self.camera.get_effective_fov(),
                    size.width as f32 / size.height.max(1) as f32);
                camera[4][3] = if metrics.mode == crate::render::terrain_layers::Mode::Near {
                    self.zoom.far_plane()
                } else { self.composition.far_plane(&client) };
                Some(std::sync::Arc::new(crate::render::terrain_feature::Frame {
                    geometry, camera, metrics, prepared_at: frame_started,
                }))
            });
            let deps = self.camera.dependents();
            let cloud_camera = crate::render::terrain_feature::camera_from_view(
                deps.view_mat_inv, self.camera.get_focus_pos(), self.camera.get_effective_fov(),
                size.width as f32 / size.height.max(1) as f32,
            );
            let clouds = crate::render::flat_cloud_native::from_client(
                &client, cloud_camera, global.settings.graphics.ambiance, true,
            ).map(std::sync::Arc::new);
            if let Err(error) = global.window.present_terrain_scene(sun_z, terrain, clouds) {
                tracing::error!(%error, "Native world presentation failed");
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
