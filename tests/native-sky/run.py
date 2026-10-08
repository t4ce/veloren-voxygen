#!/usr/bin/env python3
"""Exercise production NativeSky against the kernel's actual open predicate.

The host facade records ownership/order only; it executes no GPU work.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def block(source, marker):
    start = source.index(marker)
    brace = source.index('{', start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


sdk = (ROOT.parent / 'TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs').read_text()
kernel = (ROOT.parent / 'TRUEOS/src/gpu/vgpu.rs').read_text()
predicate = block(kernel[kernel.index('pub(crate) fn open('):],
                  'if !capabilities.contains(Capabilities::BUFFER)')
code = r'''
#![allow(dead_code)]
extern crate self as trueos;
#[path="__SKY__"] mod sky;
use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static FAIL_QUEUE: AtomicBool = AtomicBool::new(false);
pub mod vgpu {
    use super::*;
    __CAPABILITIES__
    __CAPABILITIES_IMPL__
    #[derive(Debug)] enum VgpuError { PermissionDenied }
    fn admit(capabilities: Capabilities) -> Result<(), VgpuError> {
        __PREDICATE__
        Ok(())
    }
    pub const ERR_BUSY: i32 = -16;
    pub const ERR_IO: i32 = -5;
    #[derive(Copy, Clone)] pub struct Device;
    #[derive(Copy, Clone)] pub struct Queue;
    pub enum QueueClass { Render }
    pub struct Surface;
    pub struct Timeline { pub value: u64 }
    impl Device {
        pub fn open(caps: Capabilities) -> Result<Self, i32> {
            admit(caps).map_err(|_| -13)?;
            assert!(caps.contains(Capabilities::RENDER.union(Capabilities::PRESENT)));
            EVENTS.lock().unwrap().push("open"); Ok(Self)
        }
        pub fn create_queue(self, _: QueueClass) -> Result<Queue, i32> {
            EVENTS.lock().unwrap().push("queue");
            if FAIL_QUEUE.load(Ordering::SeqCst) { Err(-5) } else { Ok(Queue) }
        }
        pub fn acquire_ui4_surface(self, target: u32) -> Result<Surface, i32> {
            assert_eq!(target, 163841); EVENTS.lock().unwrap().push("import-backframe"); Ok(Surface)
        }
        pub fn submit_ui4_clear(self, _: Queue, _: Surface, rgba: u32) -> Result<Timeline, i32> {
            assert_eq!(rgba.to_le_bytes()[3], 255);
            EVENTS.lock().unwrap().push("gpu-clear"); Ok(Timeline { value: 17 })
        }
        pub fn wait(self, _: Queue, value: u64) -> Result<(), i32> {
            assert_eq!(value, 17); EVENTS.lock().unwrap().push("completion"); Ok(())
        }
        pub fn destroy_queue(self, _: Queue) -> Result<(), i32> {
            EVENTS.lock().unwrap().push("destroy-queue"); Ok(())
        }
        pub fn close(self) -> Result<(), i32> {
            EVENTS.lock().unwrap().push("close"); Ok(())
        }
    }
}
#[test] fn sky_opens_with_kernel_required_capabilities_and_completes_before_returning() {
    EVENTS.lock().unwrap().clear(); FAIL_QUEUE.store(false, Ordering::SeqCst);
    let sky = sky::NativeSky::open().unwrap();
    sky.draw(163841, sky::rgba8(-1.0)).unwrap();
    drop(sky);
    assert_eq!(*EVENTS.lock().unwrap(),
        ["open", "queue", "import-backframe", "gpu-clear", "completion", "destroy-queue", "close"]);
}
#[test] fn queue_failure_closes_the_open_device() {
    EVENTS.lock().unwrap().clear(); FAIL_QUEUE.store(true, Ordering::SeqCst);
    assert!(sky::NativeSky::open().is_err());
    FAIL_QUEUE.store(false, Ordering::SeqCst);
    assert_eq!(*EVENTS.lock().unwrap(), ["open", "queue", "close"]);
}
'''
for name, value in {
    'SKY': str(ROOT / 'src/render/minimal_sky.rs'),
    'CAPABILITIES': '#[derive(Copy, Clone)]\n' +
        sdk[sdk.index('pub struct Capabilities('):].split(';', 1)[0] + ';',
    'CAPABILITIES_IMPL': block(sdk, 'impl Capabilities {'),
    'PREDICATE': predicate,
}.items():
    code = code.replace('__' + name + '__', value)
with tempfile.TemporaryDirectory(prefix='voxy-native-sky-') as directory:
    source = Path(directory) / 'sky.rs'
    binary = Path(directory) / 'sky'
    source.write_text(code)
    subprocess.run(['rustc', '--edition=2024', '--test', '--cfg', 'target_os="trueos"',
                    '-Aexplicit_builtin_cfgs_in_flags', str(source), '-o', str(binary)], check=True)
    subprocess.run([str(binary), '--test-threads=1'], check=True)
