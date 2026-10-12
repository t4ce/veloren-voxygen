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
use specs::{Join, WorldExt};
use std::{cell::RefCell, rc::Rc};
use vek::*;

// Temporary isolation run: retain terrain and animate sky without in-world
// figure preparation/submission. Character selection has its own render path.
const DRAW_WORLD_FIGURES: bool = false;

pub struct SessionState {
    client: Rc<RefCell<Client>>,
    camera: Camera,
    zoom: zoom::Zoom,
    keys: KeyState,
    terrain: crate::terrain_preview::radial::Window,
    composition: crate::render::terrain_composition::Composition,
    figures: std::collections::HashMap<specs::Entity, crate::render::figure_preview::Preview>,
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
            world_figures = DRAW_WORLD_FIGURES,
            "Native world session entered; renderer=flat-clouds+near-terrain conrod=guarded camera=voxy controls=client-controller"
        );
        Self {
            client,
            camera,
            zoom: zoom::Zoom::new(),
            keys: Default::default(),
            terrain: crate::terrain_preview::radial::Window::new(),
            composition: crate::render::terrain_composition::Composition::new(),
            figures: std::collections::HashMap::new(),
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
        let frame_started = std::time::Instant::now();
        for event in events {
            match event {
                Event::Close => return PlayStateResult::Shutdown,
                Event::Focused(false) => self.keys = KeyState::default(),
                Event::Focused(true) => global.window.grab_cursor(true),
                Event::Resize(size) => {
                    self.camera.set_aspect_ratio(size.x as f32 / size.y.max(1) as f32);
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
        let observe = self.frames <= 256 || self.frames % 128 == 0;
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
        let height = client.current::<comp::Body>().map_or(2.0, |body| match body {
            comp::Body::Humanoid(body) => body.height(),
            _ => 2.0,
        });
        let chunk = common::terrain::TerrainGrid::chunk_size().map(|v| v as f32);
        let world = client.world_data();
        let world_size = world.chunk_size().map(|v| v as f32) * chunk;
        self.zoom.set_overviews(chunk.into_array(), world_size.into_array(),
            world.max_chunk_alt().abs(), self.camera.get_effective_fov(), self.camera.get_aspect_ratio());
        let zoom_distance = self.zoom.distance();
        let focus = match self.zoom.stop() {
            zoom::ZoomStop::World => Vec3::new(world_size.x * 0.5, world_size.y * 0.5, world.max_chunk_alt() * 0.5),
            zoom::ZoomStop::Far | zoom::ZoomStop::VeryFar => Vec3::new(
                (position.x / chunk.x).floor() * chunk.x + chunk.x * 0.5,
                (position.y / chunk.y).floor() * chunk.y + chunk.y * 0.5,
                position.z + self.zoom.focus_height(height)),
            _ => position + Vec3::unit_z() * self.zoom.focus_height(height),
        };
        self.camera.set_focus_pos(focus);
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
            let synced = client.terrain_snapshot_ready() && client
                .position()
                .filter(|position| {
                    position.into_array().iter().all(|value| value.is_finite())
                })
                .is_some();
            let prepared = self.terrain.prepare(synced.then_some(&*client));
            let metrics = crate::render::terrain_layers::Metrics {
                near_warm_us: prepared.warm_us,
                stream_run_us: client.terrain_stream_run_us(),
                received_at: None, width: size.width, height: size.height,
                warm_failures: prepared.failures,
                near_chunks: prepared.chunks,
                loaded_chunks: client.state().terrain().iter().filter(|(key, _)|
                    client.state().terrain().contains_key_real(*key)).count() as u32,
                ..Default::default()
            };
            // Near-only needs no composition copy: the worker publishes an immutable Arc.
            let mut metrics = metrics;
            metrics.near_revision = prepared.revision;
            metrics.near_vertices = prepared.geometry.as_ref().map_or(0, |g| g.vertices.len() as u32);
            let composed = prepared.geometry.map(|geometry| (geometry, metrics));
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
            let mut entities = Vec::new();
            if observe { tracing::info!(frame = self.frames, "Native world phase=figures"); }
            if terrain.is_some() && DRAW_WORLD_FIGURES {
                let focus = self.camera.get_focus_pos();
                let projection = crate::scene::camera::perspective_rh_zo_general(
                    self.camera.get_effective_fov(), size.width as f32 / size.height.max(1) as f32,
                    1.0 / 0.1, 1.0 / self.zoom.far_plane(),
                );
                let ecs = client.state().ecs();
                let positions = ecs.read_storage::<comp::Pos>();
                let bodies = ecs.read_storage::<comp::Body>();
                let inventories = ecs.read_storage::<comp::Inventory>();
                let orientations = ecs.read_storage::<comp::Ori>();
                let velocities = ecs.read_storage::<comp::Vel>();
                let scales = ecs.read_storage::<comp::Scale>();
                let physics = ecs.read_storage::<comp::PhysicsState>();
                let characters = ecs.read_storage::<comp::CharacterState>();
                let activities = ecs.read_storage::<comp::CharacterActivity>();
                let controllers = ecs.read_storage::<comp::Controller>();
                let lights = ecs.read_storage::<comp::LightEmitter>();
                let riders = ecs.read_storage::<common::link::Is<common::mounting::Rider>>();
                let volume_riders = ecs.read_storage::<common::link::Is<common::mounting::VolumeRider>>();
                let id_maps = ecs.read_resource::<common::uid::IdMaps>();
                let default_character = comp::CharacterState::default();
                let default_physics = comp::PhysicsState::default();
                let mut visible = std::collections::HashSet::new();
                for (entity, pos, body) in (&ecs.entities(), &positions, &bodies).join() {
                    if (pos.0 - position).magnitude_squared() > 128.0 * 128.0 { continue; }
                    let comp::Body::Humanoid(body) = body else { continue; };
                    if visible.len() >= 128 { break; }
                    visible.insert(entity);
                    let first_person = entity == client.entity() && self.zoom.hide_body();
                    // An unsynced inventory cannot identify a separate equipment
                    // mesh yet; omit the local body until equipment arrives.
                    if first_person && inventories.get(entity).is_none() { continue; }
                    let character = characters.get(entity).unwrap_or(&default_character);
                    let activity = activities.get(entity);
                    let orientation = orientations.get(entity).copied().unwrap_or_default();
                    let look_direction = activity.and_then(|a| a.look_dir)
                        .or_else(|| controllers.get(entity).map(|c| c.inputs.look_dir))
                        .unwrap_or_else(|| orientation.look_dir());
                    let interaction_position = if let comp::CharacterState::Interact(state) = character {
                        if let common::states::interact::InteractKind::Entity { target, .. } = state.static_data.interact {
                            id_maps.uid_entity(target).and_then(|target| positions.get(target)).map(|p| p.0)
                        } else { None }
                    } else { None };
                    let preview = self.figures.entry(entity).or_default();
                    match preview.world_frame(*body, inventories.get(entity),
                        Vec2::new(size.width, size.height), client.state().get_time() as f32, sun_z,
                        pos.0, orientations.get(entity).map_or(Quaternion::identity(), |ori| ori.to_quat()),
                        projection * deps.view_mat, focus, first_person,
                        crate::render::world_animation::Inputs {
                            character, physics: physics.get(entity).unwrap_or(&default_physics),
                            velocity: velocities.get(entity).map_or(Vec3::zero(), |vel| vel.0),
                            scale: scales.get(entity).map_or(1.0, |scale| scale.0), look_direction,
                            activity, volume_rider: volume_riders.get(entity),
                            riding: riders.get(entity).is_some() || volume_riders.get(entity).is_some(),
                            interaction_position, has_light: lights.get(entity).is_some(),
                            dt: global.clock.game_dt().as_secs_f32(),
                        }) {
                        Ok(Some(frame)) => entities.push(frame),
                        Ok(None) => {},
                        Err(error) => tracing::warn!(%error, "World avatar preparation failed"),
                    }
                }
                self.figures.retain(|entity, _| visible.contains(entity));
            }
            if observe { tracing::info!(frame = self.frames, entities = entities.len(), "Native world phase=publish-request"); }
            if let Err(error) = global.window.present_terrain_scene(sun_z, terrain, clouds, entities) {
                tracing::error!(%error, "Native world presentation failed");
            }
        }
        if observe { tracing::info!(frame = self.frames, "Native world phase=cleanup"); }
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
