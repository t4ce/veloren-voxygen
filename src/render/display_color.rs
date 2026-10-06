//! Final scene transfer executed by the primary display on TRUEOS.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    pub gamma: f32,
    pub fade: f32,
    pub underwater: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub exponent: f32,
    pub gain: [f32; 3],
    pub srgb: bool,
}

impl Curve {
    pub fn ramp(self) -> [u16; 3 * 256] {
        core::array::from_fn(|index| {
            let value = (index % 256) as f32 / 255.0;
            let linear = if self.srgb { decode_srgb(value) } else { value };
            let adjusted = linear.powf(self.exponent) * self.gain[index / 256];
            let encoded = if self.srgb {
                encode_srgb(adjusted)
            } else {
                adjusted
            };
            (encoded.clamp(0.0, 1.0) * 65535.0).round() as u16
        })
    }
}

fn decode_srgb(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn encode_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(target_os = "trueos")]
pub struct DisplayColor {
    window_id: u32,
    last: Option<Curve>,
}

#[cfg(target_os = "trueos")]
impl DisplayColor {
    pub fn new(window_id: u32) -> Self {
        Self {
            window_id,
            last: None,
        }
    }

    pub fn apply(&mut self, curve: Curve) -> Result<(), super::RenderError> {
        if self.last == Some(curve) {
            return Ok(());
        }
        trueos::ui4_scene::set_display_gamma_ramp(self.window_id, &curve.ramp()).map_err(
            |error| {
                super::RenderError::CustomError(format!(
                    "Display color transfer unavailable: {error:?}"
                ))
            },
        )?;
        self.last = Some(curve);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_gamma_endpoints_are_preserved() {
        for srgb in [false, true] {
            let identity = Curve {
                exponent: 1.0,
                gain: [1.0; 3],
                srgb,
            }
            .ramp();
            for i in 0..256 {
                assert!(identity[i].abs_diff(i as u16 * 257) <= 1);
            }
            let gamma = Curve {
                exponent: 1.3,
                gain: [1.0; 3],
                srgb,
            }
            .ramp();
            assert_eq!(gamma[0], 0);
            assert_eq!(gamma[255], 65535);
            assert!(gamma[128] < identity[128]);
            assert!(gamma[..256].windows(2).all(|v| v[0] <= v[1]));
        }
    }

    #[test]
    fn fades_and_water_tint_compose_in_linear_space() {
        for srgb in [false, true] {
            let curve = Curve {
                exponent: 1.14,
                gain: [0.1, 0.1, 0.4],
                srgb,
            };
            let ramp = curve.ramp();
            for channel in 0..3 {
                for i in 0..256 {
                    let x = i as f32 / 255.0;
                    let linear = if srgb { decode_srgb(x) } else { x };
                    let expected = linear.powf(1.14) * curve.gain[channel];
                    let output = ramp[channel * 256 + i] as f32 / 65535.0;
                    let output = if srgb { decode_srgb(output) } else { output };
                    assert!((output - expected).abs() < 0.00003);
                }
            }
            assert_eq!(
                Curve {
                    gain: [0.0; 3],
                    ..curve
                }
                .ramp(),
                [0; 768]
            );
        }
    }
}
