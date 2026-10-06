#!/usr/bin/env python3
"""Compile the real TRUEOS-facing game adapter and SDK facade against a mock ABI."""
from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SDK = ROOT.parent / 'TRUEOS-Blueprints/api/src/clipboard.rs'
SOURCE = r'''
extern crate alloc;
extern crate self as trueos;
extern crate self as v;
#[path = SDK_PATH] pub mod clipboard;
#[path = GAME_PATH] mod game;
mod bp_abi {
    use std::sync::Mutex;
    pub static CALLS: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
    pub unsafe fn trueos_cabi_clipboard_command_v1(_: u32, action: u32, kind: u32, _: *const u8, _: usize, _: *mut u8, _: usize) -> i32 {
        CALLS.lock().unwrap().push((action, kind));
        if kind == 2 && action != 2 { -12 } else { 0 }
    }
}
#[test] fn secure_denial_is_visible_and_never_retried_as_plain_text() {
    bp_abi::CALLS.lock().unwrap().clear();
    let mut clipboard = game::Clipboard::for_frame(7);
    clipboard.focus(game::Kind::Password);
    assert!(!clipboard.copy(game::Kind::Password, "secret".into()));
    assert!(clipboard.take_message().unwrap().contains("authenticated user configuration"));
    assert_eq!(clipboard.paste(game::Kind::Password), None);
    assert!(clipboard.take_message().unwrap().contains("authenticated user configuration"));
    assert_eq!(*bp_abi::CALLS.lock().unwrap(), [(2, 2), (1, 2), (3, 2)]);
    clipboard.focus(game::Kind::Text);
    assert!(clipboard.copy(game::Kind::Text, "ordinary".into()));
    assert_eq!(clipboard.take_message(), None);
    clipboard.blur();
    let count = bp_abi::CALLS.lock().unwrap().len();
    clipboard.blur();
    assert_eq!(bp_abi::CALLS.lock().unwrap().len(), count);
}
#[test] fn an_unbound_game_ui_fails_closed() {
    bp_abi::CALLS.lock().unwrap().clear();
    let mut clipboard = game::Clipboard::default();
    assert!(!clipboard.copy(game::Kind::Password, "secret".into()));
    assert_eq!(clipboard.paste(game::Kind::Password), None);
    assert!(clipboard.take_message().unwrap().contains("unavailable"));
    assert!(bp_abi::CALLS.lock().unwrap().is_empty());
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix='voxy-clipboard-tests-') as directory:
        root = Path(directory)
        source = SOURCE.replace('SDK_PATH', json.dumps(str(SDK))).replace('GAME_PATH', json.dumps(str(ROOT / 'src/clipboard.rs')))
        (root / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', '--cfg', 'target_os="trueos"', '-Aexplicit_builtin_cfgs_in_flags', str(root / 'tests.rs'), '-o', str(root / 'tests')], check=True)
        subprocess.run([str(root / 'tests'), '--test-threads=1'], check=True)

if __name__ == '__main__':
    main()
