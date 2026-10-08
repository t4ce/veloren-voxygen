#!/usr/bin/env python3
"""Run production voxel meshing against a small loaded/missing chunk facade."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
source = (root / 'src/headless/scene.rs').read_text()
mesh = source[source.index('const RADIUS:'):source.index('pub(crate) struct Scene')]
mesh += source[source.index('const FACES:'):source.index('#[cfg(test)]\nmod tests')]
facade = r'''
#![allow(dead_code)]
use vek::{Rgb, Vec2, Vec3};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
struct Block { color: Option<Rgb<u8>>, solid: bool }
#[allow(non_snake_case)]
fn Block(color: Option<Rgb<u8>>) -> Block { Block { solid: color.is_some(), color } }
impl Block {
 fn is_solid(&self) -> bool { self.solid }
 fn get_color(&self) -> Option<Rgb<u8>> { self.color }
}
struct TerrainGrid {
 loaded: HashSet<Vec2<i32>>, blocks: HashMap<Vec3<i32>, Block>,
 air: Block, base: Block, filled_base: bool,
}
trait ReadVol { fn get(&self, pos: Vec3<i32>) -> Result<&Block, ()>; }
impl ReadVol for TerrainGrid {
 fn get(&self, pos: Vec3<i32>) -> Result<&Block, ()> {
  if !self.loaded.contains(&Vec2::new(pos.x.div_euclid(32),pos.y.div_euclid(32))) { return Err(()); }
  Ok(self.blocks.get(&pos).unwrap_or(if self.filled_base && pos.z < 1 { &self.base } else { &self.air }))
 }
}
fn grid() -> TerrainGrid {
 TerrainGrid { loaded: HashSet::from([Vec2::zero()]), blocks: HashMap::new(),
  air: Block(None), base: Block(Some(Rgb::new(31,140,47))), filled_base: false }
}
'''
tests = r'''
#[test] fn missing_chunk_does_not_create_a_face_but_known_air_does() {
 let mut terrain=grid();
 terrain.blocks.insert(Vec3::new(31,16,0),Block(Some(Rgb::new(31,140,47))));
 let center=Vec3::new(31,16,0);
 let pending=terrain_mesh(&terrain,center);
 assert_eq!(pending.vertices.len(),30); // +X neighbor is unknown
 assert!(!pending.vertices.chunks_exact(6).any(|face| face.iter().all(|v| v.position[0]==32.)));
 terrain.loaded.insert(Vec2::new(1,0));
 let air=terrain_mesh(&terrain,center);
 assert_eq!(air.vertices.len(),36);
 assert!(air.vertices.chunks_exact(6).any(|face| face.iter().all(|v| v.position[0]==32.)));
 terrain.blocks.insert(Vec3::new(32,16,0),Block(Some(Rgb::new(31,140,47))));
 assert_eq!(terrain_mesh(&terrain,center).vertices.len(),60); // shared face hidden
}
#[test] fn solid_chunk_base_has_no_artificial_curtain_at_missing_neighbors() {
 let mut terrain=grid(); terrain.filled_base=true;
 let mesh=terrain_mesh(&terrain,Vec3::new(16,16,1));
 assert_eq!(mesh.vertices.len(),32*32*6); // only the actual top surface
 assert!(mesh.vertices.iter().all(|v| v.position[2]==1.));
 assert!(mesh.atlas.texels.contains(&[31,140,47,255]));
}
#[test] fn genuine_cliff_against_loaded_air_is_preserved() {
 let mut terrain=grid(); terrain.filled_base=true;
 terrain.loaded.insert(Vec2::new(1,0));
 // Explicit air overrides the otherwise solid base in the neighboring column.
 for z in -31..1 { for y in 0..32 { terrain.blocks.insert(Vec3::new(32,y,z),Block(None)); } }
 let mesh=terrain_mesh(&terrain,Vec3::new(16,16,1));
 assert!(mesh.vertices.chunks_exact(6).any(|face| face.iter().all(|v| v.position[0]==32.) && face.iter().any(|v| v.position[2]<0.)));
}

#[test] fn colorless_solid_sprites_are_not_black_cubes_or_occluders() {
 let mut terrain=grid();
 let pos=Vec3::new(16,16,0);
 terrain.blocks.insert(pos,Block { solid:true,color:None });
 let mesh=terrain_mesh(&terrain,pos);
 assert!(mesh.vertices.is_empty());
 terrain.blocks.insert(pos+Vec3::unit_x(),Block(Some(Rgb::new(31,140,47))));
 let mesh=terrain_mesh(&terrain,pos);
 assert_eq!(mesh.vertices.len(),36); // includes the face next to the sprite
 assert!(mesh.vertices.chunks_exact(6).any(|face| face.iter().all(|v| v.position[0]==17.)));
 assert!(!mesh.atlas.colors.contains_key(&[0,0,0]));
}
#[test] fn real_black_voxel_color_is_preserved() {
 let mut terrain=grid(); let pos=Vec3::new(16,16,0);
 terrain.blocks.insert(pos,Block(Some(Rgb::zero())));
 let mesh=terrain_mesh(&terrain,pos);
 assert_eq!(mesh.vertices.len(),36);
 assert!(mesh.atlas.colors.contains_key(&[0,0,0]));
 for vertex in &mesh.vertices {
  let x=(vertex.atlas_uv[0]*ATLAS_SIZE as f32).floor() as usize;
  let y=(vertex.atlas_uv[1]*ATLAS_SIZE as f32).floor() as usize;
  assert_eq!(mesh.atlas.texels[y*ATLAS_SIZE as usize+x],[0,0,0,255]);
 }
}
'''
with tempfile.TemporaryDirectory(prefix='voxy-terrain-mesh-') as directory:
    folder = Path(directory)
    (folder / 'src').mkdir()
    (folder / 'Cargo.toml').write_text('[package]\nname="voxy-terrain-mesh-tests"\nversion="0.1.0"\nedition="2024"\n'
        '[workspace]\n[dependencies]\nbytemuck={version="1",features=["derive"]}\nvek="0.17"\n')
    (folder / 'src/lib.rs').write_text(facade + mesh + tests)
    subprocess.run(['cargo','test','--offline','--target','x86_64-unknown-linux-gnu',
        '--target-dir',str(root / 'target/terrain-mesh-tests')],cwd=folder,check=True)
