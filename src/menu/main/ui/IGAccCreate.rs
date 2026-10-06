//! In-game account creation using the same protocol as veloren.net/js/account.js.
use super::{FILL_FRAC_ONE, FILL_FRAC_TWO, Imgs, Message};
use crate::ui::{
    fonts::IcedFonts as Fonts,
    ice::{
        Element,
        component::neat_button,
        style,
        widget::{BackgroundContainer, Image, Padding},
    },
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use i18n::Localization;
use iced::{Align, Column, Container, Length, Space, TextInput, button, text_input};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
use tokio::runtime::Runtime;

#[derive(Default)]
pub(super) struct IGAccCreate {
    username: String,
    password: String,
    confirmation: String,
    inputs: [text_input::State; 3],
    buttons: [button::State; 5],
    response: Option<mpsc::Receiver<Result<(), String>>>,
}

impl IGAccCreate {
    pub(super) fn view(
        &mut self,
        fonts: &Fonts,
        imgs: &Imgs,
        i18n: &Localization,
        button_style: style::button::Style,
    ) -> Element<'_, Message> {
        let pending = self.response.is_some();
        let mut fields = Vec::new();
        for (idx, (state, value)) in self
            .inputs
            .iter_mut()
            .zip([&self.username, &self.password, &self.confirmation])
            .enumerate()
        {
            let placeholder = i18n.get_msg(match idx {
                0 => "main-username",
                1 => "main-password",
                _ => "main-account-confirm_password",
            });
            let mut input = TextInput::new(state, &placeholder, value, move |value| {
                Message::AccountField(idx, value)
            })
            .size(fonts.cyri.scale(20))
            .on_submit(if idx < 2 {
                Message::AccountFocus(idx + 1)
            } else {
                Message::CreateAccount
            });
            if idx != 0 {
                input = input.password();
            }
            fields.push(
                BackgroundContainer::new(
                    Image::new(imgs.input_bg)
                        .width(Length::Units(230))
                        .fix_aspect_ratio(),
                    input,
                )
                .padding(Padding::new().horizontal(7).vertical(5))
                .into(),
            );
        }
        let [create, password, username, delete, back] = &mut self.buttons;
        let create = Container::new(neat_button(
            create,
            i18n.get_msg(if pending {
                "main-account-creating"
            } else {
                "common-create"
            }),
            FILL_FRAC_TWO,
            button_style,
            (!pending).then_some(Message::CreateAccount),
        ))
        .width(Length::Units(170));
        let mut links = Vec::new();
        for (state, key, path) in [
            (password, "main-account-change_password", "change-password"),
            (username, "main-account-change_username", "change-username"),
            (delete, "main-account-delete", "delete-account"),
        ] {
            links.push(
                Container::new(neat_button(
                    state,
                    i18n.get_msg(key),
                    FILL_FRAC_ONE,
                    button_style,
                    Some(Message::AccountLink(path)),
                ))
                .width(Length::Units(100))
                .height(Length::Units(25))
                .into(),
            );
        }
        let back = Container::new(neat_button(
            back,
            i18n.get_msg("common-back"),
            FILL_FRAC_TWO,
            button_style,
            Some(Message::AccountBack),
        ))
        .width(Length::Units(170));
        Container::new(
            Column::with_children(vec![
                Column::with_children(fields).spacing(5).into(),
                Space::new(Length::Fill, Length::Units(8)).into(),
                create.into(),
                back.into(),
                Column::with_children(links)
                    .spacing(5)
                    .align_items(Align::Center)
                    .into(),
            ])
            .spacing(8)
            .align_items(Align::Center),
        )
        .height(Length::Fill)
        .center_y()
        .into()
    }

    pub(super) fn field(&mut self, idx: usize, value: String) {
        if self.response.is_some() {
            return;
        }
        match idx {
            0 => self.username = value,
            1 => self.password = value,
            2 => self.confirmation = value,
            _ => {}
        }
    }

    pub(super) fn focus(&mut self, idx: usize) {
        for (i, state) in self.inputs.iter_mut().enumerate() {
            *state = if i == idx {
                text_input::State::focused()
            } else {
                text_input::State::new()
            };
            if i == idx {
                state.move_cursor_to_end();
            }
        }
    }

    pub(super) fn tab(&mut self) {
        let next = self
            .inputs
            .iter()
            .position(|state| state.is_focused())
            .map_or(0, |i| (i + 1) % 3);
        self.focus(next);
    }

    pub(super) fn submit(&mut self, runtime: &Arc<Runtime>, i18n: &Localization) -> Option<String> {
        if self.response.is_some() {
            return None;
        }
        let username = self.username.trim().to_owned();
        if let Err(key) = validate(&username, &self.password, &self.confirmation) {
            return Some(i18n.get_msg(key).into_owned());
        }
        let password = self.password.clone();
        let (sender, receiver) = mpsc::channel();
        self.response = Some(receiver);
        runtime.spawn(async move {
            let result =
                tokio::time::timeout(Duration::from_secs(30), register(username, password))
                    .await
                    .unwrap_or_else(|_| Err("Request timed out.".to_owned()));
            let _ = sender.send(result);
        });
        None
    }

    pub(super) fn poll(&mut self) -> Option<Result<String, String>> {
        let result = match self.response.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Account request stopped unexpectedly.".to_owned())
            }
        };
        self.response = None;
        result
            .map(|()| {
                self.password.clear();
                self.confirmation.clear();
                self.username.trim().to_owned()
            })
            .into()
    }
}

fn validate(username: &str, password: &str, confirmation: &str) -> Result<(), &'static str> {
    if common::comp::Player::alias_validate(username).is_err()
        || !(3..=32).contains(&username.chars().count())
    {
        return Err("main-account-invalid_username");
    }
    if password.is_empty() {
        return Err("main-account-empty_password");
    }
    if password != confirmation {
        return Err("main-account-password_mismatch");
    }
    Ok(())
}

async fn register(username: String, password: String) -> Result<(), String> {
    // Match authc/argon2wasm's network prehash; never send the plaintext password.
    let password = tokio::task::spawn_blocking(move || {
        let salt = fxhash::hash64(&password);
        let config = argon2::Config {
            variant: argon2::Variant::Argon2i,
            time_cost: 3,
            mem_cost: 4096,
            ..Default::default()
        };
        argon2::hash_raw(password.as_bytes(), &salt.to_le_bytes(), &config)
            .map(hex::encode)
            .map_err(|_| "Password hashing failed.".to_owned())
    })
    .await
    .map_err(|_| "Password hashing failed.".to_owned())??;
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(|error| error.to_string())?
        .https_only()
        .enable_http1()
        .build();
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);
    let data = serde_json::json!({ "username": username, "password": password });
    let request = hyper::Request::post("https://auth.veloren.net/register")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(data.to_string())))
        .map_err(|error| error.to_string())?;
    let response = client
        .request(request)
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    // Bound error response size and display it as plain text in the existing dialog.
    let mut body = response.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| error.to_string())?;
        if let Some(data) = frame.data_ref() {
            bytes.extend_from_slice(&data[..data.len().min(1024 - bytes.len())]);
            if bytes.len() >= 1024 {
                break;
            }
        }
    }
    response_result(status, &String::from_utf8_lossy(&bytes))
}

fn response_result(status: u16, body: &str) -> Result<(), String> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("HTTP {status}: {}", body.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_validation() {
        assert!(validate("Player_1", "secret", "secret").is_ok());
        assert!(validate("ab", "secret", "secret").is_err());
        assert!(validate("invalid name", "secret", "secret").is_err());
        assert!(validate(&"a".repeat(33), "secret", "secret").is_err());
        assert!(validate("Player", "", "").is_err());
        assert!(validate("Player", "secret", "different").is_err());
    }
    #[test]
    fn pending_submission_keeps_the_submitted_username() {
        let mut account = IGAccCreate::default();
        account.field(0, "Original".to_owned());
        account.field(1, "secret".to_owned());
        account.field(2, "secret".to_owned());
        let (sender, receiver) = mpsc::channel();
        account.response = Some(receiver);
        account.field(0, "ChangedWhilePending".to_owned());
        assert!(account.poll().is_none());
        sender.send(Ok(())).unwrap();
        assert_eq!(account.poll().unwrap().unwrap(), "Original");
        assert!(account.password.is_empty());
        assert!(account.confirmation.is_empty());
        assert!(account.response.is_none());
    }

    #[test]
    fn failure_releases_pending_state_for_retry() {
        let mut account = IGAccCreate::default();
        let (sender, receiver) = mpsc::channel();
        account.response = Some(receiver);
        sender
            .send(Err("HTTP 409: User exists".to_owned()))
            .unwrap();
        assert_eq!(
            account.poll().unwrap().unwrap_err(),
            "HTTP 409: User exists"
        );
        assert!(account.response.is_none());
        account.field(0, "AnotherUsername".to_owned());
        assert_eq!(account.username, "AnotherUsername");
    }

    #[test]
    fn http_errors_are_never_success() {
        assert!(response_result(200, "").is_ok());
        assert!(response_result(204, "").is_ok());
        assert_eq!(
            response_result(409, "Username already exists").unwrap_err(),
            "HTTP 409: Username already exists"
        );
        assert!(response_result(429, "Too many requests").is_err());
        assert!(response_result(500, "Server error").is_err());
    }
}
