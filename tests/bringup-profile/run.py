#!/usr/bin/env python3
"""Exercise the real graphics schema and startup overlay without the TRUEOS sysroot."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
render = (ROOT / 'src/render/mod.rs').read_text()
render = render[render.index('use serde::{'):render.index('\nimpl RenderMode {')]
start = render.index('impl From<PresentMode>')
end = render.index('/// Bloom factor', start)
render = render[:start] + render[end:]
experimental = (ROOT / 'src/render/mod.rs').read_text()
experimental = experimental[experimental.index('/// Experimental shader modes.'):experimental.index('\nimpl ExperimentalShader {')]
for derive in ['    strum::EnumIter,\n', '    strum::Display,\n', '    strum::EnumString,\n']:
    experimental = experimental.replace(derive, '')
render = 'use std::collections::HashSet;\n' + render + experimental
settings = (ROOT / 'src/settings/mod.rs').read_text()
method = settings[settings.index('    pub fn apply_trueos_bringup_profile'):settings.index('    pub fn load(')]
# Retain only the actual method, excluding the following load documentation.
method = method[:method.rfind('        Ok(())\n    }') + len('        Ok(())\n    }')]
method = method.replace('include_str!("../../trueos-bringup-profile.json")',
                        f'include_str!("{ROOT / "trueos-bringup-profile.json"}")')
code = '''#![allow(dead_code)]
use serde::{Serialize, Deserialize};
mod common { pub struct ViewDistances { pub terrain: u32, pub entity: u32 } }
mod client { pub const MAX_SELECTABLE_VIEW_DISTANCE: u32 = 65; }
mod window {
 use serde::{Serialize, Deserialize};
 #[derive(Clone, Debug, Default, Serialize, Deserialize)]
 pub struct WindowSettings { pub size: [u32; 2] }
 #[derive(Clone, Debug, Default, Serialize, Deserialize)]
 pub struct FullScreenSettings { pub enabled: bool }
}
mod render { @RENDER@ }
mod graphics { @GRAPHICS@ }
#[derive(Serialize, Deserialize)]
struct Settings { graphics: graphics::GraphicsSettings, username: String }
impl Settings { @METHOD@ }
#[test]
fn startup_overrides_saved_workload_without_locking_runtime_edits() {
 let mut settings = Settings { graphics: graphics::GraphicsSettings::default().into_ultra(), username: "t4ce".into() };
 settings.graphics.window.size = [1234, 567];
 settings.apply_trueos_bringup_profile().unwrap();
 assert_eq!(settings.graphics.terrain_view_distance, 1);
 assert_eq!(settings.graphics.window.size, [1234, 567]);
 assert_eq!(settings.username, "t4ce");
 assert!(!settings.graphics.particles_enabled);
 assert!(!settings.graphics.weapon_trails_enabled);
 assert!(!settings.graphics.render_mode.rain_enabled);
 assert!(settings.graphics.render_mode.experimental_shaders.contains(&render::ExperimentalShader::BareMinimum));
 assert_eq!(settings.graphics.render_mode.cloud, render::CloudMode::Flat);
 assert_eq!(settings.graphics.render_mode.aa, render::AaMode::None);
 assert_eq!(settings.graphics.render_mode.upscale_mode.factor, 0.1);
 settings.graphics.terrain_view_distance = 16;
 settings.graphics.particles_enabled = true;
 assert_eq!(settings.graphics.terrain_view_distance, 16);
 settings.apply_trueos_bringup_profile().unwrap();
 assert_eq!(settings.graphics.terrain_view_distance, 1);
 assert!(!settings.graphics.particles_enabled);
}
#[test]
fn legacy_cloud_modes_and_presets_use_flat() {
 for mode in ["None", "Flat", "Minimal", "Low", "Medium", "High", "Ultra", "FutureQuality"] {
  let cloud: render::CloudMode = serde_json::from_value(serde_json::Value::String(mode.into())).unwrap();
  assert_eq!(cloud, render::CloudMode::Flat);
  let saved: render::CloudMode = ron::from_str(mode).unwrap();
  assert_eq!(saved, render::CloudMode::Flat);
  assert_eq!(serde_json::to_value(cloud).unwrap(), serde_json::Value::String("Flat".into()));
 }
 for graphics in [
  graphics::GraphicsSettings::default(),
  graphics::GraphicsSettings::default().into_minimal(),
  graphics::GraphicsSettings::default().into_low(),
  graphics::GraphicsSettings::default().into_medium(),
  graphics::GraphicsSettings::default().into_high(),
  graphics::GraphicsSettings::default().into_ultra(),
 ] {
  assert_eq!(graphics.render_mode.cloud, render::CloudMode::Flat);
 }
}

'''.replace('@RENDER@', render).replace('@GRAPHICS@', (ROOT / 'src/settings/graphics.rs').read_text().replace('use common::ViewDistances;', 'use crate::common::ViewDistances;')).replace('@METHOD@', method)
with tempfile.TemporaryDirectory(prefix='voxy-bringup-profile-') as tmp:
    directory = Path(tmp)
    (directory / 'src').mkdir()
    (directory / 'Cargo.toml').write_text('[package]\nname="voxy-bringup-profile-check"\nversion="0.1.0"\nedition="2024"\n[workspace]\n[dependencies]\nserde={version="1",features=["derive"]}\nserde_json="1"\nron="0.12"\n')
    (directory / 'src/lib.rs').write_text(code)
    subprocess.run(['cargo', 'test', '--offline', '--manifest-path', str(directory / 'Cargo.toml')], check=True)
