//! Exact Ubuntu headless shader, baked offline for physical Intel 8086:9A49.
//! Native VS consumes the 80-byte camera through a 96-byte padded constant range.
pub const PACKAGE_FNV1A64: u64 = 0x2EAA72CFCA1B1C77;
pub const DEVICE_ID: u16 = 0x9A49;
pub const VERTEX_STRIDE: usize = 32;
pub const CAMERA_BYTES: usize = 80;
pub const VS_PUSH_BYTES: usize = 96;
pub const WGSL: &[u8] = include_bytes!("render.wgsl");
pub const VERTEX: &[u8] = include_bytes!("shaders/tgl/vs.bin");
pub const FRAGMENT: &[u8] = include_bytes!("shaders/tgl/ps.bin");
pub const METADATA: &[u8] = include_bytes!("shaders/tgl/metadata.json");
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut digest = 0xcbf29ce484222325u64;
    let mut i = 0;
    while i < bytes.len() {
        digest = (digest ^ bytes[i] as u64).wrapping_mul(0x100000001b3);
        i += 1;
    }
    digest
}
const _: () = assert!(fnv1a64(WGSL) == PACKAGE_FNV1A64);

/// Retain the shader package inside the packed application without GPU calls.
pub fn embedded_package() -> (&'static [u8], &'static [u8], &'static [u8], &'static [u8]) {
    (WGSL, VERTEX, FRAGMENT, METADATA)
}
