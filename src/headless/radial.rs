//! Native near terrain: a moving sphere, one preparation slot, one complete draw.
//! Chunk boundaries are storage boundaries only. Missing cells stay unknown.
use super::*;
use crate::render::terrain_feature::Geometry;

const RADIUS: f32 = 40.0;
const REVEAL_BAND: f32 = 8.0;
const UPDATE_INTERVAL: Duration = Duration::from_millis(125);
const LEAD_SECONDS: f32 = 0.20;
const MAX_LEAD: f32 = 6.0;

struct Complete {
    geometry: Arc<Geometry>,
    center: Vec3<f32>,
    height: Option<(f32, f32)>,
    build_us: u64,
    chunks: u32,
}
pub(crate) struct Window {
    ready: Option<Complete>,
    pending: Option<mpsc::Receiver<Complete>>,
    requested: Option<Vec3<f32>>,
    next: Instant,
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
        Self { ready: None, pending: None, requested: None, next: Instant::now(), revision: 0, failures: 0 }
    }
    pub(crate) fn prepare(&mut self, client: Option<&Client>) -> Prepared {
        let position = client.and_then(Client::position);
        if position.is_none() {
            self.ready = None; self.pending = None; self.requested = None;
        }
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(done) => {
                    self.pending = None;
                    self.ready = Some(done);
                    self.revision = self.revision.wrapping_add(1);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None; self.requested = None; self.failures += 1;
                    self.next = Instant::now() + Duration::from_secs(1);
                }
                Err(mpsc::TryRecvError::Empty) => {},
            }
        }
        if let (Some(client), Some(position)) = (client, position) {
            let velocity = client.current::<comp::Vel>().map_or(Vec3::zero(), |v| v.0);
            let center = desired_center(position, velocity);
            let due = self.requested.is_none_or(|old| (old - center).magnitude_squared() >= 1.0);
            if due && self.pending.is_none() && Instant::now() >= self.next {
                let terrain = Arc::new((*client.state().terrain()).clone());
                let (sender, receiver) = mpsc::channel();
                self.pending = Some(receiver); self.requested = Some(center);
                self.next = Instant::now() + UPDATE_INTERVAL;
                if std::thread::Builder::new().name("terrain-sphere".into()).spawn(move || {
                    let _ = sender.send(build(&terrain, center));
                }).is_err() {
                    self.pending = None; self.requested = None; self.failures += 1;
                }
            }
        }
        Prepared {
            geometry: self.ready.as_ref().map(|r| r.geometry.clone()),
            height: self.ready.as_ref().and_then(|r| r.height),
            revision: self.revision, warm_us: self.ready.as_ref().map_or(0, |r| r.build_us),
            chunks: self.ready.as_ref().map_or(0, |r| r.chunks), failures: self.failures,
        }
    }
}

fn desired_center(position: Vec3<f32>, velocity: Vec3<f32>) -> Vec3<f32> {
    // Bounded prediction gives preparation a head start without an unbounded queue.
    let mut lead = velocity * LEAD_SECONDS;
    if !lead.into_array().iter().all(|v| v.is_finite()) { lead = Vec3::zero(); }
    let length = lead.magnitude();
    if length > MAX_LEAD { lead *= MAX_LEAD / length; }
    (position + lead).map(f32::round)
}

fn face_visible(center: Vec3<f32>, position: Vec3<i32>, side: usize) -> bool {
    let distance = (position.map(|v| v as f32) + Vec3::broadcast(0.5) - center).magnitude();
    if distance >= RADIUS { return false; }
    if distance <= RADIUS - REVEAL_BAND { return true; }
    // Stable world-space dissolve, not random flicker or a whole-chunk reveal.
    let mut hash = 2166136261u32;
    for word in [position.x as u32, position.y as u32, position.z as u32, side as u32] {
        hash = (hash ^ word).wrapping_mul(16777619);
        hash ^= hash >> 13;
    }
    let sample = (hash & 65535) as f32 / 65536.0;
    sample < (RADIUS - distance) / REVEAL_BAND
}

fn build(terrain: &TerrainGrid, center: Vec3<f32>) -> Complete {
    let started = Instant::now();
    let low = (center - Vec3::broadcast(RADIUS)).map(|v| v.floor() as i32);
    let end = (center + Vec3::broadcast(RADIUS)).map(|v| v.ceil() as i32);
    let mut cells = Vec::new();
    let mut chunks = std::collections::HashSet::new();
    for z in low.z..end.z { for y in low.y..end.y { for x in low.x..end.x {
        let pos = Vec3::new(x,y,z);
        let distance = (pos.map(|v| v as f32) + Vec3::broadcast(0.5) - center).magnitude_squared();
        if distance >= RADIUS * RADIUS { continue; }
        let Some(color) = terrain.get(pos).ok().and_then(|b| b.is_solid().then(|| b.get_color()).flatten()) else { continue; };
        let faces: Vec<_> = NEIGHBORS.iter().enumerate().filter_map(|(side, offset)| {
            (face_visible(center, pos, side) && terrain.get(pos + Vec3::from(*offset))
                .is_ok_and(|b| !b.is_solid() || b.get_color().is_none())).then_some(side)
        }).collect();
        if !faces.is_empty() { cells.push((distance, pos, color, faces)); }
    }}}
    // Budget pressure removes outer details first, never the terrain underfoot.
    cells.sort_unstable_by(|a,b| a.0.total_cmp(&b.0).then_with(|| a.1.into_array().cmp(&b.1.into_array())));
    let mut vertices = Vec::new();
    let mut atlas = PaletteAtlas::new();
    let mut height = None;
    'cells: for (_, pos, color, faces) in cells {
        let corners = corners(pos.map(|v| v as f32).into_array(), (pos + 1).map(|v| v as f32).into_array());
        for side in faces {
            if vertices.len() + 6 > MAX_VERTICES { break 'cells; }
            let tile = atlas.tile(face_tile(&corners, side, face_color(color,side)));
            tiled_face(&mut vertices, &corners, side, tile);
        }
        chunks.insert(TerrainGrid::chunk_key(pos.xy()));
        height = Some(height.map_or((pos.z as f32, (pos.z+1) as f32), |(lo,hi): (f32,f32)|
            (lo.min(pos.z as f32), hi.max((pos.z+1) as f32))));
    }
    Complete { geometry: Arc::new(Geometry { vertices: vertices.into(), atlas: atlas.texels.into() }),
        center, height, build_us: started.elapsed().as_micros() as u64, chunks: chunks.len() as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn center_tracks_blocks_instead_of_chunk_boundaries_and_lead_is_bounded() {
        assert_ne!(desired_center(Vec3::new(16.,16.,0.),Vec3::zero()),desired_center(Vec3::new(17.,16.,0.),Vec3::zero()));
        let p=Vec3::new(16.,16.,0.);
        assert!((desired_center(p,Vec3::new(10000.,0.,0.))-p).magnitude()<=MAX_LEAD);
        assert_eq!(desired_center(p,Vec3::broadcast(f32::NAN)),p);
    }
    #[test] fn spherical_shell_has_stable_progressive_reveal() {
        let origin=Vec3::zero();
        for side in 0..6 {
            assert!(face_visible(origin,Vec3::new(1,1,1),side));
            assert!(!face_visible(origin,Vec3::new(40,0,0),side));
        }
        let pos=Vec3::new(36,0,0);
        for side in 0..6 { assert_eq!(face_visible(origin,pos,side),face_visible(origin,pos,side)); }
        assert!(!face_visible(origin,Vec3::new(30,30,0),0));
        assert!(!face_visible(origin,Vec3::new(0,0,41),0));
    }
}
