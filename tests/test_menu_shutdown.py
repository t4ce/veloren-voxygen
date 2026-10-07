#!/usr/bin/env python3
"""Host check of production stop polling and paired presenter destruction."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def block(source, marker):
    start = source.index(marker)
    brace = source.index('{', start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


presenter = (ROOT / 'src/ui/ice/renderer/presenter.rs').read_text()
run = (ROOT / 'src/run.rs').read_text()
terrain = (ROOT / 'src/scene/terrain/mod.rs').read_text()
main = (ROOT / 'src/main.rs').read_text()
assert main.index('ShutdownGuard::register()') < main.index('let tokio_runtime =')
poll_start = run.index('    fn about_to_wait(')
poll = run[poll_start:run.index('        if let Some(global_state)', poll_start)] + '    }'
sdk = ROOT.parent / 'TRUEOS-Blueprints/api/src/shutdown.rs'
code = r'''
#![allow(dead_code)]
extern crate self as v;
extern crate self as trueos;
extern crate self as tracing;
#[macro_export] macro_rules! info { ($($arg:tt)*) => {}; }
use std::{sync::{Arc, Mutex, Condvar, mpsc, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
#[path="__SDK__"] pub mod shutdown;
static REGISTERED: AtomicBool = AtomicBool::new(false);
static REQUESTED: AtomicBool = AtomicBool::new(false);
static ACK: AtomicBool = AtomicBool::new(false);
static JOBS: AtomicUsize = AtomicUsize::new(0);
pub mod bp_abi {
    pub unsafe fn trueos_cabi_blueprint_stop_control_v1(op:u32)->i32 {
        match op { 0=> if !super::REGISTERED.swap(true, super::Ordering::SeqCst) {0} else {-1},
        1=>super::REQUESTED.load(super::Ordering::SeqCst) as i32, _=>-1 }
    }
    pub unsafe fn trueos_cabi_blueprint_shutdown(_: *const u8, _: usize)->i32 {
        assert_eq!(super::JOBS.load(super::Ordering::SeqCst),0,"acknowledged live workers");
        super::ACK.store(true,super::Ordering::SeqCst); 0
    }
}
struct Job;
#[derive(Default)] struct ProducerCounters { queued_replacements: AtomicU64 }
__MAILBOX__
__MAILBOX_IMPL__
__PRODUCER__
__PRODUCER_DROP__
__PRESENTER__
__PRESENTER_DROP__
trait ActiveEventLoop { fn exit(&self); }
#[derive(Default)] struct EventLoop(AtomicBool);
impl ActiveEventLoop for EventLoop { fn exit(&self) { self.0.store(true,Ordering::SeqCst); } }
struct App;
impl App { __POLL__ }
struct SpriteWorkerResponse;
__PREPARATION__
__PREPARATION_IMPL__
__PREPARATION_DROP__
fn producer(own:Arc<Mailbox>,peer:Arc<Mailbox>,completed:Arc<AtomicUsize>)->Producer {
    JOBS.fetch_add(1,Ordering::SeqCst);
    let worker=Arc::clone(&own);
    let thread=thread::spawn(move|| {
        let deadline=Instant::now()+Duration::from_secs(2);
        while Instant::now()<deadline {
            if worker.stopped.load(Ordering::Acquire)&&peer.stopped.load(Ordering::Acquire) {
                completed.fetch_add(1,Ordering::SeqCst); break;
            }
            worker.wait();
        }
        JOBS.fetch_sub(1,Ordering::SeqCst);
    });
    Producer{mailbox:own,thread:Some(thread)}
}
#[test] fn menu_stop_exits_and_stops_both_workers_before_ack() {
    let event_loop=EventLoop::default();
    let mut app=App;
    {
        let _guard=shutdown::ShutdownGuard::register().unwrap();
        app.about_to_wait(&event_loop); assert!(!event_loop.0.load(Ordering::SeqCst));
        let a=Arc::new(Mailbox::default());let b=Arc::new(Mailbox::default());
        let completed=Arc::new(AtomicUsize::new(0));
        let (_,errors)=mpsc::channel();
        let presenter=LayeredPresenter{scene:producer(Arc::clone(&a),Arc::clone(&b),Arc::clone(&completed)),
          foreground:producer(b,a,Arc::clone(&completed)),errors};
        REQUESTED.store(true,Ordering::SeqCst);
        app.about_to_wait(&event_loop); assert!(event_loop.0.load(Ordering::SeqCst));
        drop(presenter); assert_eq!(completed.load(Ordering::SeqCst),2);
        // Unconsumed sprite preparation must also be joined, not detached.
        JOBS.fetch_add(1,Ordering::SeqCst);
        let preparation=PreparationThread(Some(thread::spawn(|| {
            thread::sleep(Duration::from_millis(10));
            JOBS.fetch_sub(1,Ordering::SeqCst); SpriteWorkerResponse
        })));
        drop(preparation); assert_eq!(JOBS.load(Ordering::SeqCst),0);
        assert!(!ACK.load(Ordering::SeqCst));
    }
    assert!(ACK.load(Ordering::SeqCst));
}
'''
for name, value in {
    'SDK': str(sdk),
    'MAILBOX': '#[derive(Default)]\n' + block(presenter, 'struct Mailbox {'),
    'MAILBOX_IMPL': block(presenter, 'impl Mailbox {'),
    'PRODUCER': block(presenter, 'struct Producer {'),
    'PRODUCER_DROP': block(presenter, 'impl Drop for Producer {'),
    'PRESENTER': block(presenter, 'pub(crate) struct LayeredPresenter {'),
    'PRESENTER_DROP': block(presenter, 'impl Drop for LayeredPresenter {'),
    'POLL': poll,
    'PREPARATION': terrain[terrain.index('struct PreparationThread('):terrain.index('impl PreparationThread {')].strip(),
    'PREPARATION_IMPL': block(terrain, 'impl PreparationThread {'),
    'PREPARATION_DROP': block(terrain, 'impl Drop for PreparationThread {'),
}.items():
    code = code.replace('__' + name + '__', value)
with tempfile.TemporaryDirectory(prefix='voxy-menu-stop-') as directory:
    source = Path(directory) / 'stop.rs'
    binary = Path(directory) / 'stop'
    source.write_text(code)
    subprocess.run(['rustc','--edition=2024','--test','--cfg','target_os="trueos"',
                    '-Aexplicit_builtin_cfgs_in_flags',str(source),'-o',str(binary)],check=True)
    subprocess.run([str(binary),'--test-threads=1'],check=True)
