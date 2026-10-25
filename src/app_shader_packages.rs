#![allow(unsafe_code)] // Linker section placement only; no guest execution.
// App-owned, SHA-256 authenticated native shaders. The Blueprint loader
// consumes this section before executing guest code; it is never kernel data.
#[cfg(target_os = "trueos")]
#[used]
#[unsafe(link_section = ".trueos.shaders")]
static APP_SHADER_PACKAGES: [u8; include_bytes!("../shaders/trueos/app-shaders.bundle").len()] =
    *include_bytes!("../shaders/trueos/app-shaders.bundle");
