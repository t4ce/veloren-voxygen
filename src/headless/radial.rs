//! One opaque surface for the frozen 9x9 capture. Movement only changes camera.
use super::*;
use crate::render::terrain_feature::Geometry;

struct Complete {
    geometry: Arc<Geometry>,
    height: Option<(f32, f32)>,
    build_us: u64,
    chunks: u32,
}
pub(crate) struct Window {
    ready: Option<Complete>,
    pending: Option<mpsc::Receiver<Result<Complete, String>>>,
    failed: bool,
    revision: u64,
    failures: u64,
}
pub(crate) struct Prepared {
    pub geometry: Option<Arc<Geometry>>,
    pub height: Option<(f32, f32)>,
    pub revision: u64,
    pub warm_us: u64,
    pub chunks: u32,
    pub failures: u64,
}
impl Window {
    pub(crate) fn new() -> Self {
        Self {
            ready: None,
            pending: None,
            failed: false,
            revision: 0,
            failures: 0,
        }
    }
    pub(crate) fn prepare(&mut self, client: Option<&Client>) -> Prepared {
        if client.is_none() {
            self.ready = None;
            self.pending = None;
            self.failed = false;
        }
        if let Some(receiver) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("terrain preparation worker disconnected".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.pending = None;
                match result {
                    Ok(done) => {
                        tracing::info!(
                            vertices = done.geometry.vertices.len(),
                            rectangles = done.geometry.vertices.len() / 6,
                            vertex_bytes = done.geometry.vertices.len() * 32,
                            index_bytes = done.geometry.vertices.len() * 4,
                            chunks = done.chunks,
                            build_us = done.build_us,
                            "terrain-static: complete opaque capture; no movement remeshing"
                        );
                        self.ready = Some(done);
                        self.revision = self.revision.wrapping_add(1);
                    }
                    Err(error) => {
                        self.failed = true;
                        self.failures += 1;
                        tracing::error!(%error, "terrain-static: preparation rejected; capture is not truncated");
                    }
                }
            }
        }
        if self.ready.is_none() && self.pending.is_none() && !self.failed {
            if let Some(client) = client.filter(|client| client.terrain_snapshot_ready()) {
                if let Some(center) = client.terrain_snapshot_center() {
                    let terrain = Arc::new((*client.state().terrain()).clone());
                    let (sender, receiver) = mpsc::channel();
                    self.pending = Some(receiver);
                    if std::thread::Builder::new()
                        .name("terrain-static".into())
                        .spawn(move || {
                            let _ = sender.send(build(&terrain, center));
                        })
                        .is_err()
                    {
                        self.pending = None;
                        self.failed = true;
                        self.failures += 1;
                    }
                }
            }
        }
        Prepared {
            geometry: self.ready.as_ref().map(|r| r.geometry.clone()),
            height: self.ready.as_ref().and_then(|r| r.height),
            revision: self.revision,
            warm_us: self.ready.as_ref().map_or(0, |r| r.build_us),
            chunks: self.ready.as_ref().map_or(0, |r| r.chunks),
            failures: self.failures,
        }
    }
}

fn build(terrain: &TerrainGrid, center: Vec2<i32>) -> Result<Complete, String> {
    let started = Instant::now();
    let mut vertices = Vec::new();
    let mut height = None;
    let mut chunks = 0;
    for y in center.y - 4..=center.y + 4 {
        for x in center.x - 4..=center.x + 4 {
            let key = Vec2::new(x, y);
            if !terrain.contains_key_real(key) {
                return Err(format!("capture chunk {key:?} is missing"));
            }
            let chunk = terrain
                .get_key_arc(key)
                .ok_or_else(|| format!("capture chunk {key:?} is unavailable"))?;
            let low_z = chunk.get_min_z().saturating_sub(1);
            let high_z = chunk.get_max_z();
            if high_z < low_z || i64::from(high_z) - i64::from(low_z) > 16_384 {
                return Err(format!("capture chunk {key:?} has invalid height bounds"));
            }
            let low = TerrainGrid::key_chunk(key);
            let end = TerrainGrid::key_chunk(key + 1);
            // Merge exposed coplanar coverage, preserving cliffs and holes.
            // This opaque baseline averages color within each rectangle;
            // per-voxel color variation must not fragment its geometry.
            let mut planes =
                std::collections::BTreeMap::<(usize, i32), Vec<(i32, i32, [u8; 3])>>::new();
            for z in low_z..high_z {
                for y in low.y..end.y {
                    for x in low.x..end.x {
                        let pos = Vec3::new(x, y, z);
                        let Some(color) = terrain
                            .get(pos)
                            .ok()
                            .and_then(|b| b.is_solid().then(|| b.get_color()).flatten())
                        else {
                            continue;
                        };
                        for (side, offset) in NEIGHBORS.iter().enumerate() {
                            if !terrain
                                .get(pos + Vec3::from(*offset))
                                .is_ok_and(|b| !b.is_solid() || b.get_color().is_none())
                            {
                                continue;
                            }
                            let (plane, u, v) = match side {
                                0 | 1 => (x + (side == 1) as i32, y, z),
                                2 | 3 => (y + (side == 3) as i32, x, z),
                                _ => (z + (side == 5) as i32, x, y),
                            };
                            planes.entry((side, plane)).or_default().push((
                                u,
                                v,
                                face_color(color, side),
                            ));
                        }
                    }
                }
            }
            for ((side, plane), faces) in planes {
                merge_plane(&mut vertices, side, plane, faces)?;
            }
            chunks += 1;
            height = Some(
                height.map_or((low_z as f32, high_z as f32), |(lo, hi): (f32, f32)| {
                    (lo.min(low_z as f32), hi.max(high_z as f32))
                }),
            );
        }
    }
    Ok(Complete {
        geometry: Arc::new(Geometry {
            vertices: vertices.into(),
            atlas: Arc::from([]),
        }),
        height,
        build_us: started.elapsed().as_micros() as u64,
        chunks,
    })
}

fn merge_plane(
    vertices: &mut Vec<Vertex>,
    side: usize,
    plane: i32,
    faces: Vec<(i32, i32, [u8; 3])>,
) -> Result<(), String> {
    let Some(min_u) = faces.iter().map(|f| f.0).min() else {
        return Ok(());
    };
    let max_u = faces.iter().map(|f| f.0).max().unwrap();
    let min_v = faces.iter().map(|f| f.1).min().unwrap();
    let max_v = faces.iter().map(|f| f.1).max().unwrap();
    let width = (max_u - min_u + 1) as usize;
    let height = (max_v - min_v + 1) as usize;
    let mut mask = vec![None; width * height];
    for (u, v, color) in faces {
        mask[(v - min_v) as usize * width + (u - min_u) as usize] = Some(color);
    }
    for v in 0..height {
        for u in 0..width {
            let Some(_) = mask[v * width + u] else {
                continue;
            };
            let mut w = 1;
            while u + w < width && mask[v * width + u + w].is_some() {
                w += 1;
            }
            let mut h = 1;
            while v + h < height && (u..u + w).all(|x| mask[(v + h) * width + x].is_some()) {
                h += 1;
            }
            let mut sum = [0u64; 3];
            for y in v..v + h {
                for cell in &mut mask[y * width + u..y * width + u + w] {
                    let color = cell.take().unwrap();
                    for channel in 0..3 {
                        sum[channel] += u64::from(color[channel]);
                    }
                }
            }
            let area = (w * h) as u64;
            let color = sum.map(|value| ((value + area / 2) / area) as u8);
            let u0 = min_u as f32 + u as f32;
            let v0 = min_v as f32 + v as f32;
            let u1 = u0 + w as f32;
            let v1 = v0 + h as f32;
            let p = plane as f32;
            let (min, max) = match side {
                0 | 1 => ([p, u0, v0], [p, u1, v1]),
                2 | 3 => ([u0, p, v0], [u1, p, v1]),
                _ => ([u0, v0, p], [u1, v1, p]),
            };
            let rgba = [
                color[0] as f32 / 255.,
                color[1] as f32 / 255.,
                color[2] as f32 / 255.,
                1.,
            ];
            face(vertices, &corners(min, max), side, rgba);
        }
    }
    Ok(())
}
