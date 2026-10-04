Voxygen client-only snapshot

Run: cargo run --manifest-path voxygen/Cargo.toml --no-default-features

Copied from /home/t4ce/Repos/veloren; the original is untouched.
Includes shared client dependencies, vendored patches and game assets.
No server, server-cli, world generation or rtsim crate is included.
Singleplayer is unavailable; default features are empty.
Only the game client is exposed as a binary. Development tools, tests,
examples and benchmarks are omitted. Optional client features are retained.
The sibling ../wgpu repository and TRUEOS-Blueprints API path are unchanged.
The source Git version is pinned in .cargo/config.toml.
Assets are retained in full to preserve data-driven client behavior.

Cargo sets VELOREN_ASSETS to this repository's assets directory because
automatic repository discovery requires a .git directory.

Each crate manifest is self-contained, with explicit dependencies and package
metadata. Profiles and dependency patches live in voxygen/Cargo.toml.
You can also run `cd voxygen && cargo run --no-default-features`.
