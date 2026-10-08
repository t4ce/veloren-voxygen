//! Brief cloud-camera handoff between character selection and the live world.
use std::time::{Duration, Instant};
use vek::{Quaternion, Vec3};

type Camera = [[f32; 4]; 5];
const STEP: Duration = Duration::from_millis(100);
const STEPS: u32 = 4;

#[derive(Default)]
pub(crate) struct CloudView {
    previous: Option<(bool, Camera)>,
    handoff: Option<(Instant, Camera)>,
}
impl CloudView {
    pub fn update(&mut self, in_world: bool, target: Camera, now: Instant) -> Option<Camera> {
        if let Some((previous_world, previous)) = self.previous {
            if previous_world != in_world {
                self.handoff = Some((now, previous));
            }
        }
        let camera = self.handoff.and_then(|(started, from)| {
            let steps = (now.duration_since(started).as_millis() / STEP.as_millis()).min(u128::from(STEPS)) as u32;
            if steps >= STEPS {
                self.handoff = None;
                None
            } else {
                Some(interpolate(from, target, steps as f32 / STEPS as f32))
            }
        });
        self.previous = Some((in_world, camera.unwrap_or(target)));
        camera
    }
}
fn orientation(camera: Camera) -> Quaternion<f32> {
    let right = Vec3::new(camera[1][0], camera[1][1], camera[1][2]).normalized();
    let up = Vec3::new(camera[2][0], camera[2][1], camera[2][2]).normalized();
    let align: Quaternion<f32> = Quaternion::rotation_from_to_3d(Vec3::<f32>::unit_x(), right);
    let aligned_up: Vec3<f32> = align * Vec3::<f32>::unit_y();
    let roll = aligned_up.cross(up).dot(right).atan2(aligned_up.dot(up));
    (Quaternion::rotation_3d(roll, right) * align).normalized()
}
fn interpolate(from: Camera, target: Camera, t: f32) -> Camera {
    if t == 0.0 { return from; }
    let mut camera = target;
    for i in 0..3 {
        camera[0][i] = from[0][i] + (target[0][i] - from[0][i]) * t;
    }
    // Slerp preserves a valid camera basis even across opposite viewing directions.
    let rotation = Quaternion::slerp(orientation(from), orientation(target), t).normalized();
    for (i, axis) in [Vec3::<f32>::unit_x(), Vec3::<f32>::unit_y(), -Vec3::<f32>::unit_z()].into_iter().enumerate() {
        let v = rotation * axis;
        camera[i+1] = [v.x,v.y,v.z,0.0];
    }
    camera[4][0] = from[4][0] + (target[4][0] - from[4][0]) * t;
    // Use the live aspect ratio during maximize/resize, rather than stretching the viewport.
    camera
}

#[cfg(test)]
mod tests {
    use super::*;
    fn camera() -> Camera {
        [[0.;4],[1.,0.,0.,0.],[0.,0.,1.,0.],[0.,1.,0.,0.],[1.,1.,0.1,256.]]
    }
    #[test]
    fn world_entry_uses_four_steps_then_tracks_live_camera_without_lag() {
        let now=Instant::now(); let mut view=CloudView::default();
        let from=camera(); let mut target=from; target[0][0]=100.;
        assert!(view.update(false,from,now).is_none());
        assert_eq!(view.update(true,target,now).unwrap(),from);
        for step in 1..4 {
            let blended=view.update(true,target,now+STEP*step).unwrap();
            assert_eq!(blended[0][0],25.*step as f32);
        }
        assert!(view.update(true,target,now+STEP*4).is_none());
        target[0][0]=200.;
        assert!(view.update(true,target,now+STEP*5).is_none());
    }
    #[test]
    fn opposite_directions_keep_finite_orthonormal_camera_rays() {
        let from=camera(); let mut target=from;
        target[1]=[-1.,0.,0.,0.]; target[3]=[0.,-1.,0.,0.];
        for step in 1..4 {
            let camera=interpolate(from,target,step as f32/4.);
            let axis=|i:usize|Vec3::new(camera[i][0],camera[i][1],camera[i][2]);
            for i in 1..4 { assert!((axis(i).magnitude()-1.).abs()<0.00001); }
            assert!(axis(1).dot(axis(2)).abs()<0.00001);
            assert!(axis(2).dot(axis(3)).abs()<0.00001);
            assert!((axis(1).cross(axis(2))+axis(3)).magnitude()<0.00001);
        }
    }
    #[test]
    fn returning_to_selection_starts_from_the_visible_intermediate_camera() {
        let now=Instant::now();let mut view=CloudView::default();let from=camera();let mut world=from;
        world[0][0]=100.;view.update(false,from,now);view.update(true,world,now);
        let visible=view.update(true,world,now+STEP).unwrap();
        assert_eq!(view.update(false,from,now+STEP).unwrap(),visible);
        assert!(view.update(false,from,now+STEP*5).is_none());
    }
}
