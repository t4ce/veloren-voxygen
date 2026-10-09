//! Host facade checks the real terrain transport; no GPU work is executed.
#![allow(dead_code)]
extern crate self as trueos;
#[path = "../../../src/render/terrain_feature.rs"]
mod terrain;
#[path = "../../../src/render/terrain_layers.rs"]
mod terrain_layers;
#[path = "../../../src/render/terrain_heartbeat.rs"]
mod terrain_heartbeat;
#[cfg(target_os = "trueos")]
#[path = "../target/cloud_transport.rs"]
mod clouds;
#[path = "../../../src/render/cloud_view.rs"]
mod cloud_view;
#[path = "../../../src/session/native_zoom.rs"]
mod native_zoom;
#[cfg(target_os = "trueos")]
#[path = "../target/camera_zoom.rs"]
mod camera_zoom;
#[path = "../../../src/render/terrain_composition.rs"]
mod terrain_composition;
mod terrain_feature {pub(crate) use crate::terrain::Geometry;}
mod client {
    pub struct Client {pub world:WorldData}
    pub struct WorldData {pub size:vek::Vec2<u16>,pub lod_base:Colors}
    pub struct Colors(pub u32);
    impl Colors {pub fn get(&self,_:vek::Vec2<i32>)->Option<&u32>{Some(&self.0)}}
    impl WorldData {
        pub fn chunk_size(&self)->vek::Vec2<u16>{self.size}
        pub fn min_chunk_alt(&self)->f32{0.}
        pub fn max_chunk_alt(&self)->f32{1.}
        pub fn alt_at(&self,_:vek::Vec2<i32>)->Option<f32>{Some(0.)}
    }
    impl Client {pub fn world_data(&self)->&WorldData{&self.world}}
}
mod render { pub mod terrain_feature {pub(crate) use crate::terrain::Geometry;} }
mod terrain_preview {
    pub const ATLAS_SIZE: u32 = 1024;
    pub const MAX_VERTICES: usize = 600_000;
    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    pub struct Vertex {
        pub position: [f32; 4],
        pub atlas_uv: [f32; 4],
    }
}
#[cfg(target_os = "trueos")]
pub mod vsys { pub const LOG_LEVEL_IMPORTANT:u32=1; pub fn log_record(_:u32,_:&str,_:&str)->Result<(),i32>{Ok(())} }
#[cfg(target_os = "trueos")]
pub mod vgpu {
    use std::sync::Mutex;
    pub const ERR_IO: i32 = -5;
    pub const ERR_BUSY: i32 = -16;
    pub const ERR_UNSUPPORTED: i32 = -95;
    pub const BUFFER_USAGE_MAP_WRITE: u32 = 1;
    pub const BUFFER_USAGE_VERTEX: u32 = 2;
    pub const BUFFER_USAGE_INDEX: u32 = 4;
    pub const PRIMITIVE_TOPOLOGY_TRIANGLE_LIST: u32 = 4;
    pub const SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64: u64 = 0xF84D_E655_632E_F102;
    pub const SHADER_PACKAGE_VOXY_FLAT_CLOUD_FNV1A64: u64 = 0x07442C9AD2E3AAF8;
    pub const INDEXED_DRAW_LOAD_COLOR: u32 = 1;
    pub const INDEXED_DRAW_DRAWABLE_DEPTH: u32 = 1 << 1;
    pub const INDEXED_DRAW_DEPTH_TEST: u32 = 1 << 2;
    pub const INDEXED_DRAW_DEPTH_WRITE: u32 = 1 << 3;
    pub const INDEXED_DRAW_CLEAR_DEPTH: u32 = 1 << 4;
    pub const INDEXED_DRAW_DEPTH_COMPARE_SHIFT: u32 = 8;
    #[derive(Copy, Clone)]
    pub struct Capabilities(u32);
    impl Capabilities {
        pub const BUFFER: Self = Self(1);
        pub const QUEUE: Self = Self(2);
        pub const TIMELINE: Self = Self(4);
        pub const RENDER: Self = Self(8);
        pub const PRESENT: Self = Self(16);
        pub fn union(self, other: Self) -> Self {
            Self(self.0 | other.0)
        }
    }
    #[derive(Copy, Clone)]
    pub struct Device;
    #[derive(Copy, Clone)]
    pub struct Queue;
    #[derive(Copy, Clone)]
    pub struct ShaderModule;
    #[derive(Copy, Clone)]
    pub struct RenderPipeline;
    #[derive(Copy, Clone)]
    pub struct Buffer(u32);
    impl Buffer {
        pub fn raw(self) -> u32 {
            self.0
        }
    }
    pub struct Surface;
    pub struct Timeline {
        pub value: u64,
    }
    pub enum QueueClass {
        Render,
    }
    #[derive(Default, Clone)]
    pub struct IndexedDraw {
        pub vertex_offset: u64,
        pub index_count: u32,
        pub clear_rgba8_srgb: u32,
        pub topology: u32,
        pub sampled_texture: u32,
        pub texture_width: u32,
        pub texture_height: u32,
        pub texture_pitch: u32,
        pub texture_reserved: u32,
        pub sampler_flags: u32,
    }
    #[derive(Default)]
    pub struct Recorder {
        pub events: Vec<&'static str>,
        pub buffers: Vec<Option<Vec<u8>>>,
        pub writes: Vec<(u32, usize, usize)>,
        pub draws: Vec<IndexedDraw>,
        pub busy_import: bool,
        pub fail_wait: bool,
        pub fail_buffer: bool,
    }
    pub static RECORD: Mutex<Option<Recorder>> = Mutex::new(None);
    fn record<T>(f: impl FnOnce(&mut Recorder) -> T) -> T {
        f(RECORD.lock().unwrap().as_mut().unwrap())
    }
    impl Device {
        pub fn open(caps: Capabilities) -> Result<Self, i32> {
            assert_eq!(caps.0, 31);
            record(|r| r.events.push("open"));
            Ok(Self)
        }
        pub fn create_queue(self, _: QueueClass) -> Result<Queue, i32> {
            Ok(Queue)
        }
        pub fn create_shader_module(self, digest: u64) -> Result<ShaderModule, i32> {
            assert!(digest == SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64 || digest == SHADER_PACKAGE_VOXY_FLAT_CLOUD_FNV1A64);
            Ok(ShaderModule)
        }
        pub fn create_render_pipeline(
            self,
            _: ShaderModule,
            stride: u32,
            flags: u32,
        ) -> Result<RenderPipeline, i32> {
            assert!(stride==32 || stride==20);
            assert_eq!(flags, 0);
            Ok(RenderPipeline)
        }
        pub fn create_buffer(self, size: usize, _: u32) -> Result<Buffer, i32> {
            record(|r| {
                if r.fail_buffer {
                    return Err(ERR_IO);
                }
                r.buffers.push(Some(vec![0; size]));
                Ok(Buffer(r.buffers.len() as u32))
            })
        }
        pub fn write_buffer(
            self,
            buffer: Buffer,
            offset: usize,
            bytes: &[u8],
        ) -> Result<usize, i32> {
            record(|r| {
                r.buffers[buffer.0 as usize - 1].as_mut().unwrap()[offset..offset + bytes.len()]
                    .copy_from_slice(bytes);
                r.writes.push((buffer.0, offset, bytes.len()));
                Ok(bytes.len())
            })
        }
        pub fn acquire_ui4_surface(self, _: u32) -> Result<Surface, i32> {
            record(|r| {
                if r.busy_import {
                    return Err(ERR_BUSY);
                }
                r.events.push("import");
                Ok(Surface)
            })
        }
        pub fn submit_ui4_indexed(
            self,
            _: Queue,
            _: Surface,
            _: RenderPipeline,
            _: Buffer,
            _: Buffer,
            draw: IndexedDraw,
        ) -> Result<Timeline, i32> {
            record(|r| {
                r.events.push("submit");
                r.draws.push(draw);
            });
            Ok(Timeline { value: 17 })
        }
        pub fn wait(self, _: Queue, value: u64) -> Result<(), i32> {
            assert_eq!(value, 17);
            record(|r| {
                r.events.push("wait");
                if r.fail_wait { Err(ERR_IO) } else { Ok(()) }
            })
        }
        pub fn destroy_buffer(self, buffer: Buffer) -> Result<(), i32> {
            record(|r| {
                r.events.push("destroy-buffer");
                r.buffers[buffer.0 as usize - 1] = None;
            });
            Ok(())
        }
        pub fn destroy_shader_module(self, _: ShaderModule) -> Result<(), i32> {
            record(|r| r.events.push("destroy-shader"));
            Ok(())
        }
        pub fn destroy_render_pipeline(self, _: RenderPipeline) -> Result<(), i32> {
            record(|r| r.events.push("destroy-pipeline"));
            Ok(())
        }
        pub fn destroy_queue(self, _: Queue) -> Result<(), i32> {
            Ok(())
        }
        pub fn close(self) -> Result<(), i32> {
            record(|r| r.events.push("close"));
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn frame() -> terrain::Frame {
        terrain::Frame {
            geometry: Arc::new(terrain::Geometry {
                vertices: vec![
                    terrain_preview::Vertex {
                        position: [1., 2., 3., 1.],
                        atlas_uv: [0.5, 0.5, 0., 1.]
                    };
                    3
                ]
                .into(),
                atlas: vec![[31, 140, 47, 255]; 1024 * 1024].into(),
            }),
            metrics: Default::default(),
            prepared_at: std::time::Instant::now(),
            camera: terrain::camera(
                [320., 320., 18.],
                [1., 0., 0.],
                [0., 0., 1.],
                [0., 1., 0.],
                65f32.to_radians(),
                16. / 9.,
            ),
        }
    }
    #[test]
    fn projection_preserves_eye_basis_fov_and_forward_depth() {
        let camera = frame().camera;
        let project = |p: [f32; 3]| {
            let delta: [f32; 3] = std::array::from_fn(|i| p[i] - camera[0][i]);
            let dot = |row: usize| (0..3).map(|i| delta[i] * camera[row][i]).sum::<f32>();
            let z = dot(3);
            let [focal, aspect, near, far] = camera[4];
            [
                dot(1) * focal / aspect / z,
                dot(2) * focal / z,
                far / (far - near) - near * far / (far - near) / z,
            ]
        };
        assert_eq!(&project([320., 330., 18.])[..2], &[0., 0.]);
        assert!(project([321., 330., 18.])[0] > 0.);
        assert!(project([320., 330., 19.])[1] > 0.);
        assert!(project([320., 320.1, 18.])[2].abs() < 0.001);
        assert!((project([320., 576., 18.])[2] - 1.).abs() < 0.00001);
        assert_eq!(size_of_val(&camera), 80);
    }

    #[test]
    fn inverse_view_mapping_preserves_negative_focus_and_camera_roll() {
        use vek::{Mat4, Vec3, Vec4};
        let focus = Vec3::new(-320.75, 112.5, 18.25);
        let inverse = Mat4::<f32>::translation_3d(Vec3::new(-0.75, 0.5, 4.25))
            * Mat4::<f32>::rotation_z(0.3)
            * Mat4::<f32>::rotation_x(-0.7)
            * Mat4::<f32>::rotation_y(1.2);
        let camera = terrain::camera_from_view(inverse, focus, 65f32.to_radians(), 16. / 9.);
        let offset = focus.map(f32::trunc);
        let eye = Vec3::from(inverse * Vec4::unit_w()) + offset;
        assert_eq!(&camera[0][..3], &eye.into_array());
        for local in [Vec3::new(1., 2., -10.), Vec3::new(-3., -1., -15.)] {
            let world = Vec3::from(inverse * Vec4::from(local).with_w(1.)) + offset;
            let delta = world - eye;
            let components: [f32; 3] = std::array::from_fn(|i| {
                delta.dot(Vec3::new(
                    camera[i + 1][0],
                    camera[i + 1][1],
                    camera[i + 1][2],
                ))
            });
            // The demo's positive forward-z equals Voxy's negative view-z.
            for (actual, expected) in components.into_iter().zip([local.x, local.y, -local.z]) {
                assert!((actual - expected).abs() < 0.0001);
            }
        }
    }
    #[test]
    fn background_composition_keeps_valid_near_when_far_fails_and_rejects_stale_results() {
        use std::time::{Duration,Instant};
        let client=client::Client{world:client::WorldData{size:vek::Vec2::zero(),lod_base:client::Colors(0)}};
        let near=frame().geometry;
        let coverage=terrain_layers::Coverage::new([0,0],[32,32]);
        let mut composition=terrain_composition::Composition::new();
        let source=terrain_layers::Metrics{near_warm_us:200,..Default::default()};
        assert!(composition.prepare(&client,Some(near.clone()),1,coverage,source).is_none());
        // A newer revision arrives while the first worker is in flight.
        let deadline=Instant::now()+Duration::from_secs(2);
        let (complete,metrics)=loop {
            if let Some(result)=composition.prepare(&client,Some(near.clone()),2,coverage,source) {
                assert_eq!(result.1.near_revision,2,"a stale worker must not become drawable");break result;
            }
            assert!(Instant::now()<deadline,"far retry must not block valid near");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(metrics.near_chunks,1);assert!(!metrics.far_ready);assert_eq!(metrics.warm_failures,1);
        assert_eq!(complete.vertices.len(),near.vertices.len());
        let retained=composition.prepare(&client,Some(near),3,coverage,
            terrain_layers::Metrics{near_warm_us:900,..Default::default()}).unwrap();
        assert!(std::sync::Arc::ptr_eq(&retained.0,&complete));
        assert_eq!(retained.1.near_revision,2);assert_eq!(retained.1.near_warm_us,200);
    }
    #[cfg(target_os = "trueos")]
    #[test]
    fn uploads_are_reused_and_gpu_ownership_survives_busy_and_failure() {
        use vgpu::*;
        *RECORD.lock().unwrap() = Some(Recorder::default());
        let cloud_frame=clouds::Frame {pixels: vec![0u8;32*3*4].into(),width:32,height:3,camera: frame().camera,in_world:false};
        let mut cloud_renderer=clouds::NativeClouds::open().unwrap();
        cloud_renderer.draw(123,&cloud_frame).unwrap();
        cloud_renderer.draw(123,&cloud_frame).unwrap();
        {
            let record=RECORD.lock().unwrap(); let r=record.as_ref().unwrap();
            assert_eq!(r.buffers.len(),3,"fullscreen buffers and packet reused");
            let vertices: Vec<[f32;5]> = r.buffers[0].as_ref().unwrap().chunks_exact(20)
                .map(|bytes|std::array::from_fn(|i|f32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap())))
                .collect();
            for vertex in vertices {
                // Resident GLSL viewport maps clip +Y to screen top.
                // The shader derives camera-up from 2*uv.y-1.
                assert_eq!(vertex[4]*2.0-1.0,vertex[1],"cloud ray must follow clip Y");
                assert_eq!(vertex[3]*2.0-1.0,vertex[0],"cloud ray must follow clip X");
            }

            assert_eq!(r.writes.len(),4,"only the dynamic packet is rewritten");
            assert_eq!(r.draws[0].clear_rgba8_srgb,0);
            assert_eq!(r.draws[0].texture_reserved,0,"clouds do not load or write depth");
            assert_eq!(r.draws[0].index_count,3);
        }
        let mut world_camera=cloud_frame.camera;
        world_camera[0][0]+=1000.;
        let world_frame=clouds::Frame {
            pixels: cloud_frame.pixels.clone(), width: cloud_frame.width, height: cloud_frame.height,
            camera: world_camera, in_world: true,
        };
        cloud_renderer.draw(123,&world_frame).unwrap();
        {
            let record=RECORD.lock().unwrap();let r=record.as_ref().unwrap();
            assert_eq!(r.writes.last().unwrap().2,68,"handoff replaces only the packed camera header");
            let packet=r.buffers[2].as_ref().unwrap();
            let value=|i:usize|f32::from_le_bytes(packet[i*4..i*4+4].try_into().unwrap());
            assert_eq!(value(0),cloud_frame.camera[0][0],"first world draw retains the selection eye");
            assert_eq!(value(15),cloud_frame.camera[4][0],"focal length keeps the shader ABI slot");
            assert_eq!(value(16),cloud_frame.camera[4][1],"aspect keeps the shader ABI slot");
        }
        RECORD.lock().unwrap().as_mut().unwrap().busy_import=true;
        assert_eq!(cloud_renderer.draw(123,&cloud_frame),Err(ERR_BUSY));
        RECORD.lock().unwrap().as_mut().unwrap().busy_import=false;
        cloud_renderer.draw(123,&cloud_frame).unwrap();
        drop(cloud_renderer);
        assert!(RECORD.lock().unwrap().as_ref().unwrap().buffers.iter().all(Option::is_none));
        *RECORD.lock().unwrap()=Some(Recorder {fail_wait:true,..Default::default()});
        let mut cloud_renderer=clouds::NativeClouds::open().unwrap();
        assert_eq!(cloud_renderer.draw(123,&cloud_frame),Err(ERR_IO));
        drop(cloud_renderer);
        assert!(!RECORD.lock().unwrap().as_ref().unwrap().events.contains(&"destroy-buffer"),"ambiguous completion keeps GPU ownership");
        *RECORD.lock().unwrap()=Some(Recorder::default());
        let mut renderer = terrain::NativeTerrain::open().unwrap();
        let mut frame = frame();
        renderer.draw(123, &frame, false).unwrap();
        frame.camera[0][0] += 1.;
        renderer.draw(123, &frame, false).unwrap();
        {
            let lock = RECORD.lock().unwrap();
            let r = lock.as_ref().unwrap();
            assert_eq!(r.writes.len(), 5); // mesh, indices, atlas, two cameras
            assert_eq!(
                &r.events[..7],
                &[
                    "open", "import", "submit", "wait", "import", "submit", "wait"
                ]
            );
            assert_eq!(r.writes[0].1, 80);
            assert_eq!(r.writes[2].2, 1024 * 1024 * 4);
            assert_eq!(&r.buffers[2].as_ref().unwrap()[..4], &[31, 140, 47, 255]);
            let draw = &r.draws[0];
            assert_eq!(
                (draw.vertex_offset, draw.index_count, draw.topology),
                (80, 3, 4)
            );
            assert_eq!(draw.clear_rgba8_srgb, 0);
            assert_eq!(
                (draw.texture_width, draw.texture_height, draw.texture_pitch),
                (1024, 1024, 4096)
            );
            assert_eq!(draw.texture_reserved >> INDEXED_DRAW_DEPTH_COMPARE_SHIFT, 3);
        }
        renderer.draw(123, &frame, true).unwrap();
        {
            let record = RECORD.lock().unwrap();
            let draw = record.as_ref().unwrap().draws.last().unwrap();
            assert_ne!(draw.texture_reserved & INDEXED_DRAW_LOAD_COLOR, 0);
            assert_ne!(draw.texture_reserved & INDEXED_DRAW_CLEAR_DEPTH, 0);
            assert_eq!(draw.texture_reserved >> INDEXED_DRAW_DEPTH_COMPARE_SHIFT, 3);
        }
        RECORD.lock().unwrap().as_mut().unwrap().busy_import = true;
        assert_eq!(renderer.draw(123, &frame, false), Err(ERR_BUSY));
        RECORD.lock().unwrap().as_mut().unwrap().busy_import = false;
        renderer.draw(123, &frame, false).unwrap();
        drop(renderer);
        assert!(
            RECORD
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .buffers
                .iter()
                .all(Option::is_none)
        );

        *RECORD.lock().unwrap() = Some(Recorder {
            fail_wait: true,
            ..Default::default()
        });
        let mut renderer = terrain::NativeTerrain::open().unwrap();
        assert_eq!(renderer.draw(123, &frame, false), Err(ERR_IO));
        drop(renderer);
        assert_eq!(
            RECORD
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .buffers
                .iter()
                .filter(|b| b.is_some())
                .count(),
            3
        );
        assert!(
            !RECORD
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .events
                .contains(&"close")
        );

        *RECORD.lock().unwrap() = Some(Recorder {
            fail_buffer: true,
            ..Default::default()
        });
        let mut renderer = terrain::NativeTerrain::open().unwrap();
        assert_eq!(renderer.draw(123, &frame, false), Err(ERR_IO));
        drop(renderer);
        let lock = RECORD.lock().unwrap();
        let r = lock.as_ref().unwrap();
        assert!(r.events.contains(&"destroy-pipeline"));
        assert!(r.events.contains(&"destroy-shader"));
        assert!(r.events.contains(&"close"));
    }
}
