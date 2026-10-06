mod touch;

use crate::{
    Direction, GlobalState, PlayState, PlayStateResult,
    menu::main::MainMenuState,
    settings::get_fps,
    ui,
    window::{Event, EventLoop},
};
use common_base::span;
use core::{mem, time::Duration};
use tracing::debug;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    window::WindowId,
};

/// Construct GPU resources only once Winit permits surface creation.
pub fn run<F>(event_loop: EventLoop, initialize: F) -> Result<(), winit::error::EventLoopError>
where
    F: FnOnce(&dyn ActiveEventLoop) -> GlobalState + 'static,
{
    event_loop.run_app(App {
        initialize: Some(initialize),
        global_state: None,
        states: Vec::new(),
        file_drop: ui::ice::FileDropAdapter::default(),
        touches: touch::TouchTracker::default(),
    })
}

struct App<F> {
    initialize: Option<F>,
    global_state: Option<GlobalState>,
    states: Vec<Box<dyn PlayState>>,
    file_drop: ui::ice::FileDropAdapter,
    touches: touch::TouchTracker,
}

impl<F: FnOnce(&dyn ActiveEventLoop) -> GlobalState> ApplicationHandler for App<F> {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Some(initialize) = self.initialize.take() else {
            return;
        };
        let mut global_state = initialize(event_loop);
        let mut state = Box::new(MainMenuState::new(&mut global_state));
        state.enter(&mut global_state, Direction::Forwards);
        debug!(current_state = state.name(), "Started game with state");
        self.states.push(state);
        self.global_state = Some(global_state);
        // Voxy applies its own FPS cap in the game tick.
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        span!(_guard, "Handle WindowEvent");
        let Some(global_state) = self.global_state.as_mut() else {
            return;
        };
        let window = &mut global_state.window;
        if id != window.window().id() {
            return;
        }
        for (finger, position) in self.touches.handle(&event) {
            let scale = window.scale_factor();
            let logical = position.to_logical::<f64>(scale);
            let size = window.window().surface_size().to_logical::<f64>(scale);
            window.send_event(Event::Ui(ui::Event(conrod_core::event::Input::Touch(
                conrod_core::input::Touch {
                    phase: conrod_core::input::touch::Phase::Cancel,
                    id: conrod_core::input::touch::Id::new(finger.into_raw() as u64),
                    xy: [
                        logical.x - size.width / 2.0,
                        -(logical.y - size.height / 2.0),
                    ],
                },
            ))));
            window.send_event(Event::IcedUi(iced::Event::Touch(
                iced::touch::Event::FingerLost {
                    id: iced::touch::Finger(finger.into_raw() as u64),
                    position: iced::Point::new(logical.x as f32, logical.y as f32),
                },
            )));
        }
        for event in self.file_drop.handle(&event, event_loop) {
            window.send_event(Event::IcedUi(event));
        }
        // The window deduplicates resizes and emits the final UI dimensions.
        if !matches!(event, WindowEvent::SurfaceResized(_)) {
            if let Some(event) = ui::Event::try_from(&event, window.window(), window.modifiers()) {
                window.send_event(Event::Ui(event));
            }
            if let Some(event) =
                ui::ice::window_event(&event, window.scale_factor(), window.modifiers())
            {
                window.send_event(Event::IcedUi(event));
            }
        }
        if let WindowEvent::Focused(focused) = event {
            global_state.audio.set_master_volume(if focused {
                global_state.settings.audio.master_volume.get_checked()
            } else {
                global_state
                    .settings
                    .audio
                    .inactive_master_volume_perc
                    .get_checked()
                    * global_state.settings.audio.master_volume.get_checked()
            });
        }
        window.handle_window_event(event, &mut global_state.settings);
    }

    fn device_event(&mut self, _: &dyn ActiveEventLoop, _: Option<DeviceId>, event: DeviceEvent) {
        span!(_guard, "Handle DeviceEvent");
        if let Some(global_state) = self.global_state.as_mut() {
            global_state.window.handle_device_event(event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if let Some(global_state) = self.global_state.as_mut() {
            for event in self.file_drop.poll() {
                global_state.window.send_event(Event::IcedUi(event));
            }
            handle_main_events_cleared(&mut self.states, event_loop, global_state);
        }
    }
}

impl<F> Drop for App<F> {
    fn drop(&mut self) {
        if let Some(global_state) = self.global_state.as_mut() {
            global_state
                .settings
                .save_to_file_warn(&global_state.config_dir);
            global_state
                .profile
                .save_to_file_warn(&global_state.config_dir);
        }
    }
}

fn handle_main_events_cleared(
    states: &mut Vec<Box<dyn PlayState>>,
    event_loop: &dyn ActiveEventLoop,
    global_state: &mut GlobalState,
) {
    span!(guard, "Handle MainEventsCleared");
    // Screenshot / Fullscreen toggle
    global_state
        .window
        .resolve_deduplicated_events(&mut global_state.settings, &global_state.config_dir);
    // Run tick here

    // What's going on here?
    // ---------------------
    // The state system used by Voxygen allows for the easy development of
    // stack-based menus. For example, you may want a "title" state
    // that can push a "main menu" state on top of it, which can in
    // turn push a "settings" state or a "game session" state on top of it.
    // The code below manages the state transfer logic automatically so that we
    // don't have to re-engineer it for each menu we decide to add
    // to the game.
    let mut exit = true;
    while let Some(state_result) = states.last_mut().map(|last| {
        let events = global_state.window.fetch_events(&mut global_state.settings);
        last.tick(global_state, events)
    }) {
        // Implement state transfer logic.
        match state_result {
            PlayStateResult::Continue => {
                exit = false;
                break;
            }
            PlayStateResult::Shutdown => {
                debug!("Shutting down all states...");
                while states.last().is_some() {
                    states.pop().map(|old_state| {
                        debug!("Popped state '{}'.", old_state.name());
                        global_state.on_play_state_changed();
                    });
                }
            }
            PlayStateResult::Pop => {
                states.pop().map(|old_state| {
                    debug!("Popped state '{}'.", old_state.name());
                    global_state.on_play_state_changed();
                });
                states.last_mut().map(|new_state| {
                    new_state.enter(global_state, Direction::Backwards);
                });
            }
            PlayStateResult::Push(mut new_state) => {
                new_state.enter(global_state, Direction::Forwards);
                debug!("Pushed state '{}'.", new_state.name());
                states.push(new_state);
                global_state.on_play_state_changed();
            }
            PlayStateResult::Switch(mut new_state) => {
                new_state.enter(global_state, Direction::Forwards);
                states.last_mut().map(|old_state| {
                    debug!(
                        "Switching to state '{}' from state '{}'.",
                        new_state.name(),
                        old_state.name()
                    );
                    mem::swap(old_state, &mut new_state);
                    global_state.on_play_state_changed();
                });
            }
        }
    }

    if exit {
        event_loop.exit();
    }

    let mut capped_fps = false;

    drop(guard);

    if let Some(last) = states.last_mut() {
        capped_fps = last.capped_fps();

        span!(guard, "Render");

        #[cfg(target_os = "trueos")]
        let scene_display_ready = last.uses_native_ui() || global_state.window.prepare_scene_display();
        #[cfg(not(target_os = "trueos"))]
        let scene_display_ready = true;

        // Render the screen using the global renderer
        if scene_display_ready && !last.uses_native_ui() && let Some(mut drawer) = global_state
            .window
            .renderer_mut()
            .start_recording_frame(last.globals_bind_group())
            .expect("Unrecoverable render error when starting a new frame!")
        {
            if global_state.clear_shadows_next_frame {
                drawer.clear_shadows();
            }

            last.render(&mut drawer, &global_state.settings);
        };

        if global_state.clear_shadows_next_frame {
            global_state.clear_shadows_next_frame = false;
        }

        drop(guard);
    }

    if !exit {
        // Wait for the next tick.
        span!(guard, "Main thread sleep");

        // Enforce an FPS cap for the non-game session play states to prevent them
        // running at hundreds/thousands of FPS resulting in high GPU usage for
        // effectively doing nothing.
        let max_fps = get_fps(global_state.settings.graphics.max_fps);
        let max_background_fps = u32::min(
            max_fps,
            get_fps(global_state.settings.graphics.max_background_fps),
        );
        let max_fps_focus_adjusted = if global_state.window.focused {
            max_fps
        } else {
            max_background_fps
        };

        const TITLE_SCREEN_FPS_CAP: u32 = 60;

        let target_fps = if capped_fps {
            u32::min(TITLE_SCREEN_FPS_CAP, max_fps_focus_adjusted)
        } else {
            max_fps_focus_adjusted
        };

        global_state
            .clock
            .set_target_dt(Duration::from_secs_f64(1.0 / target_fps as f64));
        global_state.clock.tick();
        drop(guard);

        // Maintain global state.
        global_state.maintain();
    }
}
