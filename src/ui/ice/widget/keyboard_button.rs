//! Adds keyboard focus and activation to an existing pointer-driven button.
use crate::ui::ice::{Element, renderer::IcedRenderer as Renderer};
use iced::{Clipboard, Event, Hasher, Layout, Length, Point, Rectangle, Widget, keyboard, layout};

#[derive(Default)]
pub struct State {
    pub focused: bool,
    held: Option<keyboard::KeyCode>,
}
impl State {
    pub fn focus(&mut self, focused: bool) {
        self.focused = focused;
        self.held = None;
    }
}
pub struct KeyboardButton<'a, M> {
    state: &'a mut State,
    message: M,
    content: Element<'a, M>,
}
impl<'a, M> KeyboardButton<'a, M> {
    pub fn new(state: &'a mut State, message: M, content: Element<'a, M>) -> Self {
        Self {
            state,
            message,
            content,
        }
    }
}
impl<M: Clone> Widget<M, Renderer> for KeyboardButton<'_, M> {
    fn width(&self) -> Length {
        self.content.width()
    }
    fn height(&self) -> Length {
        self.content.height()
    }
    fn layout(&self, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.layout(renderer, limits)
    }
    fn hash_layout(&self, state: &mut Hasher) {
        self.content.hash_layout(state);
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        defaults: &<Renderer as iced::Renderer>::Defaults,
        layout: Layout<'_>,
        cursor: Point,
        viewport: &Rectangle,
    ) -> <Renderer as iced::Renderer>::Output {
        // Reuse the pointer hover artwork for a keyboard-focused button.
        let cursor = if self.state.focused {
            Point::new(layout.bounds().center_x(), layout.bounds().center_y())
        } else {
            cursor
        };
        self.content
            .draw(renderer, defaults, layout, cursor, viewport)
    }
    fn on_event(
        &mut self,
        event: Event,
        layout: Layout<'_>,
        cursor: Point,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        messages: &mut Vec<M>,
    ) -> iced::event::Status {
        if self.state.focused {
            match &event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key_code,
                    modifiers,
                }) if matches!(
                    key_code,
                    keyboard::KeyCode::Enter
                        | keyboard::KeyCode::NumpadEnter
                        | keyboard::KeyCode::Space
                ) && !modifiers.control
                    && !modifiers.alt
                    && !modifiers.logo =>
                {
                    if self.state.held.is_none() {
                        self.state.held = Some(*key_code);
                        messages.push(self.message.clone());
                    }
                    return iced::event::Status::Captured;
                }
                Event::Keyboard(keyboard::Event::KeyReleased { key_code, .. })
                    if self.state.held == Some(*key_code) =>
                {
                    self.state.held = None;
                    return iced::event::Status::Captured;
                }
                Event::Keyboard(keyboard::Event::CharacterReceived(_)) => {
                    return iced::event::Status::Captured;
                }
                _ => {}
            }
        }
        self.content
            .on_event(event, layout, cursor, renderer, clipboard, messages)
    }
}
impl<'a, M: Clone + 'a> From<KeyboardButton<'a, M>> for Element<'a, M> {
    fn from(button: KeyboardButton<'a, M>) -> Self {
        Self::new(button)
    }
}
