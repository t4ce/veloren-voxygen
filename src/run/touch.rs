use std::collections::BTreeMap;
use winit::{
    dpi::PhysicalPosition,
    event::{
        ButtonSource, DeviceId, ElementState, FingerId, PointerKind, PointerSource, WindowEvent,
    },
};

/// Tracks active contacts so PointerLeft can cancel an interrupted touch without
/// duplicating the normal release followed by PointerLeft sequence.
#[derive(Default)]
pub(super) struct TouchTracker {
    held: BTreeMap<(Option<DeviceId>, FingerId), PhysicalPosition<f64>>,
}

impl TouchTracker {
    pub(super) fn handle(&mut self, event: &WindowEvent) -> Vec<(FingerId, PhysicalPosition<f64>)> {
        match event {
            WindowEvent::PointerButton {
                device_id,
                button: ButtonSource::Touch { finger_id, .. },
                state,
                position,
                ..
            } => {
                let key = (*device_id, *finger_id);
                if *state == ElementState::Pressed {
                    self.held.insert(key, *position);
                } else {
                    self.held.remove(&key);
                }
            }
            WindowEvent::PointerMoved {
                device_id,
                source: PointerSource::Touch { finger_id, .. },
                position,
                ..
            } => {
                if let Some(previous) = self.held.get_mut(&(*device_id, *finger_id)) {
                    *previous = *position;
                }
            }
            WindowEvent::PointerLeft {
                device_id,
                kind: PointerKind::Touch(finger_id),
                position,
                ..
            } => {
                if let Some(previous) = self.held.remove(&(*device_id, *finger_id)) {
                    return vec![(*finger_id, position.unwrap_or(previous))];
                }
            }
            WindowEvent::Focused(false) => {
                return std::mem::take(&mut self.held)
                    .into_iter()
                    .map(|((_, finger), position)| (finger, position))
                    .collect();
            }
            _ => {}
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(finger: usize, state: ElementState) -> WindowEvent {
        WindowEvent::PointerButton {
            device_id: None,
            button: ButtonSource::Touch {
                finger_id: FingerId::from_raw(finger),
                force: None,
            },
            state,
            position: PhysicalPosition::new(10.0, 20.0),
            primary: true,
            is_macos_activation_click: false,
        }
    }

    fn left(finger: usize) -> WindowEvent {
        WindowEvent::PointerLeft {
            device_id: None,
            kind: PointerKind::Touch(FingerId::from_raw(finger)),
            position: None,
            primary: true,
        }
    }

    #[test]
    fn ordinary_release_is_not_followed_by_a_second_touch_ending() {
        let mut tracker = TouchTracker::default();
        tracker.handle(&button(1, ElementState::Pressed));
        tracker.handle(&button(1, ElementState::Released));
        assert!(tracker.handle(&left(1)).is_empty());
    }

    #[test]
    fn interrupted_contact_cancels_once_and_preserves_other_contacts() {
        let mut tracker = TouchTracker::default();
        tracker.handle(&button(1, ElementState::Pressed));
        tracker.handle(&button(2, ElementState::Pressed));
        let cancelled = tracker.handle(&left(1));
        assert_eq!(
            cancelled,
            vec![(FingerId::from_raw(1), PhysicalPosition::new(10.0, 20.0))]
        );
        assert!(tracker.handle(&left(1)).is_empty());
        assert_eq!(tracker.handle(&WindowEvent::Focused(false)).len(), 1);
        assert!(tracker.handle(&WindowEvent::Focused(false)).is_empty());
    }
}
