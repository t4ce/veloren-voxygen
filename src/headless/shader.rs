//! Exact Ubuntu headless shader, validated offline for Intel TGL and ADL-S.
//! Native VS consumes the 80-byte camera through a 96-byte padded constant range.
pub const PACKAGE_FNV1A64: u64 = 0x2EAA72CFCA1B1C77;
pub const VENDOR_ID: u16 = 0x8086;
/// Physical device/revision pairs admitted by the kernel. Both compiler device
/// targets produce identical ISA, state metadata and constant-buffer layout.
pub const TARGETS: &[(u16, u8)] = &[(0x9A49, 0x01), (0x4680, 0x0C)];
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

/// Additive voxel-color atlas package. Native stages are reused artifacts,
/// composed by the kernel; metadata records their individual provenance.
pub mod textured {
    pub const PACKAGE_FNV1A64: u64 = 0xF84D_E655_632E_F102;
    pub const TARGETS: &[(u16, u8)] = &[(0x4680, 0x0C)];
    pub const WGSL: &[u8] = include_bytes!("render_textured.wgsl");
    pub const VERTEX: &[u8] = super::VERTEX;
    pub const FRAGMENT: &[u8] = include_bytes!("shaders/textured/ps.bin");
    pub const METADATA: &[u8] = include_bytes!("shaders/textured/metadata.json");

    const _: () = assert!(super::fnv1a64(WGSL) == PACKAGE_FNV1A64);
    const _: () = assert!(VERTEX.len() == 528 && FRAGMENT.len() == 160);

    pub fn embedded_package() -> (&'static [u8], &'static [u8], &'static [u8], &'static [u8]) {
        (WGSL, VERTEX, FRAGMENT, METADATA)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn textured_composite_embeds_exact_source_and_truthful_native_components() {
        let (wgsl, vertex, fragment, metadata) = textured::embedded_package();
        assert_eq!(fnv1a64(wgsl), textured::PACKAGE_FNV1A64);
        assert_eq!(
            wgsl,
            include_bytes!(
                "../../../TRUEOS-Blueprints/crates/trueos-wgpu/src/voxy_headless_textured.wgsl"
            )
        );
        assert_eq!(vertex, VERTEX);
        assert_eq!(vertex.len(), 528);
        assert_eq!(fragment.len(), 160);
        assert_eq!(textured::TARGETS, &[(0x4680, 0x0C)]);
        assert_eq!(
            sha256(wgsl),
            "2bcf3c3f6c8183725fd82180b4df3621be2c6ae8b7b4f4d4fdaeb33b58db04aa"
        );
        assert_eq!(
            sha256(vertex),
            "3f12e447eb4afb6197938b5e8f676ffe714639dedd66e3d03c2c11f43da537d2"
        );
        assert_eq!(
            sha256(fragment),
            "220dd52a7439b428f150f3e363ae253ec2c15cd5804a06c3092faacbfd46e7e2"
        );
        let metadata = std::str::from_utf8(metadata).unwrap();
        assert!(metadata.contains("\"new_native_compilation\": false"));
        assert!(metadata.contains("\"host_render_verified\": false"));
        assert!(metadata.contains("\"baremetal_render_verified\": false"));
        for bytes in [wgsl, vertex, fragment] {
            assert!(metadata.contains(&sha256(bytes)));
        }
        // Validate the actual shared shader, including the texture/sampler
        // resource contract, without creating a GPU or requiring visual proof.
        let module =
            wgpu::naga::front::wgsl::parse_str(std::str::from_utf8(wgsl).unwrap()).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}
