#!/usr/bin/env bash
# Ubuntu geometry-only game client. All arguments are passed to the client.
# Example: tools/run-minimal-wgpu.sh --server HOST:14004 --username NAME
# Offline GPU validation: tools/run-minimal-wgpu.sh --render-smoke-test
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
exec cargo run --locked --no-default-features --features minimal-wgpu -- "$@"
