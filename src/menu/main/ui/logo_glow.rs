use crate::ui::{
    Graphic as UiGraphic, GraphicId,
    ice::{
        IcedUi,
        widget::compound_graphic::{CompoundGraphic, Graphic},
    },
};
use common::assets::{AssetExt, Image};
use iced::Length;
use std::sync::Arc;

/// A static gradient texture, uploaded once and scaled together with the logo.
pub(super) struct LogoGlow {
    handle: GraphicId,
    logo_size: [u16; 2],
    glow_size: [u16; 2],
}

impl LogoGlow {
    pub(super) fn new(ui: &mut IcedUi) -> Self {
        let logo = Image::load_expect("voxygen.element.v_logo");
        let logo = logo.read();
        let logo_size = [logo.0.width() as u16, logo.0.height() as u16];
        let glow_size = logo_size.map(|size| (size as f32 * 1.05).ceil() as u16);
        let glow = image::RgbaImage::from_fn(glow_size[0] as u32, glow_size[1] as u32, |x, y| {
            let x = 2.0 * x as f32 / (glow_size[0] - 1) as f32 - 1.0;
            let y = 2.0 * y as f32 / (glow_size[1] - 1) as f32 - 1.0;
            // A rounded halo fading completely at its edges. The upper center
            // is slightly brighter, suggesting a soft light behind the logo.
            let fade = (1.0 - x.powi(6) - y.powi(6)).clamp(0.0, 1.0).powf(1.5);
            let light = 0.85 + 0.15 * (-3.0 * (x * x + (y + 0.3).powi(2))).exp();
            image::Rgba([255, 140, 45, (102.0 * fade * light).round() as u8])
        });
        let handle = ui.add_graphic(UiGraphic::Image(
            Arc::new(image::DynamicImage::ImageRgba8(glow)),
            None,
        ));
        Self {
            handle,
            logo_size,
            glow_size,
        }
    }

    pub(super) fn view(&self, logo: GraphicId, logo_width: u16) -> CompoundGraphic {
        let offset = [
            (self.glow_size[0] - self.logo_size[0]) / 2,
            (self.glow_size[1] - self.logo_size[1]) / 2,
        ];
        CompoundGraphic::from_graphics(vec![
            Graphic::image(self.handle, self.glow_size, [0, 0]),
            Graphic::image(logo, self.logo_size, offset),
        ])
        .fix_aspect_ratio()
        .width(Length::Units(
            (logo_width as f32 * self.glow_size[0] as f32 / self.logo_size[0] as f32).round()
                as u16,
        ))
    }
}
