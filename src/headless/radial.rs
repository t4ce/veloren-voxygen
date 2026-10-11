//! Native near terrain: a moving sphere, one preparation slot, one complete draw.
//! Chunk boundaries are storage boundaries only. Missing cells stay unknown.
use super::*;
use crate::render::terrain_feature::Geometry;

const RADIUS: f32 = 40.0;
const TILE_EDGE: i32 = 8;
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
    pending: Option<mpsc::Receiver<(Complete, SurfaceCache)>>,
    cache: SurfaceCache,
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
        Self { ready: None, pending: None, cache: SurfaceCache::default(), requested: None, next: Instant::now(), revision: 0, failures: 0 }
    }
    pub(crate) fn prepare(&mut self, client: Option<&Client>) -> Prepared {
        let position = client.and_then(Client::position);
        if position.is_none() {
            self.ready = None; self.pending = None; self.requested = None;
            self.cache = SurfaceCache::default();
        }
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok((done, cache)) => {
                    self.pending = None;
                    self.ready = Some(done);
                    self.cache = cache;
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
                let mut cache = std::mem::take(&mut self.cache);
                self.pending = Some(receiver); self.requested = Some(center);
                self.next = Instant::now() + UPDATE_INTERVAL;
                if std::thread::Builder::new().name("terrain-sphere".into()).spawn(move || {
                    let done = build_cached(&terrain, center, &mut cache);
                    let _ = sender.send((done, cache));
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

fn cell_visible(center: Vec3<f32>, position: Vec3<i32>) -> bool {
    (position.map(|v| v as f32) + Vec3::broadcast(0.5) - center).magnitude_squared() < RADIUS * RADIUS
}

#[derive(Clone, Copy)]
struct Surface { position: Vec3<i32>, color: Rgb<u8>, faces: u8 }

// Cache exposed surfaces, rather than rescan the entire voxel sphere on every
// step. Tiles are CPU preparation units only, never independent draw/reveal units.
#[derive(Default)]
struct SurfaceCache {
    tiles: std::collections::HashMap<Vec3<i32>, Vec<Surface>>,
    source: Option<Arc<TerrainGrid>>,
}

impl SurfaceCache {
    fn validate(&mut self, terrain: &Arc<TerrainGrid>) {
        if let Some(old) = &self.source {
            let changed = self.tiles.keys().any(|key| {
                let low = TerrainGrid::chunk_key((key * TILE_EDGE - 1).xy());
                let high = TerrainGrid::chunk_key((key * TILE_EDGE + TILE_EDGE).xy());
                (low.y..=high.y).any(|y| (low.x..=high.x).any(|x| {
                    match (old.get_key_arc(Vec2::new(x,y)), terrain.get_key_arc(Vec2::new(x,y))) {
                        (Some(a), Some(b)) => !Arc::ptr_eq(a,b),
                        (None, None) => false,
                        _ => true,
                    }
                }))
            });
            if changed { self.tiles.clear(); }
        }
        self.source = Some(terrain.clone());
    }

    fn tile(&mut self, terrain: &TerrainGrid, key: Vec3<i32>) -> &[Surface] {
        self.tiles.entry(key).or_insert_with(|| {
            let low = key * TILE_EDGE;
            let end = low + TILE_EDGE;
            let mut surfaces = Vec::new();
            for z in low.z..end.z { for y in low.y..end.y { for x in low.x..end.x {
                let position = Vec3::new(x,y,z);
                let Some(color) = terrain.get(position).ok().and_then(|b| b.is_solid().then(|| b.get_color()).flatten()) else { continue; };
                let mut faces = 0;
                for (side, offset) in NEIGHBORS.iter().enumerate() {
                    if terrain.get(position + Vec3::from(*offset)).is_ok_and(|b| !b.is_solid() || b.get_color().is_none()) {
                        faces |= 1 << side;
                    }
                }
                if faces != 0 { surfaces.push(Surface { position, color, faces }); }
            }}}
            surfaces
        })
    }
}

#[cfg(test)]
fn build(terrain: &TerrainGrid, center: Vec3<f32>) -> Complete {
    build_cached(&Arc::new(terrain.clone()), center, &mut SurfaceCache::default())
}

fn build_cached(terrain: &Arc<TerrainGrid>, center: Vec3<f32>, cache: &mut SurfaceCache) -> Complete {
    let started = Instant::now();
    cache.validate(terrain);
    let low = (center - Vec3::broadcast(RADIUS)).map(|v| v.floor() as i32);
    let end = (center + Vec3::broadcast(RADIUS)).map(|v| v.ceil() as i32);
    let mut cells = Vec::new();
    let mut chunks = std::collections::HashSet::new();
    let first = low.map(|v| v.div_euclid(TILE_EDGE));
    let last = (end - 1).map(|v| v.div_euclid(TILE_EDGE));
    cache.tiles.retain(|key,_| (0..3).all(|i| key[i] >= first[i]-1 && key[i] <= last[i]+1));
    for z in first.z..=last.z { for y in first.y..=last.y { for x in first.x..=last.x {
        let key = Vec3::new(x,y,z);
        let tile_low = (key * TILE_EDGE).map(|v| v as f32);
        let nearest = Vec3::new(center.x.clamp(tile_low.x,tile_low.x+TILE_EDGE as f32),
            center.y.clamp(tile_low.y,tile_low.y+TILE_EDGE as f32),
            center.z.clamp(tile_low.z,tile_low.z+TILE_EDGE as f32));
        if (nearest-center).magnitude_squared() >= RADIUS * RADIUS { continue; }
        for surface in cache.tile(terrain, key) {
            if !cell_visible(center,surface.position) { continue; }
            let distance = (surface.position.map(|v| v as f32) + Vec3::broadcast(0.5) - center).magnitude_squared();
            cells.push((distance, surface.position, surface.color, surface.faces));
        }
    }}}
    // Budget pressure removes outer details first, never the terrain underfoot.
    cells.sort_unstable_by(|a,b| a.0.total_cmp(&b.0).then_with(|| a.1.into_array().cmp(&b.1.into_array())));
    let mut vertices = Vec::new();
    let mut atlas = PaletteAtlas::new();
    let mut height = None;
    for (_, pos, color, faces) in cells {
        // Admit complete cells even when the draw budget is exhausted.
        if vertices.len() + faces.count_ones() as usize * 6 > MAX_VERTICES { break; }
        let corners = corners(pos.map(|v| v as f32).into_array(), (pos + 1).map(|v| v as f32).into_array());
        for side in 0..6 {
            if faces & (1 << side) == 0 { continue; }
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
    #[test] fn spherical_boundary_keeps_complete_cells() {
        let origin=Vec3::zero();
        assert!(cell_visible(origin,Vec3::new(1,1,1)));
        assert!(cell_visible(origin,Vec3::new(36,0,0)));
        assert!(!cell_visible(origin,Vec3::new(40,0,0)));
        assert!(!cell_visible(origin,Vec3::new(30,30,0)));
        assert!(!cell_visible(origin,Vec3::new(0,0,41)));
    }
}
