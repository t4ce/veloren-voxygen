#!/usr/bin/env python3
"""Run native terrain ownership and camera checks through an inert host facade."""
from pathlib import Path
import os
import subprocess

root = Path(__file__).resolve().parent
source = (root / '../../src/render/flat_cloud_native.rs').read_text()
(root / 'target').mkdir(exist_ok=True)
(root / 'target/cloud_transport.rs').write_text(
    'pub struct Frame { pub pixels: std::sync::Arc<[u8]>, pub width: u32, pub height: u32 }\n'
    + source[source.index('pub(crate) struct NativeClouds'):])
env = os.environ.copy()
env['RUSTFLAGS'] = ('--cfg target_os="trueos" -Aexplicit_builtin_cfgs_in_flags '
                    '--check-cfg=cfg(target_os,values("trueos"))')
subprocess.run(['cargo', 'test', '--offline'], cwd=Path(__file__).resolve().parent,
               env=env, check=True)
