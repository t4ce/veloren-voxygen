//! Paired producers. No GPU admission/fence wait runs on the input thread.
//! The broker coordinates plane placement and paired resize epochs. Neither
//! producer gates the other on SURFLIVE: a paired resize needs both publications
//! before either replacement can become display-live.
use super::bcs::{FramePlan, LayerPlan};
use std::{
    collections::{HashSet, VecDeque},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use trueos::ui4_solara_text::{Damage, Error, SceneTarget, SpriteCommand};
use vek::Vec2;

struct Job {
    revision: u64,
    size: Vec2<u32>,
    plan: LayerPlan,
}
#[derive(Default)]
struct Mailbox {
    latest: Mutex<Option<Job>>,
    wake: Condvar,
    stopped: AtomicBool,
}
impl Mailbox {
    fn submit(&self, job: Job) {
        *self.latest.lock().unwrap() = Some(job);
        self.wake.notify_one();
    }
    fn wait(&self) {
        let guard = self.latest.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let _ = self
            .wake
            .wait_timeout(guard, Duration::from_millis(8))
            .unwrap();
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
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
        });
        self.foreground.mailbox.submit(Job {
            revision,
            size,
            plan: plan.foreground,
        });
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
    let mut uploaded = HashSet::new();
    let mut previous: Option<(Vec2<u32>, Vec<SpriteCommand>)> = None;
    let mut job: Option<Job> = None;
    let mut phase = 0;
    let mut receipts = VecDeque::new();
    let mut first = true;
    while !mailbox.stopped.load(Ordering::Acquire) {
        // Receipts can be superseded during resize and are never admission
        // tokens. Only begin/publish may apply producer backpressure.
        if first {
            for &serial in &receipts {
                if target.was_presented(serial).unwrap_or(false) {
                    tracing::info!(producer = name, serial, "Native layer reached SURFLIVE");
                    first = false;
                    break;
                }
            }
            if !first {
                receipts.clear();
            }
        }
        if job.is_none() {
            job = mailbox.latest.lock().unwrap().take();
            phase = 0;
        }
        if phase <= 1 {
            if let Some(latest) = mailbox.latest.lock().unwrap().take() {
                job = Some(latest);
                phase = 0;
            }
        }
        let Some(current) = job.as_ref() else {
            mailbox.wait();
            continue;
        };
        if phase == 0
            && previous.as_ref().is_some_and(|(size, commands)| {
                *size == current.size && *commands == current.plan.commands
            })
        {
            job = None;
            continue;
        }
        let result = match phase {
            0 => {
                target
                    .set_extent(current.size.x, current.size.y)
                    .map_err(|e| format!("extent: {e:?}"))?;
                let mut result = Ok(());
                for upload in &current.plan.uploads {
                    if uploaded.contains(&upload.id) {
                        continue;
                    }
                    result = target.upload_sprite_rgba8(
                        upload.id,
                        upload.image.width(),
                        upload.image.height(),
                        upload.image.as_raw(),
                    );
                    if result.is_err() {
                        break;
                    }
                    uploaded.insert(upload.id);
                }
                result
            }
            1 => target.begin_gpu_frame(),
            2 => target.draw_sprite_commands(&current.plan.commands),
            3 => match target.publish_tracked(Damage::full(current.size.x, current.size.y)) {
                Ok(serial) => {
                    tracing::trace!(
                        producer = name,
                        revision = current.revision,
                        serial,
                        "Native layer published"
                    );
                    if first {
                        receipts.push_back(serial);
                        if receipts.len() > 16 {
                            receipts.pop_front();
                        }
                    }
                    Ok(())
                }
                Err(error) => Err(error),
            },
            _ => {
                previous = Some((current.size, current.plan.commands.clone()));
                job = None;
                continue;
            }
        };
        match result {
            Ok(()) => phase += 1,
            Err(Error::Busy) => {
                // A rejected draw cancels its write lease. Begin a fresh frame;
                // admission and publish Busy retain their own transaction state.
                if phase == 2 {
                    phase = 1;
                }
                mailbox.wait();
            }
            Err(error) => return Err(format!("phase {phase}: {error:?}")),
        }
    }
    Ok(())
}
