//! One CPU load at a time; superseded avatar results never reach the renderer.
use std::{sync::mpsc, thread};

pub struct Loader<K, V> {
    ready: Option<(K, V)>,
    pending: Option<(K, mpsc::Receiver<Result<V, String>>)>,
    failed: Option<K>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    #[test]
    fn a_blocked_load_keeps_ui_polling_and_discards_superseded_avatar() {
        let mut loader = Loader::<u32, u32>::default();
        let (release, blocked) = mpsc::channel();
        let (entered, started) = mpsc::channel();
        assert!(
            loader
                .get_or_start(1, move |_| {
                    entered.send(()).unwrap();
                    blocked.recv().unwrap();
                    Ok(11)
                })
                .unwrap()
                .is_none()
        );
        started.recv_timeout(Duration::from_secs(1)).unwrap();
        for key in [1, 2, 3, 2] {
            assert!(
                loader
                    .get_or_start(key, |_| panic!("parallel mesh load"))
                    .unwrap()
                    .is_none()
            );
        }
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(value) = loader.get_or_start(2, |key| Ok(key * 10)).unwrap() {
                assert_eq!(*value, 20);
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(
            loader
                .get_or_start(2, |_| panic!("cached mesh reloaded"))
                .unwrap(),
            Some(&20)
        );
    }

    #[test]
    fn returning_to_the_selected_avatar_uses_its_cache_during_a_new_load() {
        let mut loader = Loader::<u32, u32>::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while loader.get_or_start(1, |_| Ok(10)).unwrap().is_none() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        let (release, blocked) = mpsc::channel();
        assert!(
            loader
                .get_or_start(2, move |_| {
                    blocked.recv().unwrap();
                    Ok(20)
                })
                .unwrap()
                .is_none()
        );
        assert_eq!(
            loader
                .get_or_start(1, |_| panic!("selected mesh reloaded"))
                .unwrap(),
            Some(&10)
        );
        release.send(()).unwrap();
        while loader.pending.is_some() {
            assert_eq!(
                loader
                    .get_or_start(1, |_| panic!("selected mesh reloaded"))
                    .unwrap(),
                Some(&10)
            );
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(
            loader
                .get_or_start(1, |_| panic!("stale result replaced selected mesh"))
                .unwrap(),
            Some(&10)
        );
    }

    #[test]
    fn a_failed_avatar_reports_once_and_does_not_retry_each_tick() {
        let mut loader = Loader::<u32, u32>::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker_calls = calls.clone();
        loader
            .get_or_start(1, move |_| {
                worker_calls.fetch_add(1, Ordering::SeqCst);
                Err("missing mesh".into())
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Err(error) = loader.get_or_start(1, |_| panic!("failed mesh reloaded")) {
                assert_eq!(error, "missing mesh");
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(
            loader
                .get_or_start(1, |_| panic!("failed mesh reloaded"))
                .unwrap()
                .is_none()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
impl<K, V> Default for Loader<K, V> {
    fn default() -> Self {
        Self {
            ready: None,
            pending: None,
            failed: None,
        }
    }
}
impl<K: Clone + Eq + Send + 'static, V: Send + 'static> Loader<K, V> {
    pub fn get_or_start(
        &mut self,
        key: K,
        load: impl FnOnce(K) -> Result<V, String> + Send + 'static,
    ) -> Result<Option<&V>, String> {
        if let Some((_, receiver)) = &self.pending {
            let completed = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("figure mesh worker disconnected".into()))
                }
            };
            if let Some(result) = completed {
                let (completed_key, _) = self.pending.take().unwrap();
                if completed_key == key {
                    match result {
                        Ok(value) => {
                            self.ready = Some((completed_key, value));
                            self.failed = None;
                        }
                        Err(error) => {
                            self.failed = Some(completed_key);
                            return Err(error);
                        }
                    }
                }
            }
        }
        if self
            .ready
            .as_ref()
            .is_some_and(|(ready_key, _)| *ready_key == key)
        {
            return Ok(self.ready.as_ref().map(|(_, value)| value));
        }
        if self.pending.is_none() && self.failed.as_ref() != Some(&key) {
            let (sender, receiver) = mpsc::channel();
            let worker_key = key.clone();
            thread::Builder::new()
                .name("voxy-figure-mesh".into())
                .spawn(move || {
                    let _ = sender.send(load(worker_key));
                })
                .map_err(|error| format!("start figure mesh worker: {error}"))?;
            self.pending = Some((key, receiver));
        }
        Ok(None)
    }
}
