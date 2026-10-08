#!/usr/bin/env python3
"""Run native terrain ownership and camera checks through an inert host facade."""
from pathlib import Path
import os
import subprocess

env = os.environ.copy()
env['RUSTFLAGS'] = ('--cfg target_os="trueos" -Aexplicit_builtin_cfgs_in_flags '
                    '--check-cfg=cfg(target_os,values("trueos"))')
subprocess.run(['cargo', 'test', '--offline'], cwd=Path(__file__).resolve().parent,
               env=env, check=True)
