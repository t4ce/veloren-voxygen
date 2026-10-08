//! The selected character's original mesh, atlas and CPU animation uniforms.
//! This is the fixed figure input shared with the native draw contract.
use crate::{
    mesh::segment::generate_mesh_base_vol_figure,
    render::{
        Mesh, TerrainVertex,
        pipelines::figure::{FigureModel, Locals},
    },
    scene::{
        camera::{CameraMode, perspective_rh_zo_general},
        figure::{
            cache::{CharacterCacheKey, FigureKey},
            load::BodySpec,
        },
    },
};
use anim::{
    Animation,
    character::{CharacterSkeleton, IdleAnimation, SkeletonAttr},
};
use common::comp::{humanoid::Body, inventory::Inventory};
use std::sync::Arc;
use vek::*;

pub struct Geometry {
    pub vertices: Vec<[u32; 2]>,
    pub indices: Vec<u32>,
    pub atlas_size: [u32; 2],
    pub atlas: Vec<[u8; 4]>,
}
pub struct Frame {
    pub geometry: Arc<Geometry>,
    pub state: [u8; trueos::vgpu::VOXY_FIGURE_STATE_BYTES],
}
#[derive(Default)]
pub struct Preview {
    loader: super::figure_preview_loader::Loader<FigureKey<Body>, Arc<Geometry>>,
}
impl Preview {
    pub fn frame(
        &mut self,
        body: Body,
        inventory: Option<&Inventory>,
        size: Vec2<u32>,
        time: f32,
        sun_z: f32,
    ) -> Result<Option<Arc<Frame>>, String> {
        let key = FigureKey {
            body,
            item_key: None,
            extra: inventory.map(|inventory| {
                Arc::new(CharacterCacheKey::from(
                    None,
                    CameraMode::ThirdPerson,
                    inventory,
                ))
            }),
        };
        let geometry = self.loader.get_or_start(key, |key| {
            tracing::info!("Figure preview: selected mesh preparation entered");
            let manifests = Body::load_spec().map_err(|e| format!("figure manifests: {e}"))?;
            let meshes = Body::bone_meshes(&key, &manifests, ());
            let mut greedy = FigureModel::make_greedy();
            let mut mesh = Mesh::<TerrainVertex>::new();
            for (bone, part) in meshes.iter().enumerate() {
                if let Some((segment, offset)) = part {
                    generate_mesh_base_vol_figure(
                        segment,
                        (&mut greedy, &mut mesh, *offset, Vec3::one(), bone as u8),
                    );
                }
            }
            let (atlas, extent) = greedy.finalize();
            let vertices: Vec<[u32; 2]> = bytemuck::cast_slice(mesh.vertices()).to_vec();
            if vertices.is_empty() || vertices.len() % 4 != 0 {
                return Err("selected character has no quad mesh".into());
            }
            let indices = (0..vertices.len() as u32)
                .step_by(4)
                .flat_map(|i| [i, i + 1, i + 2, i + 2, i + 1, i + 3])
                .collect();
            let geometry = Arc::new(Geometry {
                vertices,
                indices,
                atlas_size: [extent.x as u32, extent.y as u32],
                atlas: atlas.col_lights,
            });
            let atlas_hash = geometry.atlas.iter().flatten().fold(
                0xcbf29ce484222325u64,
                |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3),
            );
            tracing::info!(body = ?key.body, vertices = geometry.vertices.len(),
                atlas_width = geometry.atlas_size[0], atlas_height = geometry.atlas_size[1],
                atlas_hash, "Figure preview: selected mesh and atlas ready");
            Ok(geometry)
        })?;
        let Some(geometry) = geometry else {
            return Ok(None);
        };
        let skeleton = IdleAnimation::update_skeleton(
            &CharacterSkeleton::new(false, 0.0, 1.0),
            (None, None, (None, None), time),
            time,
            &mut 0.0,
            &SkeletonAttr::from(&body),
        );
        let mut bones = [anim::FigureBoneData::default(); anim::MAX_BONE_COUNT];
        anim::compute_matrices(
            &skeleton,
            anim::vek::Mat4::rotation_z(-core::f32::consts::FRAC_PI_2),
            &mut bones,
            body,
        );
        let view = Mat4::look_at_rh(
            Vec3::new(4.0, -7.0, 3.0),
            Vec3::new(0.0, 0.0, 0.9),
            Vec3::unit_z(),
        );
        let projection = perspective_rh_zo_general(
            0.65f32,
            size.x.max(1) as f32 / size.y.max(1) as f32,
            1.0 / 1000.0,
            1.0 / 0.1,
        );
        let all = (projection * view).into_col_arrays();
        let mut state = [0u8; trueos::vgpu::VOXY_FIGURE_STATE_BYTES];
        // Exact std140 byte offsets of the original Globals block. Only the
        // ranges captured for this static profile are consumed by the pair.
        state[128..192].copy_from_slice(bytemuck::bytes_of(&all));
        state[224..236].copy_from_slice(bytemuck::bytes_of(&[0.0f32, 0.0, 0.9]));
        state[280..284].copy_from_slice(&sun_z.to_le_bytes());
        const _: () = assert!(core::mem::size_of::<Locals>() == 144);
        const _: () = assert!(core::mem::size_of::<[anim::FigureBoneData; 16]>() == 2048);
        state[512..656].copy_from_slice(bytemuck::bytes_of(&Locals::default()));
        state[672..].copy_from_slice(bytemuck::cast_slice(&bones));
        Ok(Some(Arc::new(Frame {
            geometry: geometry.clone(),
            state,
        })))
    }
}
