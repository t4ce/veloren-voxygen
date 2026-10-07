//! User-driven character join while the Conrod scene overlay is out-gated.
use super::ui;
use crate::{
    GlobalState,
    client::Client,
    ui::{
        ice::{Element, IcedUi, style},
        img_ids::ImageGraphic,
    },
    window::Event,
};
use iced::{Align, Button, Column, Container, Length, Text, button};

image_ids_ice! {
    struct Imgs {
        <ImageGraphic>
        background: "voxygen.background.bg_main",
        button: "voxygen.element.ui.generic.buttons.button",
        button_hover: "voxygen.element.ui.generic.buttons.button_hover",
        button_press: "voxygen.element.ui.generic.buttons.button_press",
    }
}

#[derive(Clone)]
enum Message {
    Play(common::character::CharacterId),
    Spectate,
    Logout,
}

pub(super) struct Selector {
    ui: IcedUi,
    imgs: Imgs,
    buttons: Vec<button::State>,
    spectate: button::State,
    logout: button::State,
    joining: bool,
    first_plan_queued: bool,
    failure_recorded: bool,
}

impl Selector {
    pub fn new(state: &GlobalState) -> Self {
        let mut ui = IcedUi::new_native(state.window.physical_size(), state.window.scale_factor());
        let imgs = Imgs::load(&mut ui).expect("Failed to load character selector images");
        ui.mark_scene_image(imgs.background);
        Self {
            ui,
            imgs,
            buttons: Vec::new(),
            spectate: button::State::new(),
            logout: button::State::new(),
            joining: false,
            first_plan_queued: false,
            failure_recorded: false,
        }
    }

    pub fn enter(&mut self, state: &mut GlobalState) {
        let _ = trueos::logl::log_record(
            trueos::logl::level::IMPORTANT, "apps::voxygen",
            format_args!("Voxygen character selection: entering native selector"),
        );
        self.first_plan_queued = false;
        self.failure_recorded = false;
        state.window.resume_menu();
        self.ui.invalidate_native();
        self.joining = false;
    }

    pub fn handle(&mut self, event: &Event) {
        if let Event::IcedUi(event) = event {
            self.ui.handle_event(event.clone());
        }
    }

    pub fn maintain(&mut self, state: &mut GlobalState, client: &Client) -> Vec<ui::Event> {
        let size = state.window.physical_size();
        if size.x == 0 || size.y == 0 {
            return Vec::new();
        }
        let list = client.character_list();
        self.buttons
            .resize_with(list.characters.len(), button::State::new);
        let button_style = style::button::Style::new(self.imgs.button)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .text_color(iced::Color::WHITE);
        let mut column = Column::new()
            .padding(24)
            .spacing(18)
            .align_items(Align::Center)
            .push(Text::new("Character Selection").size(30));
        if self.joining {
            column = column.push(Text::new("Joining world...").size(24));
        } else if list.loading {
            column = column.push(Text::new("Loading characters...").size(24));
        } else {
            for (button, item) in self.buttons.iter_mut().zip(&list.characters) {
                if let Some(id) = item.character.id {
                    column = column.push(
                        Button::new(
                            button,
                            Text::new(format!("Play: {}", item.character.alias)).size(24),
                        )
                        .padding(8)
                        .width(Length::Fill)
                        .style(button_style)
                        .on_press(Message::Play(id)),
                    );
                }
            }
            if list.characters.is_empty() {
                column = column.push(Text::new("No characters on this server.").size(20));
            }
            column = column.push(
                Button::new(&mut self.spectate, Text::new("Spectate").size(24))
                    .padding(8)
                    .width(Length::Fill)
                    .style(button_style)
                    .on_press(Message::Spectate),
            );
        }
        column = column.push(
            Button::new(&mut self.logout, Text::new("Back to main menu").size(24))
                .padding(8)
                .width(Length::Fill)
                .style(button_style)
                .on_press(Message::Logout),
        );
        let panel = Container::new(column)
            .width(Length::Fill)
            .max_width(520)
            .style(style::container::Style::color_with_double_cornerless_border(
                (22, 18, 16, 255).into(),
                (11, 11, 11, 255).into(),
                (54, 46, 38, 255).into(),
            ));
        let root: Element<'_, Message> = Container::new(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Align::Center)
            .align_y(Align::Center)
            .padding(40)
            .style(style::container::Style::image(self.imgs.background))
            .into();
        let (messages, plan) = match self.ui.maintain_native(root, size, &mut state.clipboard) {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(%error, "Native character selector failed");
                if !self.failure_recorded {
                    self.failure_recorded = true;
                    let _ = trueos::logl::log_record(
                        trueos::logl::level::ERROR, "apps::voxygen",
                        format_args!("Voxygen character selection: native layout failed: {error}"),
                    );
                }
                return Vec::new();
            }
        };
        if let Some(plan) = plan {
            match state.window.present_menu(size, plan) {
                Ok(()) => {
                    if !self.first_plan_queued {
                        self.first_plan_queued = true;
                        let _ = trueos::logl::log_record(
                            trueos::logl::level::IMPORTANT, "apps::voxygen",
                            format_args!("Voxygen character selection: first native plan queued extent={}x{} characters={} loading={}", size.x, size.y, list.characters.len(), list.loading),
                        );
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "Character selector presentation failed");
                    if !self.failure_recorded {
                        self.failure_recorded = true;
                        let _ = trueos::logl::log_record(
                            trueos::logl::level::ERROR, "apps::voxygen",
                            format_args!("Voxygen character selection: native presentation failed: {error}"),
                        );
                    }
                }
            }
        }
        messages
            .into_iter()
            .filter_map(|message| match message {
                Message::Play(id) if !self.joining => {
                    self.joining = true;
                    Some(ui::Event::Play(id))
                }
                Message::Spectate if !self.joining => {
                    self.joining = true;
                    Some(ui::Event::Spectate)
                }
                Message::Logout => Some(ui::Event::Logout),
                _ => None,
            })
            .collect()
    }

    pub fn failed(&mut self) {
        self.joining = false;
    }
}
