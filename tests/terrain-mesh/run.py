#!/usr/bin/env python3
"""Run production voxel meshing against a small loaded/missing chunk facade."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
source = (root / 'src/headless/scene.rs').read_text()
mesh = source[source.index('const CHUNK_RADIUS:'):source.index('pub(crate) struct Scene')]
mesh += source[source.index('struct MeshSource {'):source.index('#[derive(Clone, Copy, Debug)]')]
mesh += source[source.index('const FACES:'):source.index('#[cfg(test)]\nmod tests')]
facade = r'''
#![allow(dead_code)]
use vek::{Rgb, Vec2, Vec3};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use std::sync::Arc;
#[derive(Clone)]
struct Block { color: Option<Rgb<u8>>, solid: bool }
#[allow(non_snake_case)]
fn Block(color: Option<Rgb<u8>>) -> Block { Block { solid: color.is_some(), color } }
impl Block {
 fn is_solid(&self) -> bool { self.solid }
 fn get_color(&self) -> Option<Rgb<u8>> { self.color }
}
#[derive(Clone)]
struct TerrainGrid {
 loaded: HashSet<Vec2<i32>>, blocks: HashMap<Vec3<i32>, Block>,
 air: Block, base: Block, filled_base: bool, bounds: Arc<ChunkBounds>,
}
struct ChunkBounds { min: i32, max: i32 }
impl ChunkBounds { fn get_min_z(&self)->i32 {self.min} fn get_max_z(&self)->i32 {self.max} }
impl TerrainGrid {
 fn chunk_size() -> Vec2<u32> { Vec2::new(32,32) }
 fn chunk_key(pos: Vec2<i32>) -> Vec2<i32> { pos.map(|p| p.div_euclid(32)) }
 fn key_chunk(key: Vec2<i32>) -> Vec2<i32> {key*32}
 fn get_key_arc(&self,key:Vec2<i32>)->Option<&Arc<ChunkBounds>> {
  self.loaded.contains(&key).then_some(&self.bounds)
 }
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
  air: Block(None), base: Block(Some(Rgb::new(31,140,47))), filled_base: false,
  bounds: Arc::new(ChunkBounds {min:-48,max:128}) }
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
 assert!(mesh.atlas.texels.iter().any(|c| c[0].abs_diff(31)<=6 && c[1].abs_diff(140)<=6 && c[2].abs_diff(47)<=6));
}
#[test] fn genuine_cliff_against_loaded_air_is_preserved() {
 let mut terrain=grid(); terrain.filled_base=true;
 terrain.loaded.insert(Vec2::new(1,0));
 // Explicit air overrides the otherwise solid base in the neighboring column.
 for z in -49..1 { for y in 0..32 { terrain.blocks.insert(Vec3::new(32,y,z),Block(None)); } }
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
 assert!(!mesh.atlas.tiles.contains_key(&[[0,0,0];9]));
}
#[test] fn real_black_voxel_color_is_preserved() {
 let mut terrain=grid(); let pos=Vec3::new(16,16,0);
 terrain.blocks.insert(pos,Block(Some(Rgb::zero())));
 let mesh=terrain_mesh(&terrain,pos);
 assert_eq!(mesh.vertices.len(),36);
 assert!(mesh.atlas.tiles.contains_key(&[[0,0,0];9]));
 for vertex in &mesh.vertices {
  let x=(vertex.atlas_uv[0]*ATLAS_SIZE as f32).floor() as usize;
  let y=(vertex.atlas_uv[1]*ATLAS_SIZE as f32).floor() as usize;
  assert_eq!(mesh.atlas.texels[y*ATLAS_SIZE as usize+x],[0,0,0,255]);
 }
}

#[test] fn tiles_follow_face_positions_and_stay_fixed_across_camera_centers() {
 let mut terrain=grid(); let pos=Vec3::new(16,16,0);
 terrain.blocks.insert(pos,Block(Some(Rgb::new(120,150,80))));
 let mesh=terrain_mesh(&terrain,pos);
 assert_eq!(mesh.vertices.len(),36);
 let moved=terrain_mesh(&terrain,pos+Vec3::new(2,-3,1));
 assert_eq!(mesh.atlas.texels,moved.atlas.texels);
 for (side,face) in mesh.vertices.chunks_exact(6).enumerate() {
  let expected=face_tile(&corners([16.,16.,0.],[17.,17.,1.]),side,face_color(Rgb::new(120,150,80),side));
  assert!(expected.iter().any(|c| *c!=expected[0]), "side {side} should have variation");
  for row in 0..3 { for col in 0..3 {
   let u=(col as f32+0.5)/3.; let v=(row as f32+0.5)/3.;
   let uv=std::array::from_fn::<_,2,_>(|axis| face[0].atlas_uv[axis]
     +(face[1].atlas_uv[axis]-face[0].atlas_uv[axis])*u
     +(face[5].atlas_uv[axis]-face[0].atlas_uv[axis])*v);
   let x=(uv[0]*ATLAS_SIZE as f32).floor() as usize;
   let y=(uv[1]*ATLAS_SIZE as f32).floor() as usize;
   assert_eq!(&mesh.atlas.texels[y*ATLAS_SIZE as usize+x][..3],&expected[row*3+col]);
  }}
 }
}
#[test] fn tiles_match_chunk_local_noise_on_negative_and_positive_boundaries() {
 let color=[120,150,80];
 for x in [-33,-32,-1,0,31,32] {
  for side in 0..6 {
   let a=face_tile(&corners([x as f32,-1.,0.],[x as f32+1.,0.,1.]),side,color);
   let b=face_tile(&corners([x as f32+32.,31.,0.],[x as f32+33.,32.,1.]),side,color);
   assert_eq!(a,b);
  }
 }
 // GLSL fract differs from Rust fract for negative values.
 assert!((terrain_hash([-3.,2.,-1.,0.])-(-0.1630859375)).abs()<0.000001);
}
#[test] fn atlas_fits_the_full_face_budget_and_deduplicates_tiles() {
 let mut atlas=PaletteAtlas::new();
 for key in 0..MAX_VERTICES as u32/6 {
  let tile=[[key as u8,(key>>8) as u8,(key>>16) as u8];9];
  let slot=atlas.tile(tile); assert_eq!(slot,atlas.tile(tile));
  let x=slot%TILES_PER_ROW*TILE_SIZE; let y=slot/TILES_PER_ROW*TILE_SIZE;
  assert!(x+3<=ATLAS_SIZE && y+3<=ATLAS_SIZE);
  assert_eq!(&atlas.texels[((y+2)*ATLAS_SIZE+x+2) as usize][..3],&tile[8]);
 }
 assert_eq!(atlas.tiles.len(),MAX_VERTICES/6+1);
}

#[test] fn all_nine_chunks_use_full_stored_height_and_exclude_the_tenth() {
 let mut terrain=grid();
 for y in -1..=1 { for x in -1..=1 {
  terrain.loaded.insert(Vec2::new(x,y));
  // Edges and heights outside the former player-centered crop.
  terrain.blocks.insert(Vec3::new(x*32+1,y*32+1,100),Block(Some(Rgb::new(31,140,47))));
  terrain.blocks.insert(Vec3::new(x*32+30,y*32+30,-40),Block(Some(Rgb::new(31,140,47))));
 }}
 terrain.loaded.insert(Vec2::new(2,0));
 terrain.blocks.insert(Vec3::new(65,1,100),Block(Some(Rgb::new(31,140,47))));
 let mesh=terrain_mesh(&terrain,Vec3::zero());
 assert_eq!(mesh.vertices.len(),18*36);
 assert!(!mesh.truncated);
 assert!(mesh.vertices.iter().any(|v| v.position[0] < -30.));
 assert!(mesh.vertices.iter().any(|v| v.position[0]>60.));
 assert!(mesh.vertices.iter().any(|v| v.position[2]==101.));
 assert!(mesh.vertices.iter().any(|v| v.position[2]==-40.));
 assert!(mesh.vertices.iter().all(|v| v.position[0]<64.));
 let moved=terrain_mesh(&terrain,Vec3::new(31,31,50));
 assert_eq!(mesh.atlas.texels,moved.atlas.texels);
 assert_eq!(mesh.vertices.iter().map(|v|v.position).collect::<Vec<_>>(),
     moved.vertices.iter().map(|v|v.position).collect::<Vec<_>>());
 assert_eq!(terrain_xy_bounds(Vec3::new(-1,-1,0)),(Vec2::new(-64,-64),Vec2::new(32,32)));
}
#[test] fn mesh_cache_uses_chunk_boundaries_and_detects_halo_updates() {
 let mut terrain=grid(); let center=Vec3::new(0,0,0);
 let source=MeshSource {terrain:Arc::new(terrain.clone()),center};
 assert!(source.matches(&terrain,Vec3::new(31,31,100)));
 assert!(!source.matches(&terrain,Vec3::new(32,31,100)));
 assert!(!source.matches(&terrain,Vec3::new(-1,0,0)));
 terrain.loaded.insert(Vec2::new(2,2)); // diagonal halo not read
 assert!(source.matches(&terrain,center));
 terrain.loaded.insert(Vec2::new(2,1)); // face halo read
 assert!(!source.matches(&terrain,center));
 let source=MeshSource {terrain:Arc::new(terrain.clone()),center};
 terrain.bounds=Arc::new(ChunkBounds {min:-48,max:200});
 assert!(!source.matches(&terrain,center));
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
