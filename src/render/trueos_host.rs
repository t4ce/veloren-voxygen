//! Host resource and command bridge for the explicitly restricted terrain-line stage.
//! Original shader modules/textures/passes are metadata only. Only packed terrain
//! draws are extracted, clipped, and submitted to the native line executor.
#![allow(unused_variables, dead_code)]
#![expect(unsafe_code)]
use std::{
    collections::BTreeMap,
    ops::Range,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use wgpu::WriteOnly;
use wgpu::custom::*;

pub const MAX_LINE_VERTICES: usize = 65_536;
#[derive(Debug, Default)]
pub struct Host {
    lines: Mutex<Vec<[f32; 3]>>,
    spare_lines: Mutex<Vec<[f32; 3]>>,
    serial: AtomicU64,
    buffer_bytes: Arc<AtomicU64>,
}
impl Host {
    pub fn take_lines(&self) -> Vec<[f32; 3]> {
        std::mem::take(&mut *self.lines.lock().unwrap())
    }
    pub fn recycle_lines(&self, mut lines: Vec<[f32; 3]>) {
        lines.clear();
        let mut spare = self.spare_lines.lock().unwrap();
        if lines.capacity() > spare.capacity() {
            *spare = lines;
        }
    }
}
pub fn device() -> (DispatchDevice, DispatchQueue, Arc<Host>) {
    let h = Arc::new(Host::default());
    (
        DispatchDevice::custom(Device(h.clone())),
        DispatchQueue::custom(Queue(h.clone())),
        h,
    )
}
pub fn features() -> wgpu::Features {
    wgpu::Features::DEPTH_CLIP_CONTROL
        | wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER
        | wgpu::Features::IMMEDIATES
}
pub fn limits() -> wgpu::Limits {
    wgpu::Limits {
        max_texture_dimension_2d: 8192,
        max_immediate_size: 128,
        ..wgpu::Limits::default()
    }
}
pub fn info() -> wgpu::AdapterInfo {
    wgpu::AdapterInfo {
        name: "TRUEOS terrain-line host extraction".into(),
        vendor: 0,
        device: 0,
        device_type: wgpu::DeviceType::Cpu,
        driver: "TRUEOS direct line bringup".into(),
        driver_info: "Original GPU passes explicitly gated".into(),
        backend: wgpu::Backend::Noop,
        device_pci_bus_id: String::new(),
        subgroup_min_size: 0,
        subgroup_max_size: 0,
        transient_saves_memory: None,
        limit_bucket: None,
    }
}
#[derive(Debug)]
struct Device(Arc<Host>);
#[derive(Debug)]
struct Queue(Arc<Host>);
#[derive(Debug, Clone)]
struct Buffer {
    bytes: Arc<BufferBytes>,
    usage: wgpu::BufferUsages,
    size: u64,
}
#[derive(Debug)]
struct BufferBytes {
    data: Mutex<Vec<u8>>,
    budget: Arc<AtomicU64>,
    size: u64,
}
impl std::ops::Deref for BufferBytes {
    type Target = Mutex<Vec<u8>>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl Drop for BufferBytes {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.size, Ordering::Relaxed);
    }
}
fn buffer_bytes(size: u64, budget: Arc<AtomicU64>) -> Arc<BufferBytes> {
    budget
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(size).filter(|n| *n <= 256 * 1024 * 1024)
        })
        .expect("terrain-line host resource budget exceeded (256MiB)");
    Arc::new(BufferBytes {
        data: Mutex::new(vec![0; size as usize]),
        budget,
        size,
    })
}
#[derive(Debug)]
struct Mapping {
    parent: Arc<BufferBytes>,
    offset: usize,
    bytes: Vec<u8>,
}
impl Drop for Mapping {
    fn drop(&mut self) {
        let mut p = self.parent.lock().unwrap();
        p[self.offset..self.offset + self.bytes.len()].copy_from_slice(&self.bytes);
    }
}
#[derive(Debug, Clone)]
pub struct HostTexture {
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    dimension: wgpu::TextureDimension,
    mips: u32,
    samples: u32,
}
impl HostTexture {
    pub fn frame(width: u32, height: u32) -> Self {
        Self {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            dimension: wgpu::TextureDimension::D2,
            mips: 1,
            samples: 1,
        }
    }
}
#[derive(Debug)]
struct Empty;
#[derive(Debug, Clone)]
struct Binding {
    buffer: Buffer,
    offset: usize,
    size: usize,
}
#[derive(Debug, Clone)]
struct Bind(BTreeMap<u32, Binding>);
#[derive(Debug)]
struct Pipeline {
    terrain: bool,
}
#[derive(Debug)]
struct Encoder {
    lines: Arc<Mutex<Vec<[f32; 3]>>>,
}
#[derive(Debug)]
struct Commands(Mutex<Vec<[f32; 3]>>);
#[derive(Debug)]
struct Pass {
    lines: Arc<Mutex<Vec<[f32; 3]>>>,
    terrain: bool,
    groups: BTreeMap<u32, Bind>,
    vertex: Option<Binding>,
    index: Option<(Binding, wgpu::IndexFormat)>,
}
impl Drop for Pass {
    fn drop(&mut self) {}
}
impl BindGroupLayoutInterface for Empty {}
impl PipelineLayoutInterface for Empty {}
impl BindGroupInterface for Bind {}
impl TextureViewInterface for Empty {}
impl SamplerInterface for Empty {}
impl CommandBufferInterface for Commands {}
impl RenderPipelineInterface for Pipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(Empty)
    }
}
impl ShaderModuleInterface for Empty {
    fn get_compilation_info(&self) -> Pin<Box<dyn ShaderCompilationInfoFuture>> {
        Box::pin(std::future::ready(wgpu::CompilationInfo {
            messages: vec![],
        }))
    }
}
fn binding(buffer: &DispatchBuffer, offset: u64, size: Option<u64>) -> Binding {
    let buffer = buffer
        .as_custom::<Buffer>()
        .expect("foreign buffer")
        .clone();
    let size = size.unwrap_or(buffer.size.checked_sub(offset).expect("buffer offset"));
    assert!(
        offset
            .checked_add(size)
            .is_some_and(|end| end <= buffer.size)
    );
    Binding {
        buffer,
        offset: offset as usize,
        size: size as usize,
    }
}
fn write(buffer: &DispatchBuffer, offset: u64, data: &[u8]) {
    let b = binding(buffer, offset, Some(data.len() as u64));
    b.buffer.bytes.lock().unwrap()[b.offset..b.offset + b.size].copy_from_slice(data);
}

impl DeviceInterface for Device {
    fn features(&self) -> wgpu::Features {
        features()
    }
    fn limits(&self) -> wgpu::Limits {
        limits()
    }
    fn adapter_info(&self) -> wgpu::AdapterInfo {
        info()
    }
    fn create_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
        shader_bound_checks: wgpu::ShaderRuntimeChecks,
    ) -> DispatchShaderModule {
        DispatchShaderModule::custom(Empty)
    }
    unsafe fn create_shader_module_passthrough(
        &self,
        desc: &wgpu::ShaderModuleDescriptorPassthrough<'_>,
    ) -> DispatchShaderModule {
        DispatchShaderModule::custom(Empty)
    }
    fn create_bind_group_layout(
        &self,
        desc: &wgpu::BindGroupLayoutDescriptor<'_>,
    ) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(Empty)
    }
    fn create_bind_group(&self, desc: &wgpu::BindGroupDescriptor<'_>) -> DispatchBindGroup {
        let mut bindings = BTreeMap::new();
        for e in desc.entries {
            if let wgpu::BindingResource::Buffer(ref b) = e.resource {
                let hb = b
                    .buffer
                    .as_custom::<Buffer>()
                    .expect("foreign binding buffer")
                    .clone();
                let size = b.size.map(|s| s.get()).unwrap_or(hb.size - b.offset);
                assert!(b.offset.checked_add(size).is_some_and(|end| end <= hb.size));
                bindings.insert(
                    e.binding,
                    Binding {
                        buffer: hb,
                        offset: b.offset as usize,
                        size: size as usize,
                    },
                );
            }
        }
        DispatchBindGroup::custom(Bind(bindings))
    }
    fn create_pipeline_layout(
        &self,
        desc: &wgpu::PipelineLayoutDescriptor<'_>,
    ) -> DispatchPipelineLayout {
        DispatchPipelineLayout::custom(Empty)
    }
    fn create_render_pipeline(
        &self,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> DispatchRenderPipeline {
        assert!(
            matches!(
                desc.label,
                Some(
                    "Terrain pipeline"
                        | "LoD object pipeline"
                        | "UI pipeline"
                        | "Premultiply alpha pipeline"
                        | "Trail pipeline"
                        | "Fluid pipeline"
                        | "Figure pipeline"
                        | "Sprite pipeline"
                        | "Debug pipeline"
                        | "Skybox pipeline"
                        | "Directed shadow figure pipeline"
                        | "Directed shadow pipeline"
                        | "Point shadow pipeline"
                        | "Directed shadow debug pipeline"
                        | "Clouds pipeline"
                        | "Rope pipeline"
                        | "Blit pipeline"
                        | "Rain occlusion figure pipeline"
                        | "Rain occlusion pipeline"
                        | "Post process pipeline"
                        | "Particle pipeline"
                        | "Lod terrain pipeline"
                        | "Bloom downsample filtered pipeline"
                        | "Bloom downsample pipeline"
                        | "Bloom upsample pipeline"
                )
            ),
            "Unknown pipeline outside documented terrain-line gates: {:?}",
            desc.label
        );
        let terrain = desc.label == Some("Terrain pipeline");
        if terrain {
            assert_eq!(desc.vertex.buffers.len(), 1);
            let layout = desc.vertex.buffers[0]
                .as_ref()
                .expect("terrain vertex layout");
            assert_eq!(layout.step_mode, wgpu::VertexStepMode::Vertex);
            assert_eq!(layout.attributes.len(), 2);
            for (index, attr) in layout.attributes.iter().enumerate() {
                assert_eq!(attr.shader_location, index as u32);
                assert_eq!(attr.offset, index as u64 * 4);
                assert_eq!(attr.format, wgpu::VertexFormat::Uint32);
            }
            assert_eq!(
                desc.primitive.topology,
                wgpu::PrimitiveTopology::TriangleList
            );

            assert_eq!(
                desc.vertex.buffers[0]
                    .as_ref()
                    .expect("terrain vertex layout")
                    .array_stride,
                8
            );
        }
        tracing::debug!(target:"voxy_scene_contract",label=?desc.label,terrain,"Host pipeline registered; original shader execution excluded");
        DispatchRenderPipeline::custom(Pipeline { terrain })
    }
    fn create_mesh_pipeline(
        &self,
        desc: &wgpu::MeshPipelineDescriptor<'_>,
    ) -> DispatchRenderPipeline {
        panic!("TRUEOS terrain-line stage: create_mesh_pipeline is out-gated")
    }
    fn create_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> DispatchComputePipeline {
        panic!("TRUEOS terrain-line stage: create_compute_pipeline is out-gated")
    }
    unsafe fn create_pipeline_cache(
        &self,
        desc: &wgpu::PipelineCacheDescriptor<'_>,
    ) -> DispatchPipelineCache {
        panic!("TRUEOS terrain-line stage: create_pipeline_cache is out-gated")
    }
    fn create_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> DispatchBuffer {
        assert!(
            desc.size <= 256 * 1024 * 1024,
            "host buffer budget per resource"
        );
        DispatchBuffer::custom(Buffer {
            bytes: buffer_bytes(desc.size, self.0.buffer_bytes.clone()),
            size: desc.size,
            usage: desc.usage,
        })
    }
    fn create_texture(&self, desc: &wgpu::TextureDescriptor<'_>) -> DispatchTexture {
        DispatchTexture::custom(HostTexture {
            size: desc.size,
            format: desc.format,
            usage: desc.usage,
            dimension: desc.dimension,
            mips: desc.mip_level_count,
            samples: desc.sample_count,
        })
    }
    fn create_external_texture(
        &self,
        desc: &wgpu::ExternalTextureDescriptor<'_>,
        planes: &[&wgpu::TextureView],
    ) -> DispatchExternalTexture {
        panic!("TRUEOS terrain-line stage: create_external_texture is out-gated")
    }
    fn create_blas(
        &self,
        desc: &wgpu::CreateBlasDescriptor<'_>,
        sizes: wgpu::BlasGeometrySizeDescriptors,
    ) -> (Option<u64>, DispatchBlas) {
        panic!("TRUEOS terrain-line stage: create_blas is out-gated")
    }
    fn create_tlas(&self, desc: &wgpu::CreateTlasDescriptor<'_>) -> DispatchTlas {
        panic!("TRUEOS terrain-line stage: create_tlas is out-gated")
    }
    fn create_sampler(&self, desc: &wgpu::SamplerDescriptor<'_>) -> DispatchSampler {
        DispatchSampler::custom(Empty)
    }
    fn create_query_set(&self, desc: &wgpu::QuerySetDescriptor<'_>) -> DispatchQuerySet {
        panic!("TRUEOS terrain-line stage: create_query_set is out-gated")
    }
    fn create_command_encoder(
        &self,
        desc: &wgpu::CommandEncoderDescriptor<'_>,
    ) -> DispatchCommandEncoder {
        DispatchCommandEncoder::custom(Encoder {
            lines: Arc::new(Mutex::new(std::mem::take(
                &mut *self.0.spare_lines.lock().unwrap(),
            ))),
        })
    }
    fn create_render_bundle_encoder(
        &self,
        desc: &wgpu::RenderBundleEncoderDescriptor<'_>,
    ) -> DispatchRenderBundleEncoder {
        panic!("TRUEOS terrain-line stage: create_render_bundle_encoder is out-gated")
    }
    fn set_device_lost_callback(&self, device_lost_callback: BoxDeviceLostCallback) {}
    fn on_uncaptured_error(&self, handler: Arc<dyn wgpu::UncapturedErrorHandler>) {}
    fn push_error_scope(&self, filter: wgpu::ErrorFilter) -> u32 {
        0
    }
    fn pop_error_scope(&self, index: u32) -> Pin<Box<dyn PopErrorScopeFuture>> {
        Box::pin(std::future::ready(None))
    }
    unsafe fn start_graphics_debugger_capture(&self) {
        panic!("TRUEOS terrain-line stage: start_graphics_debugger_capture is out-gated")
    }
    unsafe fn stop_graphics_debugger_capture(&self) {
        panic!("TRUEOS terrain-line stage: stop_graphics_debugger_capture is out-gated")
    }
    fn poll(
        &self,
        poll_type: wgpu::wgt::PollType<u64>,
    ) -> Result<wgpu::PollStatus, wgpu::PollError> {
        Ok(wgpu::PollStatus::QueueEmpty)
    }
    fn get_internal_counters(&self) -> wgpu::InternalCounters {
        Default::default()
    }
    fn generate_allocator_report(&self) -> Option<wgpu::AllocatorReport> {
        None
    }
    fn destroy(&self) {}
}

impl QueueInterface for Queue {
    fn write_buffer(&self, buffer: &DispatchBuffer, offset: wgpu::BufferAddress, data: &[u8]) {
        write(buffer, offset, data)
    }
    fn create_staging_buffer(&self, size: wgpu::BufferSize) -> Option<DispatchQueueWriteBuffer> {
        panic!("TRUEOS terrain-line stage: create_staging_buffer is out-gated")
    }
    fn validate_write_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgpu::BufferAddress,
        size: wgpu::BufferSize,
    ) -> Option<()> {
        let _ = binding(buffer, offset, Some(size.get()));
        Some(())
    }
    fn write_staging_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgpu::BufferAddress,
        staging_buffer: DispatchQueueWriteBuffer,
    ) {
        panic!("TRUEOS terrain-line stage: write_staging_buffer is out-gated")
    }
    fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) { /* Texture contents belong to excluded textured passes. Descriptor retained only. */
    }
    fn submit(&self, command_buffers: &mut dyn Iterator<Item = DispatchCommandBuffer>) -> u64 {
        let mut dst = self.0.lines.lock().unwrap();
        for commands in command_buffers {
            let commands = commands
                .as_custom::<Commands>()
                .expect("foreign command buffer");
            let mut src = commands.0.lock().unwrap();
            let available = MAX_LINE_VERTICES.saturating_sub(dst.len());
            if dst.is_empty() && src.len() <= available {
                std::mem::swap(&mut *dst, &mut *src);
            } else {
                dst.extend(src.drain(..).take(available));
            }
        }
        self.0.serial.fetch_add(1, Ordering::Relaxed) + 1
    }
    fn get_timestamp_period(&self) -> f32 {
        1.0
    }
    fn on_submitted_work_done(&self, callback: BoxSubmittedWorkDoneCallback) {
        callback()
    }
    fn compact_blas(&self, blas: &DispatchBlas) -> (Option<u64>, DispatchBlas) {
        panic!("TRUEOS terrain-line stage: compact_blas is out-gated")
    }
    fn present(&self, detail: &DispatchSurfaceOutputDetail) {
        super::trueos_scene::present(detail, &self.0)
    }
}

impl BufferInterface for Buffer {
    fn map_async(
        &self,
        mode: wgpu::MapMode,
        range: Range<wgpu::BufferAddress>,
        callback: BufferMapCallback,
    ) {
        assert!(range.end <= self.size);
        callback(Ok(()))
    }
    fn get_mapped_range(
        &self,
        sub_range: Range<wgpu::BufferAddress>,
    ) -> Result<DispatchBufferMappedRange, wgpu::MapRangeError> {
        assert!(sub_range.start <= sub_range.end && sub_range.end <= self.size);
        Ok(DispatchBufferMappedRange::custom(Mapping {
            parent: self.bytes.clone(),
            offset: sub_range.start as usize,
            bytes: self.bytes.lock().unwrap()[sub_range.start as usize..sub_range.end as usize]
                .to_vec(),
        }))
    }
    fn unmap(&self) {}
    fn destroy(&self) {}
    fn size(&self) -> wgpu::BufferAddress {
        self.size
    }
    fn usage(&self) -> wgpu::BufferUsages {
        self.usage
    }
}

impl BufferMappedRangeInterface for Mapping {
    fn len(&self) -> usize {
        self.bytes.len()
    }
    unsafe fn read_slice(&self) -> &[u8] {
        &self.bytes
    }
    unsafe fn write_slice(&mut self) -> WriteOnly<'_, [u8]> {
        unsafe { WriteOnly::new(std::ptr::NonNull::from(self.bytes.as_mut_slice())) }
    }
}

impl TextureInterface for HostTexture {
    fn create_view(&self, desc: &wgpu::TextureViewDescriptor<'_>) -> DispatchTextureView {
        DispatchTextureView::custom(Empty)
    }
    fn destroy(&self) {}
    fn size(&self) -> wgpu::wgt::Extent3d {
        self.size
    }
    fn mip_level_count(&self) -> u32 {
        self.mips
    }
    fn sample_count(&self) -> u32 {
        self.samples
    }
    fn dimension(&self) -> wgpu::wgt::TextureDimension {
        self.dimension
    }
    fn format(&self) -> wgpu::wgt::TextureFormat {
        self.format
    }
    fn usage(&self) -> wgpu::wgt::TextureUsages {
        self.usage
    }
}

impl CommandEncoderInterface for Encoder {
    fn copy_buffer_to_buffer(
        &self,
        source: &DispatchBuffer,
        source_offset: wgpu::BufferAddress,
        destination: &DispatchBuffer,
        destination_offset: wgpu::BufferAddress,
        copy_size: Option<wgpu::BufferAddress>,
    ) {
        let s = binding(source, source_offset, copy_size);
        let data = s.buffer.bytes.lock().unwrap()[s.offset..s.offset + s.size].to_vec();
        write(destination, destination_offset, &data);
    }
    fn copy_buffer_to_texture(
        &self,
        source: wgpu::TexelCopyBufferInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) { /* excluded texture payload */
    }
    fn copy_texture_to_buffer(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyBufferInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        panic!("TRUEOS terrain-line stage: copy_texture_to_buffer is out-gated")
    }
    fn copy_texture_to_texture(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) { /* excluded texture payload */
    }
    fn begin_compute_pass(&self, desc: &wgpu::ComputePassDescriptor<'_>) -> DispatchComputePass {
        panic!("TRUEOS terrain-line stage: begin_compute_pass is out-gated")
    }
    fn begin_render_pass(&self, desc: &wgpu::RenderPassDescriptor<'_>) -> DispatchRenderPass {
        DispatchRenderPass::custom(Pass {
            lines: self.lines.clone(),
            terrain: false,
            groups: BTreeMap::new(),
            vertex: None,
            index: None,
        })
    }
    fn finish(&mut self) -> DispatchCommandBuffer {
        DispatchCommandBuffer::custom(Commands(Mutex::new(std::mem::take(
            &mut *self.lines.lock().unwrap(),
        ))))
    }
    fn clear_texture(
        &self,
        texture: &DispatchTexture,
        subresource_range: &wgpu::ImageSubresourceRange,
    ) { /* excluded texture payload */
    }
    fn clear_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferAddress>,
    ) {
        let b = binding(buffer, offset, size);
        b.buffer.bytes.lock().unwrap()[b.offset..b.offset + b.size].fill(0);
    }
    fn insert_debug_marker(&self, label: &str) {}
    fn push_debug_group(&self, label: &str) {}
    fn pop_debug_group(&self) {}
    fn write_timestamp(&self, query_set: &DispatchQuerySet, query_index: u32) {
        panic!("TRUEOS terrain-line stage: write_timestamp is out-gated")
    }
    fn resolve_query_set(
        &self,
        query_set: &DispatchQuerySet,
        first_query: u32,
        query_count: u32,
        destination: &DispatchBuffer,
        destination_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS terrain-line stage: resolve_query_set is out-gated")
    }
    fn mark_acceleration_structures_built<'a>(
        &self,
        blas: &mut dyn Iterator<Item = &'a wgpu::Blas>,
        tlas: &mut dyn Iterator<Item = &'a wgpu::Tlas>,
    ) {
        panic!("TRUEOS terrain-line stage: mark_acceleration_structures_built is out-gated")
    }
    fn build_acceleration_structures<'a>(
        &self,
        blas: &mut dyn Iterator<Item = &'a wgpu::BlasBuildEntry<'a>>,
        tlas: &mut dyn Iterator<Item = &'a wgpu::Tlas>,
    ) {
        panic!("TRUEOS terrain-line stage: build_acceleration_structures is out-gated")
    }
    fn transition_resources<'a>(
        &mut self,
        buffer_transitions: &mut dyn Iterator<
            Item = wgpu::wgt::BufferTransition<&'a DispatchBuffer>,
        >,
        texture_transitions: &mut dyn Iterator<
            Item = wgpu::wgt::TextureTransition<&'a DispatchTexture>,
        >,
    ) {
        panic!("TRUEOS terrain-line stage: transition_resources is out-gated")
    }
}

impl RenderPassInterface for Pass {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        self.terrain = pipeline
            .as_custom::<Pipeline>()
            .expect("foreign pipeline")
            .terrain;
    }
    fn set_bind_group(
        &mut self,
        index: u32,
        bind_group: Option<&DispatchBindGroup>,
        offsets: &[wgpu::DynamicOffset],
    ) {
        assert!(
            offsets.is_empty(),
            "dynamic offsets outside terrain-line contract"
        );
        if let Some(b) = bind_group {
            self.groups
                .insert(index, b.as_custom::<Bind>().expect("foreign group").clone());
        } else {
            self.groups.remove(&index);
        }
    }
    fn set_index_buffer(
        &mut self,
        buffer: &DispatchBuffer,
        index_format: wgpu::IndexFormat,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferAddress>,
    ) {
        self.index = Some((binding(buffer, offset, size), index_format));
    }
    fn set_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: Option<&DispatchBuffer>,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferAddress>,
    ) {
        if slot == 0 {
            self.vertex = buffer.map(|b| binding(b, offset, size));
        }
    }
    fn set_immediates(&mut self, offset: u32, data: &[u8]) {}
    fn set_blend_constant(&mut self, color: wgpu::Color) {}
    fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {}
    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
    }
    fn set_stencil_reference(&mut self, reference: u32) {}
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        assert!(!self.terrain, "terrain requires indexed quad draw");
    }
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        if self.terrain {
            self.extract(indices, base_vertex, instances);
        }
    }
    fn draw_mesh_tasks(&mut self, group_count_x: u32, group_count_y: u32, group_count_z: u32) {
        panic!("TRUEOS terrain-line stage: draw_mesh_tasks is out-gated")
    }
    fn draw_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS terrain-line stage: draw_indirect is out-gated")
    }
    fn draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS terrain-line stage: draw_indexed_indirect is out-gated")
    }
    fn draw_mesh_tasks_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS terrain-line stage: draw_mesh_tasks_indirect is out-gated")
    }
    fn multi_draw_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count: u32,
    ) {
        panic!("TRUEOS terrain-line stage: multi_draw_indirect is out-gated")
    }
    fn multi_draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count: u32,
    ) {
        panic!("TRUEOS terrain-line stage: multi_draw_indexed_indirect is out-gated")
    }
    fn multi_draw_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: wgpu::BufferAddress,
        max_count: u32,
    ) {
        panic!("TRUEOS terrain-line stage: multi_draw_indirect_count is out-gated")
    }
    fn multi_draw_mesh_tasks_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count: u32,
    ) {
        panic!("TRUEOS terrain-line stage: multi_draw_mesh_tasks_indirect is out-gated")
    }
    fn multi_draw_indexed_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: wgpu::BufferAddress,
        max_count: u32,
    ) {
        panic!("TRUEOS terrain-line stage: multi_draw_indexed_indirect_count is out-gated")
    }
    fn multi_draw_mesh_tasks_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: wgpu::BufferAddress,
        max_count: u32,
    ) {
        panic!("TRUEOS terrain-line stage: multi_draw_mesh_tasks_indirect_count is out-gated")
    }
    fn insert_debug_marker(&mut self, label: &str) {}
    fn push_debug_group(&mut self, group_label: &str) {}
    fn pop_debug_group(&mut self) {}
    fn write_timestamp(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        panic!("TRUEOS terrain-line stage: write_timestamp is out-gated")
    }
    fn begin_occlusion_query(&mut self, query_index: u32) {
        panic!("TRUEOS terrain-line stage: begin_occlusion_query is out-gated")
    }
    fn end_occlusion_query(&mut self) {
        panic!("TRUEOS terrain-line stage: end_occlusion_query is out-gated")
    }
    fn begin_pipeline_statistics_query(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        panic!("TRUEOS terrain-line stage: begin_pipeline_statistics_query is out-gated")
    }
    fn end_pipeline_statistics_query(&mut self) {
        panic!("TRUEOS terrain-line stage: end_pipeline_statistics_query is out-gated")
    }
    fn execute_bundles(&mut self, render_bundles: &mut dyn Iterator<Item = &DispatchRenderBundle>) {
        panic!("TRUEOS terrain-line stage: execute_bundles is out-gated")
    }
}

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn matrix_at(bytes: &[u8], offset: usize) -> [f32; 16] {
    std::array::from_fn(|i| f32_at(bytes, offset + i * 4))
}
fn mul(m: &[f32; 16], v: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|r| (0..4).map(|c| m[c * 4 + r] * v[c]).sum())
}
fn packed_position(p: u32) -> [f32; 4] {
    [
        (p & 63) as f32,
        ((p >> 6) & 63) as f32,
        ((p >> 12) & 65535) as f32 - 32768.0,
        1.0,
    ]
}
/// Clip before division, including the WGPU 0 <= z <= w depth interval.
fn clip_line(mut a: [f32; 4], mut b: [f32; 4]) -> Option<[[f32; 3]; 2]> {
    if !a.into_iter().chain(b).all(f32::is_finite) {
        return None;
    }
    let planes = [
        [1., 0., 0., 1.],
        [-1., 0., 0., 1.],
        [0., 1., 0., 1.],
        [0., -1., 0., 1.],
        [0., 0., 1., 0.],
        [0., 0., -1., 1.],
    ];
    for p in planes {
        let dot = |v: [f32; 4]| (0..4).map(|i| p[i] * v[i]).sum::<f32>();
        let da = dot(a);
        let db = dot(b);
        if da < 0. && db < 0. {
            return None;
        }
        if (da < 0.) != (db < 0.) {
            let t = da / (da - db);
            let x = std::array::from_fn(|i| a[i] + t * (b[i] - a[i]));
            if da < 0. {
                a = x;
            } else {
                b = x;
            }
        }
    }
    if a[3] <= 1e-6 || b[3] <= 1e-6 {
        return None;
    }
    Some([
        std::array::from_fn(|i| a[i] / a[3]),
        std::array::from_fn(|i| b[i] / b[3]),
    ])
}
impl Pass {
    fn extract(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        if indices.is_empty() || instances.is_empty() {
            return;
        }
        assert_eq!(
            instances,
            0..1,
            "terrain-line extraction supports one terrain instance"
        );
        assert_eq!(indices.start % 6, 0);
        assert_eq!((indices.end - indices.start) % 6, 0);
        let vertex = self.vertex.as_ref().expect("terrain vertex buffer");
        let (index, index_format) = self.index.as_ref().expect("terrain index buffer");
        let globals = self
            .groups
            .get(&0)
            .and_then(|g| g.0.get(&0))
            .expect("terrain globals binding0:0");
        let locals = self
            .groups
            .get(&3)
            .and_then(|g| g.0.get(&0))
            .expect("terrain locals binding3:0");
        assert!(globals.size >= 224 && locals.size >= 64);
        let (all, focus, cam) = {
            let g = globals.buffer.bytes.lock().unwrap();
            let g = &g[globals.offset..globals.offset + globals.size];
            (
                matrix_at(g, 128),
                std::array::from_fn::<_, 3, _>(|i| f32_at(g, 208 + i * 4)),
                std::array::from_fn::<_, 3, _>(|i| f32_at(g, 192 + i * 4)),
            )
        };
        let model = {
            let l = locals.buffer.bytes.lock().unwrap();
            matrix_at(&l, locals.offset)
        };
        let vertices = vertex.buffer.bytes.lock().unwrap();
        let vertices = &vertices[vertex.offset..vertex.offset + vertex.size];
        let ib = index.buffer.bytes.lock().unwrap();
        let ib = &ib[index.offset..index.offset + index.size];
        let stride = if *index_format == wgpu::IndexFormat::Uint32 {
            4
        } else {
            2
        };
        assert!(
            (indices.end as usize)
                .checked_mul(stride)
                .is_some_and(|end| end <= ib.len())
        );
        let index_at = |i: usize| -> usize {
            let raw = if stride == 4 {
                u32::from_le_bytes(ib[i * 4..i * 4 + 4].try_into().unwrap())
            } else {
                u16::from_le_bytes(ib[i * 2..i * 2 + 2].try_into().unwrap()) as u32
            };
            let value = i64::from(raw) + i64::from(base_vertex);
            assert!(value >= 0);
            value as usize
        };
        let mut output = self.lines.lock().unwrap();
        for start in (indices.start as usize..indices.end as usize)
            .step_by(6)
            .take(65_536)
        {
            if output.len() + 8 > MAX_LINE_VERTICES {
                break;
            }
            let ids = std::array::from_fn::<_, 6, _>(|j| index_at(start + j));
            // The shared quad index buffer is [0,1,2,2,1,3]. Reject a changed layout.
            assert!(
                ids[0] + 1 == ids[1]
                    && ids[0] + 2 == ids[2]
                    && ids[2] == ids[3]
                    && ids[1] == ids[4]
                    && ids[0] + 3 == ids[5],
                "terrain quad index contract changed"
            );
            let mut world = [[0.; 4]; 4];
            for (i, v) in world.iter_mut().enumerate() {
                let offset = (ids[0] + i)
                    .checked_mul(8)
                    .expect("terrain offset overflow");
                assert!(offset + 8 <= vertices.len());
                let packed = u32::from_le_bytes(vertices[offset..offset + 4].try_into().unwrap());
                *v = mul(&model, packed_position(packed));
                for c in 0..3 {
                    v[c] -= focus[c];
                }
            }
            if !world
                .iter()
                .any(|v| (0..3).map(|i| (v[i] - cam[i]).powi(2)).sum::<f32>() <= 64. * 64.)
            {
                continue;
            }
            let clip = world.map(|v| mul(&all, v));
            // Mesh::push_quad stores b,c,a,d: these are the four perimeter edges.
            for (a, b) in [(0, 1), (1, 3), (3, 2), (2, 0)] {
                if let Some(line) = clip_line(clip[a], clip[b]) {
                    output.extend(line);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_height_and_matrix_columns() {
        let p = 3 | (5 << 6) | ((32768 - 12) << 12);
        assert_eq!(packed_position(p), [3., 5., -12., 1.]);
        let m = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 20., 30., 40., 1.,
        ];
        assert_eq!(mul(&m, packed_position(p)), [23., 35., 28., 1.]);
    }
    #[test]
    fn homogeneous_clipping() {
        assert!(clip_line([0., 0., -2., 1.], [0., 1., -1., 1.]).is_none());
        let l = clip_line([-2., 0., 0.5, 1.], [0., 0., 0.5, 1.]).unwrap();
        assert_eq!(l, [[-1., 0., 0.5], [0., 0., 0.5]]);
        let l = clip_line([0., 0., -1., 1.], [0.5, 0., 0.5, 1.]).unwrap();
        assert_eq!(l[0][2], 0.);
        assert!(l.iter().flatten().all(|x| x.is_finite()));
    }
    #[test]
    fn mapped_write_commits_to_retained_resource() {
        let b = Buffer {
            bytes: buffer_bytes(16, Arc::new(AtomicU64::new(0))),
            size: 16,
            usage: wgpu::BufferUsages::VERTEX,
        };
        {
            let mut m = Mapping {
                parent: b.bytes.clone(),
                offset: 4,
                bytes: vec![1, 2, 3, 4],
            };
            m.bytes[0] = 9;
        }
        assert_eq!(&b.bytes.lock().unwrap()[4..8], &[9, 2, 3, 4]);
    }
    #[test]
    fn real_quad_offsets_and_perimeter() {
        let budget = Arc::new(AtomicU64::new(0));
        let make = |bytes: Vec<u8>| {
            let n = bytes.len() as u64;
            let storage = buffer_bytes(n, budget.clone());
            *storage.lock().unwrap() = bytes;
            Buffer {
                bytes: storage,
                size: n,
                usage: wgpu::BufferUsages::VERTEX
                    | wgpu::BufferUsages::UNIFORM
                    | wgpu::BufferUsages::INDEX,
            }
        };
        let identity = [
            1f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let put = |dst: &mut [u8], offset: usize, values: &[f32]| {
            for (i, v) in values.iter().enumerate() {
                dst[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        let mut gb = vec![0; 16 + 224];
        put(&mut gb, 16 + 128, &identity);
        let globals = make(gb);
        let mut lb = vec![0; 16 + 64];
        let mut model = identity;
        model[14] = 0.5;
        put(&mut lb, 16, &model);
        let locals = make(lb);
        let mut vb = vec![0; 16];
        for (x, y) in [(0u32, 0u32), (1, 0), (0, 1), (1, 1)] {
            let p = x | (y << 6) | (32768 << 12);
            vb.extend(p.to_le_bytes());
            vb.extend(0u32.to_le_bytes());
        }
        let vertices = make(vb);
        let mut ib = vec![255; 4];
        for v in [0u32, 1, 2, 2, 1, 3] {
            ib.extend(v.to_le_bytes());
        }
        let indices = make(ib);
        let bind = |buffer: Buffer, offset: usize, size: usize| Binding {
            buffer,
            offset,
            size,
        };
        let lines = Arc::new(Mutex::new(Vec::new()));
        let mut pass = Pass {
            lines: lines.clone(),
            terrain: true,
            groups: BTreeMap::from([
                (0, Bind(BTreeMap::from([(0, bind(globals, 16, 224))]))),
                (3, Bind(BTreeMap::from([(0, bind(locals, 16, 64))]))),
            ]),
            vertex: Some(bind(vertices, 8, 40)),
            index: Some((bind(indices, 4, 24), wgpu::IndexFormat::Uint32)),
        };
        pass.extract(0..6, 1, 0..1);
        let lines = lines.lock().unwrap();
        assert_eq!(lines.len(), 8);
        assert_eq!(lines[0], [0., 0., 0.5]);
        assert_eq!(lines[1], [1., 0., 0.5]);
        for edge in lines.chunks_exact(2) {
            assert!(
                edge[0][0] == edge[1][0] || edge[0][1] == edge[1][1],
                "no quad diagonal"
            );
        }
    }
    #[test]
    fn allocation_budget_released_after_last_reference() {
        let budget = Arc::new(AtomicU64::new(0));
        let first = buffer_bytes(32, budget.clone());
        let second = first.clone();
        drop(first);
        assert_eq!(budget.load(Ordering::Relaxed), 32);
        drop(second);
        assert_eq!(budget.load(Ordering::Relaxed), 0);
    }
}
