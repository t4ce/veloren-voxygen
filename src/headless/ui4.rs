//! TRUEOS frame-broker transport; login and game simulation remain in App.
use super::{App, FRAME, edit_password, render_trueos};
use std::{io, time::Instant};
use trueos::{
    input,
    ui4_scene::{Error, Frame, KeyboardState, output_dimensions},
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

#[derive(Default)]
struct LoopTimings {
    loops: u64,
    display_retries: u64,
    // Whole loop, publication, input, simulation, draw, and sleep wall time.
    micros: [u128; 6],
}

impl LoopTimings {
    fn record(&mut self, micros: [u128; 6], display_retry: bool) {
        self.loops += 1;
        self.display_retries += u64::from(display_retry);
        for (total, sample) in self.micros.iter_mut().zip(micros) {
            *total += sample;
        }
        if self.loops == 128 {
            let averages = self.micros.map(|total| total / u128::from(self.loops));
            super::connection_progress(format_args!(
                "Voxygen loop timing: loops={} display_retries={} total_us={} publish_us={} input_us={} tick_us={} draw_us={} sleep_us={} sample=window-average",
                self.loops,
                self.display_retries,
                averages[0],
                averages[1],
                averages[2],
                averages[3],
                averages[4],
                averages[5],
            ));
            *self = Self::default();
        }
    }
}

pub(super) fn run(mut app: App) -> Result<(), Box<dyn std::error::Error>> {
    let (display_width, display_height) = output_dimensions().map_err(ui_error)?;
    let (x, y, mut width, mut height) = super::scene::placement(display_width, display_height);
    let mut frame = Frame::open_streaming(x, y, width, height).map_err(ui_error)?;
    frame.set_position(x, y).map_err(ui_error)?;
    let mut renderer = render_trueos::Renderer::new(width, height)?;
    frame
        .set_escape_key_action(FrameEscapeKeyAction::DeliverToApplication)
        .map_err(ui_error)?;
    app.window = Some(frame);
    app.password = super::file_password(
        trueos::async_fs::block_on(trueos::async_fs::read_file_utf8(b"/apps/voxy/voxy.pw"))
            .map_err(|code| {
                io::Error::other(format!("Voxygen password file read failed (code {code})"))
            })?,
    )?;
    super::connection_progress(format_args!(
        "Voxygen headless: automatic login from /apps/voxy/voxy.pw"
    ));
    app.login();
    let mut timings = LoopTimings::default();
    loop {
        let started = Instant::now();
        let mut micros = [0; 6];
        // Publication must retire before a resize or a fresh write lease.
        let publication = renderer.publish(
            app.window.as_mut().expect("UI4 frame"),
            width,
            height,
            app.world_joined,
        );
        micros[1] = started.elapsed().as_micros();
        match publication {
            Ok(()) => {}
            Err(render_trueos::Error::Ui(Error::Busy)) => {
                // Poll the exact display receipt without adding a whole frame
                // of latency; keep simulation at its normal tick cadence.
                let tick_started = Instant::now();
                if tick_started.duration_since(app.last_tick) >= FRAME {
                    app.tick();
                }
                micros[3] = tick_started.elapsed().as_micros();
                let sleep_started = Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(1));
                micros[5] = sleep_started.elapsed().as_micros();
                micros[0] = started.elapsed().as_micros();
                timings.record(micros, true);
                continue;
            }
            Err(render_trueos::Error::Ui(Error::NotFound | Error::InvalidState)) => break,
            Err(error) => return Err(error.into()),
        }
        app.terrain_presented = renderer.terrain_presented();
        let input_started = Instant::now();
        let result = pump(&mut app, &mut width, &mut height);
        micros[2] = input_started.elapsed().as_micros();
        match result {
            Ok(()) | Err(Error::Busy) => {}
            // The broker revoked the frame after a user close or Blueprint stop.
            Err(Error::NotFound | Error::InvalidState) => break,
            Err(error) => return Err(ui_error(error).into()),
        }
        let tick_started = Instant::now();
        app.tick();
        micros[3] = tick_started.elapsed().as_micros();
        let draw_started = Instant::now();
        let result = renderer.draw(
            app.window.as_mut().expect("UI4 frame"),
            width,
            height,
            app.client.as_ref(),
            app.input.yaw,
            app.input.pitch,
            app.world_joined,
        );
        micros[4] = draw_started.elapsed().as_micros();
        match result {
            Ok(()) | Err(render_trueos::Error::Ui(Error::Busy)) => {}
            Err(render_trueos::Error::Ui(Error::NotFound | Error::InvalidState)) => break,
            Err(error) => return Err(error.into()),
        }
        if let Some(remaining) = FRAME.checked_sub(started.elapsed()) {
            let sleep_started = Instant::now();
            std::thread::sleep(remaining);
            micros[5] = sleep_started.elapsed().as_micros();
        }
        micros[0] = started.elapsed().as_micros();
        timings.record(micros, false);
    }
    app.capture(false);
    Ok(())
}

fn pump(app: &mut App, width: &mut u32, height: &mut u32) -> Result<(), Error> {
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
