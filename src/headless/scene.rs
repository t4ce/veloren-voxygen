//! Mesh and camera preparation shared by Ubuntu and TRUEOS.
use crate::client::Client;
use common::{comp, terrain::TerrainGrid, vol::ReadVol};
use specs::{Join, WorldExt};
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use vek::{Rgb, Vec2, Vec3, Vec4};

pub(super) fn placement(display_width: u32, display_height: u32) -> (i32, i32, u32, u32) {
    let units = (display_width / 32).min(display_height / 18).clamp(1, 80);
    let (width, height) = (units * 16, units * 9);
    (
        ((display_width.saturating_sub(width)) / 2) as i32,
        ((display_height.saturating_sub(height)) / 2) as i32,
        width,
        height,
    )
}

const RADIUS: i32 = 40;
const VERTICAL_RADIUS: i32 = 32;
pub(super) const MAX_VERTICES: usize = 600_000;
pub(super) const ATLAS_SIZE: u32 = 512;
const MESH_INTERVAL: Duration = Duration::from_millis(500);
const PROXY_COLOR: [u8; 3] = [230, 140, 64];

// There cannot be more distinct colors than exposed faces. Each face has
// six vertices; one additional texel is reserved for the entity proxies.
const _: () = assert!(MAX_VERTICES / 6 + 1 <= (ATLAS_SIZE * ATLAS_SIZE) as usize);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    // Homogeneous world position (w=1).
    pub(super) position: [f32; 4],
    // Normalized texel-center UV, followed by the opaque vertex contract 0,1.
    pub(super) atlas_uv: [f32; 4],
}

struct Mesh {
    vertices: Vec<Vertex>,
    atlas: PaletteAtlas,
    truncated: bool,
}

struct PaletteAtlas {
    texels: Vec<[u8; 4]>,
    colors: HashMap<[u8; 3], u32>,
}

impl PaletteAtlas {
    fn new() -> Self {
        let mut texels = vec![[0, 0, 0, 255]; (ATLAS_SIZE * ATLAS_SIZE) as usize];
        texels[0] = [PROXY_COLOR[0], PROXY_COLOR[1], PROXY_COLOR[2], 255];
        Self {
            texels,
            colors: HashMap::from([(PROXY_COLOR, 0)]),
        }
    }

    fn color_uv(&mut self, color: [u8; 3]) -> [f32; 4] {
        let next = self.colors.len() as u32;
        let index = *self.colors.entry(color).or_insert_with(|| {
            assert!(next < ATLAS_SIZE * ATLAS_SIZE, "terrain palette capacity");
            self.texels[next as usize] = [color[0], color[1], color[2], 255];
            next
        });
        atlas_uv(index)
    }
}

fn atlas_uv(index: u32) -> [f32; 4] {
    [
        (index % ATLAS_SIZE) as f32 / ATLAS_SIZE as f32 + 0.5 / ATLAS_SIZE as f32,
        (index / ATLAS_SIZE) as f32 / ATLAS_SIZE as f32 + 0.5 / ATLAS_SIZE as f32,
        0.0,
        1.0,
    ]
}

fn face_color(color: Rgb<u8>, side: usize) -> [u8; 3] {
    // Retain the minimal renderer's axis contrast without adding lights.
    // The color itself is the same voxel RGB used by full Voxygen's atlas.
    let factor = match side {
        0 | 1 => 0.70,
        2 | 3 => 0.82,
        _ => 1.0,
    };
    [color.r, color.g, color.b].map(|channel| (channel as f32 * factor).round() as u8)
}

pub(super) struct Scene {
    terrain: Mesh,
    pending_mesh: Option<mpsc::Receiver<Mesh>>,
    next_mesh: Instant,
    revision: u64,
    next_missing_log: Instant,
    mesh_source: Option<MeshSource>,
}

struct MeshSource {
    // Retain the exact worker snapshot: live chunk edits must copy on write,
    // so pointer identity changes even after the worker has finished.
    terrain: Arc<TerrainGrid>,
    center: Vec3<i32>,
}

impl MeshSource {
    fn matches(&self, terrain: &TerrainGrid, center: Vec3<i32>) -> bool {
        if self.center != center {
            return false;
        }
        let min = Vec2::new(center.x - RADIUS, center.y - RADIUS);
        let max = Vec2::new(center.x + RADIUS - 1, center.y + RADIUS - 1);
        let first = TerrainGrid::chunk_key(min - 1);
        let last = TerrainGrid::chunk_key(max + 1);
        let size = TerrainGrid::chunk_size().map(|value| value as i32);
        for y in first.y..=last.y {
            for x in first.x..=last.x {
                let key = Vec2::new(x, y);
                let chunk_min = TerrainGrid::key_chunk(key);
                let chunk_max = chunk_min + size - 1;
                // Face culling reads one neighbor along a single axis; corner
                // chunks that touch only the diagonal halo are never sampled.
                let overlaps_x = chunk_min.x <= max.x && chunk_max.x >= min.x;
                let overlaps_y = chunk_min.y <= max.y && chunk_max.y >= min.y;
                if !overlaps_x && !overlaps_y {
                    continue;
                }
                match (self.terrain.get_key_arc(key), terrain.get_key_arc(key)) {
                    (Some(old), Some(current)) if Arc::ptr_eq(old, current) => {}
                    (None, None) => {}
                    _ => return false,
                }
            }
        }
        true
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrameInfo {
    pub(super) terrain_revision: u64,
    pub(super) terrain_vertices: u32,
    pub(super) overlay_vertices: u32,
    pub(super) position: Option<Vec3<f32>>,
}

pub(super) struct PreparedFrame<'a> {
    pub(super) camera: [[f32; 4]; 5],
    pub(super) terrain: &'a [Vertex],
    pub(super) atlas: &'a [[u8; 4]],
    pub(super) overlay: Vec<Vertex>,
    pub(super) revision: u64,
}

impl Scene {
    pub(super) fn new() -> Self {
        Self {
            terrain: Mesh {
                vertices: Vec::new(),
                atlas: PaletteAtlas::new(),
                truncated: false,
            },
            pending_mesh: None,
            next_mesh: Instant::now(),
            revision: 0,
            next_missing_log: Instant::now() + Duration::from_secs(5),
            mesh_source: None,
        }
    }

    fn needs_mesh(&self, terrain: &TerrainGrid, center: Vec3<i32>, now: Instant) -> bool {
        self.pending_mesh.is_none()
            && now >= self.next_mesh
            && self
                .mesh_source
                .as_ref()
                .is_none_or(|source| !source.matches(terrain, center))
    }

    pub(super) fn prepare(
        &mut self,
        client: Option<&Client>,
        yaw: f32,
        pitch: f32,
        width: u32,
        height: u32,
    ) -> PreparedFrame<'_> {
        let position = client.and_then(Client::position);
        if position.is_none() {
            if !self.terrain.vertices.is_empty() {
                self.terrain.vertices.clear();
                self.revision = self.revision.wrapping_add(1);
            }
            // Discard pending work from the previous session after disconnect.
            self.pending_mesh = None;
            self.mesh_source = None;
            self.next_mesh = Instant::now();
            self.next_missing_log = Instant::now() + Duration::from_secs(5);
        }
        if let Some(mesh) = self.pending_mesh.as_ref().and_then(|r| r.try_recv().ok()) {
            self.pending_mesh = None;
            if mesh.truncated && !self.terrain.truncated {
                super::connection_progress(format_args!(
                    "Voxygen geometry: mesh budget reached; some geometry omitted"
                ));
            }
            let was_empty = self.terrain.vertices.is_empty();
            self.terrain = mesh;
            self.revision = self.revision.wrapping_add(1);
            if was_empty != self.terrain.vertices.is_empty() {
                super::connection_progress(format_args!(
                    "Voxygen terrain: revision={} vertices={} ready={} truncated={} palette_colors={}",
                    self.revision,
                    self.terrain.vertices.len(),
                    !self.terrain.vertices.is_empty(),
                    self.terrain.truncated,
                    self.terrain.atlas.colors.len(),
                ));
            }
        }
        if let (Some(client), Some(position)) = (client, position) {
            if self.terrain.vertices.is_empty() && Instant::now() >= self.next_missing_log {
                self.next_missing_log = Instant::now() + Duration::from_secs(5);
                super::connection_progress(format_args!(
                    "Voxygen terrain waiting: position={position:?} loaded_chunks={} revision={} pending_mesh={}",
                    client.state().terrain().iter().count(),
                    self.revision,
                    self.pending_mesh.is_some(),
                ));
            }
            let center = position.map(|v| v.floor() as i32);
            let current_terrain = client.state().terrain();
            if self.needs_mesh(&current_terrain, center, Instant::now()) {
                // Arc-backed chunk snapshot: meshing never blocks input or the network tick.
                let terrain = Arc::new((*current_terrain).clone());
                self.mesh_source = Some(MeshSource {
                    terrain: Arc::clone(&terrain),
                    center,
                });
                let (sender, receiver) = mpsc::channel();
                self.pending_mesh = Some(receiver);
                self.next_mesh = Instant::now() + MESH_INTERVAL;
                std::thread::spawn(move || {
                    let _ = sender.send(terrain_mesh(&terrain, center));
                });
            }
        }
        let eye = position.unwrap_or_default() + Vec3::new(0.0, 0.0, 1.65);
        let forward = Vec3::new(
            yaw.sin() * pitch.cos(),
            yaw.cos() * pitch.cos(),
            pitch.sin(),
        );
        let right = Vec3::new(yaw.cos(), -yaw.sin(), 0.0);
        let up = right.cross(forward);
        let camera = [
            Vec4::from(eye).with_w(0.0).into_array(),
            Vec4::from(right).with_w(0.0).into_array(),
            Vec4::from(up).with_w(0.0).into_array(),
            Vec4::from(forward).with_w(0.0).into_array(),
            [
                1.0 / (65.0_f32.to_radians() * 0.5).tan(),
                width as f32 / height as f32,
                0.1,
                256.0,
            ],
        ];
        let mut overlay = Vec::new();
        if let Some(client) = client {
            let ecs = client.state().ecs();
            let positions = ecs.read_storage::<comp::Pos>();
            let bodies = ecs.read_storage::<comp::Body>();
            for (entity, pos, _) in (&ecs.entities(), &positions, &bodies).join().take(512) {
                if entity != client.entity() && (pos.0 - eye).magnitude_squared() < 128.0 * 128.0 {
                    let p = pos.0;
                    cuboid(
                        &mut overlay,
                        [p.x - 0.35, p.y - 0.35, p.z],
                        [p.x + 0.35, p.y + 0.35, p.z + 1.8],
                        atlas_uv(0),
                    );
                }
            }
        }
        PreparedFrame {
            camera,
            terrain: &self.terrain.vertices,
            atlas: &self.terrain.atlas.texels,
            overlay,
            revision: self.revision,
        }
    }
}

// Faces share vertices conceptually; expand triangles to avoid an index buffer.
const FACES: [[usize; 4]; 6] = [
    [0, 4, 6, 2],
    [1, 3, 7, 5],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 2, 3, 1],
    [4, 5, 7, 6],
];
const NEIGHBORS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

fn corners(min: [f32; 3], max: [f32; 3]) -> [[f32; 4]; 8] {
    std::array::from_fn(|i| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
            1.0,
        ]
    })
}

fn face(vertices: &mut Vec<Vertex>, corners: &[[f32; 4]; 8], side: usize, atlas_uv: [f32; 4]) {
    for index in [0, 1, 2, 0, 2, 3] {
        vertices.push(Vertex {
            position: corners[FACES[side][index]],
            atlas_uv,
        });
    }
}

fn cuboid(vertices: &mut Vec<Vertex>, min: [f32; 3], max: [f32; 3], atlas_uv: [f32; 4]) {
    let corners = corners(min, max);
    for side in 0..6 {
        face(vertices, &corners, side, atlas_uv);
    }
}

fn terrain_mesh(terrain: &TerrainGrid, center: Vec3<i32>) -> Mesh {
    voxel_mesh(center, |pos| {
        terrain.get(pos).ok().and_then(|block| {
            block
                .is_solid()
                .then(|| block.get_color().unwrap_or_else(Rgb::zero))
        })
    })
}

fn voxel_mesh(center: Vec3<i32>, voxel_color: impl Fn(Vec3<i32>) -> Option<Rgb<u8>>) -> Mesh {
    let mut vertices = Vec::new();
    let mut atlas = PaletteAtlas::new();
    for z in center.z - VERTICAL_RADIUS..center.z + VERTICAL_RADIUS {
        for y in center.y - RADIUS..center.y + RADIUS {
            for x in center.x - RADIUS..center.x + RADIUS {
                let pos = Vec3::new(x, y, z);
                let Some(color) = voxel_color(pos) else {
                    continue;
                };
                let corners = corners(
                    [x as f32, y as f32, z as f32],
                    [(x + 1) as f32, (y + 1) as f32, (z + 1) as f32],
                );
                for (side, offset) in NEIGHBORS.iter().enumerate() {
                    if voxel_color(pos + Vec3::from(*offset)).is_some() {
                        continue;
                    }
                    if vertices.len() + 6 > MAX_VERTICES {
                        return Mesh {
                            vertices,
                            atlas,
                            truncated: true,
                        };
                    }
                    let uv = atlas.color_uv(face_color(color, side));
                    face(&mut vertices, &corners, side, uv);
                }
            }
        }
    }
    Mesh {
        vertices,
        atlas,
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_terrain() -> TerrainGrid {
        use common::terrain::{Block, MapSizeLg, SpriteKind, TerrainChunk, TerrainChunkMeta};
        let air = Block::air(SpriteKind::Empty);
        TerrainGrid::new(
            MapSizeLg::new(Vec2::new(5, 5)).unwrap(),
            Arc::new(TerrainChunk::new(0, air, air, TerrainChunkMeta::void())),
        )
        .unwrap()
    }

    fn source_for(terrain: &TerrainGrid, center: Vec3<i32>) -> MeshSource {
        MeshSource {
            terrain: Arc::new(terrain.clone()),
            center,
        }
    }

    #[test]
    fn dirty_source_stationary_skips_and_integer_movement_invalidates() {
        let terrain = empty_terrain();
        let center = Vec3::new(320, 320, 16);
        let mut scene = Scene::new();
        scene.mesh_source = Some(source_for(&terrain, center));
        let due = scene.next_mesh;
        assert!(!scene.needs_mesh(&terrain, center, due + MESH_INTERVAL));
        assert!(scene.needs_mesh(&terrain, center + Vec3::unit_x(), due));
        assert!(scene.needs_mesh(&terrain, center + Vec3::unit_z(), due));
        assert!(!scene.needs_mesh(&terrain, center + Vec3::unit_x(), due - MESH_INTERVAL));
        let (_sender, receiver) = mpsc::channel();
        scene.pending_mesh = Some(receiver);
        assert!(!scene.needs_mesh(&terrain, center + Vec3::unit_x(), due));
    }

    #[test]
    fn dirty_source_detects_relevant_halo_add_remove_and_replacement() {
        let mut terrain = empty_terrain();
        let center = Vec3::new(24, 64, 16);
        // The last meshed x is 63; x=64 is read only for face culling.
        let key = Vec2::new(2, 2);
        let chunk = Arc::clone(terrain.get_key_arc(Vec2::new(-1, -1)).unwrap());
        let absent = source_for(&terrain, center);
        terrain.insert(key, Arc::clone(&chunk));
        assert!(!absent.matches(&terrain, center));
        let loaded = source_for(&terrain, center);
        terrain.remove(key);
        assert!(!loaded.matches(&terrain, center));
        terrain.insert(key, Arc::new((*chunk).clone()));
        assert!(!loaded.matches(&terrain, center));
    }

    #[test]
    fn dirty_source_retained_snapshot_detects_real_copy_on_write_edits() {
        use common::{
            terrain::{Block, BlockKind},
            vol::WriteVol,
        };
        let mut terrain = empty_terrain();
        let center = Vec3::new(320, 320, 16);
        let key = TerrainGrid::chunk_key(center);
        let chunk = Arc::clone(terrain.get_key_arc(Vec2::new(-1, -1)).unwrap());
        terrain.insert(key, chunk);
        let source = source_for(&terrain, center);
        assert!(Arc::ptr_eq(
            source.terrain.get_key_arc(key).unwrap(),
            terrain.get_key_arc(key).unwrap()
        ));
        let color = Rgb::new(31, 140, 47);
        terrain
            .set(center, Block::new(BlockKind::Grass, color))
            .unwrap();
        assert!(!Arc::ptr_eq(
            source.terrain.get_key_arc(key).unwrap(),
            terrain.get_key_arc(key).unwrap()
        ));
        assert!(!source.matches(&terrain, center));
        assert_eq!(source.terrain.get(center).unwrap().get_color(), None);
        assert_eq!(terrain.get(center).unwrap().get_color(), Some(color));
    }

    #[test]
    fn dirty_source_ignores_distant_chunks_and_diagonal_only_halo() {
        let mut terrain = empty_terrain();
        let center = Vec3::new(24, 24, 16);
        let source = source_for(&terrain, center);
        let chunk = Arc::clone(terrain.get_key_arc(Vec2::new(-1, -1)).unwrap());
        let far = Vec2::new(16, 16);
        terrain.insert(far, Arc::clone(&chunk));
        assert!(source.matches(&terrain, center));
        terrain.insert(far, Arc::new((*chunk).clone()));
        assert!(source.matches(&terrain, center));
        terrain.remove(far);
        assert!(source.matches(&terrain, center));
        // Both coordinates are 64: face-neighbor reads never reach this corner.
        terrain.insert(Vec2::new(2, 2), Arc::clone(&chunk));
        assert!(source.matches(&terrain, center));
        terrain.insert(Vec2::new(2, 1), chunk);
        assert!(!source.matches(&terrain, center));
    }

    fn sampled_texel(mesh: &Mesh, vertex: &Vertex) -> [u8; 4] {
        let x = (vertex.atlas_uv[0] * ATLAS_SIZE as f32).floor() as usize;
        let y = (vertex.atlas_uv[1] * ATLAS_SIZE as f32).floor() as usize;
        mesh.atlas.texels[y * ATLAS_SIZE as usize + x]
    }

    #[test]
    fn actual_terrain_block_colors_are_sampled_and_geometry_stays_unchanged() {
        use common::{
            terrain::{Block, BlockKind, MapSizeLg, SpriteKind, TerrainChunk, TerrainChunkMeta},
            vol::WriteVol,
        };
        use std::sync::Arc;
        let air = Block::air(SpriteKind::Empty);
        let empty_chunk = TerrainChunk::new(0, air, air, TerrainChunkMeta::void());
        let mut terrain = TerrainGrid::new(
            MapSizeLg::new(vek::Vec2::new(1, 1)).unwrap(),
            Arc::new(empty_chunk.clone()),
        )
        .unwrap();
        let mut chunk = empty_chunk;
        chunk
            .set(
                Vec3::new(1, 1, 1),
                Block::new(BlockKind::Grass, Rgb::new(31, 140, 47)),
            )
            .unwrap();
        chunk
            .set(
                Vec3::new(2, 1, 1),
                Block::new(BlockKind::Earth, Rgb::new(200, 100, 50)),
            )
            .unwrap();
        terrain.insert(vek::Vec2::zero(), Arc::new(chunk));
        let mesh = terrain_mesh(&terrain, Vec3::new(1, 1, 1));
        // The shared face still disappears, and real server block RGB replaces
        // the old debug axis palette without changing mesh topology.
        assert_eq!(mesh.vertices.len(), 10 * 6);
        assert!(!mesh.truncated);
        let colors: Vec<_> = mesh
            .vertices
            .iter()
            .map(|vertex| sampled_texel(&mesh, vertex))
            .collect();
        assert!(colors.contains(&[31, 140, 47, 255]));
        assert!(colors.contains(&[200, 100, 50, 255]));
        assert!(colors.contains(&[140, 70, 35, 255]));
        assert!(colors.contains(&[164, 82, 41, 255]));
        for vertex in &mesh.vertices {
            assert_eq!(vertex.position[3], 1.0);
            assert_eq!(vertex.atlas_uv[2..], [0.0, 1.0]);
            assert!(vertex.atlas_uv[..2].iter().all(|uv| *uv > 0.0 && *uv < 1.0));
            assert_eq!(sampled_texel(&mesh, vertex)[3], 255);
        }
        assert_eq!(size_of::<Vertex>(), 32);
        assert_eq!(mesh.atlas.texels.len(), (ATLAS_SIZE * ATLAS_SIZE) as usize);
    }

    #[test]
    fn atlas_deduplicates_colors_and_keeps_proxy_texel_and_last_uv_bounded() {
        let mut atlas = PaletteAtlas::new();
        assert_eq!(atlas.color_uv(PROXY_COLOR), atlas_uv(0));
        let first = atlas.color_uv([10, 20, 30]);
        assert_eq!(atlas.color_uv([10, 20, 30]), first);
        assert_eq!(atlas.colors.len(), 2);
        assert_eq!(atlas.texels[0], [230, 140, 64, 255]);
        assert_eq!(atlas.texels[1], [10, 20, 30, 255]);
        for index in [0, ATLAS_SIZE - 1, ATLAS_SIZE, ATLAS_SIZE * ATLAS_SIZE - 1] {
            let uv = atlas_uv(index);
            assert_eq!(
                (uv[0] * ATLAS_SIZE as f32).floor() as u32,
                index % ATLAS_SIZE
            );
            assert_eq!(
                (uv[1] * ATLAS_SIZE as f32).floor() as u32,
                index / ATLAS_SIZE
            );
            assert!(uv[..2].iter().all(|uv| *uv > 0.0 && *uv < 1.0));
        }
    }

    #[test]
    fn atlas_has_room_for_every_budgeted_face_even_without_color_reuse() {
        let mut atlas = PaletteAtlas::new();
        for key in 0..MAX_VERTICES as u32 / 6 {
            let uv = atlas.color_uv([key as u8, (key >> 8) as u8, (key >> 16) as u8]);
            assert!(uv[..2].iter().all(|uv| *uv > 0.0 && *uv < 1.0));
        }
        assert_eq!(atlas.colors.len(), MAX_VERTICES / 6 + 1);
    }

    #[test]
    fn textured_shader_is_exactly_the_backend_package_and_binds_only_one_atlas() {
        let shader = include_bytes!("render_textured.wgsl");
        assert_eq!(
            shader,
            include_bytes!(
                "../../../TRUEOS-Blueprints/crates/trueos-wgpu/src/voxy_headless_textured.wgsl"
            )
        );
        assert_eq!(super::super::shader::fnv1a64(shader), 0xF84D_E655_632E_F102);
        let source = std::str::from_utf8(shader).unwrap();
        assert_eq!(
            source.matches("var atlas_texture: texture_2d<f32>").count(),
            1
        );
        assert_eq!(source.matches("textureSample(").count(), 1);
        assert!(!source.contains("font"));
    }
    #[test]
    fn frame_is_centered_quarter_area_and_stays_bounded_on_large_outputs() {
        assert_eq!(placement(2560, 1440), (640, 360, 1280, 720));
        assert_eq!(placement(1920, 1080), (480, 270, 960, 540));
        assert_eq!(placement(3840, 2160), (1280, 720, 1280, 720));
    }
    #[test]
    fn adjacent_voxels_have_no_internal_faces() {
        let mesh = voxel_mesh(Vec3::zero(), |p| {
            (p == Vec3::zero() || p == Vec3::unit_x()).then(|| Rgb::new(200, 100, 50))
        });
        assert_eq!(mesh.vertices.len(), 10 * 6);
        assert!(!mesh.truncated);
    }
    #[test]
    fn missing_world_has_no_terrain_or_overlay_geometry() {
        assert!(voxel_mesh(Vec3::zero(), |_| None).vertices.is_empty());
        let mut scene = Scene::new();
        scene.mesh_source = Some(source_for(&empty_terrain(), Vec3::zero()));
        let frame = scene.prepare(None, 0.0, 0.0, 640, 480);
        assert!(frame.terrain.is_empty());
        assert!(frame.overlay.is_empty());
        assert!(scene.mesh_source.is_none());
    }
}
