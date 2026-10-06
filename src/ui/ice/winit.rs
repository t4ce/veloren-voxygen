// Copied and adapted from `iced_winit` (MIT licensed)
// Original version at https://github.com/Imberflur/iced/tree/veloren-winit-0.28/winit

use iced::{Event, Point, keyboard, mouse, touch, window};
use std::{collections::HashMap, sync::Arc};
use winit::{event::WindowEvent, keyboard::NamedKey};

pub use crate::clipboard::Clipboard;
impl Clipboard {
    pub fn connect(_window: &dyn winit::window::Window) -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy)]
enum FileRequestKind {
    Hover,
    Drop,
}

enum PendingFileTransfer {
    Requested(winit::data_transfer::DataTransferId, FileRequestKind),
    Deferred(
        winit::data_transfer::DataTransferId,
        FileRequestKind,
        Arc<dyn winit::data_transfer::TypedData>,
    ),
}

/// Bridges winit 0.31's asynchronous URI data transfer events to iced's file events.
#[derive(Default)]
pub struct FileDropAdapter {
    hover: Option<winit::data_transfer::DataTransferId>,
    pending: HashMap<winit::event_loop::AsyncRequestSerial, PendingFileTransfer>,
}

impl FileDropAdapter {
    /// Processes drag-and-drop events and returns any corresponding iced file events.
    pub fn handle(
        &mut self,
        event: &WindowEvent,
        event_loop: &dyn winit::event_loop::ActiveEventLoop,
    ) -> Vec<Event> {
        use winit::{
            data_transfer::TypeHint, event::WindowEvent as WinitEvent, event_loop::DndAction,
        };

        match event {
            WinitEvent::DragEntered { id, .. } => {
                let left = self.hover.is_some_and(|hover| hover != *id);
                self.cancel_hover_requests();
                self.hover = Some(*id);
                self.request_uris(*id, FileRequestKind::Hover, event_loop, &TypeHint::UriList);
                if left {
                    vec![Event::Window(window::Event::FilesHoveredLeft)]
                } else {
                    Vec::new()
                }
            }
            WinitEvent::DragDropped { id, .. } => {
                self.cancel_hover_for(*id);
                if self.hover == Some(*id) {
                    self.hover = None;
                }
                let events = vec![Event::Window(window::Event::FilesHoveredLeft)];
                if let Ok(transfer) = event_loop.data_transfer(*id)
                    && transfer.has_type(&TypeHint::UriList)
                {
                    let _ = event_loop.set_valid_dnd_actions(*id, &[DndAction::Copy]);
                    if let Ok(serial) = event_loop.fetch_data_transfer(*id, &TypeHint::UriList) {
                        self.pending.insert(
                            serial,
                            PendingFileTransfer::Requested(*id, FileRequestKind::Drop),
                        );
                    }
                }
                events
            }
            WinitEvent::DragLeft { id } => {
                if self.hover == Some(*id) {
                    self.hover = None;
                    self.cancel_hover_for(*id);
                    vec![Event::Window(window::Event::FilesHoveredLeft)]
                } else {
                    Vec::new()
                }
            }
            WinitEvent::DataTransferReceived { id, serial, value } => {
                self.receive(*id, *serial, Arc::clone(value))
            }
            _ => Vec::new(),
        }
    }

    /// Retries data reads deferred because the platform had not finished streaming them.
    pub fn poll(&mut self) -> Vec<Event> {
        let deferred = self
            .pending
            .iter()
            .filter_map(|(serial, request)| match request {
                PendingFileTransfer::Deferred(id, kind, value) => {
                    Some((*serial, *id, *kind, Arc::clone(value)))
                }
                PendingFileTransfer::Requested(..) => None,
            })
            .collect::<Vec<_>>();

        let mut events = Vec::new();
        for (serial, id, kind, value) in deferred {
            events.extend(self.try_complete(serial, id, kind, value));
        }
        events
    }

    fn receive(
        &mut self,
        id: winit::data_transfer::DataTransferId,
        serial: winit::event_loop::AsyncRequestSerial,
        value: Arc<dyn winit::data_transfer::TypedData>,
    ) -> Vec<Event> {
        let Some(request) = self.pending.get(&serial) else {
            return Vec::new();
        };
        let (pending_id, kind) = match request {
            PendingFileTransfer::Requested(pending_id, kind)
            | PendingFileTransfer::Deferred(pending_id, kind, _) => (*pending_id, *kind),
        };
        if pending_id != id || (matches!(kind, FileRequestKind::Hover) && self.hover != Some(id)) {
            self.pending.remove(&serial);
            return Vec::new();
        }
        self.try_complete(serial, id, kind, value)
    }

    fn try_complete(
        &mut self,
        serial: winit::event_loop::AsyncRequestSerial,
        id: winit::data_transfer::DataTransferId,
        kind: FileRequestKind,
        value: Arc<dyn winit::data_transfer::TypedData>,
    ) -> Vec<Event> {
        if matches!(kind, FileRequestKind::Hover) && self.hover != Some(id) {
            self.pending.remove(&serial);
            return Vec::new();
        }
        match value.try_as_file_paths() {
            Ok(paths) => {
                self.pending.remove(&serial);
                paths
                    .into_iter()
                    .map(|path| {
                        Event::Window(match kind {
                            FileRequestKind::Hover => window::Event::FileHovered(path),
                            FileRequestKind::Drop => window::Event::FileDropped(path),
                        })
                    })
                    .collect()
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                self.pending
                    .insert(serial, PendingFileTransfer::Deferred(id, kind, value));
                Vec::new()
            }
            Err(_) => {
                self.pending.remove(&serial);
                Vec::new()
            }
        }
    }

    fn cancel_hover_for(&mut self, id: winit::data_transfer::DataTransferId) {
        self.pending.retain(|_, request| match request {
            PendingFileTransfer::Requested(pending_id, FileRequestKind::Hover)
            | PendingFileTransfer::Deferred(pending_id, FileRequestKind::Hover, _) => {
                *pending_id != id
            }
            _ => true,
        });
    }

    fn cancel_hover_requests(&mut self) {
        self.pending.retain(|_, request| match request {
            PendingFileTransfer::Requested(_, FileRequestKind::Hover)
            | PendingFileTransfer::Deferred(_, FileRequestKind::Hover, _) => false,
            _ => true,
        });
    }

    fn request_uris(
        &mut self,
        id: winit::data_transfer::DataTransferId,
        kind: FileRequestKind,
        event_loop: &dyn winit::event_loop::ActiveEventLoop,
        type_hint: &dyn winit::data_transfer::TransferType,
    ) {
        if self.pending.values().any(|request| {
            matches!(request, PendingFileTransfer::Requested(pending_id, FileRequestKind::Hover) | PendingFileTransfer::Deferred(pending_id, FileRequestKind::Hover, _) if *pending_id == id)
        }) {
            return;
        }
        if let Ok(transfer) = event_loop.data_transfer(id)
            && transfer.has_type(type_hint)
        {
            let _ = event_loop.set_valid_dnd_actions(id, &[winit::event_loop::DndAction::Copy]);
            if let Ok(serial) = event_loop.fetch_data_transfer(id, type_hint) {
                self.pending
                    .insert(serial, PendingFileTransfer::Requested(id, kind));
            }
        }
    }
}

#[cfg(test)]
mod file_drop_tests {
    use super::*;
    use std::{
        io::{self, BufRead, ErrorKind},
        sync::atomic::{AtomicUsize, Ordering},
    };

    static URI_LIST: winit::data_transfer::TypeHint = winit::data_transfer::TypeHint::UriList;

    #[derive(Debug)]
    struct UriData {
        reads: AtomicUsize,
    }

    impl winit::data_transfer::TypedData for UriData {
        fn type_(&self) -> &dyn winit::data_transfer::TransferType {
            &URI_LIST
        }

        fn try_read(&self) -> Option<Box<dyn BufRead>> {
            None
        }

        fn try_as_uris(&self) -> io::Result<Vec<String>> {
            if self.reads.fetch_add(1, Ordering::Relaxed) == 0 {
                Err(io::Error::new(ErrorKind::WouldBlock, "still streaming"))
            } else {
                Ok(vec!["file:///tmp/voxy-test.txt".to_owned()])
            }
        }

        fn try_as_string(&self) -> io::Result<String> {
            Err(io::Error::new(ErrorKind::InvalidData, "not plain text"))
        }
    }

    fn pending_drop(
        adapter: &mut FileDropAdapter,
    ) -> (
        winit::data_transfer::DataTransferId,
        winit::event_loop::AsyncRequestSerial,
        Arc<UriData>,
    ) {
        let id = winit::data_transfer::DataTransferId::from_raw(7);
        let serial = winit::event_loop::AsyncRequestSerial::get();
        let value = Arc::new(UriData {
            reads: AtomicUsize::new(0),
        });
        adapter.pending.insert(
            serial,
            PendingFileTransfer::Requested(id, FileRequestKind::Drop),
        );
        (id, serial, value)
    }

    #[test]
    fn retries_would_block_data_and_emits_drop_once() {
        let mut adapter = FileDropAdapter::default();
        let (id, serial, value) = pending_drop(&mut adapter);

        assert!(adapter.receive(id, serial, value.clone()).is_empty());
        let events = adapter.poll();
        assert!(matches!(
            events.as_slice(),
            [Event::Window(window::Event::FileDropped(path))]
                if path == &std::path::PathBuf::from("/tmp/voxy-test.txt")
        ));
        assert!(adapter.receive(id, serial, value).is_empty());
        assert!(adapter.poll().is_empty());
    }

    #[test]
    fn stale_hover_result_is_discarded_after_drag_left() {
        let mut adapter = FileDropAdapter::default();
        let (id, serial, value) = pending_drop(&mut adapter);
        adapter.pending.insert(
            serial,
            PendingFileTransfer::Requested(id, FileRequestKind::Hover),
        );
        adapter.hover = Some(id);
        adapter.cancel_hover_for(id);
        adapter.hover = None;

        assert!(adapter.receive(id, serial, value).is_empty());
        assert!(adapter.pending.is_empty());
    }
}

impl iced::Clipboard for Clipboard {
    fn read(&self) -> Option<String> {
        self.paste(crate::clipboard::Kind::Text)
    }
    fn write(&mut self, contents: String) {
        self.copy(crate::clipboard::Kind::Text, contents);
    }
}

/// Converts a winit window event into an iced event.
pub fn window_event(
    event: &WindowEvent,
    scale_factor: f64,
    modifiers: winit::keyboard::ModifiersState,
) -> Option<Event> {
    match event {
        WindowEvent::SurfaceResized(new_size) => {
            let logical_size = new_size.to_logical(scale_factor);

            Some(Event::Window(window::Event::Resized {
                width: logical_size.width,
                height: logical_size.height,
            }))
        }
        WindowEvent::CloseRequested => Some(Event::Window(window::Event::CloseRequested)),
        WindowEvent::PointerMoved {
            position,
            source: winit::event::PointerSource::Touch { finger_id, .. },
            ..
        } => {
            let position = position.to_logical::<f64>(scale_factor);
            Some(Event::Touch(touch::Event::FingerMoved {
                id: touch::Finger(finger_id.into_raw() as u64),
                position: Point::new(position.x as f32, position.y as f32),
            }))
        }
        WindowEvent::PointerMoved { position, .. } => {
            let position = position.to_logical::<f64>(scale_factor);

            Some(Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(position.x as f32, position.y as f32),
            }))
        }
        WindowEvent::PointerEntered {
            kind: winit::event::PointerKind::Touch(_),
            ..
        } => None,
        WindowEvent::PointerEntered {
            kind: winit::event::PointerKind::Mouse,
            ..
        } => Some(Event::Mouse(mouse::Event::CursorEntered)),
        WindowEvent::PointerEntered { .. } => None,
        // A normal touch release is followed by PointerLeft too; translating both would
        // produce duplicate lift events. Touch cancellation is not distinguishable here.
        WindowEvent::PointerLeft {
            kind: winit::event::PointerKind::Touch(_),
            ..
        } => None,
        WindowEvent::PointerLeft {
            kind: winit::event::PointerKind::Mouse,
            ..
        } => Some(Event::Mouse(mouse::Event::CursorLeft)),
        WindowEvent::PointerLeft { .. } => None,
        WindowEvent::PointerButton {
            button: winit::event::ButtonSource::Touch { finger_id, .. },
            state,
            position,
            ..
        } => {
            let position = position.to_logical::<f64>(scale_factor);
            let id = touch::Finger(finger_id.into_raw() as u64);
            let position = Point::new(position.x as f32, position.y as f32);
            match state {
                winit::event::ElementState::Pressed => {
                    Some(Event::Touch(touch::Event::FingerPressed { id, position }))
                }
                winit::event::ElementState::Released => {
                    Some(Event::Touch(touch::Event::FingerLifted { id, position }))
                }
            }
        }
        WindowEvent::PointerButton {
            button: winit::event::ButtonSource::Mouse(button),
            state,
            ..
        } => {
            let button = mouse_button(*button)?;

            Some(Event::Mouse(match state {
                winit::event::ElementState::Pressed => mouse::Event::ButtonPressed(button),
                winit::event::ElementState::Released => mouse::Event::ButtonReleased(button),
            }))
        }
        WindowEvent::MouseWheel { delta, .. } => match delta {
            winit::event::MouseScrollDelta::LineDelta(delta_x, delta_y) => {
                Some(Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines {
                        x: *delta_x,
                        y: *delta_y,
                    },
                }))
            }
            winit::event::MouseScrollDelta::PixelDelta(position) => {
                Some(Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Pixels {
                        x: position.x as f32,
                        y: position.y as f32,
                    },
                }))
            }
            _ => None,
        },
        WindowEvent::KeyboardInput { event, .. } => Some(Event::Keyboard({
            let modifiers = self::modifiers(modifiers);

            // `iced` expects different events for text input and pressed keys.
            // We work around that by sending the key as text but only if no modifier is
            // pressed, so shortcuts still work.
            if let Some(text) = &event.text
                && let Some(c) = text.chars().next()
                && !c.is_control()
                && !modifiers.alt
                && !modifiers.control
                && !modifiers.logo
            {
                return event
                    .state
                    .is_pressed()
                    .then_some(Event::Keyboard(keyboard::Event::CharacterReceived(c)));
            }

            let key_code = key_code(&event.logical_key)?;
            match event.state {
                winit::event::ElementState::Pressed => keyboard::Event::KeyPressed {
                    key_code,
                    modifiers,
                },
                winit::event::ElementState::Released => keyboard::Event::KeyReleased {
                    key_code,
                    modifiers,
                },
            }
        })),
        WindowEvent::ModifiersChanged(new_modifiers) => Some(Event::Keyboard(
            keyboard::Event::ModifiersChanged(self::modifiers(new_modifiers.state())),
        )),
        WindowEvent::Focused(focused) => Some(Event::Window(if *focused {
            window::Event::Focused
        } else {
            window::Event::Unfocused
        })),
        // Winit 0.31 exposes drag data through DataTransferId and asynchronous fetch requests;
        // it no longer sends paths in the drag-enter/drop window event itself.
        _ => None,
    }
}

/// Converts a `MouseButton` from [`winit`] to an [`iced`] mouse button.
pub fn mouse_button(mouse_button: winit::event::MouseButton) -> Option<mouse::Button> {
    Some(match mouse_button {
        winit::event::MouseButton::Left => mouse::Button::Left,
        winit::event::MouseButton::Right => mouse::Button::Right,
        winit::event::MouseButton::Middle => mouse::Button::Middle,
        button => mouse::Button::Other(button as u8),
    })
}

/// Converts some `ModifiersState` from [`winit`] to an [`iced`]
/// modifiers state.
pub fn modifiers(modifiers: winit::keyboard::ModifiersState) -> keyboard::Modifiers {
    keyboard::Modifiers {
        shift: modifiers.shift_key(),
        control: modifiers.control_key(),
        alt: modifiers.alt_key(),
        logo: modifiers.meta_key(),
    }
}

/// Converts a `VirtualKeyCode` from [`winit`] to an [`iced`] key code.
pub fn key_code(key: &winit::keyboard::Key) -> Option<keyboard::KeyCode> {
    use keyboard::KeyCode;

    Some(match key {
        winit::keyboard::Key::Named(key) => match key {
            NamedKey::Escape => KeyCode::Escape,
            NamedKey::F1 => KeyCode::F1,
            NamedKey::F2 => KeyCode::F2,
            NamedKey::F3 => KeyCode::F3,
            NamedKey::F4 => KeyCode::F4,
            NamedKey::F5 => KeyCode::F5,
            NamedKey::F6 => KeyCode::F6,
            NamedKey::F7 => KeyCode::F7,
            NamedKey::F8 => KeyCode::F8,
            NamedKey::F9 => KeyCode::F9,
            NamedKey::F10 => KeyCode::F10,
            NamedKey::F11 => KeyCode::F11,
            NamedKey::F12 => KeyCode::F12,
            NamedKey::F13 => KeyCode::F13,
            NamedKey::F14 => KeyCode::F14,
            NamedKey::F15 => KeyCode::F15,
            NamedKey::F16 => KeyCode::F16,
            NamedKey::F17 => KeyCode::F17,
            NamedKey::F18 => KeyCode::F18,
            NamedKey::F19 => KeyCode::F19,
            NamedKey::F20 => KeyCode::F20,
            NamedKey::F21 => KeyCode::F21,
            NamedKey::F22 => KeyCode::F22,
            NamedKey::F23 => KeyCode::F23,
            NamedKey::F24 => KeyCode::F24,
            NamedKey::ScrollLock => KeyCode::Scroll,
            NamedKey::Pause => KeyCode::Pause,
            NamedKey::Insert => KeyCode::Insert,
            NamedKey::Home => KeyCode::Home,
            NamedKey::Delete => KeyCode::Delete,
            NamedKey::End => KeyCode::End,
            NamedKey::PageDown => KeyCode::PageDown,
            NamedKey::PageUp => KeyCode::PageUp,
            NamedKey::ArrowLeft => KeyCode::Left,
            NamedKey::ArrowUp => KeyCode::Up,
            NamedKey::ArrowRight => KeyCode::Right,
            NamedKey::ArrowDown => KeyCode::Down,
            NamedKey::Backspace => KeyCode::Backspace,
            NamedKey::Enter => KeyCode::Enter,
            NamedKey::Compose => KeyCode::Compose,
            NamedKey::NumLock => KeyCode::Numlock,
            NamedKey::Convert => KeyCode::Convert,
            NamedKey::KanaMode => KeyCode::Kana,
            NamedKey::KanjiMode => KeyCode::Kanji,
            NamedKey::MediaStop => KeyCode::MediaStop,
            NamedKey::AudioVolumeMute => KeyCode::Mute,
            NamedKey::MediaTrackNext => KeyCode::NextTrack,
            NamedKey::NonConvert => KeyCode::NoConvert,
            NamedKey::MediaPlayPause => KeyCode::PlayPause,
            NamedKey::Power => KeyCode::Power,
            NamedKey::MediaTrackPrevious => KeyCode::PrevTrack,
            NamedKey::Tab => KeyCode::Tab,
            NamedKey::AudioVolumeDown => KeyCode::VolumeDown,
            NamedKey::AudioVolumeUp => KeyCode::VolumeUp,
            NamedKey::WakeUp => KeyCode::Wake,
            NamedKey::Copy => KeyCode::Copy,
            NamedKey::Paste => KeyCode::Paste,
            NamedKey::Cut => KeyCode::Cut,
            _ => return None,
        },
        winit::keyboard::Key::Character(c) => match c.as_str() {
            "a" | "A" => KeyCode::A,
            "b" | "B" => KeyCode::B,
            "c" | "C" => KeyCode::C,
            "d" | "D" => KeyCode::D,
            "e" | "E" => KeyCode::E,
            "f" | "F" => KeyCode::F,
            "g" | "G" => KeyCode::G,
            "h" | "H" => KeyCode::H,
            "i" | "I" => KeyCode::I,
            "j" | "J" => KeyCode::J,
            "k" | "K" => KeyCode::K,
            "l" | "L" => KeyCode::L,
            "m" | "M" => KeyCode::M,
            "n" | "N" => KeyCode::N,
            "o" | "O" => KeyCode::O,
            "p" | "P" => KeyCode::P,
            "q" | "Q" => KeyCode::Q,
            "r" | "R" => KeyCode::R,
            "s" | "S" => KeyCode::S,
            "t" | "T" => KeyCode::T,
            "u" | "U" => KeyCode::U,
            "v" | "V" => KeyCode::V,
            "w" | "W" => KeyCode::W,
            "x" | "X" => KeyCode::X,
            "y" | "Y" => KeyCode::Y,
            "z" | "Z" => KeyCode::Z,
            "0" => KeyCode::Key0,
            "1" => KeyCode::Key1,
            "2" => KeyCode::Key2,
            "3" => KeyCode::Key3,
            "4" => KeyCode::Key4,
            "5" => KeyCode::Key5,
            "6" => KeyCode::Key6,
            "7" => KeyCode::Key7,
            "8" => KeyCode::Key8,
            "9" => KeyCode::Key9,
            "'" => KeyCode::Apostrophe,
            "*" => KeyCode::Asterisk,
            "\\" => KeyCode::Backslash,
            "^" => KeyCode::Caret,
            ":" => KeyCode::Colon,
            "," => KeyCode::Comma,
            "=" => KeyCode::Equals,
            "-" => KeyCode::Minus,
            "." => KeyCode::Period,
            "+" => KeyCode::Plus,
            ";" => KeyCode::Semicolon,
            "/" => KeyCode::Slash,
            " " => KeyCode::Space,
            "_" => KeyCode::Underline,
            _ => return None,
        },
        winit::keyboard::Key::Unidentified(_) | winit::keyboard::Key::Dead(_) => return None,
    })
}
