//! Hold an authenticated scene transition until graphics startup succeeds. The caller handles
//! UI input (including cancellation) before polling this gate each tick.

pub(super) struct DeferredScene<T> {
    request: Option<T>,
}

pub(super) enum Status<T> {
    Idle,
    Waiting(usize, usize),
    Ready(T),
    Failed(String),
}

impl<T> Default for DeferredScene<T> {
    fn default() -> Self {
        Self { request: None }
    }
}

impl<T> DeferredScene<T> {
    pub fn is_pending(&self) -> bool {
        self.request.is_some()
    }

    pub fn begin(&mut self, request: T) {
        self.request = Some(request);
    }

    pub fn cancel(&mut self) {
        self.request = None;
    }

    pub fn poll(
        &mut self,
        graphics: impl FnOnce() -> Result<Option<(usize, usize)>, String>,
    ) -> Status<T> {
        if self.request.is_none() {
            return Status::Idle;
        }
        match graphics() {
            Ok(Some((done, total))) => Status::Waiting(done, total),
            Ok(None) => Status::Ready(self.request.take().unwrap()),
            Err(error) => {
                self.cancel();
                Status::Failed(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_and_cancel_never_poll_graphics_or_release_a_scene_transition() {
        let mut gate = DeferredScene::default();
        assert!(matches!(
            gate.poll(|| panic!("boot requested graphics")),
            Status::<u8>::Idle
        ));
        gate.begin(7);
        assert!(matches!(
            gate.poll(|| Ok(Some((1, 4)))),
            Status::Waiting(1, 4)
        ));
        gate.cancel();
        assert!(matches!(
            gate.poll(|| panic!("cancelled login requested graphics")),
            Status::Idle
        ));
    }

    #[test]
    fn authenticated_scene_is_released_once_and_only_after_graphics_succeeds() {
        let mut gate = DeferredScene::default();
        gate.begin("connection");
        for progress in [(0, 0), (0, 3), (2, 3)] {
            assert!(matches!(
                gate.poll(|| Ok(Some(progress))),
                Status::Waiting(..)
            ));
            assert!(gate.is_pending());
        }
        assert!(matches!(
            gate.poll(|| Ok(None)),
            Status::Ready("connection")
        ));
        assert!(matches!(
            gate.poll(|| panic!("duplicate connection")),
            Status::Idle
        ));
    }

    #[test]
    fn device_failure_discards_request_and_allows_retry() {
        let mut gate = DeferredScene::default();
        gate.begin("first");
        assert!(
            matches!(gate.poll(|| Err("device unavailable".into())), Status::Failed(message) if message == "device unavailable")
        );
        assert!(!gate.is_pending());
        assert!(matches!(
            gate.poll(|| panic!("failed login connected")),
            Status::Idle
        ));
        gate.begin("retry");
        assert!(matches!(gate.poll(|| Ok(None)), Status::Ready("retry")));
    }
}
