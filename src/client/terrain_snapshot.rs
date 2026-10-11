//! Temporary native render-test policy: one fixed, complete terrain capture.
//! Uses the existing chunk request/update protocol; no server changes.
use std::time::{Duration, Instant};

pub const RADIUS: i32 = 4;
pub const VIEW_DISTANCE: u32 = 5;
pub const REQUEST_INTERVAL: Duration = Duration::from_millis(100);

pub struct Capture {
    pub center: [i32; 2],
    pub keys: Vec<[i32; 2]>,
}
impl Capture {
    pub fn new(position: [f32; 2], size: [u32; 2]) -> Option<Self> {
        if size.contains(&0) || position.iter().any(|v| !v.is_finite()) { return None; }
        let center = std::array::from_fn::<_, 2, _>(|i|
            (f64::from(position[i]) / f64::from(size[i])).floor());
        if center.iter().any(|v| *v < f64::from(i32::MIN) + 5.0 || *v > f64::from(i32::MAX) - 5.0) {
            return None;
        }
        let center = center.map(|v| v as i32);
        let mut keys = Vec::with_capacity(81);
        for y in -RADIUS..=RADIUS { for x in -RADIUS..=RADIUS {
            keys.push([center[0] + x, center[1] + y]);
        }}
        keys.sort_by_key(|key| {
            let dx = key[0] - center[0]; let dy = key[1] - center[1];
            (dx * dx + dy * dy, *key)
        });
        Some(Self { center, keys })
    }
    pub fn contains(&self, key: [i32; 2]) -> bool {
        (0..2).all(|i| (i64::from(key[i]) - i64::from(self.center[i])).abs() <= i64::from(RADIUS))
    }
}

pub struct Snapshot {
    pub capture: Option<Capture>,
    pub frozen: bool,
    pub next_request: Instant,
    pub next_report: Instant,
}
impl Snapshot {
    pub fn new() -> Self {
        let now = Instant::now();
        Self { capture: None, frozen: false, next_request: now, next_report: now }
    }
    pub fn accepts_chunk(&self, key: [i32; 2], already_loaded: bool) -> bool {
        !self.frozen && self.capture.as_ref().is_none_or(|capture|
            capture.contains(key) && !already_loaded)
    }
    pub fn finish(&mut self, ready: usize) -> bool {
        if !self.frozen && self.capture.as_ref().is_some_and(|c| ready == c.keys.len()) {
            self.frozen = true;
            true
        } else { false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_fixed_square_handles_negative_world_coordinates() {
        let capture = Capture::new([-0.1, -32.1], [32, 32]).unwrap();
        assert_eq!(capture.center, [-1, -2]);
        assert_eq!(capture.keys.len(), 81);
        assert_eq!(capture.keys[0], capture.center);
        for key in &capture.keys { assert!(capture.contains(*key)); }
        assert!(!capture.contains([4, -2]));
        assert!(Capture::new([f32::NAN, 0.], [32, 32]).is_none());
    }
    #[test]
    fn freeze_requires_every_chunk_and_rejects_every_later_chunk() {
        let mut snapshot = Snapshot::new();
        snapshot.capture = Capture::new([16., 16.], [32, 32]);
        assert!(snapshot.accepts_chunk([0, 0], false));
        assert!(!snapshot.accepts_chunk([0, 0], true));
        assert!(!snapshot.accepts_chunk([5, 0], false));
        assert!(!snapshot.finish(80));
        assert!(snapshot.finish(81));
        assert!(!snapshot.accepts_chunk([0, 0], false));
        assert!(!snapshot.finish(81));
    }
    #[test]
    fn requested_square_is_inside_existing_server_admission_at_every_entry_offset() {
        for x in 0..32 { for y in 0..32 {
            let position = [x as f32, y as f32];
            let capture = Capture::new(position, [32, 32]).unwrap();
            for key in capture.keys {
                let radius = (VIEW_DISTANCE as f64 - 1. + 2.5 * 2_f64.sqrt()) * 32.;
                let distance = (0..2).map(|i|
                    ((key[i] as f64 + 0.5) * 32. - position[i] as f64).powi(2)).sum::<f64>();
                assert!(distance < (radius - 1.).powi(2));
            }
        }}
    }
}
