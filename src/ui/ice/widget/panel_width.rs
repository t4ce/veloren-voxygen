//! Prefer a narrower panel while preserving enough width for its longest line.
use crate::ui::ice::{Element, renderer::IcedRenderer as Renderer};
use core::hash::Hash;
use iced::{Clipboard, Event, Hasher, Layout, Length, Point, Rectangle, Widget, layout};

pub struct PanelWidth<'a, M> {
    text: String,
    text_size: u16,
    content: Element<'a, M>,
}
impl<'a, M> PanelWidth<'a, M> {
    pub fn new(text: String, text_size: u16, content: Element<'a, M>) -> Self {
        Self {
            text,
            text_size,
            content,
        }
    }
}
impl<M> Widget<M, Renderer> for PanelWidth<'_, M> {
    fn width(&self) -> Length {
        self.content.width()
    }
    fn height(&self) -> Length {
        self.content.height()
    }
    fn layout(&self, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let (text_width, _) = iced::text::Renderer::measure(
            renderer,
            &self.text,
            self.text_size,
            Default::default(),
            iced::Size::new(f32::INFINITY, f32::INFINITY),
        );
        let available = limits.max().width;
        let width = (available * 0.8).max(text_width + 48.).min(available);
        self.content.layout(
            renderer,
            &limits.clone().max_width(width as u32).width(Length::Fill),
        )
    }
    fn hash_layout(&self, state: &mut Hasher) {
        self.text.hash(state);
        self.text_size.hash(state);
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
        self.content
            .on_event(event, layout, cursor, renderer, clipboard, messages)
    }
    fn overlay(&mut self, layout: Layout<'_>) -> Option<iced::overlay::Element<'_, M, Renderer>> {
        self.content.overlay(layout)
    }
}
impl<'a, M: 'a> From<PanelWidth<'a, M>> for Element<'a, M> {
    fn from(dialog: PanelWidth<'a, M>) -> Self {
        Self::new(dialog)
    }
}
