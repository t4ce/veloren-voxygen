//! Mesh and camera preparation shared by Ubuntu and TRUEOS.
use crate::client::Client;
use common::{comp, terrain::TerrainGrid, vol::ReadVol};
use specs::{Join, WorldExt};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use vek::{Vec3, Vec4};

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
const MESH_INTERVAL: Duration = Duration::from_millis(500);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    // Homogeneous world position (w=1).
    pub(super) position: [f32; 4],
    pub(super) color: [f32; 4],
}

struct Mesh {
    vertices: Vec<Vertex>,
    truncated: bool,
}

pub(super) struct Scene {
    terrain: Mesh,
    pending_mesh: Option<mpsc::Receiver<Mesh>>,
    next_mesh: Instant,
    revision: u64,
}

pub(super) struct PreparedFrame<'a> {
    pub(super) camera: [[f32; 4]; 5],
    pub(super) terrain: &'a [Vertex],
    pub(super) overlay: Vec<Vertex>,
    pub(super) revision: u64,
}

impl Scene {
    pub(super) fn new() -> Self {
        Self {
            terrain: Mesh {
                vertices: Vec::new(),
                truncated: false,
            },
            pending_mesh: None,
            next_mesh: Instant::now(),
            revision: 0,
        }
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
            self.next_mesh = Instant::now();
        }
        if let Some(mesh) = self.pending_mesh.as_ref().and_then(|r| r.try_recv().ok()) {
            self.pending_mesh = None;
            if mesh.truncated && !self.terrain.truncated {
                super::connection_progress(format_args!(
                    "Voxygen geometry: mesh budget reached; some geometry omitted"
                ));
            }
            self.terrain = mesh;
            self.revision = self.revision.wrapping_add(1);
        }
        if let (Some(client), Some(position)) = (client, position) {
            if self.pending_mesh.is_none() && Instant::now() >= self.next_mesh {
                // Arc-backed chunk snapshot: meshing never blocks input or the network tick.
                let terrain = (*client.state().terrain()).clone();
                let center = position.map(|v| v.floor() as i32);
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
                        [0.9, 0.55, 0.25, 1.0],
                    );
                }
            }
        }
        PreparedFrame {
            camera,
            terrain: &self.terrain.vertices,
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

fn face(vertices: &mut Vec<Vertex>, corners: &[[f32; 4]; 8], side: usize, color: [f32; 4]) {
    for index in [0, 1, 2, 0, 2, 3] {
        vertices.push(Vertex {
            position: corners[FACES[side][index]],
            color,
        });
    }
}

fn cuboid(vertices: &mut Vec<Vertex>, min: [f32; 3], max: [f32; 3], color: [f32; 4]) {
    let corners = corners(min, max);
    for side in 0..6 {
        face(vertices, &corners, side, color);
    }
}

fn terrain_mesh(terrain: &TerrainGrid, center: Vec3<i32>) -> Mesh {
    let occupied = |pos| terrain.get(pos).is_ok_and(|block| block.is_solid());
    voxel_mesh(center, occupied)
}

fn voxel_mesh(center: Vec3<i32>, occupied: impl Fn(Vec3<i32>) -> bool) -> Mesh {
    let mut vertices = Vec::new();
    for z in center.z - VERTICAL_RADIUS..center.z + VERTICAL_RADIUS {
        for y in center.y - RADIUS..center.y + RADIUS {
            for x in center.x - RADIUS..center.x + RADIUS {
                let pos = Vec3::new(x, y, z);
                if !occupied(pos) {
                    continue;
                }
                let corners = corners(
                    [x as f32, y as f32, z as f32],
                    [(x + 1) as f32, (y + 1) as f32, (z + 1) as f32],
                );
                for (side, offset) in NEIGHBORS.iter().enumerate() {
                    if occupied(pos + Vec3::from(*offset)) {
                        continue;
                    }
                    if vertices.len() + 6 > MAX_VERTICES {
                        return Mesh {
                            vertices,
                            truncated: true,
                        };
                    }
                    // Fixed axis palette conveys shape without lights or normals.
                    let color = match side {
                        0 | 1 => [0.34, 0.42, 0.46, 1.0],
                        2 | 3 => [0.46, 0.53, 0.56, 1.0],
                        _ => [0.58, 0.65, 0.59, 1.0],
                    };
                    face(&mut vertices, &corners, side, color);
                }
            }
        }
    }
    Mesh {
        vertices,
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_is_centered_quarter_area_and_stays_bounded_on_large_outputs() {
        assert_eq!(placement(2560, 1440), (640, 360, 1280, 720));
        assert_eq!(placement(1920, 1080), (480, 270, 960, 540));
        assert_eq!(placement(3840, 2160), (1280, 720, 1280, 720));
    }
    #[test]
    fn adjacent_voxels_have_no_internal_faces() {
        let mesh = voxel_mesh(Vec3::zero(), |p| p == Vec3::zero() || p == Vec3::unit_x());
        assert_eq!(mesh.vertices.len(), 10 * 6);
        assert!(!mesh.truncated);
    }
    #[test]
    fn missing_world_has_no_terrain_or_overlay_geometry() {
        assert!(voxel_mesh(Vec3::zero(), |_| false).vertices.is_empty());
        let mut scene = Scene::new();
        let frame = scene.prepare(None, 0.0, 0.0, 640, 480);
        assert!(frame.terrain.is_empty());
        assert!(frame.overlay.is_empty());
    }
}
