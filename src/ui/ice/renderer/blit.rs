//! Small deterministic CPU rasterizer used by the TRUEOS UI4 proof path.
//!
//! This produces a tightly packed opaque RGBA8 frame for the UI4 native
//! presentation adapter. UI4 can retain/upload these pixels as a sprite and
//! use its opaque BCS0 copy path where eligible. This does not attempt to
//! reproduce Voxy's GPU renderer or its high-quality font rasterization.

use super::primitive::Primitive;
use crate::ui::graphic;
use alloc::{sync::Arc, vec, vec::Vec};
use vek::{Aabr, Rgba};

type Pixel = [f32; 4];

pub(super) struct Renderer {
    width: u32,
    height: u32,
    /// Linear-light, premultiplied pixels. The target is opaque, but keeping
    /// premultiplied color makes nested opacity and image alpha well-defined.
    pixels: Vec<Pixel>,
    images: Vec<(graphic::Id, Arc<::image::RgbaImage>)>,
    unsupported_images: Vec<graphic::Id>,
    next_image_id: u32,
    output: Vec<u8>,
}

impl Renderer {
    pub(super) fn new(width: u32, height: u32) -> Self {
        let pixels = vec![[0.0, 0.0, 0.0, 1.0]; pixel_count(width, height)];
        Self {
            width,
            height,
            pixels,
            images: Vec::new(),
            unsupported_images: Vec::new(),
            next_image_id: 0,
            output: Vec::new(),
        }
    }

    pub(super) fn add_image(&mut self, image: Arc<::image::RgbaImage>) -> graphic::Id {
        let id = graphic::Id::from_index(self.next_image_id);
        self.next_image_id = self.next_image_id.saturating_add(1);
        self.images.push((id, image));
        id
    }

    pub(super) fn replace_image(&mut self, id: graphic::Id, image: Arc<::image::RgbaImage>) {
        self.unsupported_images
            .retain(|unsupported| *unsupported != id);
        if let Some((_, slot)) = self.images.iter_mut().find(|(key, _)| *key == id) {
            *slot = image;
        } else {
            self.images.push((id, image));
        }
    }

    pub(super) fn mark_unsupported_image(&mut self, id: graphic::Id) {
        if !self.unsupported_images.contains(&id) {
            self.unsupported_images.push(id);
        }
    }

    pub(super) fn dimensions(&self, id: graphic::Id) -> (u32, u32) {
        self.images
            .iter()
            .find(|(key, _)| *key == id)
            .map_or((0, 0), |(_, image)| image.dimensions())
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels = vec![[0.0, 0.0, 0.0, 1.0]; pixel_count(width, height)];
    }

    pub(super) fn rasterize(&mut self, primitive: &Primitive) -> Result<&[u8], String> {
        self.pixels.fill([0.0, 0.0, 0.0, 1.0]);
        let clip = Rect {
            x0: 0,
            y0: 0,
            x1: self.width as i64,
            y1: self.height as i64,
        };
        self.draw(primitive, 0.0, 0.0, 1.0, clip)?;
        let mut rgba = Vec::with_capacity(self.pixels.len().saturating_mul(4));
        for [r, g, b, _] in self.pixels.iter().copied() {
            rgba.extend_from_slice(&[
                linear_to_srgb(r),
                linear_to_srgb(g),
                linear_to_srgb(b),
                u8::MAX,
            ]);
        }
        self.output = rgba;
        Ok(&self.output)
    }

    fn draw(
        &mut self,
        primitive: &Primitive,
        ox: f32,
        oy: f32,
        opacity: f32,
        clip: Rect,
    ) -> Result<(), String> {
        match primitive {
            Primitive::Group { primitives } => {
                for child in primitives {
                    self.draw(child, ox, oy, opacity, clip)?;
                }
            }
            Primitive::Rectangle {
                bounds,
                linear_color,
            } => {
                self.fill_rect(*bounds, ox, oy, clip, color_linear(*linear_color, opacity));
            }
            Primitive::Gradient {
                bounds,
                top_linear_color,
                bottom_linear_color,
            } => {
                let rect = pixel_rect(*bounds, ox, oy).intersect(clip);
                if rect.empty() {
                    return Ok(());
                }
                for y in rect.y0..rect.y1 {
                    let t = (((y as f32 + 0.5) - (bounds.y - oy))
                        / bounds.height.max(f32::EPSILON))
                    .clamp(0.0, 1.0);
                    let c = mix_color(*top_linear_color, *bottom_linear_color, t, opacity);
                    for x in rect.x0..rect.x1 {
                        self.blend(x as u32, y as u32, c);
                    }
                }
            }
            Primitive::Text {
                glyphs,
                linear_color,
                ..
            } => {
                let color = color_linear(*linear_color, opacity);
                for glyph in glyphs {
                    let scale = (glyph.glyph.scale.y / microfont::FHEIGHT as f32)
                        .round()
                        .max(1.0) as u32;
                    // The TRUEOS bridge positions cells by their baseline.
                    let left = (glyph.glyph.position.x - ox).floor() as i64;
                    let top =
                        (glyph.glyph.position.y - microfont::FHEIGHT as f32 * scale as f32 - oy)
                            .floor() as i64;
                    self.draw_microfont_glyph(
                        glyph.glyph.id.0 as u8,
                        left,
                        top,
                        scale,
                        color,
                        clip,
                    );
                }
            }
            Primitive::Image {
                handle: (id, rotation),
                bounds,
                color,
                source_rect,
            } => {
                if self.unsupported_images.contains(id) {
                    return Err(format!(
                        "CPU UI proof cannot rasterize non-bitmap graphic {id:?}"
                    ));
                }
                if !matches!(rotation, graphic::Rotation::None) {
                    return Err(format!(
                        "CPU UI proof does not support image rotation {rotation:?}"
                    ));
                }
                let image = self
                    .images
                    .iter()
                    .find(|(key, _)| key == id)
                    .map(|(_, image)| image.clone())
                    .ok_or_else(|| format!("missing UI image {id:?}"))?;
                self.draw_image(&image, *bounds, *color, *source_rect, ox, oy, opacity, clip);
            }
            Primitive::Clip {
                bounds,
                offset,
                content,
            } => {
                let clip_rect = pixel_rect(*bounds, 0.0, 0.0).intersect(clip);
                if !clip_rect.empty() {
                    self.draw(
                        content,
                        ox + offset.x as f32,
                        oy + offset.y as f32,
                        opacity,
                        clip_rect,
                    )?;
                }
            }
            Primitive::Opacity { alpha, content } => {
                self.draw(content, ox, oy, opacity * alpha.clamp(0.0, 1.0), clip)?;
            }
            Primitive::Nothing => {}
        }
        Ok(())
    }

    fn fill_rect(&mut self, bounds: iced::Rectangle, ox: f32, oy: f32, clip: Rect, color: Pixel) {
        let rect = pixel_rect(bounds, ox, oy).intersect(clip);
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                self.blend(x as u32, y as u32, color);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_image(
        &mut self,
        image: &::image::RgbaImage,
        bounds: iced::Rectangle,
        tint: Rgba<u8>,
        source: Option<Aabr<f32>>,
        ox: f32,
        oy: f32,
        opacity: f32,
        clip: Rect,
    ) {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return;
        }
        let rect = pixel_rect(bounds, ox, oy).intersect(clip);
        if rect.empty() {
            return;
        }
        let (iw, ih) = image.dimensions();
        if iw == 0 || ih == 0 {
            return;
        }
        let (sx0, sy0, sx1, sy1) = source.map_or((0.0, 0.0, iw as f32, ih as f32), |s| {
            (s.min.x, s.min.y, s.max.x, s.max.y)
        });
        let tint = [
            srgb_to_linear(tint.r as f32 / 255.0),
            srgb_to_linear(tint.g as f32 / 255.0),
            srgb_to_linear(tint.b as f32 / 255.0),
            tint.a as f32 / 255.0 * opacity,
        ];
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let u = ((x as f32 + 0.5 - (bounds.x - ox)) / bounds.width).clamp(0.0, 1.0);
                let v = ((y as f32 + 0.5 - (bounds.y - oy)) / bounds.height).clamp(0.0, 1.0);
                let ix = ((sx0 + u * (sx1 - sx0)).floor() as i64).clamp(0, iw as i64 - 1) as u32;
                let iy = ((sy0 + v * (sy1 - sy0)).floor() as i64).clamp(0, ih as i64 - 1) as u32;
                let p = image.get_pixel(ix, iy).0;
                let a = p[3] as f32 / 255.0 * tint[3];
                self.blend(
                    x as u32,
                    y as u32,
                    [
                        srgb_to_linear(p[0] as f32 / 255.0) * tint[0] * a,
                        srgb_to_linear(p[1] as f32 / 255.0) * tint[1] * a,
                        srgb_to_linear(p[2] as f32 / 255.0) * tint[2] * a,
                        a,
                    ],
                );
            }
        }
    }

    fn blend(&mut self, x: u32, y: u32, src: Pixel) {
        if x >= self.width || y >= self.height {
            return;
        }
        let dst = &mut self.pixels[(y as usize) * self.width as usize + x as usize];
        let a = src[3].clamp(0.0, 1.0);
        *dst = [
            src[0] + dst[0] * (1.0 - a),
            src[1] + dst[1] * (1.0 - a),
            src[2] + dst[2] * (1.0 - a),
            a + dst[3] * (1.0 - a),
        ];
    }

    fn draw_microfont_glyph(
        &mut self,
        glyph: u8,
        left: i64,
        top: i64,
        scale: u32,
        color: Pixel,
        clip: Rect,
    ) {
        let bits = (microfont::font_pixels(glyph) as u128) << 2;
        for gy in 0..microfont::FHEIGHT {
            for gx in 0..microfont::FWIDTH {
                let bit =
                    microfont::FWIDTH * microfont::FHEIGHT - 1 - (gy * microfont::FWIDTH + gx);
                if bits & (1u128 << bit) == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let x = left + (gx as u32 * scale + sx) as i64;
                        let y = top + (gy as u32 * scale + sy) as i64;
                        if clip.contains(x, y) {
                            self.blend(x as u32, y as u32, color);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}
impl Rect {
    fn intersect(self, rhs: Self) -> Self {
        Self {
            x0: self.x0.max(rhs.x0),
            y0: self.y0.max(rhs.y0),
            x1: self.x1.min(rhs.x1),
            y1: self.y1.min(rhs.y1),
        }
    }
    fn empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    fn contains(self, x: i64, y: i64) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

fn pixel_rect(bounds: iced::Rectangle, ox: f32, oy: f32) -> Rect {
    Rect {
        x0: (bounds.x - ox).floor() as i64,
        y0: (bounds.y - oy).floor() as i64,
        x1: (bounds.x + bounds.width - ox).ceil() as i64,
        y1: (bounds.y + bounds.height - oy).ceil() as i64,
    }
}
fn pixel_count(w: u32, h: u32) -> usize {
    (w as usize).saturating_mul(h as usize)
}
fn color_linear(c: Rgba<f32>, opacity: f32) -> Pixel {
    let a = c.a.clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0);
    [c.r * a, c.g * a, c.b * a, a]
}
fn mix_color(a: Rgba<f32>, b: Rgba<f32>, t: f32, opacity: f32) -> Pixel {
    color_linear(
        Rgba::new(
            a.r + (b.r - a.r) * t,
            a.g + (b.g - a.g) * t,
            a.b + (b.b - a.b) * t,
            a.a + (b.a - a.a) * t,
        ),
        opacity,
    )
}
fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

pub(super) fn font_scale(size: u16) -> u32 {
    ((size as u32 + microfont::FHEIGHT as u32 / 2) / microfont::FHEIGHT as u32).max(1)
}

pub(super) fn native_text_lines(text: &str, max_columns: usize) -> Vec<Vec<(char, usize)>> {
    if text.is_empty() {
        return Vec::new();
    }
    let max_columns = max_columns.max(1);
    let mut lines = vec![Vec::new()];
    for (byte_index, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(Vec::new());
            continue;
        }
        if lines.last().is_some_and(|line| line.len() >= max_columns) {
            lines.push(Vec::new());
        }
        lines
            .last_mut()
            .expect("at least one line")
            .push((ch, byte_index));
    }
    lines
}

pub(super) fn native_line_metrics(text: &str, max_columns: usize) -> (usize, usize) {
    let lines = native_text_lines(text, max_columns);
    (lines.iter().map(Vec::len).max().unwrap_or(0), lines.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> iced::Rectangle {
        iced::Rectangle {
            x,
            y,
            width,
            height,
        }
    }
    fn red() -> Rgba<f32> {
        Rgba::new(1.0, 0.0, 0.0, 1.0)
    }

    #[test]
    fn rasterized_microfont_matches_the_exported_glyph_mask() {
        let mut renderer = Renderer::new(6, 11);
        let clip = Rect {
            x0: 0,
            y0: 0,
            x1: 6,
            y1: 11,
        };
        renderer.draw_microfont_glyph(b'A', 0, 0, 1, color_linear(red(), 1.0), clip);
        let bits = (microfont::font_pixels(b'A') as u128) << 2;
        for (index, pixel) in renderer.pixels.iter().enumerate() {
            assert_eq!(pixel[0] > 0.0, bits & (1u128 << (65 - index)) != 0);
        }
    }

    #[test]
    fn clip_and_child_offset_are_applied_before_painting() {
        let mut renderer = Renderer::new(4, 3);
        let primitive = Primitive::Clip {
            bounds: rect(1.0, 1.0, 2.0, 1.0),
            offset: vek::Vec2::new(1, 0),
            content: Box::new(Primitive::Rectangle {
                bounds: rect(1.0, 1.0, 4.0, 1.0),
                linear_color: red(),
            }),
        };
        let bytes = renderer.rasterize(&primitive).unwrap();
        assert_eq!(bytes[4 * (1 * 4 + 1)..4 * (1 * 4 + 2)], [255, 0, 0, 255]);
        assert_eq!(bytes[4 * (1 * 4 + 2)..4 * (1 * 4 + 3)], [255, 0, 0, 255]);
        assert_eq!(bytes[4 * (1 * 4)..4 * (1 * 4 + 1)], [0, 0, 0, 255]);
    }

    #[test]
    fn translucent_linear_shapes_composite_to_an_opaque_srgb_frame() {
        let mut renderer = Renderer::new(1, 1);
        let primitive = Primitive::Rectangle {
            bounds: rect(0.0, 0.0, 1.0, 1.0),
            linear_color: Rgba::new(1.0, 0.0, 0.0, 0.5),
        };
        assert_eq!(renderer.rasterize(&primitive).unwrap(), &[188, 0, 0, 255]);
    }

    #[test]
    fn image_is_nearest_scaled_and_transparent_pixels_blend() {
        let mut renderer = Renderer::new(4, 1);
        let mut image = ::image::RgbaImage::new(2, 1);
        image.put_pixel(0, 0, ::image::Rgba([255, 0, 0, 128]));
        image.put_pixel(1, 0, ::image::Rgba([0, 255, 0, 255]));
        let id = renderer.add_image(Arc::new(image));
        assert_eq!(renderer.dimensions(id), (2, 1));
        let primitive = Primitive::Image {
            handle: (id, graphic::Rotation::None),
            bounds: rect(0.0, 0.0, 4.0, 1.0),
            color: Rgba::broadcast(255),
            source_rect: None,
        };
        let bytes = renderer.rasterize(&primitive).unwrap();
        assert_eq!(&bytes[0..4], &[188, 0, 0, 255]);
        assert_eq!(&bytes[8..12], &[0, 255, 0, 255]);
    }

    #[test]
    fn unsupported_image_rotation_fails_explicitly() {
        let mut renderer = Renderer::new(2, 2);
        let id = renderer.add_image(Arc::new(::image::RgbaImage::new(1, 1)));
        let primitive = Primitive::Image {
            handle: (id, graphic::Rotation::Cw90),
            bounds: rect(0.0, 0.0, 1.0, 1.0),
            color: Rgba::broadcast(255),
            source_rect: None,
        };
        assert!(
            renderer
                .rasterize(&primitive)
                .unwrap_err()
                .contains("does not support image rotation")
        );
    }

    #[test]
    fn unicode_layout_tracks_utf8_byte_offsets_and_wraps_by_cells() {
        assert_eq!(
            native_text_lines("AéB", 2),
            vec![vec![('A', 0), ('é', 1)], vec![('B', 3)]]
        );
        assert_eq!(native_line_metrics("AéB", 2), (2, 2));
        assert_eq!(native_line_metrics("", 2), (0, 0));
    }
}
