use super::{Imgs, Message};
use crate::ui::ice::{
    Element,
    widget::{
        BackgroundContainer, Padding,
        compound_graphic::{CompoundGraphic, Graphic},
    },
};
use iced::Length;
use vek::Rgba;

pub(super) const ROW_HEIGHT: u32 = 56;
pub(super) const ROW_TEXT_SIZE: u16 = 25;
pub(super) const PANEL_BLACK: Rgba<u8> = Rgba::new(0, 0, 0, 230);

pub(super) fn panel<'a>(imgs: &Imgs, content: Element<'a, Message>) -> Element<'a, Message> {
    BackgroundContainer::new(
        CompoundGraphic::from_graphics(vec![
            Graphic::image(imgs.banner_top, [138, 17], [0, 0]),
            Graphic::rect(PANEL_BLACK, [130, 165], [4, 17]),
            Graphic::gradient(PANEL_BLACK, Rgba::zero(), [130, 50], [4, 182]),
        ])
        .fix_aspect_ratio()
        .height(Length::Fill),
        content,
    )
    .padding(Padding::new().horizontal(5).top(15).bottom(50))
    .max_width(350)
    .into()
}
