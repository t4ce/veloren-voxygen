//! Fixed BareMinimum + Flat-cloud sky. Matches include/sky.glsl's
//! get_sky_color(): there are no directional features in this profile.
pub fn rgba8(sun_z: f32) -> u32 {
    let night = sun_z.max(0.0);
    let day = (-sun_z).max(0.0);
    let dusk = [1.78, 0.2, 0.15];
    let dark = [0.001, 0.003, 0.01125];
    let light = [0.14, 0.39, 0.75];
    let rgb: [u8; 3] = core::array::from_fn(|i| {
        let twilight = dusk[i] * (1.0 - night) + dark[i] * night;
        let value = twilight * (1.0 - day) + light[i] * day;
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    });
    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255])
}

#[cfg(target_os = "trueos")]
pub struct NativeSky {
    device: trueos::vgpu::Device,
    queue: trueos::vgpu::Queue,
}
#[cfg(target_os = "trueos")]
impl NativeSky {
    pub fn open() -> Result<Self, String> {
        use trueos::vgpu::{Capabilities, Device, QueueClass};
        let device = Device::open(
            // vgpu::open requires BUFFER, QUEUE, and TIMELINE for every
            // tenant device, including this clear-only producer.
            Capabilities::BUFFER
                .union(Capabilities::QUEUE)
                .union(Capabilities::TIMELINE)
                .union(Capabilities::RENDER)
                .union(Capabilities::PRESENT),
        )
        .map_err(|e| format!("sky device open: {e}"))?;
        match device.create_queue(QueueClass::Render) {
            Ok(queue) => Ok(Self { device, queue }),
            Err(e) => {
                let _ = device.close();
                Err(format!("sky render queue: {e}"))
            }
        }
    }
    /// Caller owns the acquired UI4 lease and publishes after this returns.
    /// The vgpu clear consumes the import and stages its exact GPU release.
    pub fn draw(&self, target: u32, rgba: u32) -> Result<(), i32> {
        let surface = self.device.acquire_ui4_surface(target)?;
        // Import Busy retains the caller's write lease. Submission failure
        // drops the import and cancels it, so it must not be retried as if the
        // old write lease were still active. This worker owns its queue alone.
        let point = self
            .device
            .submit_ui4_clear(self.queue, surface, rgba)
            .map_err(|e| {
                if e == trueos::vgpu::ERR_BUSY {
                    trueos::vgpu::ERR_IO
                } else {
                    e
                }
            })?;
        self.device.wait(self.queue, point.value)
    }
}
#[cfg(target_os = "trueos")]
impl Drop for NativeSky {
    fn drop(&mut self) {
        let _ = self.device.destroy_queue(self.queue);
        let _ = self.device.close();
    }
}
