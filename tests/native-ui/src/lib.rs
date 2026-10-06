//! Host harness for the production native draw planner; UI4 records are inert.
#![allow(dead_code)]
extern crate alloc;
#[path = "../../../src/menu/connection_screen.rs"]
mod connection_screen;
extern crate self as iced;
extern crate self as trueos;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rectangle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
pub mod ui4_solara_text {
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
    pub struct Damage;
    impl Damage {
        pub fn full(_: u32, _: u32) -> Self {
            Self
        }
    }
    #[derive(Default)]
    pub struct Observations {
        pub begins: usize,
        pub draws: usize,
        pub publications: u64,
        pub lease: bool,
        pub busy_draw: bool,
        pub background: bool,
        pub pending_resize: bool,
        pub extent: Option<(u32, u32)>,
        pub tracked_attempts: usize,
        pub commands: Vec<Vec<SpriteCommand>>,
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
        pub fn publish(&mut self, _: Damage) -> Result<(), Error> {
            let mut state = self.observations.lock().unwrap();
            if !state.lease {
                return Err(Error::Invalid);
            }
            state.lease = false;
            state.publications += 1;
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
        ui4_solara_text::*,
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
                commands: vec![SpriteCommand {
                    quad,
                    backend: SpriteBackend::Bcs0,
                }],
            },
            background: LayerPlan::default(),
        }
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
}
