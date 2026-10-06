use super::{TEXT_COLOR, img_ids::Imgs, settings_window::SettingsTab};
use crate::ui::fonts::Fonts;
use conrod_core::{
    Color, Colorable, Labelable, Positionable, Sizeable, Widget, WidgetCommon,
    widget::{self, Button, Image},
    widget_ids,
};
use i18n::Localization;

widget_ids! {
    struct Ids {
        esc_bg,
        banner_top,
        menu_button_1,
        menu_button_2,
        menu_button_3,
        menu_button_4,
        menu_button_5,
        menu_button_6,
        quit_prompt,
        quit_confirm,
        quit_back,
    }
}

#[derive(WidgetCommon)]
pub struct EscMenu<'a> {
    imgs: &'a Imgs,
    fonts: &'a Fonts,
    localized_strings: &'a Localization,
    confirm_quit: bool,

    #[conrod(common_builder)]
    common: widget::CommonBuilder,
}

impl<'a> EscMenu<'a> {
    pub fn new(imgs: &'a Imgs, fonts: &'a Fonts, localized_strings: &'a Localization) -> Self {
        Self {
            imgs,
            fonts,
            localized_strings,
            confirm_quit: false,
            common: widget::CommonBuilder::default(),
        }
    }

    pub fn confirm_quit(mut self, confirm_quit: bool) -> Self {
        self.confirm_quit = confirm_quit;
        self
    }
}

pub struct State {
    ids: Ids,
}

pub enum Event {
    OpenSettings(SettingsTab),
    CharacterSelection,
    Logout,
    Quit,
    RequestQuit,
    BackFromQuit,
    Close,
}

impl Widget for EscMenu<'_> {
    type Event = Option<Event>;
    type State = State;
    type Style = ();

    fn init_state(&self, id_gen: widget::id::Generator) -> Self::State {
        State {
            ids: Ids::new(id_gen),
        }
    }

    fn style(&self) -> Self::Style {}

    fn update(self, args: widget::UpdateArgs<Self>) -> Self::Event {
        common_base::prof_span!("EscMenu::update");
        let widget::UpdateArgs { state, ui, .. } = args;

        Image::new(self.imgs.esc_frame)
            .w_h(240.0, if self.confirm_quit { 220.0 } else { 380.0 })
            .color(Some(Color::Rgba(1.0, 1.0, 1.0, 0.9)))
            .middle_of(ui.window)
            .set(state.ids.esc_bg, ui);

        Image::new(self.imgs.banner_top)
            .w_h(250.0, 34.0)
            .mid_top_with_margin_on(state.ids.esc_bg, -34.0)
            .set(state.ids.banner_top, ui);

        if self.confirm_quit {
            widget::Text::new(&format!(
                "{}?",
                self.localized_strings.get_msg("esc_menu-quit_game")
            ))
            .mid_top_with_margin_on(state.ids.esc_bg, 20.0)
            .w_h(210.0, 60.0)
            .center_justify()
            .color(TEXT_COLOR)
            .font_size(self.fonts.cyri.scale(20))
            .font_id(self.fonts.cyri.conrod_id)
            .set(state.ids.quit_prompt, ui);

            if Button::image(self.imgs.button)
                .mid_top_with_margin_on(state.ids.esc_bg, 95.0)
                .w_h(210.0, 50.0)
                .hover_image(self.imgs.button_hover)
                .press_image(self.imgs.button_press)
                .label(&self.localized_strings.get_msg("common-quit"))
                .label_y(conrod_core::position::Relative::Scalar(3.0))
                .label_color(TEXT_COLOR)
                .label_font_size(self.fonts.cyri.scale(20))
                .label_font_id(self.fonts.cyri.conrod_id)
                .set(state.ids.quit_confirm, ui)
                .was_clicked()
            {
                return Some(Event::Quit);
            }
            if Button::image(self.imgs.button)
                .down_from(state.ids.quit_confirm, 10.0)
                .w_h(210.0, 50.0)
                .hover_image(self.imgs.button_hover)
                .press_image(self.imgs.button_press)
                .label(&self.localized_strings.get_msg("common-back"))
                .label_y(conrod_core::position::Relative::Scalar(3.0))
                .label_color(TEXT_COLOR)
                .label_font_size(self.fonts.cyri.scale(20))
                .label_font_id(self.fonts.cyri.conrod_id)
                .set(state.ids.quit_back, ui)
                .was_clicked()
            {
                return Some(Event::BackFromQuit);
            }
            return None;
        }

        // Resume
        if Button::image(self.imgs.button)
            .mid_bottom_with_margin_on(state.ids.banner_top, -60.0)
            .w_h(210.0, 50.0)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .label(&self.localized_strings.get_msg("common-resume"))
            .label_y(conrod_core::position::Relative::Scalar(3.0))
            .label_color(TEXT_COLOR)
            .label_font_size(self.fonts.cyri.scale(20))
            .label_font_id(self.fonts.cyri.conrod_id)
            .set(state.ids.menu_button_1, ui)
            .was_clicked()
        {
            return Some(Event::Close);
        };

        // Settings
        if Button::image(self.imgs.button)
            .mid_bottom_with_margin_on(state.ids.menu_button_1, -65.0)
            .w_h(210.0, 50.0)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .label(&self.localized_strings.get_msg("common-settings"))
            .label_y(conrod_core::position::Relative::Scalar(3.0))
            .label_color(TEXT_COLOR)
            .label_font_size(self.fonts.cyri.scale(20))
            .label_font_id(self.fonts.cyri.conrod_id)
            .set(state.ids.menu_button_2, ui)
            .was_clicked()
        {
            return Some(Event::OpenSettings(SettingsTab::Interface));
        };
        // Controls
        if Button::image(self.imgs.button)
            .mid_bottom_with_margin_on(state.ids.menu_button_2, -55.0)
            .w_h(210.0, 50.0)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .label(&self.localized_strings.get_msg("common-controls"))
            .label_y(conrod_core::position::Relative::Scalar(3.0))
            .label_color(TEXT_COLOR)
            .label_font_size(self.fonts.cyri.scale(20))
            .label_font_id(self.fonts.cyri.conrod_id)
            .set(state.ids.menu_button_3, ui)
            .was_clicked()
        {
            return Some(Event::OpenSettings(SettingsTab::Controls));
        };
        // Characters
        if Button::image(self.imgs.button)
            .mid_bottom_with_margin_on(state.ids.menu_button_3, -55.0)
            .w_h(210.0, 50.0)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .label(&self.localized_strings.get_msg("common-characters"))
            .label_y(conrod_core::position::Relative::Scalar(3.0))
            .label_color(TEXT_COLOR)
            .label_font_size(self.fonts.cyri.scale(20))
            .label_font_id(self.fonts.cyri.conrod_id)
            .set(state.ids.menu_button_4, ui)
            .was_clicked()
        {
            return Some(Event::CharacterSelection);
        };
        // Logout
        if Button::image(self.imgs.button)
            .mid_bottom_with_margin_on(state.ids.menu_button_4, -65.0)
            .w_h(210.0, 50.0)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .label(&self.localized_strings.get_msg("esc_menu-logout"))
            .label_y(conrod_core::position::Relative::Scalar(3.0))
            .label_color(TEXT_COLOR)
            .label_font_size(self.fonts.cyri.scale(20))
            .label_font_id(self.fonts.cyri.conrod_id)
            .set(state.ids.menu_button_5, ui)
            .was_clicked()
        {
            return Some(Event::Logout);
        };
        // Quit
        if Button::image(self.imgs.button)
            .mid_bottom_with_margin_on(state.ids.menu_button_5, -55.0)
            .w_h(210.0, 50.0)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .label(&self.localized_strings.get_msg("esc_menu-quit_game"))
            .label_y(conrod_core::position::Relative::Scalar(3.0))
            .label_color(TEXT_COLOR)
            .label_font_size(self.fonts.cyri.scale(20))
            .label_font_id(self.fonts.cyri.conrod_id)
            .set(state.ids.menu_button_6, ui)
            .was_clicked()
        {
            return Some(Event::RequestQuit);
        };
        None
    }
}
