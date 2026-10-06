#![deny(unsafe_code)]
#![expect(incomplete_features)]
#![expect(
    clippy::identity_op,
    clippy::option_map_unit_fn,
    clippy::needless_pass_by_ref_mut //until we find a better way for specs
)]
#![deny(clippy::clone_on_ref_ptr)]
#![feature(generic_const_exprs)]
#![recursion_limit = "2048"]

extern crate alloc;

/// Fixed processor budget for this client.
pub const CPU_COUNT: usize = 4;
#[macro_use]
#[cfg(not(feature = "headless"))]
pub mod ui;
#[cfg(all(not(feature = "headless"), feature = "audio"))]
pub mod audio;
#[cfg(all(not(feature = "headless"), not(feature = "audio")))]
#[path = "audio/silent.rs"]
pub mod audio;
#[cfg(not(feature = "headless"))]
pub mod cli;
pub mod client;
pub mod clipboard;
#[cfg(not(feature = "headless"))]
pub mod cmd;
#[cfg(not(feature = "headless"))]
mod credits;
#[cfg(not(feature = "headless"))]
mod ecs;
#[cfg(not(feature = "headless"))]
pub mod error;
#[cfg(not(feature = "headless"))]
pub mod game_input;
#[cfg(not(feature = "headless"))]
pub mod hud;
#[cfg(not(feature = "headless"))]
pub mod key_state;
#[cfg(not(feature = "headless"))]
pub mod menu;
#[cfg(not(feature = "headless"))]
pub mod mesh;
#[cfg(not(feature = "headless"))]
pub mod panic_handler;
#[cfg(not(feature = "headless"))]
pub mod profile;
#[cfg(not(feature = "headless"))]
pub mod render;
#[cfg(not(feature = "headless"))]
pub mod run;
#[cfg(not(feature = "headless"))]
pub mod scene;
#[cfg(not(feature = "headless"))]
pub mod session;
#[cfg(not(feature = "headless"))]
pub mod settings;
#[cfg(not(feature = "headless"))]
pub mod window;

#[cfg(not(feature = "headless"))]
use crate::{
    audio::AudioFrontend,
    profile::Profile,
    render::{Drawer, GlobalsBindGroup},
    settings::Settings,
    window::{Event, Window},
};
#[cfg(not(feature = "headless"))]
use common::clock::Clock;
#[cfg(not(feature = "headless"))]
use common_base::span;
#[cfg(not(feature = "headless"))]
use i18n::LocalizationHandle;
#[cfg(not(feature = "headless"))]
use std::path::PathBuf;

#[cfg(not(feature = "headless"))]
use alloc::sync::Arc;
#[cfg(not(feature = "headless"))]
use tokio::runtime::Runtime;

/// A type used to store state that is shared between all play states.
#[cfg(not(feature = "headless"))]
pub struct GlobalState {
    pub userdata_dir: PathBuf,
    pub config_dir: PathBuf,
    pub settings: Settings,
    pub profile: Profile,
    pub window: Window,
    pub tokio_runtime: Arc<Runtime>,

    pub lazy_init: scene::terrain::SpriteRenderContextLazy,
    pub audio: AudioFrontend,
    pub info_message: Option<String>,
    pub clock: Clock,
    // TODO: redo this so that the watcher doesn't have to exist for reloading to occur
    pub i18n: LocalizationHandle,
    pub clipboard: ui::ice::Clipboard,
    /// Used to clear the shadow textures when entering a PlayState that doesn't
    /// utilise shadows.
    pub clear_shadows_next_frame: bool,
    /// CLI arguments passed to voxygen
    pub args: crate::cli::Args,
}

#[cfg(not(feature = "headless"))]
impl GlobalState {
    /// Called after a change in play state has occurred (usually used to
    /// reverse any temporary effects a state may have made).
    pub fn on_play_state_changed(&mut self) {
        self.window.grab_cursor(false);
        self.window.needs_refresh_resize();
    }

    pub fn maintain(&mut self) {
        span!(_guard, "maintain", "GlobalState::maintain");
        self.audio.maintain();
        self.window.renderer().maintain()
    }

}

// TODO: appears to be currently unused by playstates
#[cfg(not(feature = "headless"))]
pub enum Direction {
    Forwards,
    Backwards,
}

/// States can either close (and revert to a previous state), push a new state
/// on top of themselves, or switch to a totally different state.
#[cfg(not(feature = "headless"))]
pub enum PlayStateResult {
    /// Keep running this play state.
    Continue,
    /// Pop all play states in reverse order and shut down the program.
    Shutdown,
    /// Close the current play state and pop it from the play state stack.
    Pop,
    /// Push a new play state onto the play state stack.
    Push(Box<dyn PlayState>),
    /// Switch the current play state with a new play state.
    Switch(Box<dyn PlayState>),
}

/// A trait representing a playable game state. This may be a menu, a game
/// session, the title screen, etc.
#[cfg(not(feature = "headless"))]
pub trait PlayState {
    /// Called when entering this play state from another
    fn enter(&mut self, global_state: &mut GlobalState, direction: Direction);

    /// Tick the play state
    fn tick(&mut self, global_state: &mut GlobalState, events: Vec<Event>) -> PlayStateResult;

    /// Get a descriptive name for this state type.
    fn name(&self) -> &'static str;

    /// Determines whether the play state should have an enforced FPS cap
    fn capped_fps(&self) -> bool;

    fn globals_bind_group(&self) -> &GlobalsBindGroup;

    /// Draw the play state.
    fn render(&self, drawer: &mut Drawer<'_>, settings: &Settings);
}

#[allow(dead_code)]
#[cfg(not(feature = "headless"))]
mod debug_overlay;

#[cfg(feature = "headless")]
pub mod headless;
