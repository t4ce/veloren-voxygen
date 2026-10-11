//! Named camera stops. One wheel event selects one adjacent stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ZoomStop {
    Firstperson,
    Portrait,
    ThirdPerson1,
    ThirdPerson2,
    ThirdPerson3,
    Far,
    VeryFar,
    World,
}
const STOPS: [ZoomStop; 8] = [
    ZoomStop::Firstperson, ZoomStop::Portrait,
    ZoomStop::ThirdPerson1, ZoomStop::ThirdPerson2, ZoomStop::ThirdPerson3,
    ZoomStop::Far, ZoomStop::VeryFar, ZoomStop::World,
];
const FIRSTPERSON_DISTANCE: f32 = 0.1;
const PORTRAIT_DISTANCE: f32 = 3.0;
const THIRD_PERSON_DISTANCES: [f32; 3] = [5.0, 8.0, 12.0];
const VERY_FAR_CHUNKS_PER_SIDE: f32 = 9.0;

pub(super) struct Zoom {
    index: usize,
    overview_distances: [f32; 3],
}
impl Zoom {
    pub(super) fn new() -> Self {
        Self { index: 0, overview_distances: [48.0, 432.0, 4096.0] }
    }
    pub(super) fn stop(&self) -> ZoomStop { STOPS[self.index] }
    pub(super) fn scroll(&mut self, raw: f32) {
        if !raw.is_finite() || raw == 0.0 { return; }
        self.index = if raw > 0.0 { (self.index + 1).min(STOPS.len() - 1) }
                     else { self.index.saturating_sub(1) };
    }
    pub(super) fn toggle(&mut self) {
        self.index = if self.index == 0 { 1 } else { 0 };
    }
    pub(super) fn hide_body(&self) -> bool { self.stop() == ZoomStop::Firstperson }
    pub(super) fn focus_height(&self, height: f32) -> f32 { height * 0.9 }
    pub(super) fn set_overviews(
        &mut self, chunk: [f32; 2], world: [f32; 2], world_height: f32,
        fov: f32, aspect: f32,
    ) {
        let far = fit_footprint(chunk, 0.0, fov, aspect).max(THIRD_PERSON_DISTANCES[2] + 1.0);
        let very_far = fit_footprint(chunk.map(|v| v * VERY_FAR_CHUNKS_PER_SIDE), 0.0, fov, aspect).max(far);
        let world = fit_footprint(world, world_height, fov, aspect).max(very_far);
        self.overview_distances = [far, very_far, world];
    }
    pub(super) fn distance(&self) -> f32 {
        match self.stop() {
            ZoomStop::Firstperson => FIRSTPERSON_DISTANCE,
            ZoomStop::Portrait => PORTRAIT_DISTANCE,
            ZoomStop::ThirdPerson1 => THIRD_PERSON_DISTANCES[0],
            ZoomStop::ThirdPerson2 => THIRD_PERSON_DISTANCES[1],
            ZoomStop::ThirdPerson3 => THIRD_PERSON_DISTANCES[2],
            ZoomStop::Far => self.overview_distances[0],
            ZoomStop::VeryFar => self.overview_distances[1],
            ZoomStop::World => self.overview_distances[2],
        }
    }
    pub(super) fn far_plane(&self) -> f32 { (self.distance() * 4.0 + 256.0).max(256.0) }
}

fn fit_footprint(size: [f32; 2], height: f32, fov: f32, aspect: f32) -> f32 {
    let half_vertical = fov.clamp(15.0_f32.to_radians(), 120.0_f32.to_radians()) * 0.5;
    let half_horizontal = (half_vertical.tan() * aspect.max(0.01)).atan();
    let radius = (size[0] * 0.5).hypot(size[1] * 0.5).hypot(height * 0.5);
    radius / half_vertical.min(half_horizontal).sin() * 1.1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wheel_walks_named_stops_and_clamps_at_endpoints() {
        let mut zoom = Zoom::new();
        for expected in STOPS {
            assert_eq!(zoom.stop(), expected);
            assert_eq!(zoom.hide_body(), expected == ZoomStop::Firstperson);
            zoom.scroll(15000.0);
        }
        assert_eq!(zoom.stop(), ZoomStop::World);
        for expected in STOPS.into_iter().rev() {
            assert_eq!(zoom.stop(), expected);
            zoom.scroll(-15.0);
        }
        assert_eq!(zoom.stop(), ZoomStop::Firstperson);
    }
    #[test]
    fn framing_uses_server_dimensions_and_viewport_shape() {
        let mut zoom = Zoom::new();
        zoom.set_overviews([32.0; 2], [16384.0; 2], 1024.0, 1.0, 1.0);
        let [far, very_far, world] = zoom.overview_distances;
        assert!((very_far / far - 9.0).abs() < 0.0001);
        assert!(world > very_far);
        zoom.set_overviews([32.0; 2], [32768.0; 2], 1024.0, 1.0, 0.5);
        assert!(zoom.overview_distances[2] > world * 2.0);
        zoom.index = 7;
        assert!(zoom.far_plane() > zoom.distance() * 2.0);
    }
    #[test]
    fn portrait_stays_at_three_metres_and_invalid_input_does_nothing() {
        let mut zoom = Zoom::new();
        zoom.scroll(0.01);
        assert_eq!(zoom.stop(), ZoomStop::Portrait);
        assert_eq!(zoom.distance(), 3.0);
        for raw in [0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] { zoom.scroll(raw); }
        assert_eq!(zoom.stop(), ZoomStop::Portrait);
        zoom.toggle();
        assert_eq!(zoom.distance(), 0.1);
        zoom.toggle();
        assert_eq!(zoom.stop(), ZoomStop::Portrait);
    }
}
