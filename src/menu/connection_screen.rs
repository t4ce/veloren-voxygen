//! Delay connection outcomes until the loading screen has been published for
//! three seconds. The caller keeps polling input and networking throughout.
// Share Winit's std monotonic clock for appearance timing.
use std::time::{self, Duration};

const MINIMUM_VISIBLE: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct ConnectionScreen {
    attempt: Option<Attempt>,
}

#[derive(Default)]
struct Attempt {
    first_revision: Option<u64>,
    visible_since: Option<time::Instant>,
    outcome: Option<String>,
}

impl ConnectionScreen {
    pub fn begin(&mut self) {
        self.attempt = Some(Attempt::default());
    }

    pub fn cancel(&mut self) {
        self.attempt = None;
    }

    pub fn is_active(&self) -> bool {
        self.attempt.is_some()
    }

    pub fn submitted(&mut self, revision: u64) {
        if let Some(attempt) = &mut self.attempt {
            attempt.first_revision.get_or_insert(revision);
        }
    }

    /// Both independent layers must reach the first loading-screen revision.
    /// Animation frames can supersede queued work, so later revisions count too.
    pub fn published(&mut self, revision: u64, now: time::Instant) -> bool {
        if let Some(attempt) = &mut self.attempt
            && attempt.visible_since.is_none()
            && attempt
                .first_revision
                .is_some_and(|first| revision >= first)
        {
            attempt.visible_since = Some(now);
            return true;
        }
        false
    }

    pub fn complete(&mut self, message: String) {
        if let Some(attempt) = &mut self.attempt {
            attempt.outcome = Some(message);
        }
    }

    pub fn take_ready(&mut self, now: time::Instant) -> Option<String> {
        let attempt = self.attempt.as_ref()?;
        if now.saturating_duration_since(attempt.visible_since?) < MINIMUM_VISIBLE
            || attempt.outcome.is_none()
        {
            return None;
        }
        self.attempt.take()?.outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_success_and_failure_wait_three_seconds_after_publication() {
        for outcome in ["Login complete", "No server running"] {
            let now = time::Instant::now();
            let mut screen = ConnectionScreen::default();
            screen.begin();
            screen.complete(outcome.into());
            screen.submitted(8);
            // Slow producers must not consume the minimum visible duration.
            assert!(!screen.published(7, now));
            assert!(screen.take_ready(now + Duration::from_secs(40)).is_none());
            let visible = now + Duration::from_secs(40);
            assert!(screen.published(9, visible));
            assert!(
                screen
                    .take_ready(visible + Duration::from_millis(2999))
                    .is_none()
            );
            assert_eq!(
                screen.take_ready(visible + MINIMUM_VISIBLE).as_deref(),
                Some(outcome)
            );
            assert!(!screen.is_active());
        }
    }

    #[test]
    fn animation_does_not_restart_the_minimum_and_slow_login_has_no_extra_delay() {
        let now = time::Instant::now();
        let mut screen = ConnectionScreen::default();
        screen.begin();
        screen.submitted(1);
        assert!(screen.published(1, now));
        screen.submitted(2);
        assert!(!screen.published(2, now + Duration::from_secs(2)));
        assert!(screen.take_ready(now + MINIMUM_VISIBLE).is_none());
        screen.complete("Login complete".into());
        assert!(screen.take_ready(now + Duration::from_secs(5)).is_some());
    }

    #[test]
    fn cancel_before_or_after_completion_discards_the_result_immediately() {
        for completed in [false, true] {
            let now = time::Instant::now();
            let mut screen = ConnectionScreen::default();
            screen.begin();
            screen.submitted(1);
            screen.published(1, now);
            if completed {
                screen.complete("Login complete".into());
            }
            screen.cancel();
            assert!(!screen.is_active());
            screen.complete("Late response".into());
            assert!(screen.take_ready(now + Duration::from_secs(10)).is_none());
        }
    }

    #[test]
    fn retry_gets_a_fresh_minimum_without_the_previous_result() {
        let now = time::Instant::now();
        let mut screen = ConnectionScreen::default();
        screen.begin();
        screen.submitted(1);
        screen.published(1, now);
        screen.complete("Old error".into());
        screen.cancel();
        screen.begin();
        screen.submitted(20);
        assert!(!screen.published(19, now + Duration::from_secs(10)));
        screen.published(20, now + Duration::from_secs(10));
        assert!(screen.take_ready(now + Duration::from_secs(13)).is_none());
        screen.complete("New success".into());
        assert_eq!(
            screen.take_ready(now + Duration::from_secs(13)).as_deref(),
            Some("New success")
        );
    }
}
