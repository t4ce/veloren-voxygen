//! Fixed-size samples; wall-clock, rate-limited heartbeat, even during retries.
use super::terrain_layers::Metrics;
use std::time::Instant;
use std::{collections::VecDeque, time::Duration};
const WINDOW: Duration = Duration::from_secs(2);
pub const FRAME_BUDGET_US: u64 = 33_333; // packed bring-up profile: 30 FPS
pub fn record(message: std::fmt::Arguments<'_>) {
    #[cfg(target_os = "trueos")]
    {
        let _ = trueos::vsys::log_record(
            trueos::vsys::LOG_LEVEL_IMPORTANT,
            "apps::voxygen",
            &message.to_string(),
        );
    }
    #[cfg(not(target_os = "trueos"))]
    tracing::info!(target:"voxy_terrain", "{message}");
}
pub fn percentile(samples: &VecDeque<u64>, percent: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut values: Vec<_> = samples.iter().copied().collect();
    values.sort_unstable();
    values[(values.len() * percent)
        .div_ceil(100)
        .saturating_sub(1)
        .min(values.len() - 1)]
}
fn sample(samples: &mut VecDeque<u64>, value: u64) {
    if samples.len() == 128 {
        samples.pop_front();
    }
    samples.push_back(value);
}
pub struct Heartbeat {
    run_us: u64,
    sequence: u64,
    last: Instant,
    frames: u64,
    publications: u64,
    chunk_frames: u64,
    uploads: u64,
    busy: u64,
    revisions: u64,
    failures_seen: u64,
    last_near: Option<(u64, Option<Instant>)>,
    receipts: VecDeque<u64>,
    prepare: VecDeque<u64>,
    submit: VecDeque<u64>,
    publish: VecDeque<u64>,
    metrics: Metrics,
}
impl Heartbeat {
    pub fn new() -> Self {
        Self {
            sequence: 0,
            run_us: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_micros() as u64),
            last: Instant::now(),
            frames: 0,
            publications: 0,
            chunk_frames: 0,
            uploads: 0,
            busy: 0,
            revisions: 0,
            failures_seen: 0,
            last_near: None,
            receipts: VecDeque::with_capacity(128),
            prepare: VecDeque::with_capacity(128),
            submit: VecDeque::with_capacity(128),
            publish: VecDeque::with_capacity(128),
            metrics: Metrics::default(),
        }
    }
    fn mode(&mut self, metrics: Metrics) {
        if metrics.mode != self.metrics.mode || metrics.stream_run_us != self.metrics.stream_run_us
        {
            if metrics.stream_run_us != self.metrics.stream_run_us {
                self.last_near = None;
            }
            // Begin a fresh two-second window; F1 cannot flood heartbeat logs.
            self.reset(Instant::now());
        }
        self.metrics = metrics;
    }
    pub fn uploaded(&mut self, metrics: Metrics, bytes: u64) {
        self.mode(metrics);
        self.uploads += bytes;
    }
    pub fn retired(&mut self, metrics: Metrics, prepare_us: u64, submit_us: u64) {
        self.mode(metrics);
        self.frames += 1;
        sample(&mut self.prepare, prepare_us);
        sample(&mut self.submit, submit_us);
    }
    pub fn published(&mut self, metrics: Metrics, elapsed: u64) {
        self.mode(metrics);
        self.publications += 1;
        self.chunk_frames += metrics.near_chunks as u64;
        sample(&mut self.publish, elapsed);
        let revision = (metrics.near_revision, metrics.received_at);
        if metrics.near_chunks > 0 && self.last_near != Some(revision) {
            self.last_near = Some(revision);
            self.revisions += 1;
            if let Some(received_at) = metrics.received_at {
                sample(&mut self.receipts, received_at.elapsed().as_micros() as u64);
            }
        }
    }
    pub fn busy(&mut self) {
        self.busy += 1;
    }
    pub fn tick(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last) >= WINDOW {
            self.emit(now);
        }
    }
    fn emit(&mut self, now: Instant) {
        let seconds = now.duration_since(self.last).as_secs_f64().max(0.000001);
        let p95 = percentile(&self.publish, 95);
        let failures = self
            .metrics
            .warm_failures
            .saturating_sub(self.failures_seen);
        let ready = match self.metrics.mode {
            super::terrain_layers::Mode::Near => self.metrics.near_chunks > 0,
            super::terrain_layers::Mode::Both => {
                self.metrics.near_chunks > 0 && self.metrics.far_ready
            }
            super::terrain_layers::Mode::Far => self.metrics.far_vertices > 0,
        };
        self.sequence += 1;
        record(format_args!(
            "terrain-heartbeat: run_us={} stream_run_us={} sequence={} mode={} seconds={:.3} retired={} published={} published_fps={:.2} ready_chunk_frames={} chunk_frames_s={:.2} near_chunks={} loaded_chunks={} near_vertices={} far_vertices={} near_revision={} cpu_upload_bytes={} prepare_p95_us={} submit_wait_p95_us={} frame_to_publish_p50_us={} frame_to_publish_p95_us={} frame_to_publish_max_us={} frame_budget_us={} throughput_ok={} retries={} warm_failures={} window_warm_failures={} far_ready={} near_warm_us={} far_warm_us={} compose_us={} budget_fallback={} width={} height={} new_near_revisions={} receipt_samples={} chunk_receipt_to_first_publish_p95_us={} measurement=provisional target_fps=30 min_fps=27 boundary=gpu-retired+ui4-published physical_receipt=unavailable-background\n",
            self.run_us,
            self.metrics.stream_run_us,
            self.sequence,
            self.metrics.mode.label(),
            seconds,
            self.frames,
            self.publications,
            self.publications as f64 / seconds,
            self.chunk_frames,
            self.chunk_frames as f64 / seconds,
            self.metrics.near_chunks,
            self.metrics.loaded_chunks,
            self.metrics.near_vertices,
            self.metrics.far_vertices,
            self.metrics.near_revision,
            self.uploads,
            percentile(&self.prepare, 95),
            percentile(&self.submit, 95),
            percentile(&self.publish, 50),
            p95,
            self.publish.iter().copied().max().unwrap_or(0),
            FRAME_BUDGET_US,
            ready
                && self.publications as f64 / seconds >= 27.
                && p95 <= FRAME_BUDGET_US
                && !self.metrics.budget_fallback
                && failures == 0,
            self.busy,
            self.metrics.warm_failures,
            failures,
            self.metrics.far_ready,
            self.metrics.near_warm_us,
            self.metrics.far_warm_us,
            self.metrics.compose_us,
            self.metrics.budget_fallback,
            self.metrics.width,
            self.metrics.height,
            self.revisions,
            self.receipts.len(),
            percentile(&self.receipts, 95)
        ));
        self.failures_seen = self.metrics.warm_failures;
        self.reset(now);
    }
    fn reset(&mut self, now: Instant) {
        self.last = now;
        self.frames = 0;
        self.publications = 0;
        self.chunk_frames = 0;
        self.uploads = 0;
        self.busy = 0;
        self.revisions = 0;
        self.receipts.clear();
        self.prepare.clear();
        self.submit.clear();
        self.publish.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentiles_handle_empty_tail_spikes_and_bounded_history() {
        let mut samples = VecDeque::new();
        assert_eq!(percentile(&samples, 95), 0);
        for _ in 0..127 {
            sample(&mut samples, 10);
        }
        sample(&mut samples, 1_000_000);
        assert_eq!(percentile(&samples, 95), 10);
        assert_eq!(percentile(&samples, 100), 1_000_000);
        for _ in 0..128 {
            sample(&mut samples, 20);
        }
        assert_eq!(samples.len(), 128);
        assert_eq!(percentile(&samples, 100), 20);
    }
    #[test]
    fn mode_windows_and_first_publication_receipts_do_not_mix() {
        let mut heartbeat = Heartbeat::new();
        let near = Metrics {
            mode: super::super::terrain_layers::Mode::Near,
            near_chunks: 1,
            near_revision: 1,
            received_at: Some(Instant::now()),
            ..Default::default()
        };
        heartbeat.published(near, 300);
        heartbeat.published(near, 400);
        assert_eq!(heartbeat.revisions, 1);
        assert_eq!(heartbeat.receipts.len(), 1);
        let far = Metrics {
            mode: super::super::terrain_layers::Mode::Far,
            far_vertices: 6,
            ..Default::default()
        };
        heartbeat.retired(far, 100, 200);
        assert_eq!(heartbeat.publications, 0);
        assert_eq!(heartbeat.frames, 1);
        heartbeat.published(near, 500);
        assert_eq!(heartbeat.frames, 0);
        assert_eq!(heartbeat.revisions, 0);
    }
    #[test]
    fn stalls_emit_zero_progress_and_clear_window_counts() {
        let mut heartbeat = Heartbeat::new();
        let metrics = Metrics {
            near_chunks: 9,
            ..Default::default()
        };
        heartbeat.uploaded(metrics, 80);
        heartbeat.retired(metrics, 100, 200);
        heartbeat.published(metrics, 300);
        heartbeat.busy();
        assert_eq!(heartbeat.chunk_frames, 9);
        heartbeat.last -= WINDOW;
        heartbeat.tick();
        assert_eq!(heartbeat.frames, 0);
        assert_eq!(heartbeat.publications, 0);
        assert_eq!(heartbeat.chunk_frames, 0);
        assert_eq!(heartbeat.uploads, 0);
        assert_eq!(heartbeat.busy, 0);
        assert!(heartbeat.publish.is_empty());
        heartbeat.last -= WINDOW;
        heartbeat.tick();
        assert_eq!(heartbeat.publications, 0);
    }
}
