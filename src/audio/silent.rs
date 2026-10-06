//! Compile-time silent frontend: no devices, decoding, audio assets or workers.
#![allow(unused_variables)]
use crate::hud::Subtitle;
use alloc::collections::VecDeque;
use vek::Vec3;

#[derive(Clone, Copy, Debug, strum::Display)]
pub enum SfxChannelSettings {
    Low,
    Medium,
    High,
}
impl SfxChannelSettings {
    pub fn from_str_slice(value: &str) -> Self {
        match value {
            "Low" => Self::Low,
            "Medium" => Self::Medium,
            _ => Self::High,
        }
    }
    pub fn to_usize(&self) -> usize {
        match self {
            Self::Low => 16,
            Self::Medium => 32,
            Self::High => 64,
        }
    }
}
#[derive(Default)]
pub struct ActiveChannels {
    pub music: usize,
    pub ambience: usize,
    pub sfx: usize,
    pub ui: usize,
}
#[derive(Default)]
pub struct AudioFrontend {
    pub subtitles_enabled: bool,
    pub subtitles: VecDeque<Subtitle>,
    pub combat_music_enabled: bool,
}
impl AudioFrontend {
    pub fn no_audio() -> Self {
        Self::default()
    }
    pub fn new(
        num_sfx_channels: usize,
        num_ui_channels: usize,
        subtitles: bool,
        combat_music_enabled: bool,
        buffer_size: usize,
        set_samplerate: Option<u32>,
    ) -> Self {
        Self::no_audio()
    }
    pub fn get_num_active_channels(&self) -> ActiveChannels {
        ActiveChannels::default()
    }
    pub fn get_cpu_usage(&mut self) -> f32 {
        0.0
    }
    pub fn get_listener_pos(&self) -> Vec3<f32> {
        Vec3::zero()
    }
    pub fn get_listener_ori(&self) -> Vec3<f32> {
        Vec3::unit_y()
    }
    pub fn emit_ui_sfx(
        &mut self,
        trigger: Option<(&sfx::SfxEvent, &sfx::SfxTriggerItem)>,
        volume: Option<f32>,
        tag: Option<()>,
    ) {
    }
    pub fn maintain(&mut self) {}
    pub fn play_title_music(&mut self) {}
    pub fn stop_all_music(&mut self) {}
    pub fn stop_all_sfx(&mut self) {}
    pub fn stop_all_ambience(&mut self) {}
    pub fn set_master_volume(&mut self, value: f32) {}
    pub fn set_music_volume(&mut self, value: f32) {}
    pub fn set_sfx_volume(&mut self, value: f32) {}
    pub fn set_instrument_volume(&mut self, value: f32) {}
    pub fn set_ambience_volume(&mut self, value: f32) {}
    pub fn set_music_spacing(&mut self, value: f32) {}
    pub fn set_num_sfx_channels(&mut self, value: usize) {}
    pub fn set_subtitles(&mut self, value: bool) {}
    pub fn music_enabled(&self) -> bool {
        false
    }
    pub fn ambience_enabled(&self) -> bool {
        false
    }
    pub fn sfx_enabled(&self) -> bool {
        false
    }
}
pub mod sfx {
    #[path = "types.rs"]
    mod types;
    pub use types::*;
    /// Empty event lookup preserves UI call sites without loading an audio manifest.
    #[derive(Default)]
    pub struct SilentTriggers;
    pub struct SilentTriggerSnapshot(pub hashbrown::HashMap<SfxEvent, SfxTriggerItem>);
    impl SilentTriggers {
        pub fn read(&self) -> SilentTriggerSnapshot {
            SilentTriggerSnapshot(hashbrown::HashMap::new())
        }
    }
    #[derive(Default)]
    pub struct SfxMgr {
        pub triggers: SilentTriggers,
    }
    impl SfxMgr {
        pub fn maintain<A, B, C, D, E, F>(
            &mut self,
            audio: &mut super::AudioFrontend,
            state: A,
            entity: B,
            camera: C,
            terrain: D,
            client: E,
            figures: F,
        ) {
        }
        pub fn handle_outcome<A, B>(
            &mut self,
            outcome: A,
            audio: &mut super::AudioFrontend,
            client: B,
        ) {
        }
    }
}
pub mod music {
    pub struct MusicMgr;
    impl MusicMgr {
        pub fn new<T>(calendar: &T) -> Self {
            Self
        }
        pub fn maintain<A, B>(&mut self, audio: &mut super::AudioFrontend, state: A, client: B) {}
        pub fn reset_track(&mut self, audio: &mut super::AudioFrontend) {}
        pub fn current_track(&self) -> String {
            String::new()
        }
        pub fn current_artist(&self) -> String {
            String::new()
        }
    }
}
pub mod ambience {
    pub struct AmbienceMgr;
    pub fn load_ambience_items() {}
    impl AmbienceMgr {
        pub fn new(items: ()) -> Self {
            Self
        }
        pub fn maintain<A, B, C, D, E>(
            &mut self,
            audio: &mut super::AudioFrontend,
            settings: A,
            state: B,
            client: C,
            camera: D,
            terrain: E,
        ) {
        }
    }
}
