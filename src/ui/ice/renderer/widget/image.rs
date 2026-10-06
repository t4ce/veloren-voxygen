use super::super::{
    super::{Rotation, widget::image},
    IcedRenderer, Primitive,
};
use iced::mouse;
use vek::Rgba;

impl image::Renderer for IcedRenderer {
    fn dimensions(&self, handle: image::Handle) -> (u32, u32) {
        self.image_dims(handle)
    }

    fn draw(
        &mut self,
        handle: image::Handle,
        color: Rgba<u8>,
        layout: iced::Layout<'_>,
        visible_height: f32,
    ) -> Self::Output {
        let image = Primitive::Image {
            handle: (handle, Rotation::None),
            bounds: layout.bounds(),
            color,
            source_rect: None,
        };
        let primitive = if visible_height < 1.0 {
            Primitive::Clip {
                bounds: iced::Rectangle {
                    height: layout.bounds().height * visible_height,
                    ..layout.bounds()
                },
                offset: vek::Vec2::new(0, 0),
                content: Box::new(image),
            }
        } else {
            image
        };
        (primitive, mouse::Interaction::default())
    }
}
