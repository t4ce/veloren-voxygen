//! Shared local camera for the selection character and retained airship.
use vek::*;

pub(crate) struct Camera {
    zoom: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self { zoom: 1.0 }
    }
}
impl Camera {
    pub fn scroll(&mut self, lines: f32) {
        if lines.is_finite() {
            self.zoom = (self.zoom * 1.2_f32.powf(-lines.clamp(-32., 32.))).clamp(1., 32.);
        }
    }
    pub fn eye(&self) -> Vec3<f32> {
        Vec3::new(0., 0., 0.9) + Vec3::new(4., -7., 2.1) * self.zoom
    }
    fn view(&self) -> Mat4<f32> {
        Mat4::look_at_rh(self.eye(), Vec3::new(0., 0., 0.9), Vec3::unit_z())
    }
    pub fn view_projection(&self, size: Vec2<u32>, reversed: bool) -> Mat4<f32> {
        let mut matrix = Mat4::perspective_rh_zo(
            0.65,
            size.x.max(1) as f32 / size.y.max(1) as f32,
            0.1,
            1000.,
        ) * self.view();
        if reversed {
            for column in 0..4 {
                matrix[(2, column)] = matrix[(3, column)] - matrix[(2, column)];
            }
        }
        matrix
    }
    pub fn terrain_camera(&self, size: Vec2<u32>) -> [[f32; 4]; 5] {
        let mut camera = super::terrain_feature::camera_from_view(
            self.view().inverted(),
            Vec3::zero(),
            0.65,
            size.x.max(1) as f32 / size.y.max(1) as f32,
        );
        camera[4][3] = 1000.;
        camera
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_is_bounded_finite_and_reversible() {
        let mut c = Camera::default();
        assert!((c.eye() - Vec3::new(4., -7., 3.)).magnitude() < 0.00001);
        c.scroll(-3.);
        let farther = c.eye();
        assert!(farther.y < -7.);
        c.scroll(3.);
        assert!((c.eye() - Vec3::new(4., -7., 3.)).magnitude() < 0.00001);
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            c.scroll(value);
        }
        assert!(c.eye().into_array().iter().all(|v| v.is_finite()));
        for _ in 0..100 {
            c.scroll(-1000.);
        }
        assert_eq!(c.zoom, 32.);
        for _ in 0..100 {
            c.scroll(1000.);
        }
        assert_eq!(c.zoom, 1.);
    }
    #[test]
    fn figure_and_platform_projection_agree_at_every_zoom() {
        let mut c = Camera::default();
        for _ in 0..24 {
            let size = Vec2::new(960, 540);
            let camera = c.terrain_camera(size);
            let eye = Vec3::from_slice(&camera[0][..3]);
            for point in [
                Vec3::zero(),
                Vec3::new(1., 0., 2.),
                Vec3::new(-20., 10., -10.),
            ] {
                let d = point - eye;
                let z = d.dot(Vec3::from_slice(&camera[3][..3]));
                let expected = Vec3::new(
                    d.dot(Vec3::from_slice(&camera[1][..3])) * camera[4][0] / camera[4][1],
                    d.dot(Vec3::from_slice(&camera[2][..3])) * camera[4][0],
                    camera[4][3] / (camera[4][3] - camera[4][2]) * z
                        - camera[4][2] * camera[4][3] / (camera[4][3] - camera[4][2]),
                ) / z;
                let clip = c.view_projection(size, false) * point.with_w(1.);
                let actual = Vec3::from(clip) / clip.w;
                assert!(
                    (actual - expected).magnitude() < 0.0002,
                    "{actual:?} {expected:?}"
                );
                let reverse = c.view_projection(size, true) * point.with_w(1.);
                assert!((reverse.z / reverse.w + actual.z - 1.).abs() < 0.0002);
            }
            c.scroll(-1.);
        }
    }
}
