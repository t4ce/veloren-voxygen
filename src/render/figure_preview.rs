//! The selected character's original mesh, atlas and CPU animation uniforms.
//! This is the fixed figure input shared with the native draw contract.
use crate::{
    mesh::segment::generate_mesh_base_vol_figure,
    render::{
        Mesh, TerrainVertex,
        pipelines::figure::{FigureModel, Locals},
    },
    scene::{
        camera::perspective_rh_zo_general,
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
    animation: super::world_animation::State,
}
impl Preview {
    /// Use the authored avatar mesh in the same camera/depth space as near terrain.
    pub fn world_frame(
        &mut self, body: Body, inventory: Option<&Inventory>, size: Vec2<u32>,
        time: f32, sun_z: f32, position: Vec3<f32>, orientation: Quaternion<f32>,
        view_projection: Mat4<f32>, focus: Vec3<f32>, first_person: bool,
        animation: super::world_animation::Inputs<'_>,
    ) -> Result<Option<Arc<Frame>>, String> {
        let Some(preview) = self.frame_with_mode(body, inventory, size, time, sun_z, first_person)? else { return Ok(None); };
        let mut state = preview.state;
        state[128..192].copy_from_slice(bytemuck::bytes_of(&view_projection.into_col_arrays()));
        state[208..220].copy_from_slice(bytemuck::bytes_of(&focus.map(f32::trunc).into_array()));
        state[224..236].copy_from_slice(bytemuck::bytes_of(&focus.into_array()));
        state[640..652].copy_from_slice(bytemuck::bytes_of(&position.into_array()));
        let bones = self.animation.update(body, inventory, position, orientation, time, animation);
        state[672..].copy_from_slice(bytemuck::cast_slice(&bones));
        Ok(Some(Arc::new(Frame { geometry: preview.geometry.clone(), state })))
    }

    pub fn frame(
        &mut self,
        body: Body,
        inventory: Option<&Inventory>,
        size: Vec2<u32>,
        time: f32,
        sun_z: f32,
    ) -> Result<Option<Arc<Frame>>, String> {
        self.frame_with_mode(body, inventory, size, time, sun_z, false)
    }

    fn frame_with_mode(
        &mut self, body: Body, inventory: Option<&Inventory>, size: Vec2<u32>,
        time: f32, sun_z: f32, first_person: bool,
    ) -> Result<Option<Arc<Frame>>, String> {
        let key = FigureKey {
            body,
            item_key: None,
            extra: inventory.map(|inventory| {
                Arc::new(CharacterCacheKey::native_equipment(inventory, first_person))
            }),
        };
        let geometry = self.loader.get_or_start(key, move |key| {
            tracing::info!("Figure preview: selected mesh preparation entered");
            let manifests = Body::load_spec().map_err(|e| format!("figure manifests: {e}"))?;
            let meshes = Body::bone_meshes(&key, &manifests, ());
            let mut greedy = FigureModel::make_greedy();
            let mut mesh = Mesh::<TerrainVertex>::new();
            for (bone, part) in meshes.iter().enumerate() {
                // Authored character bone slots: hands, glider and both tools.
                if first_person && !matches!(bone, 5 | 6 | 11 | 12 | 13) { continue; }
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
