use super::{FILL_FRAC_ONE, Message};
use crate::ui::{
    fonts::IcedFonts as Fonts,
    ice::{
        Element,
        component::neat_button,
        style,
        widget::keyboard_button::{KeyboardButton, State as KeyboardState},
    },
};
use i18n::Localization;
use iced::{Align, Column, Container, Length, Row, Text, button};

#[derive(Default)]
pub(super) struct Screen {
    quit_button: button::State,
    back_button: button::State,
    quit_focus: KeyboardState,
    back_focus: KeyboardState,
}

impl Screen {
    pub(super) fn open(&mut self) {
        *self = Self::default();
        self.back_focus.focus(true);
    }

    pub(super) fn tab(&mut self) {
        let quit = !self.quit_focus.focused;
        self.quit_focus.focus(quit);
        self.back_focus.focus(!quit);
    }

    pub(super) fn clear_focus(&mut self) {
        self.quit_focus.focus(false);
        self.back_focus.focus(false);
    }

    pub(super) fn view(
        &mut self,
        fonts: &Fonts,
        i18n: &Localization,
        button_style: style::button::Style,
    ) -> Element<'_, Message> {
        let title = Text::new(format!("{}?", i18n.get_msg("esc_menu-quit_game")))
            .size(fonts.cyri.scale(28))
            .horizontal_alignment(iced::HorizontalAlignment::Center)
            .width(Length::Fill);
        let quit = KeyboardButton::new(
            &mut self.quit_focus,
            Message::ConfirmQuit,
            neat_button(
                &mut self.quit_button,
                i18n.get_msg("common-quit"),
                FILL_FRAC_ONE,
                button_style,
                Some(Message::ConfirmQuit),
            ),
        );
        let back = KeyboardButton::new(
            &mut self.back_focus,
            Message::BackFromQuit,
            neat_button(
                &mut self.back_button,
                i18n.get_msg("common-back"),
                FILL_FRAC_ONE,
                button_style,
                Some(Message::BackFromQuit),
            ),
        );
        Container::new(
            Column::with_children(vec![
                title.into(),
                Row::with_children(vec![
                    Container::new(quit).width(Length::Fill).into(),
                    Container::new(back).width(Length::Fill).into(),
                ])
                .spacing(16)
                .width(Length::Fill)
                .height(Length::Units(42))
                .align_items(Align::Center)
                .into(),
            ])
            .spacing(24)
            .align_items(Align::Center),
        )
        .style(
            style::container::Style::color_with_double_cornerless_border(
                (22, 18, 16, 255).into(),
                (11, 11, 11, 255).into(),
                (54, 46, 38, 255).into(),
            ),
        )
        .width(Length::Units(400))
        .padding(24)
        .into()
    }
}
