//    tooltip_manager: TooltipManager,
mod cache;
pub mod component;
pub(crate) mod renderer;
pub mod widget;
mod winit;

pub use cache::{Font, FontId, RawFont, load_font};
pub use graphic::{Id, Rotation};
pub use iced::Event;
pub use renderer::{IcedRenderer, style};
pub use winit::{Clipboard, FileDropAdapter, window_event};

use super::{
    graphic::{self, Graphic},
    scale::{Scale, ScaleMode},
};
use crate::{
    error::Error,
    render::{Renderer, UiDrawer},
    window::Window,
};
use common::slowjob::SlowJobPool;
use common_base::span;
use iced::{Cache, Size, UserInterface, mouse};
use vek::*;

pub type Element<'a, M> = iced::Element<'a, M, IcedRenderer>;

pub struct IcedUi {
    renderer: IcedRenderer,
    cache: Option<Cache>,
    events: Vec<Event>,
    cursor_position: Vec2<f32>,
    // Scaling of the ui
    scale: Scale,
    scale_changed: bool,
    #[cfg(target_os = "trueos")]
    last_native_primitive: Option<renderer::primitive::Primitive>,
    #[cfg(target_os = "trueos")]
    native_activity: renderer::activity::UiActivity,
}
impl IcedUi {
    pub fn new(
        window: &mut Window,
        default_font: Font,
        scale_mode: ScaleMode,
    ) -> Result<Self, Error> {
        let scale_factor = window.scale_factor();
        let renderer = window.renderer_mut();
        let physical_resolution = renderer.resolution();
        let scale = Scale::new(physical_resolution, scale_factor, scale_mode, 1.2);

        let scaled_resolution = scale.scaled_resolution().map(|e| e as f32);

        // TODO: examine how much mem fonts take up and reduce clones if significant
        Ok(Self {
            renderer: IcedRenderer::new(
                renderer,
                scaled_resolution,
                physical_resolution,
                default_font,
            )?,
            cache: Some(Cache::new()),
            events: Vec::new(),
            // TODO: handle None
            cursor_position: Vec2::zero(),
            scale,
            scale_changed: false,
            #[cfg(target_os = "trueos")]
            last_native_primitive: None,
            #[cfg(target_os = "trueos")]
            native_activity: renderer::activity::UiActivity::default(),
        })
    }

    /// Native menu renderer: one logical pixel per surface pixel.
    #[cfg(target_os = "trueos")]
    pub fn new_native(resolution: Vec2<u32>, scale_factor: f64) -> Self {
        Self {
            renderer: IcedRenderer::new_native(resolution),
            cache: Some(Cache::new()),
            events: Vec::new(),
            cursor_position: Vec2::zero(),
            scale: Scale::new(resolution, scale_factor, ScaleMode::Absolute(1.0), 1.0),
            scale_changed: false,
            #[cfg(target_os = "trueos")]
            last_native_primitive: None,
            #[cfg(target_os = "trueos")]
            native_activity: renderer::activity::UiActivity::default(),
        }
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn invalidate_native(&mut self) {
        self.last_native_primitive = None;
    }

    #[cfg(target_os = "trueos")]
    pub fn maintain_native<'a, M, E: Into<Element<'a, M>>>(
        &mut self,
        root: E,
        resolution: Vec2<u32>,
        clipboard: &mut Clipboard,
    ) -> Result<(Vec<M>, Option<renderer::bcs::FramePlan>), String> {
        if self.scale.surface_resized(resolution) || self.scale_changed {
            self.native_activity.resize_invalidations += 1;
            self.scale_changed = false;
            self.renderer.resize_native(resolution);
            self.last_native_primitive = None;
        }
        use renderer::activity::micros;
        let input_events = self.events.len() as u64;
        self.native_activity.updates += 1;
        self.native_activity.input_events += input_events;
        let layout_started = std::time::Instant::now();
        let (messages, primitive, _) = self.update_interface(root, clipboard);
        self.native_activity.layout_us += micros(layout_started.elapsed());
        self.native_activity.messages += messages.len() as u64;
        // Iced events/layout continue every tick. Only changed draw output
        // needs a new retained command plan.
        let compare_started = std::time::Instant::now();
        let unchanged = self.last_native_primitive.as_ref() == Some(&primitive);
        self.native_activity.compare_us += micros(compare_started.elapsed());
        if unchanged && !self.renderer.native_dialog_animating() {
            self.native_activity.unchanged += 1;
            return Ok((messages, None));
        }
        if self.last_native_primitive.is_none() {
            self.native_activity.uncached_plans += 1;
        } else if input_events != 0 {
            self.native_activity.input_plans += 1;
        } else {
            self.native_activity.no_input_plans += 1;
        }
        let started = std::time::Instant::now();
        let plan = self.renderer.draw_native(&primitive)?;
        let elapsed = started.elapsed();
        self.native_activity.prepare_us += micros(elapsed);
        if elapsed >= std::time::Duration::from_millis(100) {
            tracing::warn!(
                elapsed_ms = elapsed.as_millis(),
                "Native iced asset preparation exceeded input tick budget"
            );
        }
        self.last_native_primitive = Some(primitive);
        Ok((messages, Some(plan)))
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn take_native_activity(
        &mut self,
    ) -> (
        renderer::activity::UiActivity,
        renderer::activity::PreparationActivity,
    ) {
        (
            std::mem::take(&mut self.native_activity),
            self.renderer.take_native_preparation_activity(),
        )
    }

    #[cfg(target_os = "trueos")]
    pub fn mark_scene_image(&mut self, id: graphic::Id) {
        self.renderer.mark_scene_image(id);
    }

    /// Add a new font that is referncable via the returned Id
    pub fn add_font(&mut self, font: RawFont) -> FontId {
        self.renderer.add_font(font)
    }

    /// Allows clearing out the fonts when switching languages
    pub fn clear_fonts(&mut self, default_font: Font) {
        #[cfg(target_os = "trueos")]
        {
            self.last_native_primitive = None;
        }
        self.renderer.clear_fonts(default_font);
    }

    /// Add a new graphic that is referencable via the returned Id
    pub fn add_graphic(&mut self, graphic: Graphic) -> Id {
        #[cfg(target_os = "trueos")]
        {
            self.last_native_primitive = None;
        }
        self.renderer.add_graphic(graphic)
    }

    pub fn replace_graphic(&mut self, id: Id, graphic: Graphic) {
        #[cfg(target_os = "trueos")]
        {
            self.last_native_primitive = None;
        }
        self.renderer.replace_graphic(id, graphic);
    }

    pub fn scale(&self) -> Scale {
        self.scale
    }

    pub fn set_scaling_mode(&mut self, mode: ScaleMode) {
        // Signal that change needs to be handled
        self.scale_changed |= self.scale.set_scaling_mode(mode);
    }

    /// Dpi factor changed
    /// Not to be confused with scaling mode
    pub fn scale_factor_changed(&mut self, scale_factor: f64) {
        self.scale_changed |= self.scale.scale_factor_changed(scale_factor);
    }

    pub fn handle_event(&mut self, event: Event) {
        use iced::window;
        match event {
            // Intercept resizing events
            // We check if the resolution of the renderer has changed to determine if a resize has
            // occured
            Event::Window(window::Event::Resized { .. }) => {}
            // Scale cursor movement events
            // Note: in some cases the scaling could be off if a resized event occured in the same
            // frame, in practice this shouldn't be an issue
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                // TODO: return f32 here
                let scale = self.scale.scale_factor_logical() as f32;
                let x = position.x / scale;
                let y = position.y / scale;
                // TODO: determine why iced moved cursor position out of the `Cache` and if we
                // may need to handle this in a different way to address
                // whatever issue iced was trying to address
                self.events.push(Event::Mouse(mouse::Event::CursorMoved {
                    position: iced::Point::new(x, y),
                }));
            }
            // Scale pixel scrolling events
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x, y },
            }) => {
                // TODO: return f32 here
                let scale = self.scale.scale_factor_logical() as f32;
                self.events.push(Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Pixels {
                        x: x / scale,
                        y: y / scale,
                    },
                }));
            }
            event => self.events.push(event),
        }
    }

    // TODO: produce root internally???
    // TODO: closure/trait for sending messages back? (take a look at higher level
    // iced libs)
    pub fn maintain<'a, M, E: Into<Element<'a, M>>>(
        &mut self,
        root: E,
        renderer: &mut Renderer,
        pool: Option<&SlowJobPool>,
        clipboard: &mut Clipboard,
    ) -> (Vec<M>, mouse::Interaction) {
        span!(_guard, "maintain", "IcedUi::maintain");
        // There could have been a series of resizes that put us back at the original
        // resolution.
        // Avoid resetting cache if window size didn't actually change.
        let resolution_changed = self.scale.surface_resized(renderer.resolution());

        // Handle window resizing, dpi factor change, and scale mode changing
        if self.scale_changed || resolution_changed {
            self.scale_changed = false;

            let scaled_resolution = self.scale.scaled_resolution().map(|e| e as f32);
            self.events
                .push(Event::Window(iced::window::Event::Resized {
                    width: scaled_resolution.x as u32,
                    height: scaled_resolution.y as u32,
                }));
            // Avoid panic in graphic cache when minimizing.
            // Somewhat inefficient for elements that won't change size after a window
            // resize
            let physical_resolution = renderer.resolution();
            if physical_resolution.map(|e| e > 0).reduce_and() {
                self.renderer
                    .resize(scaled_resolution, physical_resolution, renderer);
            }
        }

        let (messages, primitive, interaction) = self.update_interface(root, clipboard);
        self.renderer.draw(primitive, renderer, pool);
        (messages, interaction)
    }

    fn update_interface<'a, M, E: Into<Element<'a, M>>>(
        &mut self,
        root: E,
        clipboard: &mut Clipboard,
    ) -> (Vec<M>, renderer::primitive::Primitive, mouse::Interaction) {
        let mut cursor_position = iced::Point {
            x: self.cursor_position.x,
            y: self.cursor_position.y,
        };

        // TODO: convert to f32 at source
        let window_size = self.scale.scaled_resolution().map(|e| e as f32);

        span!(guard, "build user_interface");
        let mut user_interface = UserInterface::build(
            root,
            Size::new(window_size.x, window_size.y),
            self.cache.take().unwrap(),
            &mut self.renderer,
        );
        drop(guard);

        let messages = {
            span!(_guard, "update user_interface");
            let mut messages = Vec::new();
            for event in &self.events {
                // Replay pointer positions in order. A later move (including
                // another mouse) must not change where an earlier click lands.
                if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
                    cursor_position = *position;
                }
                if matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                        | Event::Touch(iced::touch::Event::FingerPressed { .. })
                ) {
                    clipboard.blur();
                }
                let _event_status_list = user_interface.update(
                    core::slice::from_ref(event),
                    cursor_position,
                    &self.renderer,
                    clipboard,
                    &mut messages,
                );
            }
            messages
        };
        self.cursor_position = Vec2::new(cursor_position.x, cursor_position.y);
        // Clear events
        self.events.clear();

        span!(guard, "draw user_interface");
        let (primitive, mouse_interaction) =
            user_interface.draw(&mut self.renderer, cursor_position);
        drop(guard);

        self.cache = Some(user_interface.into_cache());

        (messages, primitive, mouse_interaction)
    }

    pub fn render<'a>(&'a self, drawer: &mut UiDrawer<'_, 'a>) {
        self.renderer.render(drawer);
    }
}
