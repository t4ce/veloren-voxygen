//! Latest sky RGB for the shared display backdrop, independent of scene frames.
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Backdrop {
    desired: Option<[u8; 3]>,
    applied: Option<[u8; 3]>,
    last_call: Option<Instant>,
}
impl Backdrop {
    pub fn request(&mut self, rgba: Option<u32>) {
        self.desired = rgba.map(|rgba| {
            let [r, g, b, _] = rgba.to_le_bytes();
            [r, g, b]
        });
    }

    /// Record attempts before calling the helper, so Busy retries also obey 1 Hz.
    pub fn take_due(&mut self, now: Instant) -> Option<[u8; 3]> {
        let desired = self.desired?;
        if self.applied == Some(desired)
            || self
                .last_call
                .is_some_and(|last| now.duration_since(last) < Duration::from_secs(1))
        {
            return None;
        }
        self.last_call = Some(now);
        Some(desired)
    }

    pub fn applied(&mut self, rgb: [u8; 3]) {
        self.applied = Some(rgb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_color_is_applied_at_most_once_a_second_without_a_frame() {
        let now = Instant::now();
        let mut backdrop = Backdrop::default();
        backdrop.request(Some(u32::from_le_bytes([12, 34, 56, 255])));
        let first = backdrop.take_due(now).unwrap();
        assert_eq!(first, [12, 34, 56]);
        backdrop.applied(first);
        assert_eq!(backdrop.take_due(now + Duration::from_secs(10)), None);
        backdrop.request(Some(u32::from_le_bytes([1, 2, 3, 255])));
        assert_eq!(backdrop.take_due(now + Duration::from_millis(999)), None);
        backdrop.request(Some(u32::from_le_bytes([4, 5, 6, 255])));
        assert_eq!(
            backdrop.take_due(now + Duration::from_secs(1)),
            Some([4, 5, 6])
        );
        backdrop.applied([4, 5, 6]);
        assert_eq!(backdrop.take_due(now + Duration::from_secs(2)), None);
    }
    #[test]
    fn failed_attempts_and_policy_changes_cannot_bypass_the_rate_limit() {
        let now = Instant::now();
        let mut backdrop = Backdrop::default();
        backdrop.request(Some(0xff00_0001));
        assert_eq!(backdrop.take_due(now), Some([1, 0, 0]));
        // A failed call leaves the color pending, but cannot immediately retry.
        assert_eq!(backdrop.take_due(now + Duration::from_millis(999)), None);
        backdrop.request(None);
        assert_eq!(backdrop.take_due(now + Duration::from_secs(1)), None);
        backdrop.request(Some(0xff00_0002));
        assert_eq!(backdrop.take_due(now + Duration::from_millis(999)), None);
        assert_eq!(
            backdrop.take_due(now + Duration::from_secs(1)),
            Some([2, 0, 0])
        );
    }
}
