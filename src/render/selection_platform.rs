//! The original selection airship, loaded from local ship assets, never chunks.
use super::{
    figure_preview_loader::Loader,
    terrain_feature::{Frame, Geometry},
};
use crate::{
    scene::figure::load::BodySpec,
    terrain_preview::{Vertex, radial},
};
use anim::{
    Animation,
    ship::{IdleAnimation, ShipSkeleton, SkeletonAttr},
};
use common::{
    assets::AssetExt,
    comp::ship::{self, figuredata::ShipSpec},
    vol::{ReadVol, SizedVol},
};
use std::time::Instant;
use std::{collections::BTreeMap, sync::Arc};
use vek::*;

#[derive(Default)]
pub(crate) struct Platform {
    loader: Loader<(), Arc<Geometry>>,
}
impl Platform {
    pub fn frame(
        &mut self,
        camera: [[f32; 4]; 5],
        size: Vec2<u32>,
    ) -> Result<Option<Arc<Frame>>, String> {
        let geometry = self.loader.get_or_start((), |_| build())?;
        Ok(geometry.map(|geometry| {
            Arc::new(Frame {
                geometry: geometry.clone(),
                camera,
                metrics: super::terrain_layers::Metrics {
                    near_vertices: geometry.vertices.len() as u32,
                    near_revision: 1,
                    width: size.x,
                    height: size.y,
                    ..Default::default()
                },
                prepared_at: Instant::now(),
            })
        }))
    }
}
fn build() -> Result<Arc<Geometry>, String> {
    let body = ship::Body::DefaultAirship;
    // Same manifest/collider decoder as the original scene, restricted to this
    // body so selection does not load every other ship's voxel assets.
    let manifests =
        ShipSpec::load("preview.DefaultAirship").map_err(|e| format!("selection airship assets: {e}"))?;
    let key = crate::scene::figure::cache::FigureKey {
        body,
        item_key: None,
        extra: None,
    };
    let parts = ship::Body::bone_meshes(&key, &manifests, ());
    let skeleton = IdleAnimation::update_skeleton(
        &ShipSkeleton::default(),
        (None, None, 0., 0., Vec3::unit_y(), Vec3::unit_y()),
        0.,
        &mut 0.,
        &SkeletonAttr::from(&body),
    );
    let mut bone_data = [anim::FigureBoneData::default(); anim::MAX_BONE_COUNT];
    let pose = anim::compute_matrices(
        &skeleton,
        Mat4::<f32>::translation_3d(Vec3::<f32>::new(0., 0., -10.))
            * Mat4::rotation_z(-std::f32::consts::FRAC_PI_2),
        &mut bone_data,
        body,
    );
    let transforms = [pose.bone0, pose.bone1, pose.bone2, pose.bone3];
    let neighbors = [
        [-1, 0, 0],
        [1, 0, 0],
        [0, -1, 0],
        [0, 1, 0],
        [0, 0, -1],
        [0, 0, 1],
    ];
    let mut vertices = Vec::new();
    for (index, part) in parts.into_iter().take(4).enumerate() {
        let Some((volume, offset)) = part else {
            return Err(format!("selection airship part {index} missing"));
        };
        let mut planes = BTreeMap::<(usize, i32), Vec<(i32, i32, [u8; 3])>>::new();
        let end = volume.upper_bound();
        for z in 0..end.z {
            for y in 0..end.y {
                for x in 0..end.x {
                    let pos = Vec3::new(x, y, z);
                    let Some(color) = volume.get(pos).ok().and_then(|b| b.get_color()) else {
                        continue;
                    };
                    for (side, neighbor) in neighbors.iter().enumerate() {
                        if volume
                            .get(pos + Vec3::from(*neighbor))
                            .is_ok_and(|b| b.get_color().is_some())
                        {
                            continue;
                        }
                        let (plane, u, v) = match side {
                            0 | 1 => (x + (side == 1) as i32, y, z),
                            2 | 3 => (y + (side == 3) as i32, x, z),
                            _ => (z + (side == 5) as i32, x, y),
                        };
                        let shade = match side {
                            0 | 1 => 0.70,
                            2 | 3 => 0.82,
                            _ => 1.,
                        };
                        let rgb =
                            [color.r, color.g, color.b].map(|v| (v as f32 * shade).round() as u8);
                        planes.entry((side, plane)).or_default().push((u, v, rgb));
                    }
                }
            }
        }
        let mut local = Vec::<Vertex>::new();
        for ((side, plane), faces) in planes {
            radial::merge_plane(&mut local, side, plane, faces)?;
        }
        for mut vertex in radial::watertight::stitch(&local)? {
            let point =
                Vec3::new(vertex.position[0], vertex.position[1], vertex.position[2]) + offset;
            vertex.position = (transforms[index] * point.with_w(1.)).into_array();
            vertices.push(vertex);
        }
    }
    if vertices.is_empty() {
        return Err("selection airship has no opaque geometry".into());
    }
    tracing::info!(
        vertices = vertices.len(),
        "selection-platform: retained local DefaultAirship ready; terrain chunks not requested"
    );
    Ok(Arc::new(Geometry {
        vertices: vertices.into(),
        atlas: Arc::from([]),
    }))
}
