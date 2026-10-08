//! Host harness for the production native draw planner; UI4 records are inert.
#![allow(dead_code)]
extern crate alloc;
#[path = "../../../src/menu/connection_screen.rs"]
mod connection_screen;
#[path = "../../../src/menu/main/ui/login_focus.rs"]
mod login_focus;
extern crate self as iced;
extern crate self as trueos;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rectangle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
pub mod ui4_winit {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum SpriteBackend {
        Bcs0,
        Compositor,
        PremultipliedCompositor,
    }
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct SpriteCorner {
        pub x: f32,
        pub y: f32,
        pub u: f32,
        pub v: f32,
    }
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct SpriteQuad {
        pub sprite_id: u32,
        pub c0: SpriteCorner,
        pub c1: SpriteCorner,
        pub c2: SpriteCorner,
        pub c3: SpriteCorner,
        pub color_rgba: u32,
        pub source_over: bool,
    }
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct SpriteCommand {
        pub quad: SpriteQuad,
        pub backend: SpriteBackend,
    }
    #[derive(Clone, Copy, Debug)]
    pub enum Error {
        Busy,
        Invalid,
        InvalidState,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Damage {
        pub x: u32,
        pub y: u32,
        pub width: u32,
        pub height: u32,
    }
    impl Damage {
        pub fn full(width: u32, height: u32) -> Self {
            Self {
                x: 0,
                y: 0,
                width,
                height,
            }
        }
    }
    #[derive(Default)]
    pub struct Observations {
        pub begins: usize,
        pub clear_regions: Vec<Damage>,
        pub region_unsupported: bool,
        pub region_attempts: usize,
        pub damages: Vec<Damage>,
        pub draws: usize,
        pub publications: u64,
        pub lease: bool,
        pub busy_draw: bool,
        pub background: bool,
        pub pending_resize: bool,
        pub extent: Option<(u32, u32)>,
        pub tracked_attempts: usize,
        pub commands: Vec<Vec<SpriteCommand>>,
        pub sky_colors: Vec<u32>,
        pub busy_sky: bool,
    }
    pub struct SceneTarget {
        pub observations: std::sync::Arc<std::sync::Mutex<Observations>>,
        pub live: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl SceneTarget {
        pub fn set_extent(&mut self, width: u32, height: u32) -> Result<(), Error> {
            let mut state = self.observations.lock().unwrap();
            let extent = (width, height);
            if state.extent.is_some_and(|previous| previous != extent) {
                state.pending_resize = true;
            }
            state.extent = Some(extent);
            Ok(())
        }
        pub fn upload_sprite_rgba8(
            &mut self,
            _: u32,
            _: u32,
            _: u32,
            _: &[u8],
        ) -> Result<(), Error> {
            Ok(())
        }
        pub fn begin_gpu_frame(&mut self) -> Result<(), Error> {
            let mut state = self.observations.lock().unwrap();
            if state.lease {
                return Err(Error::Invalid);
            }
            if state.publications > 0 && !self.live.load(std::sync::atomic::Ordering::Acquire) {
                return Err(Error::Busy);
            }
            state.lease = true;
            state.begins += 1;
            Ok(())
        }
        pub fn begin_gpu_frame_region(&mut self, damage: Damage) -> Result<(), Error> {
            {
                let mut state = self.observations.lock().unwrap();
                state.region_attempts += 1;
                if state.region_unsupported {
                    return Err(Error::Invalid);
                }
            }
            self.begin_gpu_frame()?;
            self.observations.lock().unwrap().clear_regions.push(damage);
            Ok(())
        }
        pub fn draw_sprite_commands(&mut self, commands: &[SpriteCommand]) -> Result<(), Error> {
            let mut state = self.observations.lock().unwrap();
            if !state.lease {
                return Err(Error::Invalid);
            }
            if state.busy_draw {
                state.busy_draw = false;
                state.lease = false;
                return Err(Error::Busy);
            }
            state.draws += 1;
            state.commands.push(commands.to_vec());
            Ok(())
        }
        pub fn draw_sky(&mut self, rgba: u32) -> Result<(), Error> {
            let mut state = self.observations.lock().unwrap();
            if !state.lease {
                return Err(Error::Invalid);
            }
            if state.busy_sky {
                state.busy_sky = false;
                return Err(Error::Busy);
            }
            state.sky_colors.push(rgba);
            Ok(())
        }
        pub fn publish(&mut self, damage: Damage) -> Result<(), Error> {
            let mut state = self.observations.lock().unwrap();
            if !state.lease {
                return Err(Error::Invalid);
            }
            state.lease = false;
            state.publications += 1;
            state.damages.push(damage);
            state.pending_resize = false;
            Ok(())
        }
        pub fn publish_tracked(&mut self, damage: Damage) -> Result<u64, Error> {
            {
                let mut state = self.observations.lock().unwrap();
                state.tracked_attempts += 1;
                // Match the kernel: receipts describe foreground publications,
                // not background updates or staged paired resize handoffs.
                if state.background || state.pending_resize {
                    return Err(Error::InvalidState);
                }
            }
            self.publish(damage)?;
            Ok(self.observations.lock().unwrap().publications)
        }
    }
}
pub mod ui {
    pub mod graphic {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct Id(u32);
        impl Id {
            pub fn from_index(index: u32) -> Self {
                Self(index)
            }
        }
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum Rotation {
            None,
            Cw90,
            Cw180,
            Cw270,
        }
    }
    pub mod ice {
        pub mod widget {
            pub mod image {
                pub type Handle = crate::ui::graphic::Id;
            }
        }
        pub mod renderer {
            pub mod primitive {
                include!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../src/ui/ice/renderer/primitive.rs"
                ));
            }
            pub mod activity;
            pub mod bcs;
            pub mod damage;
            pub mod presenter;
        }
    }
}

#[cfg(test)]
mod scheduling {
    use super::{
        ui::ice::renderer::{
            bcs::{FramePlan, LayerPlan},
            presenter::LayeredPresenter,
        },
        ui4_winit::*,
    };
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };
    fn target(live: bool) -> (SceneTarget, Arc<Mutex<Observations>>, Arc<AtomicBool>) {
        let observations = Arc::new(Mutex::new(Observations::default()));
        let live = Arc::new(AtomicBool::new(live));
        (
            SceneTarget {
                observations: observations.clone(),
                live: live.clone(),
            },
            observations,
            live,
        )
    }
    fn wait(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !predicate() {
            assert!(Instant::now() < deadline, "producer did not make progress");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn plan(x: f32) -> FramePlan {
        let quad = SpriteQuad {
            sprite_id: 0,
            c0: SpriteCorner {
                x,
                y: 0.,
                u: 0.,
                v: 0.,
            },
            c1: SpriteCorner {
                x: x + 1.,
                y: 0.,
                u: 1.,
                v: 0.,
            },
            c2: SpriteCorner {
                x: x + 1.,
                y: 1.,
                u: 1.,
                v: 1.,
            },
            c3: SpriteCorner {
                x,
                y: 1.,
                u: 0.,
                v: 1.,
            },
            color_rgba: u32::MAX,
            source_over: true,
        };
        FramePlan {
            foreground: LayerPlan {
                uploads: vec![],
                viewport: None,
                commands: vec![SpriteCommand {
                    quad,
                    backend: SpriteBackend::Bcs0,
                }],
            },
            background: LayerPlan::default(),
        }
    }
    #[test]
    fn full_size_buffers_only_draw_and_clear_the_centered_content_viewport() {
        let (scene, scene_state, _) = target(true);
        let (foreground, ui_state, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        let viewport = Damage {
            x: 0,
            y: 360,
            width: 1280,
            height: 720,
        };
        for revision in 1..=3 {
            let mut frame = plan(revision as f32);
            frame.background = frame.foreground.clone();
            let count = frame.foreground.commands.len();
            frame.place_in_viewport(0, 360, 1280, 720);
            assert_eq!(frame.foreground.commands.len(), count);
            assert_eq!(frame.background.commands.len(), count);
            presenter.submit(revision, vek::Vec2::new(1280, 1440), frame);
            wait(|| presenter.published_revision() == revision);
        }
        let scene = scene_state.lock().unwrap();
        assert!(scene.clear_regions.iter().all(|region| *region == viewport));
        assert_eq!(scene.clear_regions.len(), 3);
        assert!(scene.damages.iter().all(|region| *region == viewport));
        for commands in scene
            .commands
            .iter()
            .chain(ui_state.lock().unwrap().commands.iter())
        {
            for command in commands {
                for corner in [
                    command.quad.c0,
                    command.quad.c1,
                    command.quad.c2,
                    command.quad.c3,
                ] {
                    assert!(corner.x >= 0. && corner.x <= 1280.);
                    assert!(corner.y >= 360. && corner.y <= 1080.);
                }
            }
        }
        presenter.check().unwrap();
    }

    #[test]
    fn changing_content_policy_clears_old_pixels_in_each_background_buffer_once() {
        let (scene, state, _) = target(true);
        let (foreground, _, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        for revision in 1..=5 {
            let mut frame = plan(revision as f32);
            frame.background = frame.foreground.clone();
            let y = if revision == 1 { 0 } else { 4 };
            frame.place_in_viewport(0, y, 8, 4);
            presenter.submit(revision, vek::Vec2::new(8, 8), frame);
            wait(|| presenter.published_revision() == revision);
        }
        let regions = &state.lock().unwrap().clear_regions;
        assert_eq!(regions.len(), 5);
        assert_eq!(
            regions[0],
            Damage {
                x: 0,
                y: 0,
                width: 8,
                height: 4
            }
        );
        for region in &regions[1..4] {
            assert_eq!(*region, Damage::full(8, 8));
        }
        assert_eq!(
            regions[4],
            Damage {
                x: 0,
                y: 4,
                width: 8,
                height: 4
            }
        );
        presenter.check().unwrap();
    }

    #[test]
    fn unsupported_region_runtime_uses_full_frames_without_an_extra_lease() {
        let (scene, _, _) = target(true);
        let (foreground, state, _) = target(true);
        state.lock().unwrap().region_unsupported = true;
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        let static_command = plan(1.).foreground.commands[0];
        for revision in 1..=3 {
            let mut frame = plan(revision as f32 + 2.);
            frame.foreground.commands.insert(0, static_command);
            presenter.submit(revision, vek::Vec2::new(8, 8), frame);
            wait(|| presenter.published_revision() == revision);
        }
        let state = state.lock().unwrap();
        assert_eq!(state.region_attempts, 1);
        assert_eq!(state.begins, 3);
        assert!(state.clear_regions.is_empty());
        assert!(state.commands.iter().all(|commands| commands.len() == 2));
        assert!(
            state
                .damages
                .iter()
                .all(|damage| *damage == Damage::full(8, 8))
        );
        presenter.check().unwrap();
    }
    #[test]
    fn replacement_sprite_pixels_are_uploaded_even_when_commands_are_unchanged() {
        let (scene, _, _) = target(true);
        let (foreground, state, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        for revision in 1..=2 {
            let mut frame = plan(1.);
            frame.foreground.commands[0].quad.sprite_id = 7;
            frame
                .foreground
                .uploads
                .push(crate::ui::ice::renderer::bcs::Upload {
                    id: 7,
                    image: std::sync::Arc::new(image::RgbaImage::from_pixel(
                        1,
                        1,
                        image::Rgba([revision as u8, 0, 0, 255]),
                    )),
                });
            presenter.submit(revision, vek::Vec2::new(8, 8), frame);
            wait(|| presenter.published_revision() == revision);
        }
        assert_eq!(state.lock().unwrap().publications, 2);
        let (_, activity) = presenter.take_activity();
        assert_eq!(activity.uploads, 2);
        presenter.check().unwrap();
    }
    #[test]
    fn scene_receipt_never_blocks_the_foreground_producer() {
        let (scene, scene_state, _) = target(false);
        let (foreground, ui_state, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit(1, vek::Vec2::new(8, 8), plan(1.));
        wait(|| scene_state.lock().unwrap().publications == 1);
        // Paired resizing requires the foreground to publish while the scene
        // has no SURFLIVE receipt yet; waiting on it would deadlock both layers.
        wait(|| ui_state.lock().unwrap().publications == 1);
        wait(|| presenter.published_revision() == 1);
        presenter.submit(2, vek::Vec2::new(8, 8), plan(2.));
        wait(|| ui_state.lock().unwrap().publications == 2);
        // The unchanged background also acknowledges the new revision.
        wait(|| presenter.published_revision() == 2);
        assert_eq!(ui_state.lock().unwrap().commands[1][0].quad.c0.x, 2.);
        presenter.check().unwrap();
    }
    #[test]
    fn only_latest_unleased_ui_work_survives_backpressure() {
        let (scene, _, _) = target(true);
        let (foreground, state, live) = target(false);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit(1, vek::Vec2::new(8, 8), plan(1.));
        wait(|| state.lock().unwrap().publications == 1);
        wait(|| presenter.published_revision() == 1);
        presenter.submit(2, vek::Vec2::new(8, 8), plan(2.));
        presenter.submit(3, vek::Vec2::new(8, 8), plan(3.));
        // A queued animation frame is not visible while UI admission is busy.
        assert_eq!(presenter.published_revision(), 1);
        live.store(true, Ordering::Release);
        wait(|| state.lock().unwrap().publications == 2);
        wait(|| presenter.published_revision() == 3);
        assert_eq!(state.lock().unwrap().commands[1][0].quad.c0.x, 3.);
        presenter.check().unwrap();
    }
    #[test]
    fn busy_and_coalesced_jobs_do_not_advance_alternating_damage_debt() {
        let (scene, _, _) = target(true);
        let (foreground, state, live) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        for (revision, x) in [(1, 1.), (2, 2.)] {
            presenter.submit(revision, vek::Vec2::new(8, 8), plan(x));
            wait(|| presenter.published_revision() == revision);
        }
        live.store(false, Ordering::Release);
        presenter.submit(3, vek::Vec2::new(8, 8), plan(3.));
        let mut busy = 0;
        wait(|| {
            busy += presenter.take_activity().1.busy_begin;
            busy > 0
        });
        presenter.submit(4, vek::Vec2::new(8, 8), plan(6.));
        // Give the worker a chance to coalesce while admission remains blocked.
        thread::sleep(Duration::from_millis(20));
        live.store(true, Ordering::Release);
        wait(|| presenter.published_revision() == 4);
        presenter.submit(5, vek::Vec2::new(8, 8), plan(7.));
        wait(|| presenter.published_revision() == 5);
        let state = state.lock().unwrap();
        assert_eq!(state.publications, 4);
        assert_eq!(
            state.damages[2],
            Damage {
                x: 2,
                y: 0,
                width: 5,
                height: 1
            }
        );
        assert_eq!(
            state.damages[3],
            Damage {
                x: 6,
                y: 0,
                width: 2,
                height: 1
            }
        );
        assert_eq!(
            state.clear_regions[3],
            Damage {
                x: 2,
                y: 0,
                width: 6,
                height: 1
            }
        );
        presenter.check().unwrap();
    }
    #[test]
    fn background_and_paired_resize_use_untracked_publication() {
        let (scene, scene_state, _) = target(true);
        let (foreground, ui_state, _) = target(true);
        scene_state.lock().unwrap().background = true;
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit(1, vek::Vec2::new(8, 8), plan(1.));
        wait(|| {
            ui_state.lock().unwrap().publications == 1
                && scene_state.lock().unwrap().publications == 1
        });
        presenter.submit(2, vek::Vec2::new(10, 10), plan(2.));
        wait(|| {
            ui_state.lock().unwrap().publications == 2
                && scene_state.lock().unwrap().publications == 2
        });
        assert_eq!(scene_state.lock().unwrap().tracked_attempts, 0);
        assert_eq!(ui_state.lock().unwrap().tracked_attempts, 0);
        presenter.check().unwrap();
    }
    #[test]
    fn unchanged_frames_report_zero_new_gpu_work_after_initial_publication() {
        let (scene, _, _) = target(true);
        let (foreground, state, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit(1, vek::Vec2::new(8, 8), plan(1.));
        wait(|| state.lock().unwrap().publications == 1);
        let mut initial_draws = 0;
        let mut initial_bcs = 0;
        let mut initial_compositor = 0;
        let mut initial_publications = 0;
        wait(|| {
            let (_, activity) = presenter.take_activity();
            initial_draws += activity.draws;
            initial_bcs += activity.bcs_commands;
            initial_compositor += activity.compositor_commands;
            initial_publications += activity.publications;
            initial_publications == 1
        });
        assert_eq!((initial_draws, initial_bcs, initial_compositor), (1, 1, 0));
        presenter.submit(2, vek::Vec2::new(8, 8), plan(1.));
        let mut unchanged = 0;
        let mut actual_work = 0;
        wait(|| {
            let (_, activity) = presenter.take_activity();
            unchanged += activity.unchanged;
            actual_work +=
                activity.draws + activity.begins + activity.publications + activity.uploads;
            unchanged == 1
        });
        assert_eq!(actual_work, 0);
        assert_eq!(state.lock().unwrap().publications, 1);
        wait(|| presenter.published_revision() == 2);
    }
    #[test]
    fn canceled_busy_draw_reacquires_a_fresh_lease() {
        let (scene, _, _) = target(true);
        let (foreground, ui_state, _) = target(true);
        ui_state.lock().unwrap().busy_draw = true;
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit(1, vek::Vec2::new(8, 8), plan(1.));
        wait(|| ui_state.lock().unwrap().publications == 1);
        assert_eq!(ui_state.lock().unwrap().begins, 2);
        assert_eq!(ui_state.lock().unwrap().draws, 1);
        let (_, activity) = presenter.take_activity();
        assert_eq!(activity.busy_draw, 1);
        assert_eq!(activity.draws, 1);
        assert_eq!(activity.begins, 2);
        presenter.check().unwrap();
    }
    #[test]
    fn scene_handoff_clears_only_foreground_and_allows_menu_to_resume() {
        let (scene, scene_state, _) = target(true);
        let (foreground, ui_state, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit(1, vek::Vec2::new(640, 480), plan(0.0));
        wait(|| presenter.published_revision() == 1);
        let background_publications = scene_state.lock().unwrap().publications;
        presenter.clear_foreground(2, vek::Vec2::new(640, 480));
        wait(|| presenter.foreground_published_revision() == 2);
        assert!(ui_state.lock().unwrap().commands.last().unwrap().is_empty());
        assert_eq!(
            scene_state.lock().unwrap().publications,
            background_publications
        );
        presenter.submit(3, vek::Vec2::new(640, 480), plan(0.0));
        wait(|| presenter.published_revision() == 3);
        assert!(!ui_state.lock().unwrap().commands.last().unwrap().is_empty());
    }

    #[test]
    fn shutdown_does_not_wait_for_a_display_receipt() {
        let (scene, state, _) = target(false);
        let (foreground, _, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit(1, vek::Vec2::new(8, 8), plan(1.));
        wait(|| state.lock().unwrap().publications == 1);
        let start = Instant::now();
        drop(presenter);
        assert!(start.elapsed() < Duration::from_millis(200));
    }
    #[test]
    fn sky_handoff_preserves_foreground_and_coalesces_only_unleased_background_work() {
        let (scene, scene_state, scene_live) = target(false);
        scene_state.lock().unwrap().background = true;
        let (foreground, ui_state, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        let size = vek::Vec2::new(640, 480);
        let day = u32::from_le_bytes([36, 99, 191, 255]);
        let dusk = u32::from_le_bytes([255, 51, 38, 255]);
        presenter.submit_sky(1, size, day);
        wait(|| presenter.scene_published_revision() == 1);
        assert_eq!(scene_state.lock().unwrap().sky_colors, [day]);
        assert_eq!(ui_state.lock().unwrap().publications, 0);
        presenter.submit_foreground(2, size, plan(1.).foreground);
        wait(|| presenter.foreground_published_revision() == 2);
        presenter.submit_sky(3, size, day);
        wait(|| presenter.scene_published_revision() == 3);
        assert_eq!(scene_state.lock().unwrap().publications, 1);
        presenter.submit_sky(4, size, 0xff00_0000);
        presenter.submit_sky(5, size, dusk);
        presenter.submit_foreground(4, size, plan(2.).foreground);
        wait(|| presenter.foreground_published_revision() == 4);
        assert_eq!(scene_state.lock().unwrap().sky_colors, [day]);
        scene_live.store(true, Ordering::Release);
        wait(|| presenter.scene_published_revision() == 5);
        assert_eq!(scene_state.lock().unwrap().sky_colors, [day, dusk]);
        // A new extent needs a fresh complete sky, even with the same colour.
        presenter.submit_sky(6, vek::Vec2::new(800, 600), dusk);
        wait(|| presenter.scene_published_revision() == 6);
        assert_eq!(scene_state.lock().unwrap().sky_colors, [day, dusk, dusk]);
        assert_eq!(scene_state.lock().unwrap().tracked_attempts, 0);
        assert_eq!(ui_state.lock().unwrap().publications, 2);
        // Returning to menu work removes the sky policy, not the UI worker.
        presenter.submit(7, size, plan(3.));
        wait(|| presenter.published_revision() == 7);
        assert_eq!(scene_state.lock().unwrap().publications, 4);
        presenter.check().unwrap();
    }
    #[test]
    fn sky_import_busy_retries_the_same_write_lease() {
        let (scene, state, _) = target(true);
        state.lock().unwrap().busy_sky = true;
        let (foreground, _, _) = target(true);
        let presenter = LayeredPresenter::new(foreground, scene).unwrap();
        presenter.submit_sky(1, vek::Vec2::new(8, 8), 0xffbf_6324);
        wait(|| presenter.scene_published_revision() == 1);
        presenter.check().unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.begins, 1);
        assert_eq!(state.sky_colors, [0xffbf_6324]);
        assert_eq!(state.publications, 1);
    }
}
