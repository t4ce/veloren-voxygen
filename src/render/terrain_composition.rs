//! Bounded background whole-map warm and retained near/far composition.
use super::{
    terrain_feature::Geometry,
    terrain_layers::{self, Coverage, Heightfield, Metrics, Mode},
};
use crate::client::Client;
use std::time::Instant;
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    mode: Mode,
    near: u64,
    far: bool,
    coverage: Option<Coverage>,
}
struct Completed {
    key: Key,
    geometry: Geometry,
    metrics: Metrics,
}
pub struct Composition {
    far: Option<Arc<Heightfield>>,
    pending_far: Option<mpsc::Receiver<Result<(Heightfield, u64), &'static str>>>,
    pending: Option<mpsc::Receiver<Result<Completed, &'static str>>>,
    cached: Option<(Key, Arc<Geometry>, Metrics)>,
    retry: Instant,
    far_retry: Instant,
    next_log: Instant,
    failures: u64,
    far_warm_us: u64,
}
impl Composition {
    pub fn new() -> Self {
        Self {
            far: None,
            pending_far: None,
            pending: None,
            cached: None,
            retry: Instant::now(),
            far_retry: Instant::now(),
            next_log: Instant::now(),
            failures: 0,
            far_warm_us: 0,
        }
    }
    pub fn far_plane(&self, client: &Client) -> f32 {
        let size = client.world_data().chunk_size().map(|v| v as f32) * 32.;
        (size.magnitude() + client.world_data().max_chunk_alt().abs() + 1024.).clamp(256., 100_000.)
    }
    pub fn prepare(
        &mut self,
        client: &Client,
        near: Option<Arc<Geometry>>,
        revision: u64,
        coverage: Option<Coverage>,
        mut source: Metrics,
    ) -> Option<(Arc<Geometry>, Metrics)> {
        let now = Instant::now();
        let mode = Mode::current();
        if let Some(receiver) = self.pending_far.as_ref() {
            match receiver.try_recv() {
                Ok(Ok((map, us))) => {
                    self.far = Some(Arc::new(map));
                    self.far_warm_us = us;
                    self.pending_far = None;
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending_far = None;
                    self.failures += 1;
                    self.far_retry = now + Duration::from_secs(2);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.far.is_none() && self.pending_far.is_none() && now >= self.far_retry {
            let started = Instant::now();
            match snapshot(client) {
                Some(map) => {
                    let (sender, receiver) = mpsc::channel();
                    self.pending_far = Some(receiver);
                    // All map reads above are bounded by the half-budget sample
                    // lattice; geometry/clipping/atlas construction stays off tick.
                    let worker = std::thread::Builder::new()
                        .name("terrain-map".into())
                        .spawn(move || {
                            let result = if map.valid() {
                                Ok((map, started.elapsed().as_micros() as u64))
                            } else {
                                Err("heightfield")
                            };
                            let _ = sender.send(result);
                        });
                    if worker.is_err() {
                        self.pending_far = None;
                        self.failures += 1;
                        self.far_retry = now + Duration::from_secs(2);
                    }
                }
                None => {
                    self.failures += 1;
                    self.far_retry = now + Duration::from_secs(2);
                }
            }
        }
        let key = Key {
            mode,
            near: if mode == Mode::Far { 0 } else { revision },
            far: mode != Mode::Near && self.far.is_some(),
            coverage: if mode == Mode::Far { None } else { coverage },
        };
        if let Some(receiver) = self.pending.as_ref() {
            match receiver.try_recv() {
                Ok(Ok(done)) => {
                    self.pending = None;
                    if done.key == key {
                        self.cached = Some((key, Arc::new(done.geometry), done.metrics));
                    }
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.failures += 1;
                    self.retry = now + Duration::from_millis(500);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none()
            && now >= self.retry
            && self.cached.as_ref().is_none_or(|(k, _, _)| *k != key)
            && (near.is_some() || self.far.is_some())
        {
            let far = self.far.clone();
            let (sender, receiver) = mpsc::channel();
            self.pending = Some(receiver);
            self.retry = now + Duration::from_millis(100);
            let result = std::thread::Builder::new()
                .name("terrain-compose".into())
                .spawn(move || {
                    let started = Instant::now();
                    let result =
                        terrain_layers::compose(far.as_deref(), near.as_deref(), coverage, mode)
                            .map(|(geometry, mut metrics)| {
                                metrics.compose_us = started.elapsed().as_micros() as u64;
                                metrics.near_warm_us = source.near_warm_us;
                                metrics.received_at = if metrics.near_chunks > 0 {
                                    source.received_at
                                } else {
                                    None
                                };
                                Completed {
                                    key,
                                    geometry,
                                    metrics,
                                }
                            });
                    let _ = sender.send(result);
                });
            if result.is_err() {
                self.pending = None;
                self.failures += 1;
                self.retry = now + Duration::from_secs(2);
            }
        }
        if now >= self.next_log {
            self.next_log = now + Duration::from_secs(2);
            super::terrain_heartbeat::record(format_args!(
                "terrain-warm: requested={} near_revision={} far_ready={} pending={} failures={} near_warm_us={} far_warm_us={} source=cpu-grid+whole-world-lod\n",
                mode.label(),
                revision,
                self.far.is_some(),
                self.pending.is_some(),
                self.failures,
                source.near_warm_us,
                self.far_warm_us
            ));
        }
        source.warm_failures += self.failures;
        source.far_warm_us = self.far_warm_us;
        let (_, geometry, metrics) = self.cached.as_ref()?;
        // The receipt describes the geometry actually submitted, including a
        // previous complete composition retained while a new worker is pending.
        let mut metrics = *metrics;
        metrics.near_revision = self.cached.as_ref().unwrap().0.near;
        metrics.loaded_chunks = source.loaded_chunks;
        metrics.width = source.width;
        metrics.height = source.height;
        metrics.far_warm_us = source.far_warm_us;
        metrics.warm_failures = source.warm_failures;
        Some((geometry.clone(), metrics))
    }
}
fn snapshot(client: &Client) -> Option<Heightfield> {
    let world = client.world_data();
    let size = world.chunk_size();
    let [nx, ny] = terrain_layers::grid_shape(size.x as usize, size.y as usize)?;
    let mut heights = Vec::with_capacity((nx + 1) * (ny + 1));
    let mut colors = Vec::with_capacity(nx * ny);
    for y in 0..=ny {
        for x in 0..=nx {
            let key = vek::Vec2::new(
                (x * size.x as usize / nx).min(size.x as usize - 1) as i32,
                (y * size.y as usize / ny).min(size.y as usize - 1) as i32,
            );
            heights.push(world.alt_at(key)?);
            if x < nx && y < ny {
                let [r, g, b, _] = world.lod_base.get(key)?.to_le_bytes();
                colors.push([r, g, b, 255]);
            }
        }
    }
    Some(Heightfield {
        cells: [nx, ny],
        extent: [size.x as f32 * 32., size.y as f32 * 32.],
        heights,
        colors,
    })
}
