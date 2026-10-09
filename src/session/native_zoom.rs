//! Continuous wheel zoom bounded by the complete nine-chunk terrain view.
const MIN_DISTANCE: f32 = 0.1;
const EASE_SECONDS: f32 = 0.12;

pub(super) struct Zoom {
    target: f32,
    distance: f32,
    limit: f32,
}

impl Zoom {
    pub(super) fn new(distance: f32) -> Self {
        Self {
            target: distance,
            distance,
            limit: 256.0,
        }
    }

    pub(super) fn set_limit(&mut self, limit: f32) {
        if !limit.is_finite() || limit < MIN_DISTANCE {
            return;
        }
        // Keep the full-area endpoint anchored when resizing or changing FoV.
        let at_limit = (self.target - self.limit).abs() < 0.001;
        self.limit = limit;
        self.target = if at_limit {
            limit
        } else {
            self.target.min(limit)
        };
    }

    pub(super) fn scroll(&mut self, raw: f32) {
        if !raw.is_finite() {
            return;
        }
        // Window input already includes sensitivity and the +/-15 wheel scale.
        // Limit coalesced input to one notch. An offset exponential gives gentle
        // motion near the player and faster travel at overview distances, with
        // equal-and-opposite wheel input returning to exactly the same target.
        let factor = (1.15_f32.ln() * raw.clamp(-15.0, 15.0) / 15.0).exp();
        self.target = ((self.target + 5.0) * factor - 5.0).clamp(MIN_DISTANCE, self.limit);
    }

    pub(super) fn toggle(&mut self) {
        self.target = if self.target > MIN_DISTANCE + 0.001 {
            MIN_DISTANCE
        } else {
            10.0_f32.min(self.limit)
        };
    }

    pub(super) fn advance(&mut self, dt: f32) -> f32 {
        if dt.is_finite() && dt > 0.0 {
            let blend = 1.0 - (-dt / EASE_SECONDS).exp();
            self.distance += (self.target - self.distance) * blend;
            if (self.distance - self.target).abs() < 0.0001 {
                self.distance = self.target;
            }
        }
        self.distance
    }

    pub(super) fn far_plane(&self) -> f32 {
        (self.limit.max(self.distance) * 2.0 + 32.0).max(256.0)
    }
}

pub(super) fn fit_distance(
    chunk_size: [f32; 2],
    focus_z: f32,
    terrain_height: Option<(f32, f32)>,
    fov: f32,
    aspect: f32,
) -> f32 {
    // A player can be half a chunk away from the 3x3 area's center. Two
    // chunk widths per axis therefore enclose every corner about the player.
    let height = terrain_height.map_or(0.0, |(low, high)| {
        (focus_z - low).abs().max((high - focus_z).abs())
    });
    let radius = (chunk_size[0] * 2.0)
        .hypot(chunk_size[1] * 2.0)
        .hypot(height);
    let vertical_half = fov.clamp(15.0_f32.to_radians(), 120.0_f32.to_radians()) * 0.5;
    let horizontal_half = (vertical_half.tan() * aspect.max(0.01)).atan();
    // A sphere fits at radius/sin(half-angle), regardless of orbit direction.
    // Leave 15% breathing room in the narrower screen dimension.
    let distance = radius / vertical_half.min(horizontal_half).sin() * 1.15;
    if distance.is_finite() {
        distance.max(10.0)
    } else {
        160.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_flow_has_no_first_person_step_and_reverses_exactly() {
        let mut zoom = Zoom::new(0.1);
        zoom.set_limit(160.0);
        zoom.scroll(1.0);
        assert!(zoom.target > 0.1 && zoom.target < 0.2);
        let target = zoom.target;
        let distance = zoom.advance(1.0 / 30.0);
        assert!(distance > 0.1 && distance < target);
        zoom.scroll(-1.0);
        assert!((zoom.target - 0.1).abs() < 0.00001);
        let mut zoom = Zoom::new(10.0);
        zoom.scroll(15.0);
        assert!((zoom.target - 12.25).abs() < 0.00001);
        zoom.scroll(-15.0);
        assert!((zoom.target - 10.0).abs() < 0.00001);
    }

    #[test]
    fn easing_is_frame_rate_independent_and_never_overshoots_after_stalls() {
        fn run(fps: usize) -> f32 {
            let mut zoom = Zoom::new(10.0);
            zoom.scroll(15.0);
            for _ in 0..fps {
                zoom.advance(1.0 / fps as f32);
            }
            zoom.distance
        }
        assert!((run(30) - run(60)).abs() < 0.00001);
        let mut zoom = Zoom::new(10.0);
        zoom.scroll(15.0);
        assert!(zoom.advance(5.0) <= zoom.target);
    }

    #[test]
    fn footprint_height_and_portrait_aspect_fit_with_margin() {
        let radius = 64.0_f32.hypot(64.0);
        let fov = 65.0_f32.to_radians();
        let wide = fit_distance([32., 32.], 0., None, fov, 16. / 9.);
        assert!((wide * (fov * 0.5).sin() / radius - 1.15).abs() < 0.00001);
        let portrait = fit_distance([32., 32.], 0., None, fov, 0.5);
        assert!(portrait > wide);
        assert!(fit_distance([32., 32.], 0., Some((-100., 150.)), fov, 16. / 9.) > wide);
        assert!(fit_distance([32., 32.], 0., None, 30.0_f32.to_radians(), 16. / 9.) > wide);
    }

    #[test]
    fn limits_coalesced_events_and_keyboard_toggle_keep_the_same_flow() {
        let mut zoom = Zoom::new(10.0);
        zoom.set_limit(160.0);
        zoom.scroll(15000.0);
        assert!((zoom.target - 12.25).abs() < 0.00001);
        for _ in 0..100 {
            zoom.scroll(15.0);
        }
        assert_eq!(zoom.target, 160.0);
        zoom.set_limit(200.0);
        assert_eq!(zoom.target, 200.0);
        zoom.toggle();
        assert_eq!(zoom.target, 0.1);
        assert!(zoom.advance(1. / 30.) > 0.1);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            zoom.scroll(invalid);
            zoom.set_limit(invalid);
            zoom.advance(invalid);
            assert_eq!(zoom.target, 0.1);
        }
        for _ in 0..100 {
            zoom.advance(1. / 30.);
        }
        assert_eq!(zoom.distance, 0.1);
        zoom.toggle();
        assert_eq!(zoom.target, 10.0);
        assert!(zoom.advance(1. / 120.) < 2.35);
        assert!(zoom.far_plane() > zoom.limit);
    }
}
