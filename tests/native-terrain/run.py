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
# Compile the actual server admission expression, rather than a second copy
# of the client's approximation. Catch server-policy drift without wire changes.
server = (root.parents[2] / 'TRUEOS-Blueprints/vendor/veloren/server/src/sys/msg/terrain.rs').read_text()
start = server.index('pos.0.xy().map(|e| e as f64).distance_squared(')
end = server.index('.powi(2)', start) + len('.powi(2)')
(root / 'target/server_prefetch_rule.rs').write_text(r'''
struct Pos(vek::Vec3<f32>);
struct ViewDistance;
impl ViewDistance {fn current(&self)->u32{1}}
struct Presence {terrain_view_distance:ViewDistance}
struct TerrainChunkSize;
impl TerrainChunkSize {const RECT_SIZE:vek::Vec2<u32>=vek::Vec2 {x:32,y:32};}
fn actual_server_admits(position:[f32;2],key:[i32;2])->bool {
 let pos=Pos(vek::Vec3::new(position[0],position[1],0.));
 let key:vek::Vec2<i32>=key.into();
 let presence=Presence{terrain_view_distance:ViewDistance};
''' + server[start:end] + r'''
}
#[test] fn warm_requests_fit_actual_server_policy_even_with_one_block_position_lag() {
 for x in -32..=32 {for y in -32..=32 {
  let position=[x as f32,y as f32];
  let plan=crate::terrain_prefetch::plan(position,[8.,8.],[32;2]).unwrap();
  for candidate in plan.warm {
   for lag in [[0.,0.],[1.,0.],[-1.,0.],[0.,1.],[0.,-1.]] {
    assert!(actual_server_admits([position[0]+lag[0],position[1]+lag[1]],candidate.key));
   }
  }
 }}
}
''')
env = os.environ.copy()
env['RUSTFLAGS'] = ('--cfg target_os="trueos" -Aexplicit_builtin_cfgs_in_flags '
                    '--check-cfg=cfg(target_os,values("trueos"))')
subprocess.run(['cargo', 'test', '--offline'], cwd=Path(__file__).resolve().parent,
               env=env, check=True)
