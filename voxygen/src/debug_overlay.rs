//! Dormant state and actions retained for a future debug overlay.
//! No UI backend or event/render integration is attached.
use crate::{
    render::ExperimentalShader,
    scene::{DebugShape, DebugShapeId, Scene},
    session::settings_change::{Graphics, SettingsChange},
    settings::Settings,
};
use alloc::{boxed::Box, string::String, vec::Vec};
use client::Client;
use common::{cmd::ServerChatCommand, comp::Body};
use core::time::Duration;

pub struct SelectedEntityInfo {
    entity_id: u32,
    debug_shape_id: Option<u64>,
    character_state_history: Vec<String>,
}

impl SelectedEntityInfo {
    fn new(entity_id: u32) -> Self {
        Self {
            entity_id,
            debug_shape_id: None,
            character_state_history: Vec::new(),
        }
    }
}

pub struct AdminCommandState {
    give_item_qty: u32,
    give_item_selected_idx: usize,
    give_item_search_text: String,
    kits_selected_idx: usize,
    spawn_entity_qty: u32,
    spawn_entity_selected_idx: usize,
    spawn_entity_search_text: String,
}

impl AdminCommandState {
    fn new() -> Self {
        Self {
            give_item_qty: 1,
            give_item_selected_idx: 0,
            give_item_search_text: String::new(),
            kits_selected_idx: 0,
            spawn_entity_qty: 1,
            spawn_entity_selected_idx: 0,
            spawn_entity_search_text: String::new(),
        }
    }
}

pub struct DebugSnapshot {
    pub frame_time: Duration,
    pub ping_ms: f64,
}

pub struct DebugOverlayState {
    selected_entity_info: Option<SelectedEntityInfo>,
    admin_command_state: AdminCommandState,
    max_entity_distance: f32,
    selected_entity_cylinder_height: f32,
    frame_times: Vec<f32>,
    windows: DebugWindows,
    debug_vectors_enabled: bool,
    new_debug_shape_id: Option<u64>,
}

#[derive(Clone, Default)]
pub struct DebugWindows {
    admin_commands: bool,
    frame_time: bool,
    ecs_entities: bool,
    experimental_shaders: bool,
}

impl Default for DebugOverlayState {
    fn default() -> Self {
        Self {
            admin_command_state: AdminCommandState::new(),
            selected_entity_info: None,
            max_entity_distance: 100000.0,
            selected_entity_cylinder_height: 10.0,
            frame_times: Vec::new(),
            windows: DebugWindows::default(),
            debug_vectors_enabled: false,
            new_debug_shape_id: None,
        }
    }
}

pub enum DebugShapeAction {
    AddCylinder {
        radius: f32,
        height: f32,
    },
    RemoveShape(u64),
    SetPosAndColor {
        id: u64,
        pos: [f32; 4],
        color: [f32; 4],
    },
}

pub enum DebugAction {
    ChatCommand {
        cmd: ServerChatCommand,
        args: Vec<String>,
    },
    DebugShape(DebugShapeAction),
    SetExperimentalShader(String, bool),
    SetShowDebugVector(bool),
}

#[derive(Default)]
pub struct DebugActions {
    pub actions: Vec<DebugAction>,
}

fn body_species(body: &Body) -> String {
    match body {
        Body::Humanoid(body) => format!("{:?}", body.species),
        Body::QuadrupedSmall(body) => format!("{:?}", body.species),
        Body::QuadrupedMedium(body) => format!("{:?}", body.species),
        Body::BirdMedium(body) => format!("{:?}", body.species),
        Body::FishMedium(body) => format!("{:?}", body.species),
        Body::Dragon(body) => format!("{:?}", body.species),
        Body::BirdLarge(body) => format!("{:?}", body.species),
        Body::FishSmall(body) => format!("{:?}", body.species),
        Body::BipedLarge(body) => format!("{:?}", body.species),
        Body::BipedSmall(body) => format!("{:?}", body.species),
        Body::Object(body) => format!("{:?}", body),
        Body::Item(body) => format!("{:?}", body),
        Body::Golem(body) => format!("{:?}", body.species),
        Body::Theropod(body) => format!("{:?}", body.species),
        Body::QuadrupedLow(body) => format!("{:?}", body.species),
        Body::Arthropod(body) => format!("{:?}", body.species),
        Body::Ship(body) => format!("{:?}", body),
        Body::Crustacean(body) => format!("{:?}", body.species),
        Body::Plugin(body) => format!("{:?}", body),
    }
}

impl DebugOverlayState {
    pub fn apply_actions(
        &mut self,
        client: &mut Client,
        scene: &mut Scene,
        settings: &Settings,
        actions: DebugActions,
    ) -> Option<SettingsChange> {
        let mut new_render_mode = None;

        actions.actions.into_iter().for_each(|action| match action {
            DebugAction::ChatCommand { cmd, args } => {
                client.send_command(cmd.keyword().into(), args);
            },
            DebugAction::DebugShape(debug_shape_action) => match debug_shape_action {
                DebugShapeAction::AddCylinder { height, radius } => {
                    let shape_id = scene
                        .debug
                        .add_shape(DebugShape::Cylinder { height, radius });
                    self.new_debug_shape_id = Some(shape_id.0);
                },
                DebugShapeAction::RemoveShape(debug_shape_id) => {
                    scene.debug.remove_shape(DebugShapeId(debug_shape_id));
                },
                DebugShapeAction::SetPosAndColor { id, pos, color } => {
                    let identity_ori = [0.0, 0.0, 0.0, 1.0];
                    scene
                        .debug
                        .set_context(DebugShapeId(id), pos, color, identity_ori);
                },
            },
            DebugAction::SetExperimentalShader(shader, enabled) => {
                if let Ok(shader) = ExperimentalShader::try_from(shader.as_str()) {
                    let shaders = &mut new_render_mode
                        .get_or_insert_with(|| settings.graphics.render_mode.clone())
                        .experimental_shaders;

                    if enabled {
                        shaders.insert(shader);
                    } else {
                        shaders.remove(&shader);
                    }
                }
            },
            DebugAction::SetShowDebugVector(enabled) => {
                scene.debug_vectors_enabled = enabled;
            },
        });

        new_render_mode.map(|rm| SettingsChange::Graphics(Graphics::ChangeRenderMode(Box::new(rm))))
    }
}
