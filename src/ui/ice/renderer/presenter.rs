//! Paired producers. No GPU admission/fence wait runs on the input thread.
//! The broker coordinates plane placement and paired resize epochs. Neither
//! producer gates the other on SURFLIVE: a paired resize needs both publications
//! before either replacement can become display-live.
use super::{
    activity::{ProducerActivity, ProducerCounters, micros},
    bcs::{FramePlan, LayerPlan},
    backdrop::Backdrop,
    damage::{self, Repaint},
};
use crossbeam_queue::ArrayQueue;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use trueos::ui4_winit::{Damage, Error, SceneTarget, SpriteBackend};
use vek::Vec2;

struct Job {
    revision: u64,
    size: Vec2<u32>,
    plan: LayerPlan,
    sky: Option<u32>,
    #[cfg(target_os = "trueos")]
    figure: Option<Arc<crate::render::figure_preview::Frame>>,
    #[cfg(target_os = "trueos")]
    terrain: Option<Arc<crate::render::terrain_feature::Frame>>,
}
struct Mailbox {
    latest: ArrayQueue<Job>,
    stopped: AtomicBool,
    published_revision: AtomicU64,
    counters: ProducerCounters,
}
impl Default for Mailbox {
    fn default() -> Self {
        Self {
            latest: ArrayQueue::new(1),
            stopped: AtomicBool::new(false),
            published_revision: AtomicU64::new(0),
            counters: ProducerCounters::default(),
        }
    }
}
impl Mailbox {
    fn submit(&self, job: Job) {
        // Keep only the newest unclaimed frame. A displaced job is dropped after
        // the queue operation, without holding a mailbox mutex on the UI thread.
        if self.latest.force_push(job).is_some() {
            self.counters
                .queued_replacements
                .fetch_add(1, Ordering::Relaxed);
        }
    }
    fn take(&self) -> Option<Job> {
        self.latest.pop()
    }
    fn wait(&self) {
        // Idle and GPU-Busy retries use the native timed sleep path. Submissions
        // no longer depend on a condvar wake/relock; admission can take one extra
        // polling interval. An acquired frame is still completed before replacement.
        if !self.stopped.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(8));
        }
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}
struct Producer {
    mailbox: Arc<Mailbox>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Producer {
    fn drop(&mut self) {
        self.mailbox.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub(crate) struct LayeredPresenter {
    scene: Producer,
    foreground: Producer,
    errors: mpsc::Receiver<String>,
}
impl Drop for LayeredPresenter {
    fn drop(&mut self) {
        // A paired resize can depend on both producers. Signal both before
        // field destruction joins either worker.
        self.scene.mailbox.stop();
        self.foreground.mailbox.stop();
    }
}
impl LayeredPresenter {
    pub fn new(foreground: SceneTarget, scene: SceneTarget) -> Result<Self, String> {
        let (errors_tx, errors) = mpsc::channel();
        let scene = spawn("scene", scene, errors_tx.clone())?;
        let foreground = spawn("foreground", foreground, errors_tx)?;
        Ok(Self {
            scene,
            foreground,
            errors,
        })
    }
    pub fn submit(&self, revision: u64, size: Vec2<u32>, plan: FramePlan) {
        // Each bounded mailbox replaces only work which has not acquired a lease.
        self.scene.mailbox.submit(Job {
            revision,
            size,
            plan: plan.background,
            sky: None,
            #[cfg(target_os = "trueos")]
            figure: None,
            #[cfg(target_os = "trueos")]
            terrain: None,
        });
        self.foreground.mailbox.submit(Job {
            revision,
            size,
            plan: plan.foreground,
            sky: None,
            #[cfg(target_os = "trueos")]
            figure: None,
            #[cfg(target_os = "trueos")]
            terrain: None,
        });
    }
    pub fn clear_foreground(&self, revision: u64, size: Vec2<u32>) {
        self.foreground.mailbox.submit(Job {
            revision,
            size,
            plan: LayerPlan::default(),
            sky: None,
            #[cfg(target_os = "trueos")]
            figure: None,
            #[cfg(target_os = "trueos")]
            terrain: None,
        });
    }
    #[cfg(target_os = "trueos")]
    pub fn submit_figure(&self, revision: u64, size: Vec2<u32>, rgba: u32, figure: Option<Arc<crate::render::figure_preview::Frame>>) {
        self.scene.mailbox.submit(Job { revision, size, plan: LayerPlan::default(), sky: Some(rgba), figure, terrain: None });
    }
    #[cfg(target_os = "trueos")]
    pub fn submit_terrain(&self, revision: u64, size: Vec2<u32>, rgba: u32, terrain: Option<Arc<crate::render::terrain_feature::Frame>>) {
        self.scene.mailbox.submit(Job { revision, size, plan: LayerPlan::default(), sky: Some(rgba), figure: None, terrain });
    }
    pub fn submit_sky(&self, revision: u64, size: Vec2<u32>, rgba: u32) {
        self.scene.mailbox.submit(Job {
            revision,
            size,
            plan: LayerPlan::default(),
            sky: Some(rgba),
            #[cfg(target_os = "trueos")]
            figure: None,
            #[cfg(target_os = "trueos")]
            terrain: None,
        });
    }
    pub fn submit_foreground(&self, revision: u64, size: Vec2<u32>, plan: LayerPlan) {
        self.foreground.mailbox.submit(Job {
            revision,
            size,
            plan,
            sky: None,
            #[cfg(target_os = "trueos")]
            figure: None,
            #[cfg(target_os = "trueos")]
            terrain: None,
        });
    }
    pub fn scene_published_revision(&self) -> u64 {
        self.scene
            .mailbox
            .published_revision
            .load(Ordering::Acquire)
    }
    pub fn foreground_published_revision(&self) -> u64 {
        self.foreground
            .mailbox
            .published_revision
            .load(Ordering::Acquire)
    }
    pub(crate) fn take_activity(&self) -> (ProducerActivity, ProducerActivity) {
        (
            self.scene.mailbox.counters.take(),
            self.foreground.mailbox.counters.take(),
        )
    }
    pub(crate) fn published_revision(&self) -> u64 {
        self.scene
            .mailbox
            .published_revision
            .load(Ordering::Acquire)
            .min(
                self.foreground
                    .mailbox
                    .published_revision
                    .load(Ordering::Acquire),
            )
    }
    pub fn check(&self) -> Result<(), String> {
        match self.errors.try_recv() {
            Ok(error) => Err(error),
            Err(_) => Ok(()),
        }
    }
}
fn spawn(
    name: &'static str,
    mut target: SceneTarget,
    errors: mpsc::Sender<String>,
) -> Result<Producer, String> {
    let mailbox = Arc::new(Mailbox::default());
    let worker = Arc::clone(&mailbox);
    let handle = thread::Builder::new()
        .name(format!("voxy-{name}"))
        .spawn(move || {
            if let Err(error) = produce(&mut target, &worker, name) {
                let _ = errors.send(format!("{name} producer: {error}"));
            }
        })
        .map_err(|e| format!("start {name} producer: {e}"))?;
    Ok(Producer {
        mailbox,
        thread: Some(handle),
    })
}
fn produce(target: &mut SceneTarget, mailbox: &Mailbox, name: &str) -> Result<(), String> {
    let mut uploaded = HashMap::new();
    let mut previous: Option<(Vec2<u32>, LayerPlan)> = None;
    let mut preceding_damage = None;
    // When policy changes without a new allocation, remove old content from
    // every member of the background's triple-buffered ring exactly once.
    let mut scene_repaint_hold: Option<(Damage, u8)> = None;
    let mut repaint: Option<Repaint> = None;
    let mut job: Option<Job> = None;
    let mut phase = 0;
    let mut acquired_frame = false;
    let mut region_supported = true;
    let mut previous_sky = None;
    let mut backdrop = Backdrop::default();
    #[cfg(target_os = "trueos")]
    let mut previous_had_geometry = false;
    #[cfg(target_os = "trueos")]
    let mut sky_renderer: Option<crate::render::minimal_sky::NativeSky> = None;
    #[cfg(target_os = "trueos")]
    let mut terrain_renderer: Option<crate::render::terrain_feature::NativeTerrain> = None;
    while !mailbox.stopped.load(Ordering::Acquire) {
        mailbox.counters.iterations.fetch_add(1, Ordering::Relaxed);
        if job.is_none() {
            job = mailbox.take();
            phase = 0;
            acquired_frame = false;
        }
        if phase <= 1 && !acquired_frame {
            if let Some(latest) = mailbox.take() {
                if job.is_some() {
                    mailbox
                        .counters
                        .queued_replacements
                        .fetch_add(1, Ordering::Relaxed);
                }
                job = Some(latest);
                phase = 0;
            }
        }
        if let Some(current) = job.as_ref() {
            backdrop.request(current.sky);
        }
        // The display backdrop does not need a scene write lease. Keep the
        // newest throttled color pending even after its scene has published.
        if let Some(rgb) = backdrop.take_due(std::time::Instant::now()) {
            match target.set_display_bottom_color(rgb) {
                Ok(()) => backdrop.applied(rgb),
                Err(Error::Busy) => {},
                Err(error) => return Err(format!("Display backdrop update failed: {error:?}")),
            }
        }
        let Some(current) = job.as_ref() else {
            mailbox.wait();
            continue;
        };
        if phase == 0
            && scene_repaint_hold.is_none()
            && {
                #[cfg(target_os = "trueos")] { current.figure.is_none() && current.terrain.is_none() && !previous_had_geometry }
                #[cfg(not(target_os = "trueos"))] { true }
            }
            && previous.as_ref().is_some_and(|(size, plan)| {
                *size == current.size
                    && (previous_sky == current.sky
                        || (previous_sky.is_some() && current.sky.is_some()))
                    && plan.viewport == current.plan.viewport
                    && plan.commands == current.plan.commands
                    && current.plan.uploads.iter().all(|upload| {
                        plan.uploads.iter().any(|previous| {
                            previous.id == upload.id && Arc::ptr_eq(&previous.image, &upload.image)
                        })
                    })
            })
        {
            mailbox.counters.unchanged.fetch_add(1, Ordering::Relaxed);
            // The existing publication already has this revision's contents.
            mailbox
                .published_revision
                .store(current.revision, Ordering::Release);
            job = None;
            continue;
        }
        let result = match phase {
            0 => {
                #[cfg(target_os = "trueos")]
                if current.sky.is_some() && sky_renderer.is_none() {
                    sky_renderer = Some(crate::render::minimal_sky::NativeSky::open()?);
                    tracing::info!(target: "voxy_scene_contract", "Minimal RGBA8 sky device and render queue ready");
                }
                #[cfg(target_os = "trueos")]
                if current.terrain.is_some() && terrain_renderer.is_none() {
                    terrain_renderer = Some(crate::render::terrain_feature::NativeTerrain::open()?);
                }
                if name == "foreground" && region_supported {
                    repaint = Some(damage::prepare(
                        previous
                            .as_ref()
                            .map(|(size, plan)| ((size.x, size.y), plan)),
                        &current.plan,
                        (current.size.x, current.size.y),
                        // Replacement buffers after resize have no retained pixels.
                        preceding_damage.filter(|_| {
                            previous
                                .as_ref()
                                .is_some_and(|(size, _)| *size == current.size)
                        }),
                    ));
                }
                if name == "scene" && region_supported {
                    if let Some(content) = current.plan.viewport {
                        if let Some((old_size, old_plan)) = &previous {
                            if *old_size != current.size {
                                scene_repaint_hold = None;
                            } else if old_plan.viewport != current.plan.viewport {
                                let old_region = old_plan
                                    .viewport
                                    .unwrap_or(Damage::full(current.size.x, current.size.y));
                                let old_region = scene_repaint_hold
                                    .map_or(old_region, |(held, _)| {
                                        damage::union(held, old_region)
                                    });
                                scene_repaint_hold = Some((damage::union(old_region, content), 3));
                            }
                        }
                        let region = scene_repaint_hold
                            .map_or(content, |(held, _)| damage::union(held, content));
                        repaint = Some(Repaint {
                            changed: region,
                            region,
                            commands: current.plan.commands.clone(),
                        });
                    }
                }
                target
                    .set_extent(current.size.x, current.size.y)
                    .map_err(|e| format!("extent: {e:?}"))?;
                let mut result = Ok(());
                for upload in &current.plan.uploads {
                    if uploaded
                        .get(&upload.id)
                        .is_some_and(|image| Arc::ptr_eq(image, &upload.image))
                    {
                        continue;
                    }
                    let call_started = std::time::Instant::now();
                    result = target.upload_sprite_rgba8(
                        upload.id,
                        upload.image.width(),
                        upload.image.height(),
                        upload.image.as_raw(),
                    );
                    mailbox
                        .counters
                        .upload_call_us
                        .fetch_add(micros(call_started.elapsed()), Ordering::Relaxed);
                    if result.is_err() {
                        break;
                    }
                    uploaded.insert(upload.id, Arc::clone(&upload.image));
                    mailbox.counters.uploads.fetch_add(1, Ordering::Relaxed);
                    mailbox
                        .counters
                        .upload_bytes
                        .fetch_add(upload.image.as_raw().len() as u64, Ordering::Relaxed);
                }
                result
            }
            1 => {
                let result = if let Some(region) = repaint.as_ref().map(|r| r.region) {
                    match target.begin_gpu_frame_region(region) {
                        // The emulator's deferred paint backend cannot preserve
                        // untouched pixels. Invalid rejects before leasing, so
                        // switch this worker to complete frames immediately.
                        Err(Error::Invalid) => {
                            region_supported = false;
                            repaint = None;
                            target.begin_gpu_frame()
                        }
                        result => result,
                    }
                } else {
                    target.begin_gpu_frame()
                };
                if result.is_ok() {
                    // Even a canceled draw may have touched the backing store.
                    // Finish this revision before coalescing newer input, so
                    // its repaint region cannot be lost after a Busy retry.
                    acquired_frame = true;
                    mailbox.counters.begins.fetch_add(1, Ordering::Relaxed);
                }
                result
            }
            2 => {
                let call_started = std::time::Instant::now();
                let commands = repaint
                    .as_ref()
                    .map_or(current.plan.commands.as_slice(), |r| r.commands.as_slice());
                let result = if current.sky.is_some() {
                    #[cfg(target_os = "trueos")]
                    {
                        let renderer = sky_renderer.as_mut().unwrap();
                        let draw = if let Some(terrain) = current.terrain.as_deref() {
                            terrain_renderer.as_mut().unwrap().draw(target.render_target(), terrain)
                        } else if let Some(figure) = current.figure.as_deref() {
                            renderer.draw_figure(target.render_target(), 0, figure)
                        } else { renderer.draw(target.render_target(), 0) };
                        draw
                            .map_err(|e| if e == trueos::vgpu::ERR_BUSY { Error::Busy } else {
                                tracing::error!(target: "voxy_scene_contract", code = e, "Native sky submission failed");
                                Error::InvalidState
                            })
                    }
                    #[cfg(all(not(target_os = "trueos"), test))]
                    {
                        target.draw_sky(0)
                    }
                    #[cfg(all(not(target_os = "trueos"), not(test)))]
                    {
                        Err(Error::InvalidState)
                    }
                } else {
                    target.draw_sprite_commands(commands)
                };
                mailbox
                    .counters
                    .draw_call_us
                    .fetch_add(micros(call_started.elapsed()), Ordering::Relaxed);
                if result.is_ok() {
                    mailbox.counters.draws.fetch_add(1, Ordering::Relaxed);
                    let bcs = commands
                        .iter()
                        .filter(|command| command.backend == SpriteBackend::Bcs0)
                        .count() as u64;
                    mailbox
                        .counters
                        .bcs_commands
                        .fetch_add(bcs, Ordering::Relaxed);
                    mailbox
                        .counters
                        .compositor_commands
                        .fetch_add(commands.len() as u64 - bcs, Ordering::Relaxed);
                }
                result
            }
            3 => {
                let call_started = std::time::Instant::now();
                // Foreground receipts cannot represent a background commit or
                // a staged paired resize. Publish both capabilities normally;
                // the kernel retains responsibility for display retirement.
                let damage = repaint
                    .as_ref()
                    .map_or(Damage::full(current.size.x, current.size.y), |r| r.changed);
                let result = target.publish(damage);
                mailbox
                    .counters
                    .publish_call_us
                    .fetch_add(micros(call_started.elapsed()), Ordering::Relaxed);
                match result {
                    Ok(()) => {
                        if let Some(rgba) = current.sky {
                            if previous_sky.is_none() {
                                tracing::info!(target: "voxy_scene_contract", revision = current.revision,
                                    rgba8 = rgba, width = current.size.x, height = current.size.y,
                                    "Transparent scene published over display-engine sky");
                            }
                        }
                        mailbox
                            .published_revision
                            .store(current.revision, Ordering::Release);
                        mailbox
                            .counters
                            .publications
                            .fetch_add(1, Ordering::Relaxed);
                        tracing::trace!(
                            producer = name,
                            revision = current.revision,
                            "Native layer published"
                        );
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            _ => {
                if name == "scene" {
                    scene_repaint_hold = scene_repaint_hold.and_then(|(region, remaining)| {
                        (remaining > 1).then_some((region, remaining.saturating_sub(1)))
                    });
                }
                preceding_damage = repaint.as_ref().map(|r| r.changed);
                previous_sky = current.sky;
                #[cfg(target_os = "trueos")]
                { previous_had_geometry = current.figure.is_some() || current.terrain.is_some(); }
                previous = Some((current.size, current.plan.clone()));
                repaint = None;
                job = None;
                continue;
            }
        };
        match result {
            Ok(()) => phase += 1,
            Err(Error::Busy) => {
                let counter = match phase {
                    0 => &mailbox.counters.busy_upload,
                    1 => &mailbox.counters.busy_begin,
                    2 => &mailbox.counters.busy_draw,
                    _ => &mailbox.counters.busy_publish,
                };
                counter.fetch_add(1, Ordering::Relaxed);
                // A rejected draw cancels its write lease. Begin a fresh frame;
                // admission and publish Busy retain their own transaction state.
                if phase == 2 && current.sky.is_none() {
                    phase = 1;
                }
                mailbox.wait();
            }
            Err(error) => return Err(format!("phase {phase}: {error:?}")),
        }
    }
    Ok(())
}
