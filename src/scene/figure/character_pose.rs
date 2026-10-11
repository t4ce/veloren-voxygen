//! Shared authored humanoid locomotion and action pose evaluation.
use super::*;

pub(crate) struct Inputs<'a> {
    pub character: &'a CharacterState,
    pub inventory: Option<&'a Inventory>,
    pub physics: &'a PhysicsState,
    pub character_activity: Option<&'a CharacterActivity>,
    pub is_volume_rider: Option<&'a Is<VolumeRider>>,
    pub is_riding: bool,
    pub position: Vec3<f32>,
    pub interaction_position: Option<Vec3<f32>>,
    pub ori: anim::vek::Quaternion<f32>,
    pub last_orientation: anim::vek::Quaternion<f32>,
    pub look_direction: Dir,
    pub rel_vel: anim::vek::Vec3<f32>,
    pub rel_avg_vel: anim::vek::Vec3<f32>,
    pub move_dir: anim::vek::Vec2<f32>,
    pub time: f32,
    pub state_time: f32,
    pub acc_vel: f32,
    pub squash: f32,
    pub holding_lantern: bool,
    pub back_carry_offset: f32,
}

pub(crate) fn evaluate(
    input: Inputs<'_>,
    skeleton_attr: &anim::character::SkeletonAttr,
) -> (CharacterSkeleton, f32) {
    let Inputs {
        character,
        inventory,
        physics,
        character_activity,
        is_volume_rider,
        is_riding,
        position,
        interaction_position,
        ori,
        last_orientation,
        look_direction,
        rel_vel,
        rel_avg_vel,
        move_dir,
        time,
        state_time,
        acc_vel,
        squash,
        holding_lantern,
        back_carry_offset,
    } = input;
    let pos = (position,);
    let look_dir = look_direction;
    let orientation = ori * anim::vek::Vec3::<f32>::unit_y();
    let last_ori = last_orientation * anim::vek::Vec3::<f32>::unit_y();
    let wall_dir = physics.on_wall;
    let mut state_animation_rate = 1.0;
    let tool_info = |equip_slot| {
        inventory
            .and_then(|i| i.equipped(equip_slot))
            .map(|i| {
                if let ItemKind::Tool(tool) = &*i.kind() {
                    (Some(tool.kind), Some(tool.hands), i.ability_spec())
                } else {
                    (None, None, None)
                }
            })
            .unwrap_or((None, None, None))
    };
    let (active_tool_kind, active_tool_hand, active_tool_spec) =
        tool_info(EquipSlot::ActiveMainhand);
    let active_tool_spec = active_tool_spec.as_deref();
    let (second_tool_kind, second_tool_hand, _) = tool_info(EquipSlot::ActiveOffhand);
    let hands = (active_tool_hand, second_tool_hand);
    let ability_id = character
        .ability_info()
        .and_then(|a| a.ability)
        .and_then(|a| a.ability_id(Some(character), inventory));
    let target_base = match (
        physics.on_ground.is_some(),
        rel_vel.magnitude_squared() > 0.1, // Moving
        physics.in_liquid().is_some(),     // In water
        is_riding,
        physics.skating_active,
    ) {
        // Standing or Skating
        (true, false, false, false, _) | (_, _, false, false, true) => {
            anim::character::StandAnimation::update_skeleton(
                &CharacterSkeleton::new(holding_lantern, back_carry_offset, squash),
                (
                    active_tool_kind,
                    second_tool_kind,
                    hands,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                    look_dir,
                    time,
                    rel_avg_vel,
                ),
                state_time,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        // Running
        (true, true, false, false, _) => {
            anim::character::RunAnimation::update_skeleton(
                &CharacterSkeleton::new(holding_lantern, back_carry_offset, squash),
                (
                    active_tool_kind,
                    second_tool_kind,
                    hands,
                    rel_vel,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                    *look_dir,
                    time,
                    rel_avg_vel,
                    acc_vel,
                    wall_dir,
                ),
                state_time,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        // In air
        (false, _, false, false, _) => {
            anim::character::JumpAnimation::update_skeleton(
                &CharacterSkeleton::new(holding_lantern, back_carry_offset, squash),
                (
                    active_tool_kind,
                    second_tool_kind,
                    hands,
                    rel_vel,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                    look_dir,
                    time,
                ),
                state_time,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        // Swim
        (_, _, true, false, _) => anim::character::SwimAnimation::update_skeleton(
            &CharacterSkeleton::new(holding_lantern, back_carry_offset, squash),
            (
                active_tool_kind,
                second_tool_kind,
                hands,
                rel_vel,
                // TODO: Update to use the quaternion.
                ori * anim::vek::Vec3::<f32>::unit_y(),
                last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                time,
                rel_avg_vel,
            ),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        // Mount
        (_, _, _, true, _) => {
            let base = anim::character::MountAnimation::update_skeleton(
                &CharacterSkeleton::new(holding_lantern, back_carry_offset, squash),
                (
                    active_tool_kind,
                    second_tool_kind,
                    hands,
                    time,
                    rel_vel,
                    rel_avg_vel,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                    *look_dir,
                ),
                state_time,
                &mut state_animation_rate,
                skeleton_attr,
            );
            if let Some(is_volume_rider) = is_volume_rider
                && let Some(sprite) = is_volume_rider.block.get_sprite()
            {
                match sprite {
                    _ if sprite.is_controller() => {
                        anim::character::SteerAnimation::update_skeleton(
                            &base,
                            (
                                active_tool_kind,
                                second_tool_kind,
                                character_activity.map(|a| a.steer_dir).unwrap_or(0.0),
                                time,
                            ),
                            state_time,
                            &mut state_animation_rate,
                            skeleton_attr,
                        )
                    }
                    _ if sprite.is_bed() => anim::character::SleepAnimation::update_skeleton(
                        &base,
                        (active_tool_kind, second_tool_kind, time),
                        state_time,
                        &mut state_animation_rate,
                        skeleton_attr,
                    ),
                    _ => anim::character::SitAnimation::update_skeleton(
                        &base,
                        (
                            active_tool_kind,
                            second_tool_kind,
                            last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                            look_dir,
                            time,
                        ),
                        state_time,
                        &mut state_animation_rate,
                        skeleton_attr,
                    ),
                }
            } else {
                base
            }
        }
    };
    let target_bones = match &character {
        CharacterState::Roll(s) => {
            let stage_time = s.timer.as_secs_f32();
            let wield_status = s.was_wielded;
            let stage_progress = match s.stage_section {
                StageSection::Buildup => stage_time / s.static_data.buildup_duration.as_secs_f32(),
                StageSection::Movement => {
                    stage_time / s.static_data.movement_duration.as_secs_f32()
                }
                StageSection::Recover => stage_time / s.static_data.recover_duration.as_secs_f32(),
                _ => 0.0,
            };
            anim::character::RollAnimation::update_skeleton(
                &target_base,
                (
                    active_tool_kind,
                    second_tool_kind,
                    hands,
                    wield_status,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                    time,
                    Some(s.stage_section),
                    s.prev_aimed_dir,
                ),
                stage_progress,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::Throw(s) => {
            let timer = character.timer();
            let stage_section = character.stage_section();
            let durations = character.durations();
            let progress = if let Some(((timer, stage_section), durations)) =
                timer.zip(stage_section).zip(durations)
            {
                let base_dur = match stage_section {
                    StageSection::Buildup => durations.buildup,
                    StageSection::Charge => durations.charge,
                    StageSection::Movement => None,
                    StageSection::Action => durations.action,
                    StageSection::Recover => durations.recover,
                };
                if let Some(base_dur) = base_dur {
                    timer.as_secs_f32() / base_dur.as_secs_f32()
                } else {
                    timer.as_secs_f32()
                }
            } else {
                0.0
            };

            anim::character::ThrowAnimation::update_skeleton(
                &target_base,
                (
                    stage_section,
                    s.static_data.tool_kind,
                    s.static_data.hand_info,
                ),
                progress,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::BasicMelee(_)
        | CharacterState::FinisherMelee(_)
        | CharacterState::DiveMelee(_)
        | CharacterState::SelfBuff(_)
        | CharacterState::ChargedRanged(_)
        | CharacterState::BasicRanged(_)
        | CharacterState::ChargedMelee(_)
        | CharacterState::DashMelee(_)
        | CharacterState::Shockwave(_)
        | CharacterState::BasicAura(_)
        | CharacterState::StaticAura(_)
        | CharacterState::BasicBeam(_)
        | CharacterState::BasicBlock(_)
        | CharacterState::RiposteMelee(_)
        | CharacterState::LeapRanged(_)
        | CharacterState::Simple(_) => {
            let timer = character.timer();
            let stage_section = character.stage_section();
            let durations = character.durations();
            let progress = if let Some(((timer, stage_section), durations)) =
                timer.zip(stage_section).zip(durations)
            {
                let base_dur = match stage_section {
                    StageSection::Buildup => durations.buildup,
                    StageSection::Charge => {
                        if matches!(character, CharacterState::DashMelee(_)) {
                            None
                        } else {
                            durations.charge
                        }
                    }
                    StageSection::Movement => {
                        if matches!(character, CharacterState::DiveMelee(_)) {
                            None
                        } else {
                            durations.movement
                        }
                    }
                    StageSection::Action => {
                        if matches!(
                            character,
                            CharacterState::BasicBeam(_) | CharacterState::BasicBlock(_)
                        ) {
                            None
                        } else {
                            durations.action
                        }
                    }
                    StageSection::Recover => durations.recover,
                };
                if let Some(base_dur) = base_dur {
                    checked_div(timer.as_secs_f32(), base_dur.as_secs_f32()).unwrap_or(0.0)
                } else {
                    timer.as_secs_f32()
                }
            } else {
                0.0
            };

            let look_dir_override = if let CharacterState::BasicRanged(c) = &character {
                if c.static_data.auto_aim
                    && let Some(sel_pos) = c
                        .static_data
                        .ability_info
                        .input_attr
                        .and_then(|ia| ia.select_pos)
                {
                    comp::projectile::aim_projectile(
                        c.static_data.projectile_speed,
                        pos.0,
                        sel_pos,
                        true,
                    )
                } else {
                    None
                }
            } else {
                None
            };

            anim::character::BasicAction::update_skeleton(
                &target_base,
                anim::character::BasicActionDependency {
                    ability_id,
                    hands,
                    stage_section,
                    ability_info: character.ability_info(),
                    velocity: rel_vel,
                    last_ori,
                    orientation,
                    look_dir,
                    look_dir_override,
                    is_riding,
                },
                progress,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::ComboMelee2(_)
        | CharacterState::RapidRanged(_)
        | CharacterState::RapidMelee(_) => {
            let timer = character.timer();
            let stage_section = character.stage_section();
            let durations = character.durations();
            let progress = if let Some(((timer, stage_section), durations)) =
                timer.zip(stage_section).zip(durations)
            {
                let base_dur = match stage_section {
                    StageSection::Buildup => durations.buildup,
                    StageSection::Charge => durations.charge,
                    StageSection::Movement => durations.movement,
                    StageSection::Action => durations.action,
                    StageSection::Recover => durations.recover,
                };
                if let Some(base_dur) = base_dur {
                    timer.as_secs_f32() / base_dur.as_secs_f32()
                } else {
                    timer.as_secs_f32()
                }
            } else {
                0.0
            };

            let (current_action, max_actions) = match character {
                CharacterState::ComboMelee2(s) => (
                    (s.completed_strikes % s.static_data.strikes.len()) as u32,
                    Some(s.static_data.strikes.len() as u32),
                ),
                CharacterState::RapidRanged(s) => (s.projectiles_fired, None),
                CharacterState::RapidMelee(s) => (s.current_strike, s.static_data.max_strikes),
                _ => (0, None),
            };

            anim::character::MultiAction::update_skeleton(
                &target_base,
                anim::character::MultiActionDependency {
                    ability_id,
                    stage_section,
                    ability_info: character.ability_info(),
                    current_action,
                    max_actions,
                    move_dir,
                    orientation,
                    look_dir,
                    velocity: rel_vel,
                    is_riding,
                    last_ori,
                },
                progress,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::Idle(idle::Data {
            is_sneaking: true, ..
        }) => {
            anim::character::SneakAnimation::update_skeleton(
                &target_base,
                (
                    active_tool_kind,
                    rel_vel,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                    *look_dir,
                    time,
                ),
                state_time,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::Interact(s) => {
            let stage_time = s.timer.as_secs_f32();
            let interact_pos = match s.static_data.interact {
                interact::InteractKind::Invalid => pos.0,
                interact::InteractKind::Entity { .. } => interaction_position.unwrap_or(pos.0),
                interact::InteractKind::Sprite { pos, .. } => pos.as_() + 0.5,
            };
            let stage_progress = match s.stage_section {
                StageSection::Buildup => stage_time / s.static_data.buildup_duration.as_secs_f32(),
                StageSection::Action => s.timer.as_secs_f32(),
                StageSection::Recover => stage_time / s.static_data.recover_duration.as_secs_f32(),
                _ => 0.0,
            };
            match s.static_data.interact {
                interact::InteractKind::Entity {
                    kind: InteractionKind::Pet,
                    ..
                } => anim::character::PetAnimation::update_skeleton(
                    &target_base,
                    (pos.0, interact_pos, time),
                    state_time,
                    &mut state_animation_rate,
                    skeleton_attr,
                ),
                _ => anim::character::CollectAnimation::update_skeleton(
                    &target_base,
                    (pos.0, time, Some(s.stage_section), interact_pos, is_riding),
                    stage_progress,
                    &mut state_animation_rate,
                    skeleton_attr,
                ),
            }
        }
        CharacterState::Boost(_) => anim::character::BoostAnimation::update_skeleton(
            &target_base,
            (),
            0.5,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        CharacterState::Stunned(s) => {
            let stage_time = s.timer.as_secs_f32();
            let wield_status = s.was_wielded;
            let stage_progress = match s.stage_section {
                StageSection::Buildup => stage_time / s.static_data.buildup_duration.as_secs_f32(),
                StageSection::Recover => stage_time / s.static_data.recover_duration.as_secs_f32(),
                _ => 0.0,
            };
            match s.static_data.poise_state {
                PoiseState::Normal | PoiseState::Stunned | PoiseState::Interrupted => {
                    anim::character::StunnedAnimation::update_skeleton(
                        &target_base,
                        (
                            active_tool_kind,
                            second_tool_kind,
                            hands,
                            rel_vel.magnitude(),
                            time,
                            Some(s.stage_section),
                            state_time,
                            wield_status,
                        ),
                        stage_progress,
                        &mut state_animation_rate,
                        skeleton_attr,
                    )
                }
                PoiseState::Dazed | PoiseState::KnockedDown => {
                    anim::character::StaggeredAnimation::update_skeleton(
                        &target_base,
                        (
                            active_tool_kind,
                            second_tool_kind,
                            hands,
                            rel_vel.magnitude(),
                            time,
                            Some(s.stage_section),
                            state_time,
                            wield_status,
                        ),
                        stage_progress,
                        &mut state_animation_rate,
                        skeleton_attr,
                    )
                }
            }
        }
        CharacterState::UseItem(s) => {
            let stage_time = s.timer.as_secs_f32();
            let item_kind = s.static_data.item_kind;
            let stage_progress = match s.stage_section {
                StageSection::Buildup => stage_time / s.static_data.buildup_duration.as_secs_f32(),
                StageSection::Action => stage_time,
                StageSection::Recover => stage_time / s.static_data.recover_duration.as_secs_f32(),
                _ => 0.0,
            };
            anim::character::ConsumeAnimation::update_skeleton(
                &target_base,
                (time, Some(s.stage_section), Some(item_kind)),
                stage_progress,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::Equipping(equipping::Data { is_sneaking, .. }) => {
            if *is_sneaking {
                anim::character::SneakEquipAnimation::update_skeleton(
                    &target_base,
                    (
                        active_tool_kind,
                        rel_vel,
                        // TODO: Update to use the quaternion.
                        ori * anim::vek::Vec3::<f32>::unit_y(),
                        last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                        time,
                    ),
                    state_time,
                    &mut state_animation_rate,
                    skeleton_attr,
                )
            } else {
                anim::character::EquipAnimation::update_skeleton(
                    &target_base,
                    (
                        active_tool_kind,
                        second_tool_kind,
                        rel_vel.magnitude(),
                        time,
                    ),
                    state_time,
                    &mut state_animation_rate,
                    skeleton_attr,
                )
            }
        }
        CharacterState::Talk(_) => anim::character::TalkAnimation::update_skeleton(
            &target_base,
            (
                active_tool_kind,
                second_tool_kind,
                rel_vel.magnitude(),
                time,
                look_dir,
            ),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        CharacterState::Wielding(wielding::Data { is_sneaking, .. }) => {
            if physics.in_liquid().is_some() {
                anim::character::SwimWieldAnimation::update_skeleton(
                    &target_base,
                    (
                        active_tool_kind,
                        second_tool_kind,
                        hands,
                        rel_vel.magnitude(),
                        time,
                    ),
                    state_time,
                    &mut state_animation_rate,
                    skeleton_attr,
                )
            } else if *is_sneaking {
                anim::character::SneakWieldAnimation::update_skeleton(
                    &target_base,
                    (
                        (active_tool_kind, active_tool_spec),
                        second_tool_kind,
                        hands,
                        rel_vel,
                        // TODO: Update to use the quaternion.
                        ori * anim::vek::Vec3::<f32>::unit_y(),
                        last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                        *look_dir,
                        time,
                    ),
                    state_time,
                    &mut state_animation_rate,
                    skeleton_attr,
                )
            } else {
                anim::character::WieldAnimation::update_skeleton(
                    &target_base,
                    (
                        (active_tool_kind, active_tool_spec),
                        second_tool_kind,
                        hands,
                        // TODO: Update to use the quaternion.
                        ori * anim::vek::Vec3::<f32>::unit_y(),
                        last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                        look_dir,
                        rel_vel,
                        is_riding,
                        time,
                    ),
                    state_time,
                    &mut state_animation_rate,
                    skeleton_attr,
                )
            }
        }
        CharacterState::Glide(data) => anim::character::GlidingAnimation::update_skeleton(
            &target_base,
            (rel_vel, ori, data.ori.into(), time, acc_vel),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        CharacterState::Climb { .. } => {
            anim::character::ClimbAnimation::update_skeleton(
                &target_base,
                (
                    active_tool_kind,
                    second_tool_kind,
                    rel_vel,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    time,
                ),
                state_time,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::Sit => anim::character::SitAnimation::update_skeleton(
            &target_base,
            (
                active_tool_kind,
                second_tool_kind,
                last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                look_dir,
                time,
            ),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        CharacterState::Crawl => {
            anim::character::CrawlAnimation::update_skeleton(
                &target_base,
                (
                    rel_vel,
                    // TODO: Update to use the quaternion.
                    ori * anim::vek::Vec3::<f32>::unit_y(),
                    last_orientation * anim::vek::Vec3::<f32>::unit_y(),
                    time,
                ),
                state_time,
                &mut state_animation_rate,
                skeleton_attr,
            )
        }
        CharacterState::GlideWield(data) => anim::character::GlideWieldAnimation::update_skeleton(
            &target_base,
            (ori, data.ori.into()),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        CharacterState::Wallrun(data) => anim::character::WallrunAnimation::update_skeleton(
            &target_base,
            (
                (active_tool_kind, active_tool_spec),
                second_tool_kind,
                hands,
                ori * anim::vek::Vec3::<f32>::unit_y(),
                acc_vel,
                wall_dir,
                data.was_wielded,
            ),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        CharacterState::Dance => anim::character::DanceAnimation::update_skeleton(
            &target_base,
            (active_tool_kind, second_tool_kind, time),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        CharacterState::Music(s) => anim::character::MusicAnimation::update_skeleton(
            &target_base,
            (
                hands,
                (Some(s.static_data.ability_info), time),
                rel_vel,
                ability_id,
            ),
            state_time,
            &mut state_animation_rate,
            skeleton_attr,
        ),
        _ => target_base,
    };
    (target_bones, state_animation_rate)
}
