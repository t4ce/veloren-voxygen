//! TRUEOS frame-broker transport; login and game simulation remain in App.
use super::{App, FRAME, edit_password};
use std::{io, time::Instant};
use trueos::{
    input,
    ui4_scene::{Damage, Error, Frame, KeyboardState, rgba},
    ui4_solara_text::FrameEscapeKeyAction,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) enum KeyCode {
    KeyW,
    KeyA,
    KeyS,
    KeyD,
    Space,
    ControlLeft,
    ShiftLeft,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    KeyF,
    KeyR,
    KeyX,
    KeyC,
    Escape,
    Enter,
    NumpadEnter,
    Backspace,
}

// Physical HID usages, independent of keyboard layout and text composition.
const GAME_KEYS: &[(u8, KeyCode)] = &[
    (0x1a, KeyCode::KeyW),
    (0x04, KeyCode::KeyA),
    (0x16, KeyCode::KeyS),
    (0x07, KeyCode::KeyD),
    (0x2c, KeyCode::Space),
    (0xe0, KeyCode::ControlLeft),
    (0xe1, KeyCode::ShiftLeft),
    (0x1e, KeyCode::Digit1),
    (0x1f, KeyCode::Digit2),
    (0x20, KeyCode::Digit3),
    (0x21, KeyCode::Digit4),
    (0x22, KeyCode::Digit5),
    (0x09, KeyCode::KeyF),
    (0x15, KeyCode::KeyR),
    (0x1b, KeyCode::KeyX),
    (0x06, KeyCode::KeyC),
];

fn ui_error(error: Error) -> io::Error {
    io::Error::other(format!("UI4 frame: {error:?}"))
}

fn held(state: &KeyboardState, usage: u8) -> bool {
    if usage >= 0xe0 {
        state.modifiers & (1 << (usage - 0xe0)) != 0
    } else {
        state.is_down(usage)
    }
}

pub(super) fn run(mut app: App) -> Result<(), Box<dyn std::error::Error>> {
    let mut frame = Frame::open(80, 80, 640, 160).map_err(ui_error)?;
    frame
        .set_escape_key_action(FrameEscapeKeyAction::DeliverToApplication)
        .map_err(ui_error)?;
    app.window = Some(frame);
    app.prompt("Type password and press Enter (input is hidden)");
    let (mut width, mut height) = (640, 160);
    let mut paint = true;
    loop {
        let started = Instant::now();
        let result = pump(&mut app, &mut width, &mut height, &mut paint);
        match result {
            Ok(()) | Err(Error::Busy) => {}
            // The broker revoked the frame after a user close or Blueprint stop.
            Err(Error::NotFound | Error::InvalidState) => break,
            Err(error) => return Err(ui_error(error).into()),
        }
        app.tick();
        if let Some(remaining) = FRAME.checked_sub(started.elapsed()) {
            std::thread::sleep(remaining);
        }
    }
    app.capture(false);
    Ok(())
}

fn pump(app: &mut App, width: &mut u32, height: &mut u32, paint: &mut bool) -> Result<(), Error> {
    if let Some(size) = app
        .window
        .as_mut()
        .expect("UI4 frame")
        .take_resize_event()?
    {
        app.window
            .as_mut()
            .expect("UI4 frame")
            .resize(size.width, size.height)?;
        *width = size.width;
        *height = size.height;
        *paint = true;
    }
    if *paint {
        let frame = app.window.as_mut().expect("UI4 frame");
        frame.begin(rgba(0, 0, 0, 255))?;
        frame.publish(Damage::full(*width, *height))?;
        *paint = false;
    }
    while let Some(event) = app
        .window
        .as_mut()
        .expect("UI4 frame")
        .take_keyboard_event()?
    {
        let login = app.client.is_none() && app.pending.is_none();
        if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT && login {
            if let Some(c) = char::from_u32(event.codepoint).filter(|c| !c.is_control()) {
                app.password.push(c);
            }
        } else if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
            && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0
        {
            let key = match event.key_code {
                input::KEYBOARD_KEY_ENTER => Some(KeyCode::Enter),
                input::KEYBOARD_KEY_BACKSPACE => Some(KeyCode::Backspace),
                input::KEYBOARD_KEY_ESCAPE => Some(KeyCode::Escape),
                _ => None,
            };
            if let Some(key) = key {
                if login {
                    if edit_password(&mut app.password, key, None, false) {
                        app.login();
                    }
                } else if app.client.is_some() {
                    app.key(key, true, false);
                }
            }
        }
    }
    if app.world_joined {
        let state = app.window.as_ref().expect("UI4 frame").keyboard_state()?;
        if let Some(state) = state {
            if app.captured {
                for &(usage, key) in GAME_KEYS {
                    let pressed = held(&state, usage);
                    if pressed != app.input.keys.contains(&key) {
                        app.key(key, pressed, false);
                    }
                }
            }
        } else if app.captured {
            app.capture(false);
        }
    }
    while let Some(event) = app
        .window
        .as_mut()
        .expect("UI4 frame")
        .take_pointer_event()?
    {
        if !app.world_joined {
            continue;
        }
        if !app.captured && event.buttons_pressed != 0 {
            app.capture(true);
            continue;
        }
        if !app.captured {
            continue;
        }
        app.input.mouse(f64::from(event.dx), f64::from(event.dy));
        for (button, action) in [
            (
                trueos::ui4_scene::POINTER_BUTTON_PRIMARY,
                common::comp::InputKind::Primary,
            ),
            (
                trueos::ui4_scene::POINTER_BUTTON_SECONDARY,
                common::comp::InputKind::Secondary,
            ),
            (
                trueos::ui4_scene::POINTER_BUTTON_MIDDLE,
                common::comp::InputKind::Block,
            ),
        ] {
            if event.buttons_pressed & button != 0 {
                app.action(action, true);
            }
            if event.buttons_released & button != 0 {
                app.action(action, false);
            }
        }
    }
    Ok(())
}
