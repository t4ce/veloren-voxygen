#!/usr/bin/env python3
"""Regression: editing/focusing fields never emits clipboard-operation errors."""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SDK = ROOT.parent / 'TRUEOS-Blueprints/api/src/clipboard.rs'
SOURCE = r'''
extern crate alloc;
extern crate self as trueos;
extern crate self as v;
extern crate self as winit;
#[path = SDK_PATH] pub mod clipboard;
#[path = GAME_PATH] mod game;
mod connection {
    use crate::game::Clipboard;
    CONNECTION_IMPL
}
pub mod window { pub trait Window { fn native_id(&self) -> u32; } }
pub mod platform { pub mod trueos {
    pub trait WindowExtTrueOS { fn trueos_window_id(&self) -> u32; }
    impl WindowExtTrueOS for dyn crate::window::Window + '_ {
        fn trueos_window_id(&self) -> u32 { self.native_id() }
    }
} }
mod bp_abi {
    use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
    pub static FAIL_FOCUS: AtomicBool = AtomicBool::new(true);
    pub static CALLS: Mutex<Vec<(u32, u32, u32)>> = Mutex::new(Vec::new());
    pub unsafe fn trueos_cabi_clipboard_command_v1(window: u32, action: u32, kind: u32, _: *const u8, _: usize, _: *mut u8, _: usize) -> i32 {
        CALLS.lock().unwrap().push((window, action, kind));
        match action {
            2 if kind == 0 || FAIL_FOCUS.load(Ordering::SeqCst) => -8,
            2 => 0,
            _ if kind == 2 => -12,
            _ => -8,
        }
    }
}
#[test] fn focus_is_silent_and_real_clipboard_attempts_report_errors() {
    let mut unbound = game::Clipboard::default();
    unbound.focus(game::Kind::Text);
    unbound.focus(game::Kind::Password);
    unbound.blur();
    assert_eq!(unbound.take_message(), None);
    assert!(bp_abi::CALLS.lock().unwrap().is_empty());
    assert!(!unbound.copy(game::Kind::Password, "secret".into()));
    assert!(unbound.take_message().unwrap().contains("unavailable"));

    struct Window;
    impl window::Window for Window { fn native_id(&self) -> u32 { 42 } }
    let mut clipboard = game::Clipboard::connect(&Window);
    clipboard.focus(game::Kind::Text);
    clipboard.focus(game::Kind::Text);
    assert_eq!(clipboard.take_message(), None);
    assert_eq!(*bp_abi::CALLS.lock().unwrap(), [(42, 2, 1), (42, 2, 1)]);
    bp_abi::FAIL_FOCUS.store(false, std::sync::atomic::Ordering::SeqCst);
    clipboard.focus(game::Kind::Text);
    clipboard.blur();
    assert_eq!(clipboard.take_message(), None);
    assert!(!clipboard.copy(game::Kind::Text, "ordinary".into()));
    assert!(clipboard.take_message().unwrap().contains("unavailable"));
    assert!(!clipboard.copy(game::Kind::Password, "secret".into()));
    assert!(clipboard.take_message().unwrap().contains("authenticated user configuration"));
    assert_eq!(clipboard.paste(game::Kind::Password), None);
    assert!(clipboard.take_message().unwrap().contains("authenticated user configuration"));
    assert!(bp_abi::CALLS.lock().unwrap().iter().all(|(window, _, _)| *window == 42));
}
'''

def main():
    adapter = (ROOT / 'src/ui/ice/winit.rs').read_text()
    start = adapter.index('impl Clipboard {')
    depth = 0
    end = None
    for i in range(adapter.index('{', start), len(adapter)):
        if adapter[i] == '{': depth += 1
        elif adapter[i] == '}':
            depth -= 1
            if depth == 0:
                end = i + 1
                break
    assert end is not None
    source = SOURCE.replace('SDK_PATH', json.dumps(str(SDK))).replace('GAME_PATH', json.dumps(str(ROOT / 'src/clipboard.rs'))).replace('CONNECTION_IMPL', adapter[start:end])
    with tempfile.TemporaryDirectory(prefix='voxy-clipboard-focus-') as directory:
        root = Path(directory)
        (root / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', '--cfg', 'target_os="trueos"', '-Aexplicit_builtin_cfgs_in_flags', str(root / 'tests.rs'), '-o', str(root / 'tests')], check=True)
        subprocess.run([str(root / 'tests')], check=True)

if __name__ == '__main__':
    main()
