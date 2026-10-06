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
use hashbrown::HashMap;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize, Hash, Eq)]
pub enum SfxEvent {
    Campfire,
    Embers,
    Birdcall,
    Owl,
    Cricket1,
    Cricket2,
    Cricket3,
    Frog,
    Bees,
    RunningWaterSlow,
    Lavapool,
    Idle,
    Swim,
    SplashSmall,
    SplashMedium,
    SplashBig,
    Run(BlockKind),
    QuadRun(BlockKind),
    OctoRun(BlockKind),
    Roll,
    RollCancel,
    Sneak,
    Climb,
    GliderOpen,
    Glide,
    GliderClose,
    CatchAir,
    Jump,
    Fall,
    Attack(CharacterAbilityType, ToolKind),
    Wield(ToolKind),
    Unwield(ToolKind),
    Inventory(SfxInventoryEvent),
    Explosion,
    Damage,
    Death,
    Parry,
    Block,
    BreakBlock,
    PickaxeDamage,
    PickaxeDamageStrong,
    PickaxeBreakBlock,
    SceptreBeam,
    SkillPointGain,
    ArrowHit,
    ArrowMiss,
    ArrowShot,
    FireShot,
    NapalmShot,
    NapalmImpact,
    FireBreathShot,
    FireBreathCharge,
    PyroclasmCharge,
    PyroclasmBolt,
    FlameThrower,
    PoiseChange(PoiseState),
    GroundSlam,
    FlashFreeze,
    GigaRoar,
    IceSpikes,
    IceCrack,
    Utterance(UtteranceKind, VoiceKind),
    Lightning,
    CyclopsCharge,
    TerracottaStatueCharge,
    LaserBeam,
    Steam,
    FuseCharge,
    Music(ToolKind, AbilitySpec),
    Yeet,
    Hiss,
    LongHiss,
    Klonk,
    SmashKlonk,
    FireShockwave,
    DeepLaugh,
    Whoosh,
    Swoosh,
    GroundDig,
    PortalActivated,
    TeleportedByPortal,
    FromTheAshes,
    SurpriseEgg,
    Transformation,
    Bleep,
    Charge,
    StrigoiHead,
    BloodmoonHeiressSummon,
    TrainChugg,
    TrainChuggSteam,
    TrainAmbience,
    TrainClack,
    TrainSpeed,
}

#[derive(Copy, Clone, Debug, PartialEq, Deserialize, Hash, Eq)]
pub enum VoiceKind {
    HumanFemale,
    HumanMale,
    BipedLarge,
    Wendigo,
    Reptile,
    Bird,
    Critter,
    Sheep,
    Pig,
    Cow,
    Canine,
    Dagon,
    Lion,
    Mindflayer,
    Marlin,
    Maneater,
    Adlet,
    Antelope,
    Alligator,
    SeaCrocodile,
    Saurok,
    Cat,
    Goat,
    Mandragora,
    Asp,
    Fungome,
    Truffler,
    Wolf,
    Wyvern,
    Phoenix,
    VampireBat,
    Legoom,
}

pub(super) fn body_to_voice(body: &Body) -> Option<VoiceKind> {
    Some(match body {
        Body::Humanoid(body) => match &body.body_type {
            humanoid::BodyType::Female => VoiceKind::HumanFemale,
            humanoid::BodyType::Male => VoiceKind::HumanMale,
        },
        Body::QuadrupedLow(body) => match body.species {
            quadruped_low::Species::Maneater => VoiceKind::Maneater,
            quadruped_low::Species::Alligator | quadruped_low::Species::Snaretongue => {
                VoiceKind::Alligator
            }
            quadruped_low::Species::SeaCrocodile => VoiceKind::SeaCrocodile,
            quadruped_low::Species::Dagon => VoiceKind::Dagon,
            quadruped_low::Species::Asp => VoiceKind::Asp,
            _ => return None,
        },
        Body::QuadrupedSmall(body) => match body.species {
            quadruped_small::Species::Truffler => VoiceKind::Truffler,
            quadruped_small::Species::Fungome => VoiceKind::Fungome,
            quadruped_small::Species::Sheep => VoiceKind::Sheep,
            quadruped_small::Species::Pig | quadruped_small::Species::Boar => VoiceKind::Pig,
            quadruped_small::Species::Cat => VoiceKind::Cat,
            quadruped_small::Species::Goat => VoiceKind::Goat,
            _ => VoiceKind::Critter,
        },
        Body::QuadrupedMedium(body) => match body.species {
            quadruped_medium::Species::Saber
            | quadruped_medium::Species::Tiger
            | quadruped_medium::Species::Lion
            | quadruped_medium::Species::Frostfang
            | quadruped_medium::Species::Snowleopard => VoiceKind::Lion,
            quadruped_medium::Species::Wolf => VoiceKind::Wolf,
            quadruped_medium::Species::Roshwalr
            | quadruped_medium::Species::Tarasque
            | quadruped_medium::Species::Darkhound
            | quadruped_medium::Species::Bonerattler
            | quadruped_medium::Species::Grolgar => VoiceKind::Canine,
            quadruped_medium::Species::Cattle
            | quadruped_medium::Species::Catoblepas
            | quadruped_medium::Species::Highland
            | quadruped_medium::Species::Yak
            | quadruped_medium::Species::Moose
            | quadruped_medium::Species::Dreadhorn => VoiceKind::Cow,
            quadruped_medium::Species::Antelope => VoiceKind::Antelope,
            _ => return None,
        },
        Body::BirdMedium(body) => match body.species {
            bird_medium::Species::BloodmoonBat | bird_medium::Species::VampireBat => {
                VoiceKind::VampireBat
            }
            _ => VoiceKind::Bird,
        },
        Body::BirdLarge(body) => match body.species {
            bird_large::Species::CloudWyvern
            | bird_large::Species::FlameWyvern
            | bird_large::Species::FrostWyvern
            | bird_large::Species::SeaWyvern
            | bird_large::Species::WealdWyvern => VoiceKind::Wyvern,
            bird_large::Species::Phoenix => VoiceKind::Phoenix,
            _ => VoiceKind::Bird,
        },
        Body::BipedSmall(body) => match body.species {
            biped_small::Species::Adlet => VoiceKind::Adlet,
            biped_small::Species::Mandragora => VoiceKind::Mandragora,
            biped_small::Species::Flamekeeper => VoiceKind::BipedLarge,
            biped_small::Species::GreenLegoom
            | biped_small::Species::OchreLegoom
            | biped_small::Species::PurpleLegoom
            | biped_small::Species::RedLegoom
            | biped_small::Species::UmberLegoom => VoiceKind::Legoom,
            _ => return None,
        },
        Body::BipedLarge(body) => match body.species {
            biped_large::Species::Wendigo => VoiceKind::Wendigo,
            biped_large::Species::Occultsaurok
            | biped_large::Species::Mightysaurok
            | biped_large::Species::Slysaurok => VoiceKind::Saurok,
            biped_large::Species::Mindflayer => VoiceKind::Mindflayer,
            _ => VoiceKind::BipedLarge,
        },
        Body::Theropod(_) | Body::Dragon(_) => VoiceKind::Reptile,
        Body::FishSmall(_) | Body::FishMedium(_) => VoiceKind::Marlin,
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Deserialize, Hash, Eq)]
pub enum SfxInventoryEvent {
    Collected,
    CollectedTool(ToolKind),
    CollectedItem(String),
    CollectFailed,
    Consumed(ItemKey),
    Debug,
    Dropped,
    Given,
    Swapped,
    Craft,
}

// TODO Move to a separate event mapper?
impl From<&InventoryUpdateEvent> for SfxEvent {
    fn from(value: &InventoryUpdateEvent) -> Self {
        match value {
            InventoryUpdateEvent::Collected(item) => {
                // Handle sound effects for types of collected items, falling
                // back to the default Collected event
                match &*item.kind() {
                    ItemKind::Tool(tool) => {
                        SfxEvent::Inventory(SfxInventoryEvent::CollectedTool(tool.kind))
                    }
                    ItemKind::Ingredient { .. }
                        if matches!(
                            item.item_definition_id(),
                            ItemDefinitionId::Simple(id) if id.contains("mineral.gem.")
                        ) =>
                    {
                        SfxEvent::Inventory(SfxInventoryEvent::CollectedItem(String::from(
                            "Gemstone",
                        )))
                    }
                    _ => SfxEvent::Inventory(SfxInventoryEvent::Collected),
                }
            }
            InventoryUpdateEvent::BlockCollectFailed { .. }
            | InventoryUpdateEvent::EntityCollectFailed { .. } => {
                SfxEvent::Inventory(SfxInventoryEvent::CollectFailed)
            }
            InventoryUpdateEvent::Consumed(consumable) => {
                SfxEvent::Inventory(SfxInventoryEvent::Consumed(consumable.clone()))
            }
            InventoryUpdateEvent::Debug => SfxEvent::Inventory(SfxInventoryEvent::Debug),
            InventoryUpdateEvent::Dropped => SfxEvent::Inventory(SfxInventoryEvent::Dropped),
            InventoryUpdateEvent::Given => SfxEvent::Inventory(SfxInventoryEvent::Given),
            InventoryUpdateEvent::Swapped => SfxEvent::Inventory(SfxInventoryEvent::Swapped),
            InventoryUpdateEvent::Craft => SfxEvent::Inventory(SfxInventoryEvent::Craft),
            _ => SfxEvent::Inventory(SfxInventoryEvent::Swapped),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct SfxTriggerItem {
    /// A list of SFX filepaths for this event
    pub files: Vec<String>,
    /// The time to wait before repeating this SfxEvent
    pub threshold: f32,

    #[serde(default)]
    pub subtitle: Option<String>,
}

pub type SfxTriggers = Ron<HashMap<SfxEvent, SfxTriggerItem>>;

/// Tags in descending order of priority
/// Tags lower in the list will be interrupted by tags higher in the list if no
/// channels are available
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SfxTag {
    /// Looping sfx e.g. campfires
    Looping,
    /// Hitsounds, abilities, moves
    Combat,
    /// Items, utility equipment, block interactions
    Interaction,
    /// Trains, airships, etc.
    Vehicle,
    /// NPC utterances
    Utterance,
    /// Sounds emitted by blocks meant to represent in-world entities (e.g.
    /// birds, frogs)
    Blocksound,
    /// All footsteps and some types of movement, but not movement that relates
    /// to combat
    Footstep,
    /// Unimportant sounds emitted by blocks that are meant to add to the
    /// natural soundscape (e.g. bubbling)
    Ambient,
}
