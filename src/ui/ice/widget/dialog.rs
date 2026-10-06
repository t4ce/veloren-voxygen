//! Marks the interactive dialog separately from persistent menu chrome.
use crate::ui::ice::{
    Element,
    renderer::{IcedRenderer as Renderer, primitive::Primitive},
};
use core::hash::Hash;
use iced::{Clipboard, Event, Hasher, Layout, Length, Point, Rectangle, Widget, layout};

pub struct Dialog<'a, M> {
    id: u8,
    content: Element<'a, M>,
}
impl<'a, M> Dialog<'a, M> {
    pub fn new(id: u8, content: Element<'a, M>) -> Self {
        Self { id, content }
    }
}
impl<M> Widget<M, Renderer> for Dialog<'_, M> {
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
        self.id.hash(state);
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
        let (content, interaction) = self
            .content
            .draw(renderer, defaults, layout, cursor, viewport);
        (
            Primitive::Dialog {
                id: self.id,
                content: Box::new(content),
            },
            interaction,
        )
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
        self.content
            .on_event(event, layout, cursor, renderer, clipboard, messages)
    }
    fn overlay(&mut self, layout: Layout<'_>) -> Option<iced::overlay::Element<'_, M, Renderer>> {
        self.content.overlay(layout)
    }
}
impl<'a, M: 'a> From<Dialog<'a, M>> for Element<'a, M> {
    fn from(dialog: Dialog<'a, M>) -> Self {
        Self::new(dialog)
    }
}
