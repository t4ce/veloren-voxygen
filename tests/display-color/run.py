#!/usr/bin/env python3
"""Test production TRUEOS ramp upload caching with a mocked scene ABI."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = r'''
#![allow(dead_code)]
extern crate self as trueos;
#[derive(Debug)] pub enum RenderError { CustomError(String) }
pub mod ui4_scene {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub static CALLS: AtomicUsize = AtomicUsize::new(0);
    pub static FAIL: AtomicBool = AtomicBool::new(false);
    pub fn set_display_gamma_ramp(window: u32, ramp: &[u16; 768]) -> Result<(), i32> {
        assert_eq!(window, 17);
        assert_eq!(ramp[0], 0);
        CALLS.fetch_add(1, Ordering::Relaxed);
        if FAIL.load(Ordering::Relaxed) { Err(-4) } else { Ok(()) }
    }
}
#[path = "__MODULE__"] mod display_color;
#[test]
fn uploads_only_changed_curves_and_retries_failed_updates() {
    use std::sync::atomic::Ordering;
    use display_color::{Curve, DisplayColor};
    let mut display = DisplayColor::new(17);
    let curve = Curve { exponent: 1.14, gain: [1.0; 3], srgb: true };
    display.apply(curve).unwrap();
    display.apply(curve).unwrap();
    assert_eq!(ui4_scene::CALLS.load(Ordering::Relaxed), 1);
    let next = Curve { gain: [0.2, 0.2, 0.8], ..curve };
    ui4_scene::FAIL.store(true, Ordering::Relaxed);
    assert!(display.apply(next).is_err());
    ui4_scene::FAIL.store(false, Ordering::Relaxed);
    display.apply(next).unwrap();
    display.apply(next).unwrap();
    assert_eq!(ui4_scene::CALLS.load(Ordering::Relaxed), 3);
    display.apply(Curve { srgb: false, ..next }).unwrap();
    assert_eq!(ui4_scene::CALLS.load(Ordering::Relaxed), 4);
}
'''.replace('__MODULE__', str(ROOT / 'src/render/display_color.rs'))
with tempfile.TemporaryDirectory(prefix='voxy-display-color-') as tmp:
    path = Path(tmp) / 'test.rs'
    path.write_text(source)
    binary = Path(tmp) / 'test'
    subprocess.run(['rustc', '--edition=2024', '--test', '--cfg', 'target_os="trueos"',
                    '-Aexplicit_builtin_cfgs_in_flags', str(path), '-o', str(binary)], check=True)
    subprocess.run([str(binary)], check=True)

# Check the shipped SPIR-V, rather than merely checking GLSL preprocessor text.
import collections
import struct


def extended_ops(path):
    data = path.read_bytes()
    words = struct.unpack('<' + str(len(data) // 4) + 'I', data)
    counts = collections.Counter()
    index = 5
    while index < len(words):
        size, opcode = words[index] >> 16, words[index] & 65535
        assert size > 0
        if opcode == 12:  # OpExtInst: GLSL.std.450 operation number
            counts[words[index + 4]] += 1
        index += size
    return counts


for profile in ('minimal', 'cheap', 'map'):
    software = extended_ops(ROOT / f'shaderbin/postprocess-frag.{profile}.spv')
    display = extended_ops(ROOT / f'shaderbin/postprocess-frag.{profile}.display.spv')
    assert software[26] - display[26] == 1, f'Gamma Pow still present: {profile}'
    assert software[27] == display[27] == 1, f'Exposure changed: {profile}'
print('Verified native gamma Pow removal and preserved exposure in all 3 packed profiles')
