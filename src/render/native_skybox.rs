//! Fixed native contract for the shipped BareMinimum skybox.
//!
//! These are compiled from the original bring-up SPIR-V, not a replacement
//! sky shader. Runtime admission and UI4 submission are still pending.
//! The compiler promotes two ranges of the existing Globals UBO to constants.

pub const VERTEX_STRIDE: usize = 12;
pub const VS_GLOBALS_OFFSET: usize = 128;
pub const VS_PUSH_BYTES: usize = 96;
pub const PS_GLOBALS_OFFSET: usize = 256;
pub const PS_PUSH_BYTES: usize = 32;
pub const REQUIRED_GLOBALS_BYTES: usize = PS_GLOBALS_OFFSET + PS_PUSH_BYTES;

pub const VERTEX: &[u8] = include_bytes!("../../shaderbin/native/skybox/skybox.vs.simd8.bin");
pub const FRAGMENT: &[u8] = include_bytes!("../../shaderbin/native/skybox/skybox.ps.simd16.bin");
pub const METADATA: &str = include_str!("../../shaderbin/native/skybox/metadata.json");

/// Preserve the compiler's promoted ranges, including 32-byte padding.
/// VS: all_mat, cam_pos, then the padded part of the following Globals field.
/// PS: time_of_day and sun_dir. No CPU approximation of sky colour is needed.
pub fn constants(globals: &[u8]) -> Result<(&[u8], &[u8]), &'static str> {
    if globals.len() < REQUIRED_GLOBALS_BYTES {
        return Err("skybox requires the original Globals uniform through sun_dir");
    }
    Ok((
        &globals[VS_GLOBALS_OFFSET..VS_GLOBALS_OFFSET + VS_PUSH_BYTES],
        &globals[PS_GLOBALS_OFFSET..PS_GLOBALS_OFFSET + PS_PUSH_BYTES],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_push_ranges_match_the_original_spirv_globals_members() {
        let bytes = include_bytes!("../../shaderbin/skybox-vert.trueos-bringup.spv");
        let module = wgpu::naga::front::spv::parse_u8_slice(bytes, &Default::default()).unwrap();
        // Performance-baked SPIR-V strips debug names. Find the actual uniform
        // binding instead of relying on OpName/OpMemberName surviving the bake.
        let (_, uniform) = module
            .global_variables
            .iter()
            .find(|(_, var)| {
                var.binding
                    .as_ref()
                    .is_some_and(|binding| binding.group == 0 && binding.binding == 0)
            })
            .unwrap();
        let globals = &module.types[uniform.ty];
        let wgpu::naga::TypeInner::Struct { members, .. } = &globals.inner else {
            panic!("Globals must be a struct");
        };
        assert_eq!(members[2].offset as usize, VS_GLOBALS_OFFSET); // all_mat
        assert_eq!(members[3].offset as usize, VS_GLOBALS_OFFSET + 64); // cam_pos
        assert_eq!(members[7].offset as usize, PS_GLOBALS_OFFSET); // time_of_day
        assert_eq!(members[8].offset as usize + 16, REQUIRED_GLOBALS_BYTES); // sun_dir
    }

    #[test]
    fn uploads_the_captured_ranges_without_repacking_or_losing_padding() {
        let globals: Vec<_> = (0..512).map(|i| (i % 251) as u8).collect();
        let (vs, ps) = constants(&globals).unwrap();
        assert_eq!(vs, &globals[128..224]);
        assert_eq!(ps, &globals[256..288]);
        assert!(constants(&globals[..287]).is_err());
        assert!(VERTEX.len().is_multiple_of(8));
        assert!(FRAGMENT.len().is_multiple_of(8));
        assert!(METADATA.contains("\"native_execution_admitted\": false"));
    }
}
