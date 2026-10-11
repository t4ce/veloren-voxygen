//! Stateful world poses using the same authored evaluator as FigureMgr.
use crate::scene::figure::character_pose;
use anim::character::{CharacterSkeleton, SkeletonAttr};
use common::{
    comp::{
        self,
        inventory::slot::EquipSlot,
        item::{Hands, ItemKind, armor::ArmorKind},
        slot::ArmorSlot,
    },
    link::Is,
    mounting::VolumeRider,
    util::Dir,
};
use vek::*;

pub struct Inputs<'a> {
    pub character: &'a comp::CharacterState,
    pub physics: &'a comp::PhysicsState,
    pub velocity: Vec3<f32>,
    pub scale: f32,
    pub look_direction: Dir,
    pub activity: Option<&'a comp::CharacterActivity>,
    pub volume_rider: Option<&'a Is<VolumeRider>>,
    pub riding: bool,
    pub interaction_position: Option<Vec3<f32>>,
    pub has_light: bool,
    pub dt: f32,
}

pub struct State {
    skeleton: CharacterSkeleton,
    body: Option<comp::humanoid::Body>,
    character: Option<core::mem::Discriminant<comp::CharacterState>>,
    last_orientation: Quaternion<f32>,
    last_position: Option<Vec3<f32>>,
    avg_velocity: Vec3<f32>,
    state_time: f32,
    accumulated_velocity: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            skeleton: CharacterSkeleton::new(false, 0.0, 1.0),
            body: None,
            character: None,
            last_orientation: Quaternion::identity(),
            last_position: None,
            avg_velocity: Vec3::zero(),
            state_time: 0.0,
            accumulated_velocity: 0.0,
        }
    }
}
impl State {
    pub fn update(
        &mut self,
        body: comp::humanoid::Body,
        inventory: Option<&comp::Inventory>,
        position: Vec3<f32>,
        orientation: Quaternion<f32>,
        time: f32,
        input: Inputs<'_>,
    ) -> [anim::FigureBoneData; anim::MAX_BONE_COUNT] {
        if self.body != Some(body) {
            *self = Self::default();
            self.body = Some(body);
            self.last_orientation = orientation;
        }
        let dt = input.dt.max(0.0);
        let scale = if input.scale.is_finite() && input.scale > 0.0 {
            input.scale
        } else {
            1.0
        };
        if self.character != Some(core::mem::discriminant(input.character)) {
            self.state_time = 0.0;
        }
        let rel_vel = (input.velocity - input.physics.ground_vel) / scale;
        let rel_avg_vel = (self.avg_velocity - input.physics.ground_vel) / scale;
        let hands = |slot| {
            inventory.and_then(|i| i.equipped(slot)).and_then(|item| {
                if let ItemKind::Tool(tool) = &*item.kind() {
                    Some(tool.hands)
                } else {
                    None
                }
            })
        };
        let holding_lantern = inventory.is_some_and(|i| i.equipped(EquipSlot::Lantern).is_some())
            && input.has_light
            && ((hands(EquipSlot::ActiveOffhand).is_none()
                && hands(EquipSlot::ActiveMainhand) == Some(Hands::One))
                || !input.character.is_wield())
            && !input.character.is_using_hands()
            && input.physics.in_liquid().is_none()
            && input.volume_rider.is_none_or(|rider| {
                !matches!(
                    rider.block.get_sprite(),
                    Some(common::terrain::SpriteKind::Helm)
                )
            });
        let back_carry_offset = inventory
            .and_then(|i| i.equipped(EquipSlot::Armor(ArmorSlot::Back)))
            .and_then(|item| {
                if let ItemKind::Armor(armor) = &*item.kind() {
                    match armor.kind {
                        ArmorKind::Backpack => Some(4.0),
                        ArmorKind::Back => Some(1.5),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        let forward = orientation * Vec3::unit_y();
        let theta = input.velocity.y.atan2(input.velocity.x) - forward.y.atan2(forward.x);
        let (target, rate) = character_pose::evaluate(
            character_pose::Inputs {
                character: input.character,
                inventory,
                physics: input.physics,
                character_activity: input.activity,
                is_volume_rider: input.volume_rider,
                is_riding: input.riding,
                position,
                interaction_position: input.interaction_position,
                ori: orientation,
                last_orientation: self.last_orientation,
                look_direction: input.look_direction,
                rel_vel,
                rel_avg_vel,
                move_dir: Vec2::unit_y().rotated_z(theta),
                time,
                state_time: self.state_time,
                acc_vel: self.accumulated_velocity,
                squash: 1.0,
                holding_lantern,
                back_carry_offset,
            },
            &SkeletonAttr::from(&body),
        );
        self.skeleton = Lerp::lerp(&self.skeleton, &target, (15.0 * dt).min(1.0));
        self.state_time += dt * rate / scale;
        let smoothing = 0.1f32.powf(dt);
        if dt > 0.0
            && let Some(last) = self.last_position
        {
            self.avg_velocity =
                smoothing * self.avg_velocity + (1.0 - smoothing) * (position - last) / dt;
        }
        self.last_position = Some(position);
        if self.avg_velocity.magnitude_squared() != 0.0 {
            self.accumulated_velocity += (self.avg_velocity - input.physics.ground_vel)
                .magnitude()
                .powf(0.65)
                * 2.95
                * dt
                / scale;
        } else {
            self.accumulated_velocity = 0.0;
        }
        self.last_orientation = orientation;
        self.character = Some(core::mem::discriminant(input.character));
        let mut bones = [anim::FigureBoneData::default(); anim::MAX_BONE_COUNT];
        anim::compute_matrices(
            &self.skeleton,
            anim::vek::Mat4::from(orientation)
                * anim::vek::Mat4::scaling_3d(Vec3::broadcast(scale)),
            &mut bones,
            body,
        );
        bones
    }
}
