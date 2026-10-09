//! Pure terrain composition policy. Near detail and a fixed-budget whole-map
//! heightfield share one retained indexed draw; no per-frame geometry rebuild.
use crate::{
    render::terrain_feature::Geometry,
    terrain_preview::{ATLAS_SIZE, MAX_VERTICES, Vertex},
};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

pub const FAR_CELL_GUESS: usize = 32_768;
pub const FAR_CELL_BUDGET: usize = FAR_CELL_GUESS / 2;
// Near 3x3 tiles occupy at most 883 atlas rows at the 600k vertex cap.
const FAR_ATLAS_ROW: usize = 896;
const FAR_VERTEX_LIMIT: usize = 120_000;
static MODE: AtomicU8 = AtomicU8::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Near,
    #[default]
    Both,
    Far,
}
impl Mode {
    pub fn current() -> Self {
        match MODE.load(Ordering::Relaxed) {
            0 => Self::Near,
            2 => Self::Far,
            _ => Self::Both,
        }
    }
    pub fn cycle_global() -> Self {
        let old = MODE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some((v + 1) % 3))
            .unwrap();
        match (old + 1) % 3 {
            0 => Self::Near,
            2 => Self::Far,
            _ => Self::Both,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Near => "near",
            Self::Both => "both",
            Self::Far => "far",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub min: [i32; 2],
    pub end: [i32; 2],
}
impl Coverage {
    pub fn new(min: [i32; 2], end: [i32; 2]) -> Option<Self> {
        (min[0] < end[0] && min[1] < end[1]).then_some(Self { min, end })
    }
    pub fn chunks(self, size: [u32; 2]) -> u32 {
        let width = (i64::from(self.end[0]) - i64::from(self.min[0])).max(0) as u64;
        let height = (i64::from(self.end[1]) - i64::from(self.min[1])).max(0) as u64;
        ((width / u64::from(size[0].max(1))) * (height / u64::from(size[1].max(1))))
            .min(u64::from(u32::MAX)) as u32
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub mode: Mode,
    pub stream_run_us: u64,
    pub near_revision: u64,
    pub near_vertices: u32,
    pub far_vertices: u32,
    pub far_ready: bool,
    pub near_chunks: u32,
    pub loaded_chunks: u32,
    pub near_warm_us: u64,
    pub far_warm_us: u64,
    pub compose_us: u64,
    pub warm_failures: u64,
    pub budget_fallback: bool,
    pub received_at: Option<Instant>,
    pub width: u32,
    pub height: u32,
}

pub fn grid_shape(width: usize, height: usize) -> Option<[usize; 2]> {
    if width == 0 || height == 0 {
        return None;
    }
    let nx = ((FAR_CELL_BUDGET as f64 * width as f64 / height as f64).sqrt() as usize)
        .clamp(1, width.min(FAR_CELL_BUDGET));
    let ny = (FAR_CELL_BUDGET / nx).clamp(1, height);
    Some([nx, ny])
}

pub struct Heightfield {
    pub cells: [usize; 2],
    pub extent: [f32; 2],
    pub heights: Vec<f32>,
    pub colors: Vec<[u8; 4]>,
}
impl Heightfield {
    pub fn valid(&self) -> bool {
        let [nx, ny] = self.cells;
        nx > 0
            && ny > 0
            && nx.checked_mul(ny).is_some_and(|n| n <= FAR_CELL_BUDGET)
            && self.heights.len() == (nx + 1) * (ny + 1)
            && self.colors.len() == nx * ny
            && self.extent.iter().all(|v| v.is_finite() && *v > 0.)
            && (0..2).all(|axis| self.extent[axis] / self.cells[axis] as f32 >= 1.)
            && self.heights.iter().all(|v| v.is_finite())
    }
    fn point(&self, x: usize, y: usize) -> [f32; 3] {
        [
            x as f32 / self.cells[0] as f32 * self.extent[0],
            y as f32 / self.cells[1] as f32 * self.extent[1],
            self.heights[y * (self.cells[0] + 1) + x],
        ]
    }
    pub fn height_at(&self, x: f32, y: f32) -> f32 {
        let gx = (x / self.extent[0] * self.cells[0] as f32).clamp(0., self.cells[0] as f32);
        let gy = (y / self.extent[1] * self.cells[1] as f32).clamp(0., self.cells[1] as f32);
        let ix = (gx.floor() as usize).min(self.cells[0] - 1);
        let iy = (gy.floor() as usize).min(self.cells[1] - 1);
        let u = gx - ix as f32;
        let v = gy - iy as f32;
        let a = self.point(ix, iy)[2];
        let b = self.point(ix + 1, iy)[2];
        let c = self.point(ix + 1, iy + 1)[2];
        let d = self.point(ix, iy + 1)[2];
        if v <= u {
            a + u * (b - a) + v * (c - b)
        } else {
            a + u * (c - d) + v * (d - a)
        }
    }
}

// Split into two disjoint convex polygons; interpolate Z on the same original
// triangle plane. Sequential outside pieces subtract a rectangle exactly,
// even when it is much smaller than a coarse whole-map cell.
fn split(
    poly: &[[f32; 3]],
    axis: usize,
    edge: f32,
    greater: bool,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    if poly.is_empty() {
        return (inside, outside);
    }
    let distance = |p: [f32; 3]| {
        if greater {
            p[axis] - edge
        } else {
            edge - p[axis]
        }
    };
    let mut a = *poly.last().unwrap();
    let mut da = distance(a);
    for &b in poly {
        let db = distance(b);
        if (da >= 0.) != (db >= 0.) {
            let t = da / (da - db);
            let mut p = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
            p[axis] = edge;
            inside.push(p);
            outside.push(p);
        }
        if db >= 0. {
            inside.push(b);
        } else {
            outside.push(b);
        }
        a = b;
        da = db;
    }
    (inside, outside)
}

pub fn subtract(triangle: [[f32; 3]; 3], hole: Option<Coverage>) -> Vec<Vec<[f32; 3]>> {
    let Some(hole) = hole else {
        return vec![triangle.to_vec()];
    };
    let mut remaining = triangle.to_vec();
    let mut result = Vec::new();
    for (axis, edge, greater) in [
        (0, hole.min[0] as f32, true),
        (0, hole.end[0] as f32, false),
        (1, hole.min[1] as f32, true),
        (1, hole.end[1] as f32, false),
    ] {
        let (inner, outer) = split(&remaining, axis, edge, greater);
        if outer.len() >= 3 {
            result.push(outer);
        }
        remaining = inner;
        if remaining.len() < 3 {
            break;
        }
    }
    result
}
fn push_polygon(out: &mut Vec<Vertex>, p: &[[f32; 3]], uv: [f32; 4]) {
    for i in 1..p.len().saturating_sub(1) {
        let a = p[0];
        let b = p[i];
        let c = p[i + 1];
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() <= 0.00001 {
            continue;
        }
        for p in [a, b, c] {
            out.push(Vertex {
                position: [p[0], p[1], p[2], 1.],
                atlas_uv: uv,
            });
        }
    }
}
fn far_vertices(map: &Heightfield, hole: Option<Coverage>) -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(map.cells[0] * map.cells[1] * 6);
    for y in 0..map.cells[1] {
        for x in 0..map.cells[0] {
            let index = y * map.cells[0] + x;
            let texel = FAR_ATLAS_ROW * ATLAS_SIZE as usize + index;
            let uv = [
                ((texel % ATLAS_SIZE as usize) as f32 + 0.5) / ATLAS_SIZE as f32,
                ((texel / ATLAS_SIZE as usize) as f32 + 0.5) / ATLAS_SIZE as f32,
                0.,
                1.,
            ];
            let [a, b, c, d] = [
                map.point(x, y),
                map.point(x + 1, y),
                map.point(x + 1, y + 1),
                map.point(x, y + 1),
            ];
            for triangle in [[a, b, c], [a, c, d]] {
                for poly in subtract(triangle, hole) {
                    push_polygon(&mut vertices, &poly, uv);
                }
            }
        }
    }
    vertices
}

fn seam(vertices: &mut Vec<Vertex>, near: &[Vertex], map: &Heightfield, hole: Coverage) {
    use std::collections::BTreeMap;
    // Edge skirts join the actual top envelope to the coarse triangle plane;
    // they occupy the boundary only, not a second overlapping ground surface.
    for (axis, edge, other) in [
        (0, hole.min[0], 1),
        (0, hole.end[0], 1),
        (1, hole.min[1], 0),
        (1, hole.end[1], 0),
    ] {
        if edge < 0 || edge as f32 > map.extent[axis] {
            continue;
        }
        let mut top = BTreeMap::<i32, Vertex>::new();
        for v in near {
            if v.position[axis] != edge as f32 {
                continue;
            }
            let key = v.position[other] as i32;
            if key < hole.min[other] || key > hole.end[other] {
                continue;
            }
            top.entry(key)
                .and_modify(|old| {
                    if v.position[2] > old.position[2] {
                        *old = *v;
                    }
                })
                .or_insert(*v);
        }
        for key in hole.min[other]..hole.end[other] {
            if key < 0 || (key + 1) as f32 > map.extent[other] {
                continue;
            }
            let (Some(a), Some(b)) = (top.get(&key), top.get(&(key + 1))) else {
                continue;
            };
            // Split at coarse cell edges and diagonals too. Joining only unit
            // endpoints would bridge across two different far triangle planes.
            let spacing = map.extent[other] / map.cells[other] as f32;
            let grid = (edge as f32 / map.extent[axis] * map.cells[axis] as f32)
                .clamp(0., map.cells[axis] as f32);
            let cell = (grid.floor() as usize).min(map.cells[axis] - 1);
            let fraction = grid - cell as f32;
            let first = (key as f32 / spacing).floor() as usize;
            let last = (((key + 1) as f32 / spacing).floor() as usize).min(map.cells[other] - 1);
            let mut cuts = vec![key as f32, (key + 1) as f32];
            for cell in first..=last {
                for at in [
                    (cell + 1) as f32 * spacing,
                    (cell as f32 + fraction) * spacing,
                ] {
                    if at > key as f32 && at < (key + 1) as f32 {
                        cuts.push(at);
                    }
                }
            }
            cuts.sort_by(f32::total_cmp);
            cuts.dedup();
            for pair in cuts.windows(2) {
                let top_at = |at: f32| Vertex {
                    position: std::array::from_fn(|i| {
                        a.position[i] + (b.position[i] - a.position[i]) * (at - key as f32)
                    }),
                    atlas_uv: a.atlas_uv,
                };
                let [a, b] = [top_at(pair[0]), top_at(pair[1])];
                let mut c = b;
                let mut d = a;
                c.position[2] = map.height_at(c.position[0], c.position[1]);
                d.position[2] = map.height_at(d.position[0], d.position[1]);
                vertices.extend_from_slice(&[a, b, c, a, c, d]);
            }
        }
    }
}

pub fn compose(
    map: Option<&Heightfield>,
    near: Option<&Geometry>,
    coverage: Option<Coverage>,
    mode: Mode,
) -> Result<(Geometry, Metrics), &'static str> {
    if map.is_some_and(|m| !m.valid()) {
        return Err("invalid-heightfield");
    }
    let near = if mode == Mode::Far { None } else { near };
    let mut hole = if mode == Mode::Both && near.is_some() {
        coverage
    } else {
        None
    };
    let near = if mode == Mode::Both && hole.is_none() {
        None
    } else {
        near
    };
    let map = if mode == Mode::Near { None } else { map };
    let mut far = map.map_or_else(Vec::new, |m| far_vertices(m, hole));
    if far.len() > FAR_VERTEX_LIMIT {
        return Err("far-budget");
    }
    let mut near_count = near.map_or(0, |g| g.vertices.len());
    let seam_budget = hole.map_or(0, |h| {
        ((i64::from(h.end[0]) - i64::from(h.min[0]) + i64::from(h.end[1]) - i64::from(h.min[1]))
            as usize)
            .saturating_mul(48)
    });
    let mut fallback = false;
    if near_count + far.len() + seam_budget > MAX_VERTICES {
        near_count = 0;
        hole = None;
        fallback = true;
        far = map.map_or_else(Vec::new, |m| far_vertices(m, None));
    }
    let mut atlas = near.filter(|_| near_count > 0).map_or_else(
        || vec![[0, 0, 0, 255]; (ATLAS_SIZE * ATLAS_SIZE) as usize],
        |g| g.atlas.to_vec(),
    );
    if atlas.len() != (ATLAS_SIZE * ATLAS_SIZE) as usize {
        return Err("near-atlas");
    }
    if let Some(map) = map {
        atlas[FAR_ATLAS_ROW * ATLAS_SIZE as usize
            ..FAR_ATLAS_ROW * ATLAS_SIZE as usize + map.colors.len()]
            .copy_from_slice(&map.colors);
    }
    let far_count = far.len();
    if near_count > 0 {
        far.extend_from_slice(&near.unwrap().vertices);
    }
    if near_count > 0 {
        if let (Some(map), Some(hole), Some(near)) = (map, hole, near) {
            seam(&mut far, &near.vertices, map, hole);
        }
    }
    if far.len() > MAX_VERTICES {
        return Err("composition-budget");
    }
    let near_count = far.len() - far_count;
    Ok((
        Geometry {
            vertices: far.into(),
            atlas: atlas.into(),
        },
        Metrics {
            mode,
            near_vertices: near_count as u32,
            far_vertices: far_count as u32,
            far_ready: map.is_some(),
            near_chunks: if near.is_some() && !fallback {
                coverage.map_or(0, |c| c.chunks([32, 32]))
            } else {
                0
            },
            budget_fallback: fallback,
            ..Default::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn map(cells: [usize; 2], extent: [f32; 2]) -> Heightfield {
        Heightfield {
            cells,
            extent,
            heights: vec![7.; (cells[0] + 1) * (cells[1] + 1)],
            colors: vec![[40, 90, 120, 255]; cells[0] * cells[1]],
        }
    }
    fn area(vertices: &[Vertex]) -> f64 {
        vertices
            .chunks_exact(3)
            .map(|t| {
                let [a, b, c] = [t[0].position, t[1].position, t[2].position];
                f64::from(((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs())
                    * 0.5
            })
            .sum()
    }
    fn near() -> Geometry {
        Geometry {
            vertices: Vec::<Vertex>::new().into(),
            atlas: vec![[11, 22, 33, 255]; (ATLAS_SIZE * ATLAS_SIZE) as usize].into(),
        }
    }
    #[test]
    fn whole_map_cutout_conserves_area_at_coarse_cells_and_map_edges() {
        let m = map([3, 2], [384., 256.]);
        for (min, end) in [
            ([32, 32], [64, 64]),
            ([96, 64], [192, 160]),
            ([-32, -32], [32, 32]),
            ([352, 224], [416, 288]),
            ([-96, 32], [-32, 64]),
            ([0, 0], [384, 256]),
        ] {
            let h = Coverage::new(min, end).unwrap();
            let vertices = far_vertices(&m, Some(h));
            let width = (end[0].min(384) - min[0].max(0)).max(0);
            let height = (end[1].min(256) - min[1].max(0)).max(0);
            let expected = 384. * 256. - f64::from(width) * f64::from(height);
            assert!((area(&vertices) - expected).abs() < 0.01, "{min:?} {end:?}");
            for t in vertices.chunks_exact(3) {
                let x = t.iter().map(|v| v.position[0]).sum::<f32>() / 3.;
                let y = t.iter().map(|v| v.position[1]).sum::<f32>() / 3.;
                assert!(
                    !(x > min[0] as f32
                        && x < end[0] as f32
                        && y > min[1] as f32
                        && y < end[1] as f32)
                );
            }
        }
    }
    #[test]
    fn cut_vertices_keep_the_original_sloped_triangle_plane() {
        let pieces = subtract(
            [[0., 0., 3.], [128., 0., 259.], [128., 128., 387.]],
            Coverage::new([32, 32], [64, 64]),
        );
        for p in pieces.into_iter().flatten() {
            assert!((p[2] - (3. + 2. * p[0] + p[1])).abs() < 0.0001);
        }
    }
    #[test]
    fn interpolation_follows_both_actual_triangle_planes() {
        let mut m = map([1, 1], [128., 128.]);
        m.heights = vec![0., 10., 20., 40.];
        assert_eq!(m.height_at(96., 32.), 15.);
        assert_eq!(m.height_at(32., 96.), 20.);
        assert_eq!(m.height_at(-100., -100.), 0.);
        assert_eq!(m.height_at(200., 200.), 40.);
    }
    #[test]
    fn modes_cut_only_proven_coverage_and_preserve_near_palette() {
        let m = map([1, 1], [128., 128.]);
        let mut n = near();
        let coverage = Coverage::new([32, 32], [64, 64]);
        n.vertices = vec![
            Vertex {
                position: [48., 48., 10., 1.],
                atlas_uv: [0.; 4]
            };
            3
        ]
        .into();
        let (both, metrics) = compose(Some(&m), Some(&n), coverage, Mode::Both).unwrap();
        assert_eq!(area(&both.vertices), 128. * 128. - 32. * 32.);
        assert_eq!(metrics.near_chunks, 1);
        assert_eq!(both.atlas[0], [11, 22, 33, 255]);
        let (far, metrics) = compose(Some(&m), Some(&n), coverage, Mode::Far).unwrap();
        assert_eq!(area(&far.vertices), 128. * 128.);
        assert_eq!(metrics.near_chunks, 0);
        let (unproven, _) = compose(Some(&m), Some(&n), None, Mode::Both).unwrap();
        assert_eq!(area(&unproven.vertices), 128. * 128.);
        let (only, metrics) = compose(Some(&m), Some(&n), coverage, Mode::Near).unwrap();
        assert_eq!(only.vertices.len(), 3);
        assert_eq!(metrics.far_vertices, 0);
    }
    #[test]
    fn over_budget_near_reverts_to_complete_far_without_a_hole_or_skirts() {
        let m = map([1, 1], [128., 128.]);
        let mut n = near();
        let v = Vertex {
            position: [32., 32., 10., 1.],
            atlas_uv: [0.; 4],
        };
        n.vertices = vec![v; MAX_VERTICES].into();
        let (geometry, metrics) = compose(
            Some(&m),
            Some(&n),
            Coverage::new([32, 32], [64, 64]),
            Mode::Both,
        )
        .unwrap();
        assert!(metrics.budget_fallback);
        assert_eq!(metrics.near_chunks, 0);
        assert_eq!(metrics.near_vertices, 0);
        assert_eq!(geometry.vertices.len(), 6);
        assert_eq!(area(&geometry.vertices), 128. * 128.);
    }
    #[test]
    fn lattice_budget_and_atlas_reservation_cover_extreme_map_shapes() {
        for (width, height) in [
            (1, 1),
            (1024, 1024),
            (1, 65535),
            (65535, 1),
            (3, 60000),
            (65535, 65535),
        ] {
            let [nx, ny] = grid_shape(width, height).unwrap();
            assert!(nx <= width && ny <= height && nx * ny <= FAR_CELL_BUDGET);
            let m = map([nx, ny], [width as f32 * 32., height as f32 * 32.]);
            assert!(m.valid());
            let vertices = far_vertices(&m, None);
            assert!(vertices.len() <= FAR_VERTEX_LIMIT);
        }
        assert_eq!(grid_shape(0, 1), None);
        assert!((MAX_VERTICES / 6 + 1).div_ceil((ATLAS_SIZE / 3) as usize) * 3 <= FAR_ATLAS_ROW);
        assert!(
            FAR_ATLAS_ROW * ATLAS_SIZE as usize + FAR_CELL_BUDGET
                <= (ATLAS_SIZE * ATLAS_SIZE) as usize
        );
    }
    #[test]
    fn skirt_bottom_splits_at_noninteger_far_triangle_diagonals() {
        let mut m = map([1, 1], [128., 129.]);
        m.heights = vec![0., 0., 0., 100.];
        let h = Coverage::new([32, 32], [64, 64]).unwrap();
        let mut n = near();
        n.vertices = (32..=33)
            .map(|y| Vertex {
                position: [32., y as f32, 200., 1.],
                atlas_uv: [0.; 4],
            })
            .collect::<Vec<_>>()
            .into();
        let mut vertices = Vec::new();
        seam(&mut vertices, &n.vertices, &m, h);
        assert_eq!(vertices.len(), 12);
        assert!(vertices.iter().any(|v| v.position == [32., 32.25, 25., 1.]));
        assert_eq!(area(&vertices), 0.);
    }
    #[test]
    fn boundary_skirts_have_zero_ground_area_and_join_the_far_height() {
        let m = map([1, 1], [128., 128.]);
        let h = Coverage::new([32, 32], [64, 64]).unwrap();
        let mut n = near();
        n.vertices = (32..=64)
            .map(|y| Vertex {
                position: [32., y as f32, 10., 1.],
                atlas_uv: [0.; 4],
            })
            .collect::<Vec<_>>()
            .into();
        let mut vertices = Vec::new();
        seam(&mut vertices, &n.vertices, &m, h);
        assert_eq!(vertices.len(), 32 * 6);
        assert_eq!(area(&vertices), 0.);
        assert!(
            vertices
                .iter()
                .all(|v| v.position[0] == 32. && [7., 10.].contains(&v.position[2]))
        );
    }
}
