//! VD=1 decoded warm ring, using the existing coordinate request protocol.
use std::time::Duration;
use std::time::Instant;

pub const WARM_RADIUS: i32 = 2;
pub const PENDING_LIMIT: usize = 6;
pub const REQUEST_INTERVAL: Duration = Duration::from_millis(100);
const FAILURE_BACKOFF: Duration = Duration::from_secs(3);
// Account for small client/server position disagreement, rather than requesting
// directly on the server's strict radial admission boundary.
const POSITION_MARGIN_BLOCKS: f64 = 1.;

pub struct Candidate {
    pub key: [i32; 2],
    pub eta_seconds: f64,
    distance: f64,
}
pub struct Plan {
    pub center: [i32; 2],
    pub near: Vec<[i32; 2]>,
    pub warm: Vec<Candidate>,
}

// Same distance/center convention as server/sys/msg/terrain.rs. No SetViewDistance
// or wire format change: only extra TerrainChunkRequest coordinates within VD=1.
pub fn server_admits(position: [f32; 2], key: [i32; 2], size: [u32; 2], margin: f64) -> bool {
    let radius = 2.5 * 2_f64.sqrt() * f64::from(size[0]) - margin;
    let distance = (0..2)
        .map(|i| {
            let delta = (f64::from(key[i]) + 0.5) * f64::from(size[i]) - f64::from(position[i]);
            delta * delta
        })
        .sum::<f64>();
    radius > 0. && distance < radius * radius
}

pub fn plan(position: [f32; 2], velocity: [f32; 2], size: [u32; 2]) -> Option<Plan> {
    if size.contains(&0) || position.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let cell =
        std::array::from_fn::<_, 2, _>(|i| (f64::from(position[i]) / f64::from(size[i])).floor());
    if cell
        .iter()
        .any(|p| *p < f64::from(i32::MIN) + 4. || *p > f64::from(i32::MAX) - 4.)
    {
        return None;
    }
    let center = cell.map(|p| p as i32);
    let mut near = Vec::with_capacity(9);
    let mut warm = Vec::with_capacity(16);
    for y in -WARM_RADIUS..=WARM_RADIUS {
        for x in -WARM_RADIUS..=WARM_RADIUS {
            let offset = [x, y];
            let key = [center[0] + x, center[1] + y];
            if x.abs() <= 1 && y.abs() <= 1 {
                near.push(key);
                continue;
            }
            if !server_admits(position, key, size, POSITION_MARGIN_BLOCKS) {
                continue;
            }
            // When does this chunk enter the rendered 3x3? Corners need both axes
            // to cross. At rest/turning, fill all other directions too, not just ahead.
            let mut eta = 0_f64;
            for i in 0..2 {
                if offset[i].abs() <= 1 {
                    continue;
                }
                let direction = f64::from(offset[i].signum());
                let speed = f64::from(velocity[i]) * direction;
                let edge =
                    f64::from(center[i] + if direction > 0. { 1 } else { 0 }) * f64::from(size[i]);
                let until = if speed.is_finite() && speed > 0.01 {
                    ((edge - f64::from(position[i])) * direction).max(0.) / speed
                } else {
                    f64::INFINITY
                };
                eta = eta.max(until);
            }
            // A diagonal crossing needs the next 3x3's corner before strips
            // that the same movement is already leaving behind. Predict all
            // axes at that arrival time, not just the requesting side's border.
            if eta.is_finite()
                && (0..2).any(|i| {
                    let v = if velocity[i].is_finite() {
                        f64::from(velocity[i])
                    } else {
                        0.
                    };
                    let future = ((f64::from(position[i]) + v * eta) / f64::from(size[i])
                        + v.signum() * 1e-9)
                        .floor();
                    (f64::from(key[i]) - future).abs() > 1.
                })
            {
                eta = f64::INFINITY;
            }
            let distance = (0..2)
                .map(|i| {
                    let d = (f64::from(key[i]) + 0.5) * f64::from(size[i]) - f64::from(position[i]);
                    d * d
                })
                .sum();
            warm.push(Candidate {
                key,
                eta_seconds: eta,
                distance,
            });
        }
    }
    warm.sort_by(|a, b| {
        a.eta_seconds
            .total_cmp(&b.eta_seconds)
            .then(a.distance.total_cmp(&b.distance))
            .then(a.key.cmp(&b.key))
    });
    Some(Plan { center, near, warm })
}

pub struct WarmState {
    pub run_us: u64,
    next_request: Instant,
    next_report: Instant,
    last_center: Option<[i32; 2]>,
    failures: Vec<([i32; 2], Instant)>,
    latencies: Vec<u64>,
    pub requests: u64,
    pub ready_crossings: u64,
    pub cold_crossings: u64,
    pub teleports: u64,
    pub missing_at_crossings: u64,
    pub timeouts: u64,
}
impl WarmState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            run_us: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_micros() as u64),
            next_request: now,
            next_report: now,
            last_center: None,
            failures: Vec::new(),
            latencies: Vec::with_capacity(32),
            requests: 0,
            ready_crossings: 0,
            cold_crossings: 0,
            teleports: 0,
            missing_at_crossings: 0,
            timeouts: 0,
        }
    }
    pub fn due(&self, now: Instant) -> bool {
        now >= self.next_request
    }
    pub fn sent(&mut self, now: Instant) {
        self.requests += 1;
        self.next_request = now + REQUEST_INTERVAL;
    }
    pub fn eligible(&mut self, key: [i32; 2], now: Instant) -> bool {
        self.failures.retain(|(_, until)| now < *until);
        !self.failures.iter().any(|(failed, _)| *failed == key)
    }
    pub fn failed(&mut self, key: [i32; 2], now: Instant) {
        self.failures
            .retain(|(old, until)| *old != key && now < *until);
        if self.failures.len() == 32 {
            self.failures.remove(0);
        }
        self.failures.push((key, now + FAILURE_BACKOFF));
    }
    pub fn observe(&mut self, center: [i32; 2], near_missing: usize) {
        if let Some(old) = self.last_center {
            if old != center {
                if (0..2).any(|i| (i64::from(old[i]) - i64::from(center[i])).abs() > 1) {
                    self.teleports += 1;
                } else if near_missing == 0 {
                    self.ready_crossings += 1;
                } else {
                    self.cold_crossings += 1;
                    self.missing_at_crossings += near_missing as u64;
                }
            }
        }
        self.last_center = Some(center);
    }
    pub fn delivered(&mut self, sent: Instant, decoded: Instant) {
        if decoded < sent {
            return;
        }
        if self.latencies.len() == 32 {
            self.latencies.remove(0);
        }
        self.latencies
            .push(decoded.duration_since(sent).as_micros() as u64);
    }
    pub fn latency_samples(&self) -> usize {
        self.latencies.len()
    }
    pub fn latency_p95_us(&self) -> u64 {
        let mut sorted = self.latencies.clone();
        sorted.sort_unstable();
        sorted
            .get((sorted.len() * 95).div_ceil(100).saturating_sub(1))
            .copied()
            .unwrap_or(0)
    }
    pub fn report_due(&mut self, now: Instant) -> bool {
        if now < self.next_report {
            return false;
        }
        self.next_report = now + Duration::from_secs(2);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stationary_ring_is_25_and_covers_every_adjacent_next_render_area() {
        let p = plan([16., 16.], [0.; 2], [32; 2]).unwrap();
        assert_eq!(p.near.len(), 9);
        assert_eq!(p.warm.len(), 16);
        let all: std::collections::HashSet<_> = p
            .near
            .iter()
            .copied()
            .chain(p.warm.iter().map(|c| c.key))
            .collect();
        assert_eq!(all.len(), 25);
        for y in -1..=1 {
            for x in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        assert!(all.contains(&[x + dx, y + dy]));
                    }
                }
            }
        }
    }
    #[test]
    fn every_speculative_key_fits_the_unchanged_server_rule_at_chunk_edges() {
        for x in [0., 0.01, 16., 31.99, -0.01, -32., -32.01] {
            for y in [0., 0.01, 16., 31.99, -0.01] {
                let p = plan([x, y], [8., 8.], [32; 2]).unwrap();
                assert!(
                    p.warm
                        .iter()
                        .all(|c| server_admits([x, y], c.key, [32; 2], 0.))
                );
                assert_eq!(p.near.len(), 9);
                assert!(p.warm.len() <= 16);
            }
        }
        // Server strict comparison excludes the most distant corner at (0,0).
        assert!(!server_admits(
            [0.; 2],
            [2, 2],
            [32; 2],
            POSITION_MARGIN_BLOCKS
        ));
    }
    #[test]
    fn impending_border_and_direction_prioritize_the_needed_strip() {
        let p = plan([30., 16.], [8., 0.], [32; 2]).unwrap();
        assert!(
            p.warm[..3]
                .iter()
                .all(|c| c.key[0] == 2 && c.key[1].abs() <= 1)
        );
        assert!(
            p.warm[..3]
                .iter()
                .all(|c| (c.eta_seconds - 0.25).abs() < 1e-6)
        );
        let left = plan([1., 16.], [-8., 0.], [32; 2]).unwrap();
        assert!(left.warm[..3].iter().all(|c| c.key[0] == -2));
        let diagonal = plan([30., 30.], [8., 8.], [32; 2]).unwrap();
        assert!(diagonal.warm[..5].iter().all(|c| c.eta_seconds.is_finite()));
    }
    #[test]
    fn invalid_positions_stop_planning_and_negative_positions_use_floor() {
        assert_eq!(
            plan([-0.01, -32.01], [f32::NAN, f32::INFINITY], [32; 2])
                .unwrap()
                .center,
            [-1, -2]
        );
        for pos in [[f32::NAN, 0.], [0., f32::INFINITY], [f32::MAX, 0.]] {
            assert!(plan(pos, [0.; 2], [32; 2]).is_none());
        }
        assert!(plan([0.; 2], [0.; 2], [0, 32]).is_none());
    }
    #[test]
    fn request_and_failure_limits_are_wall_clock_based_and_do_not_catch_up_after_stalls() {
        let mut state = WarmState::new();
        let start = Instant::now();
        assert!(state.due(start));
        state.sent(start);
        assert!(!state.due(start + Duration::from_millis(99)));
        assert!(state.due(start + REQUEST_INTERVAL));
        let stalled = start + Duration::from_secs(10);
        state.sent(stalled);
        assert!(!state.due(stalled));
        assert_eq!(state.requests, 2);
        state.failed([2, 0], stalled);
        assert!(!state.eligible([2, 0], stalled));
        assert!(state.eligible([0, 2], stalled));
        assert!(state.eligible([2, 0], stalled + FAILURE_BACKOFF));
    }
    #[test]
    fn rolling_delivery_latency_is_bounded_and_does_not_treat_missing_samples_as_proof() {
        let mut state = WarmState::new();
        let now = Instant::now();
        assert_eq!(state.latency_samples(), 0);
        assert_eq!(state.latency_p95_us(), 0);
        for i in 1..=40 {
            state.delivered(now, now + Duration::from_millis(i));
        }
        assert_eq!(state.latency_samples(), 32);
        assert_eq!(state.latency_p95_us(), 39_000);
    }
    #[test]
    fn ordinary_movement_model_has_warm_crossings_with_1500ms_request_to_decode_delay() {
        // Deterministic budget/coverage model, not a measured network result.
        for diagonal in [false, true] {
            let initial = plan([16., 16.], [0.; 2], [32; 2]).unwrap();
            let mut loaded: std::collections::HashSet<_> = initial.near.into_iter().collect();
            let mut in_flight: Vec<([i32; 2], Instant)> = Vec::new();
            let mut state = WarmState::new();
            let start = Instant::now();
            let speed = if diagonal { 8_f32 / 2_f32.sqrt() } else { 8. };
            for frame in 0..=900 {
                let elapsed = frame as f32 / 30.;
                let now = start + Duration::from_nanos(frame * 1_000_000_000 / 30);
                in_flight.retain(|(key, ready)| {
                    if *ready <= now {
                        loaded.insert(*key);
                        false
                    } else {
                        true
                    }
                });
                let p = plan(
                    [
                        16. + speed * elapsed,
                        16. + if diagonal { speed * elapsed } else { 0. },
                    ],
                    [speed, if diagonal { speed } else { 0. }],
                    [32; 2],
                )
                .unwrap();
                let missing = p.near.iter().filter(|key| !loaded.contains(*key)).count();
                state.observe(p.center, missing);
                assert_eq!(
                    missing, 0,
                    "unexpected cold crossing at frame {frame}, diagonal={diagonal}"
                );
                if in_flight.len() < PENDING_LIMIT && state.due(now) {
                    if let Some(candidate) = p.warm.iter().find(|c| {
                        !loaded.contains(&c.key) && !in_flight.iter().any(|(key, _)| *key == c.key)
                    }) {
                        state.sent(now);
                        in_flight.push((candidate.key, now + Duration::from_millis(1500)));
                    }
                }
                loaded.retain(|key| {
                    let dx = (i64::from(key[0]) - i64::from(p.center[0]))
                        .unsigned_abs()
                        .saturating_sub(2);
                    let dy = (i64::from(key[1]) - i64::from(p.center[1]))
                        .unsigned_abs()
                        .saturating_sub(2);
                    dx * dx + dy * dy <= 1 // actual client's VD=1 unload hysteresis
                });
                assert!(in_flight.len() <= PENDING_LIMIT);
                assert!(loaded.len() <= 45);
            }
            assert!(state.ready_crossings >= 5);
            assert_eq!(state.cold_crossings, 0);
        }
    }
    #[test]
    fn startup_teleport_and_cold_crossing_are_distinct_and_log_rate_is_bounded() {
        let mut state = WarmState::new();
        let now = Instant::now();
        state.observe([0, 0], 9);
        assert_eq!(state.cold_crossings, 0);
        state.observe([1, 0], 0);
        state.observe([1, 0], 0);
        assert_eq!(state.ready_crossings, 1);
        state.observe([2, 1], 3);
        assert_eq!(state.cold_crossings, 1);
        assert_eq!(state.missing_at_crossings, 3);
        state.observe([20, 20], 9);
        assert_eq!(state.teleports, 1);
        assert_eq!(state.cold_crossings, 1);
        assert!(state.report_due(now));
        assert!(!state.report_due(now));
        assert!(state.report_due(now + Duration::from_secs(2)));
    }
}
