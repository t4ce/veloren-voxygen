//! User-driven character join while the Conrod scene overlay is out-gated.
use super::ui;
use crate::{
    GlobalState,
    client::Client,
    ui::ice::{Element, IcedUi},
    window::Event,
};
use iced::{Button, Column, Container, Length, Text, button};

#[derive(Clone)]
enum Message {
    Play(common::character::CharacterId),
    Spectate,
    Logout,
}

pub(super) struct Selector {
    ui: IcedUi,
    buttons: Vec<button::State>,
    spectate: button::State,
    logout: button::State,
    joining: bool,
}

impl Selector {
    pub fn new(state: &GlobalState) -> Self {
        Self {
            ui: IcedUi::new_native(state.window.physical_size(), state.window.scale_factor()),
            buttons: Vec::new(),
            spectate: button::State::new(),
            logout: button::State::new(),
            joining: false,
        }
    }

    pub fn enter(&mut self, state: &mut GlobalState) {
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
        let mut column = Column::new()
            .padding(40)
            .spacing(18)
            .push(Text::new("Character Selection").size(30))
            .push(Text::new("Near terrain outline bring-up").size(20));
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
                        .on_press(Message::Play(id)),
                    );
                }
            }
            if list.characters.is_empty() {
                column = column.push(Text::new("No existing characters. Character creation is out-gated for this bring-up.").size(20));
            }
            column = column.push(
                Button::new(&mut self.spectate, Text::new("Spectate").size(24))
                    .padding(8)
                    .on_press(Message::Spectate),
            );
        }
        column = column.push(
            Button::new(&mut self.logout, Text::new("Back to main menu").size(24))
                .padding(8)
                .on_press(Message::Logout),
        );
        let root: Element<'_, Message> = Container::new(column)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let (messages, plan) = match self.ui.maintain_native(root, size, &mut state.clipboard) {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(%error, "Native character selector failed");
                return Vec::new();
            }
        };
        if let Some(plan) = plan {
            if let Err(error) = state.window.present_menu(size, plan) {
                tracing::error!(%error, "Character selector presentation failed");
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
