//! Manages individual sfx event system, listens for sfx events, and requests
//! playback at the requested position and volume
//!
//! Veloren's sfx are managed through a configuration which lives in the
//! codebase under `/assets/voxygen/audio/sfx.ron`.
//!
//! If there are errors while reading or deserialising the configuration file, a
//! warning is logged and sfx will be disabled.
//!
//! Each entry in the configuration consists of an
//! [SfxEvent](../../../veloren_common/event/enum.SfxEvent.html) item, with some
//! additional information to allow playback:
//! - `files` - the paths to the `.wav` files to be played for the sfx. minus
//!   the file extension. This can be a single item if the same sound can be
//!   played each time, or a list of files from which one is chosen at random to
//!   be played.
//! - `threshold` - the time that the system should wait between successive
//!   plays. This avoids playing the sound with very fast successive repetition
//!   when the character can maintain a state over a long period, such as
//!   running or climbing.
//!
//! The following snippet details some entries in the configuration and how they
//! map to the sound files:
//! ```ignore
//! Run(Grass): ( // depends on underfoot block
//!    files: [
//!        "voxygen.audio.sfx.footsteps.stepgrass_1",
//!        "voxygen.audio.sfx.footsteps.stepgrass_2",
//!        "voxygen.audio.sfx.footsteps.stepgrass_3",
//!        "voxygen.audio.sfx.footsteps.stepgrass_4",
//!        "voxygen.audio.sfx.footsteps.stepgrass_5",
//!        "voxygen.audio.sfx.footsteps.stepgrass_6",
//!    ],
//!    threshold: 1.6, // travelled distance before next play
//! ),
//! Wield(Sword): ( // depends on the player's weapon
//!    files: [
//!        "voxygen.audio.sfx.weapon.sword_out",
//!    ],
//!    threshold: 0.5, // wait 0.5s between plays
//! ),
//! ...
//! ```
//!
//! These items (for example, the `Wield(Sword)` occasionally depend on some
//! property which varies in game. The
//! [SfxEvent](../../../veloren_common/event/enum.SfxEvent.html) documentation
//! provides links to those variables, some examples are provided her for longer
//! items:
//!
//! ```ignore
//! // An inventory action
//! Inventory(Dropped): (
//!     files: [
//!        "voxygen.audio.sfx.footsteps.stepgrass_4",
//!    ],
//!    threshold: 0.5,
//! ),
//! // An inventory action which depends upon the item
//! Inventory(Consumed(Apple)): (
//!    files: [
//!        "voxygen.audio.sfx.inventory.consumable.apple",
//!    ],
//!    threshold: 0.5
//! ),
//! // An attack ability which depends on the weapon
//! Attack(DashMelee, Sword): (
//!     files: [
//!         "voxygen.audio.sfx.weapon.sword_dash_01",
//!         "voxygen.audio.sfx.weapon.sword_dash_02",
//!     ],
//!     threshold: 1.2,
//! ),
//! ```

mod event_mapper;
use specs::WorldExt;

use crate::{
    audio::{
        AudioFrontend,
        channel::{SFX_DIST_LIMIT_SQR, UiChannelTag},
    },
    scene::{Camera, FigureMgr, Terrain},
};

use crate::client::Client;
use common::{
    DamageSource,
    assets::{AssetExt, AssetHandle, Ron},
    comp::{
        Body, CharacterAbilityType, Health, InventoryUpdateEvent, UtteranceKind, beam, biped_large,
        biped_small, bird_large, bird_medium, crustacean, humanoid,
        item::{AbilitySpec, ItemDefinitionId, ItemDesc, ItemKind, ToolKind, item_key::ItemKey},
        object,
        poise::PoiseState,
        quadruped_low, quadruped_medium, quadruped_small,
    },
    outcome::Outcome,
    terrain::{BlockKind, SpriteKind, TerrainChunk},
    uid::Uid,
    vol::ReadVol,
};
use common_state::State;
use event_mapper::SfxEventMapper;
use hashbrown::HashMap;
use rand::prelude::*;
use serde::Deserialize;
use tracing::{debug, error, warn};
use vek::*;

mod types;
use types::body_to_voice;
pub use types::*;

pub struct SfxMgr {
    /// This is an `AssetHandle` so it is reloaded automatically
    /// when the manifest is edited.
    pub triggers: AssetHandle<SfxTriggers>,
    event_mapper: SfxEventMapper,
}

impl Default for SfxMgr {
    fn default() -> Self {
        Self {
            triggers: Self::load_sfx_items(),
            event_mapper: SfxEventMapper::new(),
        }
    }
}

impl SfxMgr {
    pub fn maintain(
        &mut self,
        audio: &mut AudioFrontend,
        state: &State,
        player_entity: specs::Entity,
        camera: &Camera,
        terrain: &Terrain<TerrainChunk>,
        client: &Client,
        figure_mgr: &FigureMgr,
    ) {
        // Checks if the SFX volume is set to zero or audio is disabled
        // This prevents us from running all the following code unnecessarily
        if !audio.sfx_enabled() && !audio.subtitles_enabled {
            return;
        }

        let cam_pos = camera.get_pos_with_focus();

        // Sets the listener position to the camera position facing the
        // same direction as the camera
        audio.set_listener_pos(cam_pos, camera.dependents().cam_dir);

        let triggers = self.triggers.read();

        let underwater = state
            .terrain()
            .get(cam_pos.map(|e| e.floor() as i32))
            .map(|b| b.is_liquid())
            .unwrap_or(false);

        if underwater {
            audio.set_sfx_master_filter(888);
        } else {
            audio.set_sfx_master_filter(20000);
        }

        let player_pos = client.position().unwrap_or_default();

        // Update continuing sounds with player position
        if let Some(inner) = audio.inner.as_mut() {
            inner.player_pos = player_pos;
            inner.channels.sfx.iter_mut().for_each(|c| {
                if !c.is_done() {
                    c.update(player_pos)
                }
            })
        }

        self.event_mapper.maintain(
            audio,
            state,
            player_entity,
            camera,
            &triggers,
            terrain,
            client,
            figure_mgr,
        );
    }

    #[expect(clippy::single_match)]
    pub fn handle_outcome(
        &mut self,
        outcome: &Outcome,
        audio: &mut AudioFrontend,
        client: &Client,
    ) {
        if !audio.sfx_enabled() && !audio.subtitles_enabled {
            return;
        }
        let triggers = self.triggers.read();
        let uids = client.state().ecs().read_storage::<Uid>();
        if audio.get_listener().is_none() {
            return;
        }
        match outcome {
            Outcome::Explosion { pos, power, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Explosion);
                audio.emit_sfx(
                    sfx_trigger_item,
                    *pos,
                    Some((power.abs() / 2.5).min(1.5)),
                    SfxTag::Combat,
                );
            },
            Outcome::Lightning { pos } => {
                let distance = pos.distance(audio.get_listener_pos());
                let power = (1.0 - distance / 6_000.0).max(0.0).powi(7);
                if power > 0.0 {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Lightning);
                    let volume = (power * 3.0).min(2.9);
                    // Delayed based on distance / speed of sound (approxmately 340 m/s)
                    audio.play_ambience_oneshot(
                        super::channel::AmbienceChannelTag::Thunder,
                        sfx_trigger_item,
                        Some(volume),
                        Some(distance / 340.0),
                    );
                }
            },
            Outcome::GroundSlam { pos, .. } | Outcome::ClayGolemDash { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::GroundSlam);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::SurpriseEgg { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::SurpriseEgg);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Interaction);
            },
            Outcome::Transformation { pos, .. } => {
                // TODO: Give this a sound
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Transformation);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Interaction);
            },
            Outcome::LaserBeam { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::LaserBeam);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::CyclopsCharge { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::CyclopsCharge);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::FlamethrowerCharge { pos, .. }
            | Outcome::TerracottaStatueCharge { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::CyclopsCharge);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::PyroclasmCharge { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::PyroclasmCharge);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::FireBreathCharge { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FireBreathCharge);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::FuseCharge { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FuseCharge);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::Charge { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::CyclopsCharge);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::FlashFreeze { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FlashFreeze);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::SummonedCreature { pos, body, .. } => {
                match body {
                    Body::BipedSmall(body) => match body.species {
                        biped_small::Species::IronDwarf => {
                            let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Bleep);
                            audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                        },
                        biped_small::Species::Boreal | biped_small::Species::Ashen => {
                            let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::GigaRoar);
                            audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                        },
                        biped_small::Species::ShamanicSpirit | biped_small::Species::Jiangshi => {
                            let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Klonk);
                            audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                        },
                        _ => {},
                    },
                    Body::BipedLarge(body) => match body.species {
                        biped_large::Species::TerracottaBesieger
                        | biped_large::Species::TerracottaPursuer => {
                            let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Klonk);
                            audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                        },
                        _ => {},
                    },
                    Body::BirdMedium(body) => match body.species {
                        bird_medium::Species::Bat => {
                            let sfx_trigger_item =
                                triggers.0.get_key_value(&SfxEvent::BloodmoonHeiressSummon);
                            audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                        },
                        _ => {},
                    },
                    Body::Crustacean(body) => match body.species {
                        crustacean::Species::SoldierCrab => {
                            let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Hiss);
                            audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                        },
                        _ => {},
                    },
                    Body::Object(object::Body::Lavathrower) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::DeepLaugh);
                        audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                    },
                    Body::Object(object::Body::SeaLantern) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::LongHiss);
                        audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                    },
                    Body::Object(object::Body::Tornado)
                    | Body::Object(object::Body::FieryTornado) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Swoosh);
                        audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                    },
                    _ => { // not mapped to sfx file
                    },
                }
            },
            Outcome::GroundDig { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::GroundDig);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Interaction);
            },
            Outcome::PortalActivated { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::PortalActivated);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Interaction);
            },
            Outcome::TeleportedByPortal { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::TeleportedByPortal);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Interaction);
            },
            Outcome::IceSpikes { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::IceSpikes);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::IceCrack { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::IceCrack);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::Steam { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Steam);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::FireShockwave { pos, .. } | Outcome::FireLowShockwave { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FlameThrower);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::FromTheAshes { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FromTheAshes);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
            },
            Outcome::ProjectileShot { pos, body, .. } => {
                match body {
                    Body::Object(
                        object::Body::Arrow
                        | object::Body::MultiArrow
                        | object::Body::ArrowSnake
                        | object::Body::ArrowTurret
                        | object::Body::ArrowClay
                        | object::Body::ArrowHeavy
                        | object::Body::BoltBesieger
                        | object::Body::HarlequinDagger
                        | object::Body::SpectralSwordSmall
                        | object::Body::SpectralSwordLarge,
                    ) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::ArrowShot);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    Body::Object(
                        object::Body::BoltFire
                        | object::Body::BoltFireBig
                        | object::Body::BoltNature
                        | object::Body::BoltIcicle
                        | object::Body::SpearIcicle
                        | object::Body::GrenadeClay
                        | object::Body::SpitPoison,
                    ) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FireShot);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    Body::Object(object::Body::NapalmShot) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::NapalmShot);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    Body::Object(object::Body::FireRing) => {},
                    Body::Object(object::Body::PyroclasmBolt) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::PyroclasmBolt);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    Body::Object(
                        object::Body::IronPikeBomb
                        | object::Body::BubbleBomb
                        | object::Body::MinotaurAxe
                        | object::Body::Pebble,
                    ) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Whoosh);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    Body::Object(
                        object::Body::LaserBeam
                        | object::Body::LaserBeamSmall
                        | object::Body::LightningBolt,
                    ) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::LaserBeam);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    Body::Object(
                        object::Body::AdletTrap | object::Body::BorealTrap | object::Body::Mine,
                    ) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Yeet);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    Body::Object(object::Body::StrigoiHead) => {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::StrigoiHead);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    },
                    _ => {
                        // not mapped to sfx file
                    },
                }
            },
            Outcome::ProjectileHit {
                pos,
                body,
                source,
                target,
                ..
            } => match body {
                Body::Object(
                    object::Body::Arrow
                    | object::Body::MultiArrow
                    | object::Body::ArrowSnake
                    | object::Body::ArrowTurret
                    | object::Body::ArrowClay
                    | object::Body::ArrowHeavy
                    | object::Body::BoltBesieger
                    | object::Body::HarlequinDagger
                    | object::Body::SpectralSwordSmall
                    | object::Body::SpectralSwordLarge
                    | object::Body::Pebble,
                ) => {
                    if target.is_none() {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::ArrowMiss);
                        audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                    } else if *source == client.uid() {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::ArrowHit);
                        audio.emit_sfx(
                            sfx_trigger_item,
                            client.position().unwrap_or(*pos),
                            Some(2.0),
                            SfxTag::Combat,
                        );
                    } else {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::ArrowHit);
                        audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                    }
                },
                Body::Object(
                    object::Body::AdletTrap
                    | object::Body::BorealTrap
                    | object::Body::Mine
                    | object::Body::StrigoiHead,
                ) => {
                    if target.is_none() {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Klonk);
                        audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                    } else if *source == client.uid() {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::SmashKlonk);
                        audio.emit_sfx(
                            sfx_trigger_item,
                            client.position().unwrap_or(*pos),
                            Some(2.0),
                            SfxTag::Combat,
                        );
                    } else {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::SmashKlonk);
                        audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                    }
                },
                Body::Object(object::Body::NapalmShot) => {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::NapalmImpact);
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(2.0), SfxTag::Combat);
                },
                _ => {},
            },
            Outcome::SkillPointGain { uid, .. } => {
                if let Some(client_uid) = uids.get(client.entity())
                    && uid == client_uid
                {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::SkillPointGain);
                    audio.emit_ui_sfx(sfx_trigger_item, Some(0.4), Some(UiChannelTag::LevelUp));
                }
            },
            Outcome::Beam { pos, specifier } => match specifier {
                beam::FrontendSpecifier::LifestealBeam
                | beam::FrontendSpecifier::Steam
                | beam::FrontendSpecifier::Poison
                | beam::FrontendSpecifier::Ink
                | beam::FrontendSpecifier::Lightning
                | beam::FrontendSpecifier::Frost
                | beam::FrontendSpecifier::Bubbles => {
                    if rand::rng().random_bool(0.5) {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::SceptreBeam);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    };
                },
                beam::FrontendSpecifier::Flamethrower
                | beam::FrontendSpecifier::Cultist
                | beam::FrontendSpecifier::PhoenixLaser
                | beam::FrontendSpecifier::FireGigasOverheat
                | beam::FrontendSpecifier::FirePillar => {
                    if rand::rng().random_bool(0.5) {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FlameThrower);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    }
                },
                beam::FrontendSpecifier::FlameWallPillar => {
                    if rand::rng().random_bool(0.02) {
                        let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::FlameThrower);
                        audio.emit_sfx(sfx_trigger_item, *pos, None, SfxTag::Combat);
                    }
                },
                beam::FrontendSpecifier::Gravewarden | beam::FrontendSpecifier::WebStrand => {},
            },
            Outcome::SpriteUnlocked { pos } => {
                // TODO: Dedicated sound effect!
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::GliderOpen);
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e as f32 + 0.5),
                    Some(2.0),
                    SfxTag::Interaction,
                );
            },
            Outcome::FailedSpriteUnlock { pos } => {
                // TODO: Dedicated sound effect!
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::BreakBlock);
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e as f32 + 0.5),
                    Some(2.0),
                    SfxTag::Interaction,
                );
            },
            Outcome::BreakBlock { pos, tool, .. } => {
                let sfx_trigger_item =
                    triggers
                        .0
                        .get_key_value(&if matches!(tool, Some(ToolKind::Pick)) {
                            SfxEvent::PickaxeBreakBlock
                        } else {
                            SfxEvent::BreakBlock
                        });
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e as f32 + 0.5),
                    Some(1.2),
                    SfxTag::Interaction,
                );
            },
            Outcome::DamagedBlock {
                pos,
                stage_changed,
                tool,
                ..
            } => {
                let sfx_trigger_item = triggers.0.get_key_value(&match (stage_changed, tool) {
                    (false, Some(ToolKind::Pick)) => SfxEvent::PickaxeDamage,
                    (true, Some(ToolKind::Pick)) => SfxEvent::PickaxeDamageStrong,
                    // SFX already emitted by ability
                    (_, Some(ToolKind::Shovel)) => return,
                    (_, _) => SfxEvent::BreakBlock,
                });

                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e as f32 + 0.5),
                    Some(1.0),
                    SfxTag::Interaction,
                );
            },
            Outcome::HealthChange { pos, info, .. } => {
                // Ignore positive damage (healing) and buffs for now
                if info.amount < Health::HEALTH_EPSILON
                    && !matches!(info.cause, Some(DamageSource::Buff(_)))
                {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Damage);
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
                }
            },
            Outcome::Death { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Death);
                audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
            },
            Outcome::Block { pos, parry, .. } => {
                if *parry {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Parry);
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
                } else {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Block);
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
                }
            },
            Outcome::PoiseChange {
                pos,
                state: poise_state,
                ..
            } => match poise_state {
                PoiseState::Normal => {},
                PoiseState::Interrupted => {
                    let sfx_trigger_item = triggers
                        .0
                        .get_key_value(&SfxEvent::PoiseChange(PoiseState::Interrupted));
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
                },
                PoiseState::Stunned => {
                    let sfx_trigger_item = triggers
                        .0
                        .get_key_value(&SfxEvent::PoiseChange(PoiseState::Stunned));
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
                },
                PoiseState::Dazed => {
                    let sfx_trigger_item = triggers
                        .0
                        .get_key_value(&SfxEvent::PoiseChange(PoiseState::Dazed));
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
                },
                PoiseState::KnockedDown => {
                    let sfx_trigger_item = triggers
                        .0
                        .get_key_value(&SfxEvent::PoiseChange(PoiseState::KnockedDown));
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.5), SfxTag::Combat);
                },
            },
            Outcome::Utterance { pos, kind, body } => {
                if let Some(voice) = body_to_voice(body) {
                    let sfx_trigger_item =
                        triggers.0.get_key_value(&SfxEvent::Utterance(*kind, voice));
                    if let Some(sfx_trigger_item) = sfx_trigger_item {
                        // TODO: Dirty hack to turn down the volume of one creature. Need another
                        // way to do this.
                        if matches!(voice, VoiceKind::Wolf) {
                            audio.emit_sfx(
                                Some(sfx_trigger_item),
                                *pos,
                                Some(0.75),
                                SfxTag::Combat,
                            );
                        } else {
                            audio.emit_sfx(
                                Some(sfx_trigger_item),
                                *pos,
                                Some(1.5),
                                SfxTag::Utterance,
                            );
                        }
                    } else {
                        debug!(
                            "No utterance sound effect exists for ({:?}, {:?})",
                            kind, voice
                        );
                    }
                }
            },
            Outcome::Glider { pos, wielded } => {
                if *wielded {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::GliderOpen);
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.0), SfxTag::Interaction);
                } else {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::GliderClose);
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(1.0), SfxTag::Interaction);
                }
            },
            Outcome::SpriteDelete {
                pos,
                sprite: SpriteKind::SeaUrchin,
            } => {
                let pos = pos.map(|e| e as f32 + 0.5);
                let power = (0.6 - pos.distance(audio.get_listener_pos()) / 5_000.0)
                    .max(0.0)
                    .powi(7);
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Explosion);
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos,
                    Some((power.abs() / 2.5).min(0.3)),
                    SfxTag::Combat,
                );
            },
            Outcome::Whoosh { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Whoosh);
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e + 0.5),
                    Some(3.0),
                    SfxTag::Combat,
                );
            },
            Outcome::Swoosh { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Swoosh);
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e + 0.5),
                    Some(3.0),
                    SfxTag::Combat,
                );
            },
            Outcome::Slash { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::SmashKlonk);
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e + 0.5),
                    Some(3.0),
                    SfxTag::Combat,
                );
            },
            Outcome::Bleep { pos, .. } => {
                let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Bleep);
                audio.emit_sfx(
                    sfx_trigger_item,
                    pos.map(|e| e + 0.5),
                    Some(3.0),
                    SfxTag::Combat,
                );
            },
            Outcome::HeadLost { uid, .. } => {
                let positions = client.state().ecs().read_storage::<common::comp::Pos>();
                if let Some(pos) = client
                    .state()
                    .ecs()
                    .read_resource::<common::uid::IdMaps>()
                    .uid_entity(*uid)
                    .and_then(|entity| positions.get(entity))
                {
                    let sfx_trigger_item = triggers.0.get_key_value(&SfxEvent::Death);
                    audio.emit_sfx(sfx_trigger_item, pos.0, Some(2.0), SfxTag::Combat);
                } else {
                    error!("Couldn't get position of entity that lost head");
                }
            },
            Outcome::Splash { vel, pos, mass, .. } => {
                let magnitude = (-vel.z).max(0.0);
                let energy = mass * magnitude;

                if energy > 0.0 {
                    let (sfx, volume) = if energy < 10.0 {
                        (SfxEvent::SplashSmall, (energy / 20.0).max(0.25))
                    } else if energy < 100.0 {
                        (SfxEvent::SplashMedium, ((energy - 10.0) / 50.0 + 0.9))
                    } else {
                        (
                            SfxEvent::SplashBig,
                            ((energy / 100.0).sqrt() + 0.5).min(2.0),
                        )
                    };
                    let sfx_trigger_item = triggers.0.get_key_value(&sfx);
                    audio.emit_sfx(sfx_trigger_item, *pos, Some(volume), SfxTag::Footstep);
                }
            },
            _ => {},
        }
    }

    fn load_sfx_items() -> AssetHandle<SfxTriggers> {
        SfxTriggers::load_or_insert_with("voxygen.audio.sfx", |error| {
            warn!(
                "Error reading sfx config file, sfx will not be available: {:#?}",
                error
            );

            SfxTriggers::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::credits::Credits;

    use super::*;
    use chumsky::container::Seq;
    use common::assets::{self, AssetExt, Ron};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_load_sfx_triggers() { let _ = SfxTriggers::load_expect("voxygen.audio.sfx"); }

    #[test]
    fn new_sfx_credited() {
        let sfx_path = assets::ASSETS_PATH.join(std::path::PathBuf::from("voxygen/audio/sfx/"));
        sfx_path.try_exists().unwrap_or_else(|_| {
            panic!(
                "{}/voxygen/audio/sfx does not exist",
                assets::ASSETS_PATH.display()
            )
        });
        let mut files = Vec::new();
        list_files(sfx_path.clone(), &mut files);

        let credits = Ron::<Credits>::load_expect_cloned("credits").into_inner();
        let mut sounds = Vec::new();
        for credit in &credits.sounds {
            sounds.append(
                &mut credit
                    .files
                    .iter()
                    .map(|f| sfx_path.clone().join(f))
                    .collect::<Vec<PathBuf>>(),
            )
        }
        for file in files.iter() {
            if !sounds.contains(file) {
                panic!(
                    "{} was not found in credits. Credit the authors of the sound in \
                     assets/credits.ron!",
                    file.display(),
                );
            }
        }
    }

    fn list_files(path: PathBuf, buffer: &mut Vec<PathBuf>) {
        for dir in fs::read_dir(path).expect("Could not read directory") {
            if dir
                .as_ref()
                .expect("Could not read file entry")
                .file_type()
                .expect("Could not read filetype")
                .is_dir()
            {
                list_files(dir.unwrap().path(), buffer);
            } else {
                buffer.push(dir.as_ref().unwrap().path());
            }
        }
    }
}
