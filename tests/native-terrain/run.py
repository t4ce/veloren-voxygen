#!/usr/bin/env python3
"""Run native terrain ownership and camera checks through an inert host facade."""
from pathlib import Path
import os
import subprocess

root = Path(__file__).resolve().parent
source = (root / '../../src/render/flat_cloud_native.rs').read_text()
(root / 'target').mkdir(exist_ok=True)
(root / 'target/cloud_transport.rs').write_text(
    'pub struct Frame { pub pixels: std::sync::Arc<[u8]>, pub width: u32, pub height: u32, pub camera: [[f32;4];5], pub in_world: bool }\n'
    + source[source.index('pub(crate) struct NativeClouds'):])
# Exercise the actual camera setter with a small state facade, including the
# first-person boundary that must not introduce a 2.35-block reset.
camera = (root / '../../src/scene/camera.rs').read_text()
method = camera[camera.index('    #[cfg(target_os = "trueos")]\n    pub fn set_distance_continuous'):camera.index('    pub fn update(&mut self, time:')]
(root / 'target/camera_zoom.rs').write_text('''
#![allow(dead_code)]
const MIN_ZOOM: f32 = 0.1;
#[derive(Debug, PartialEq)] enum CameraMode { FirstPerson, ThirdPerson }
struct Camera { dist: f32, tgt_dist: f32, mode: CameraMode }
''' + 'impl Camera {\n' + method + '}\n' + '''
#[test] fn continuous_distance_crosses_modes_without_resetting_or_jumping() {
 let mut camera=Camera {dist:0.1,tgt_dist:0.1,mode:CameraMode::FirstPerson};
 for distance in [0.1,0.11,0.2,1.0,2.34,2.35,2.36,10.0,160.0,2.0,0.2,0.1] {
  camera.set_distance_continuous(distance);
  assert_eq!(camera.dist,distance); assert_eq!(camera.tgt_dist,distance);
 }
 assert_eq!(camera.mode,CameraMode::FirstPerson);
 camera.set_distance_continuous(f32::NAN); assert_eq!(camera.dist,0.1);
}
''')
env = os.environ.copy()
env['RUSTFLAGS'] = ('--cfg target_os="trueos" -Aexplicit_builtin_cfgs_in_flags '
                    '--check-cfg=cfg(target_os,values("trueos"))')
subprocess.run(['cargo', 'test', '--offline'], cwd=Path(__file__).resolve().parent,
               env=env, check=True)
