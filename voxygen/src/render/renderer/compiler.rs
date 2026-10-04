use crate::render::RenderError;

pub(super) enum ShaderStage {
    Vertex,
    Fragment,
}

mod baked {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaderbin/catalog.rs"));
}

pub(super) struct PrecompiledCompiler {
    reg: regex::Regex,
    resolve_include: Box<dyn Fn(&str, &str) -> Result<String, String> + 'static>,
}

impl PrecompiledCompiler {
    pub(super) fn new(
        resolve_include: impl Fn(&str, &str) -> Result<String, String> + 'static,
    ) -> Result<Self, RenderError> {
        Ok(Self {
            reg: regex::Regex::new("(?mR)^#include +<(.+)>$").unwrap(),
            resolve_include: Box::new(resolve_include),
        })
    }

    pub(super) fn create_shader_module(
        &mut self,
        device: &wgpu::Device,
        source: &str,
        _stage: ShaderStage,
        name: &str,
    ) -> Result<wgpu::ShaderModule, RenderError> {
        use sha2::{Digest, Sha256};
        let sha256 = |bytes: &[u8]| -> String {
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        };
        let mut source = source.to_owned();
        for _ in 0..64 {
            let mut failure = None;
            let expanded = self
                .reg
                .replace_all(&source, |cap: &regex::Captures| {
                    match (self.resolve_include)(&cap[1], name) {
                        Ok(text) => text.trim_end_matches('\n').to_owned(),
                        Err(error) => {
                            failure = Some(error);
                            String::new()
                        }
                    }
                })
                .into_owned();
            if let Some(error) = failure {
                return Err(RenderError::CustomError(error));
            }
            if expanded == source {
                break;
            }
            source = expanded;
        }
        if self.reg.is_match(&source) {
            return Err(RenderError::CustomError(format!(
                "Unresolved shader includes: {name}"
            )));
        }
        let canonical = source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let hash = sha256(canonical.as_bytes());
        let (_, _, binary_hash, bytes) = baked::SHADERS
            .iter()
            .find(|(label, source_hash, _, _)| *label == name && *source_hash == hash)
            .ok_or_else(|| {
                RenderError::CustomError(format!(
                    "No precompiled shader for {name} and this rendering configuration; rebake \
                     shaderbin"
                ))
            })?;
        if sha256(bytes) != *binary_hash {
            return Err(RenderError::CustomError(format!(
                "Corrupt precompiled shader: {name}"
            )));
        }
        Ok(device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::util::make_spirv(bytes),
        }))
    }
}
