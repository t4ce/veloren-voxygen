//! Two center-cut halves of the language banner, anchored to the panel edges.
use crate::ui::ice::{
    Element, Rotation,
    renderer::{IcedRenderer, primitive::Primitive},
    widget::image::Handle,
};
use core::hash::Hash;
use iced::{Hasher, Layout, Length, Point, Rectangle, Size, Widget, layout, mouse};
use vek::{Aabr, Rgba, Vec2};

pub struct Ribbon {
    handle: Handle,
}
impl Ribbon {
    pub fn new(handle: Handle) -> Self {
        Self { handle }
    }
}
impl<M> Widget<M, IcedRenderer> for Ribbon {
    fn width(&self) -> Length {
        Length::Fill
    }
    fn height(&self) -> Length {
        Length::Units(34)
    }
    fn layout(&self, _: &IcedRenderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(
            limits
                .width(Length::Fill)
                .height(Length::Units(34))
                .resolve(Size::ZERO),
        )
    }
    fn hash_layout(&self, state: &mut Hasher) {
        self.handle.hash(state);
    }
    fn draw(
        &self,
        renderer: &mut IcedRenderer,
        _: &<IcedRenderer as iced::Renderer>::Defaults,
        layout: Layout<'_>,
        _: Point,
        _: &Rectangle,
    ) -> <IcedRenderer as iced::Renderer>::Output {
        let bounds = layout.bounds();
        let (width, height) =
            crate::ui::ice::widget::image::Renderer::dimensions(renderer, self.handle);
        if width == 0 || height == 0 {
            return (Primitive::Nothing, mouse::Interaction::default());
        }
        let scale = (bounds.height / height as f32).min(bounds.width / width as f32);
        let split = width as f32 * 0.5;
        let half_width = split * scale;
        let primitives = [
            (0., bounds.x),
            (split, bounds.x + bounds.width - half_width),
        ]
        .into_iter()
        .map(|(source_x, x)| Primitive::Image {
            handle: (self.handle, Rotation::None),
            bounds: Rectangle {
                x,
                y: bounds.y,
                width: half_width,
                height: height as f32 * scale,
            },
            color: Rgba::broadcast(255),
            source_rect: Some(Aabr {
                min: Vec2::new(source_x, 0.),
                max: Vec2::new(source_x + split, height as f32),
            }),
        })
        .collect();
        (
            Primitive::Group { primitives },
            mouse::Interaction::default(),
        )
    }
}
impl<'a, M: 'a> From<Ribbon> for Element<'a, M> {
    fn from(ribbon: Ribbon) -> Self {
        Self::new(ribbon)
    }
}
