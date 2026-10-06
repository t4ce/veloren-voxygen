use directories_next::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tracing::warn;

pub mod audio;
pub mod chat;
pub mod control;
pub mod controller;
pub mod gameplay;
pub mod graphics;
pub mod hud_position;
pub mod interface;
pub mod inventory;
pub mod language;
pub mod networking;

pub use audio::{AudioOutput, AudioSettings};
pub use chat::ChatSettings;
pub use control::ControlSettings;
pub use controller::{Button, ControllerSettings};
pub use gameplay::GameplaySettings;
pub use graphics::{Fps, GraphicsSettings, get_fps};
pub use hud_position::HudPositionSettings;
pub use interface::InterfaceSettings;
pub use inventory::InventorySettings;
pub use language::LanguageSettings;
pub use networking::NetworkingSettings;

/// `Settings` contains everything that can be configured in the settings.ron
/// file.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub chat: ChatSettings,
    pub controls: ControlSettings,
    pub controller: ControllerSettings,
    pub interface: InterfaceSettings,
    pub hud_position: HudPositionSettings,
    pub gameplay: GameplaySettings,
    pub networking: NetworkingSettings,
    pub graphics: GraphicsSettings,
    pub audio: AudioSettings,
    pub show_disclaimer: bool,
    pub send_logon_commands: bool,
    // TODO: Remove at a later date, for dev testing
    pub logon_commands: Vec<String>,
    pub language: LanguageSettings,
    pub screenshots_path: PathBuf,
    pub inventory: InventorySettings,
}

impl Default for Settings {
    fn default() -> Self {
        // An explicit destination wins. TRUEOS uses the app's instance root;
        // desktop platforms retain Pictures and executable-directory discovery.
        let screenshots_path = std::env::var_os("VOXYGEN_SCREENSHOT")
            .map(PathBuf::from)
            .or_else(|| {
                #[cfg(target_os = "trueos")]
                {
                    let root = std::env::var_os("TRUEOS_APP_FS_ROOT")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("apps/voxy"));
                    Some(Path::new("/").join(root).join("screenshots"))
                }
                #[cfg(not(target_os = "trueos"))]
                { None }
            })
            .or_else(|| UserDirs::new()?.picture_dir().map(|dir| dir.join("veloren")))
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|dir| dir.parent().map(PathBuf::from))
            })
            .unwrap_or_else(|| PathBuf::from("screenshots"));

        Settings {
            chat: ChatSettings::default(),
            controls: ControlSettings::default(),
            controller: ControllerSettings::default(),
            interface: InterfaceSettings::default(),
            hud_position: HudPositionSettings::default(),
            gameplay: GameplaySettings::default(),
            networking: NetworkingSettings::default(),
            graphics: GraphicsSettings::default(),
            audio: AudioSettings::default(),
            show_disclaimer: true,
            send_logon_commands: false,
            logon_commands: Vec::new(),
            language: LanguageSettings::default(),
            screenshots_path,
            inventory: InventorySettings::default(),
        }
    }
}

impl Settings {
    /// Apply the embedded deployment baseline once at startup. Runtime edits remain free.
    /// Use the existing settings schema, preserving fields absent from the JSON.
    pub fn apply_trueos_bringup_profile(&mut self) -> Result<(), serde_json::Error> {
        fn overlay(current: &mut serde_json::Value, patch: serde_json::Value) {
            match (current, patch) {
                (serde_json::Value::Object(current), serde_json::Value::Object(patch)) => {
                    for (key, value) in patch {
                        overlay(current.entry(key).or_insert(serde_json::Value::Null), value);
                    }
                }
                (current, patch) => *current = patch,
            }
        }
        let patch = serde_json::from_str(include_str!("../../trueos-bringup-profile.json"))?;
        let mut current = serde_json::to_value(&*self)?;
        overlay(&mut current, patch);
        *self = serde_json::from_value(current)?;
        Ok(())
    }

    pub fn load(config_dir: &Path) -> Self {
        let path = Self::get_path(config_dir);

        let settings = common::util::ron_from_path_recoverable::<Self>(&path);
        // Save settings to add new fields or create the file if it is not already there
        settings.save_to_file_warn(config_dir);
        settings
    }

    pub fn save_to_file_warn(&self, config_dir: &Path) {
        if let Err(e) = self.save_to_file(config_dir) {
            warn!(?e, "Failed to save settings");
        }
    }

    pub fn save_to_file(&self, config_dir: &Path) -> std::io::Result<()> {
        let path = Self::get_path(config_dir);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }

        let ron = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).unwrap();
        fs::write(path, ron.as_bytes())
    }

    fn get_path(config_dir: &Path) -> PathBuf { config_dir.join("settings.ron") }

    pub fn display_warnings(&self) {
        if !self.graphics.render_mode.experimental_shaders.is_empty() {
            warn!(
                "One or more experimental shaders are enabled, all rendering guarantees are off. \
                 Experimental shaders may be unmaintained, mutually-incompatible, entirely \
                 broken, or may cause your GPU to explode. You have been warned!"
            );
        }
    }
}
