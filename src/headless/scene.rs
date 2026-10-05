//! Mesh, camera and text preparation shared by Ubuntu and TRUEOS.
use crate::client::Client;
use common::{comp, terrain::TerrainGrid, vol::ReadVol};
use specs::{Join, WorldExt};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use vek::{Vec3, Vec4};

const RADIUS: i32 = 40;
const VERTICAL_RADIUS: i32 = 32;
pub(super) const MAX_VERTICES: usize = 600_000;
const MESH_INTERVAL: Duration = Duration::from_millis(500);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    // w=1: world position; w=0: screen coordinates for text.
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
    had_position: bool,
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
            had_position: false,
        }
    }

    pub(super) fn prepare(
        &mut self,
        client: Option<&Client>,
        yaw: f32,
        pitch: f32,
        status: &str,
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
            self.had_position = false;
        }
        if let Some(mesh) = self.pending_mesh.as_ref().and_then(|r| r.try_recv().ok()) {
            self.pending_mesh = None;
            self.terrain = mesh;
            self.revision = self.revision.wrapping_add(1);
        }
        if let (Some(client), Some(position)) = (client, position) {
            self.had_position = true;
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
        let header = "VOXYGEN HEADLESS - LIVE GAME GEOMETRY";
        let geometry = format!(
            "{} TRIANGLES  RADIUS {}  NO TEXTURES / LIGHTS",
            self.terrain.vertices.len() / 3,
            RADIUS
        );
        let location = position
            .map(|p| format!("POSITION {:.1} {:.1} {:.1}", p.x, p.y, p.z))
            .unwrap_or_else(|| "WAITING FOR WORLD POSITION".into());
        let location = if let Some(health) = client.and_then(|c| c.current::<comp::Health>()) {
            format!(
                "{location}  HEALTH {:.0}/{:.0}",
                health.current(),
                health.maximum()
            )
        } else {
            location
        };
        for (line, text) in [
            header,
            status,
            &geometry,
            &location,
            "WASD MOVE / MOUSE LOOK / SPACE JUMP / ESC RELEASE",
            "CLICK ATTACK / F WIELD / R RESPAWN / 1-5 ABILITIES",
        ]
        .iter()
        .enumerate()
        {
            text_mesh(
                &mut overlay,
                text,
                12.0,
                12.0 + line as f32 * 20.0,
                width,
                height,
            );
        }
        if self.terrain.truncated {
            text_mesh(
                &mut overlay,
                "MESH BUDGET REACHED - SOME GEOMETRY OMITTED",
                12.0,
                136.0,
                width,
                height,
            );
        }
        if self.had_position {
            text_mesh(
                &mut overlay,
                "+",
                width as f32 * 0.5 - 5.0,
                height as f32 * 0.5 - 7.0,
                width,
                height,
            );
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

fn text_mesh(vertices: &mut Vec<Vertex>, text: &str, x: f32, y: f32, width: u32, height: u32) {
    let scale = 2.0;
    let max_chars = ((width as f32 - x).max(0.0) / (6.0 * scale)) as usize;
    for (i, c) in text.chars().take(max_chars.min(120)).enumerate() {
        for (row, bits) in glyph(c.to_ascii_uppercase()).iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                let px = x + (i * 6 + col) as f32 * scale;
                let py = y + row as f32 * scale;
                let left = 2.0 * px / width as f32 - 1.0;
                let right = 2.0 * (px + scale) / width as f32 - 1.0;
                let top = 1.0 - 2.0 * py / height as f32;
                let bottom = 1.0 - 2.0 * (py + scale) / height as f32;
                for p in [
                    [left, top],
                    [left, bottom],
                    [right, bottom],
                    [left, top],
                    [right, bottom],
                    [right, top],
                ] {
                    vertices.push(Vertex {
                        position: [p[0], p[1], 0.0, 0.0],
                        color: [0.95, 0.95, 0.85, 1.0],
                    });
                }
            }
        }
    }
}

// Text is geometry too: no font asset, glyph atlas, sampler, or UI pipeline.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_voxels_have_no_internal_faces() {
        let mesh = voxel_mesh(Vec3::zero(), |p| p == Vec3::zero() || p == Vec3::unit_x());
        assert_eq!(mesh.vertices.len(), 10 * 6);
        assert!(!mesh.truncated);
    }
    #[test]
    fn missing_terrain_is_empty_and_text_uses_screen_coordinates() {
        assert!(voxel_mesh(Vec3::zero(), |_| false).vertices.is_empty());
        let mut vertices = Vec::new();
        text_mesh(&mut vertices, "A", 12.0, 12.0, 640, 480);
        assert!(!vertices.is_empty());
        assert!(
            vertices
                .iter()
                .all(|v| v.position[3] == 0.0 && v.position[2] == 0.0)
        );
    }
}
