//! Retained iced draw commands. CPU work is layout and small asset preparation;
//! UI4 writes the destination through BCS0 copies/fills. The display engine
//! blends the foreground alpha over the independent scene producer.
use super::{activity::PreparationActivity, primitive::Primitive};
use crate::ui::graphic;
use alloc::{sync::Arc, vec::Vec};
use trueos::ui4_solara_text::{SpriteBackend, SpriteCommand, SpriteCorner, SpriteQuad};
use vek::{Aabr, Rgba};

#[derive(Clone)]
pub(crate) struct Upload {
    pub id: u32,
    pub image: Arc<::image::RgbaImage>,
}
#[derive(Clone, Default)]
pub(crate) struct LayerPlan {
    pub uploads: Vec<Upload>,
    pub commands: Vec<SpriteCommand>,
}
#[derive(Clone, Default)]
pub(crate) struct FramePlan {
    pub foreground: LayerPlan,
    pub background: LayerPlan,
}
struct Asset {
    graphic: graphic::Id,
    sprite: u32,
    image: Arc<::image::RgbaImage>,
    scene: bool,
}
pub(super) struct Renderer {
    width: u32,
    height: u32,
    images: Vec<Asset>,
    unsupported: Vec<graphic::Id>,
    glyphs: Vec<((u8, u32, u32), u32, Arc<::image::RgbaImage>)>,
    prepared: Vec<(PreparedKey, u32, Arc<::image::RgbaImage>, bool)>,
    gradients: Vec<(u32, u32, u32, u32, u32, Arc<::image::RgbaImage>)>,
    next_graphic: u32,
    next_sprite: u32,
    activity: PreparationActivity,
    dialog: Option<(u8, LayerPlan)>,
    closing_dialog: Option<(LayerPlan, std::time::Instant)>,
    closing_sprite: Option<u32>,
}
impl Renderer {
    pub(super) fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            images: Vec::new(),
            unsupported: Vec::new(),
            glyphs: Vec::new(),
            prepared: Vec::new(),
            gradients: Vec::new(),
            next_graphic: 0,
            next_sprite: 1,
            activity: PreparationActivity::default(),
            dialog: None,
            closing_dialog: None,
            closing_sprite: None,
        }
    }
    pub(super) fn take_activity(&mut self) -> PreparationActivity {
        std::mem::take(&mut self.activity)
    }
    fn sprite_id(&mut self) -> u32 {
        let id = self.next_sprite;
        self.next_sprite = id
            .checked_add(1)
            .expect("native UI sprite handle exhaustion");
        id
    }
    pub(super) fn add_image(&mut self, image: Arc<::image::RgbaImage>) -> graphic::Id {
        let id = graphic::Id::from_index(self.next_graphic);
        self.next_graphic = self
            .next_graphic
            .checked_add(1)
            .expect("native UI graphic handle exhaustion");
        let sprite = self.sprite_id();
        self.images.push(Asset {
            graphic: id,
            sprite,
            image,
            scene: false,
        });
        id
    }
    pub(super) fn replace_image(&mut self, id: graphic::Id, image: Arc<::image::RgbaImage>) {
        self.unsupported.retain(|key| *key != id);
        let sprite = self.sprite_id();
        if let Some(asset) = self.images.iter_mut().find(|a| a.graphic == id) {
            let scene = asset.scene;
            *asset = Asset {
                graphic: id,
                sprite,
                image,
                scene,
            };
        }
    }
    pub(super) fn mark_scene_image(&mut self, id: graphic::Id) {
        if let Some(asset) = self.images.iter_mut().find(|a| a.graphic == id) {
            asset.scene = true;
        }
    }
    pub(super) fn mark_unsupported_image(&mut self, id: graphic::Id) {
        self.unsupported.push(id);
    }
    pub(super) fn dimensions(&self, id: graphic::Id) -> (u32, u32) {
        self.images
            .iter()
            .find(|a| a.graphic == id)
            .map_or((0, 0), |a| a.image.dimensions())
    }
    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.dialog = None;
        self.closing_dialog = None;
    }
    pub(super) fn prepare(&mut self, primitive: &Primitive) -> Result<FramePlan, String> {
        let mut plan = FramePlan::default();
        let previous = self.dialog.take();
        self.draw(
            primitive,
            (0., 0.),
            1.,
            Rect::new(0., 0., self.width as f32, self.height as f32),
            &mut plan,
        )?;
        if previous.as_ref().map(|(id, _)| *id) != self.dialog.as_ref().map(|(id, _)| *id) {
            if let Some((_, layer)) = previous {
                // Retain one noninteractive outgoing snapshot, even under rapid clicks.
                let id = if let Some(id) = self.closing_sprite {
                    id
                } else {
                    let id = self.sprite_id();
                    self.closing_sprite = Some(id);
                    id
                };
                self.closing_dialog =
                    Some((snapshot_dialog(&layer, id), std::time::Instant::now()));
            }
        }
        if let Some((_, layer)) = &self.dialog {
            plan.foreground
                .uploads
                .extend(layer.uploads.iter().cloned());
            plan.foreground
                .commands
                .extend(layer.commands.iter().cloned());
        }
        if let Some((layer, started)) = &self.closing_dialog {
            let progress = started.elapsed().as_secs_f32() / 0.5;
            if progress < 1.0 {
                let mut outgoing = layer.clone();
                animate_dialog(&mut outgoing, progress);
                plan.foreground.uploads.extend(outgoing.uploads);
                plan.foreground.commands.extend(outgoing.commands);
            } else {
                self.closing_dialog = None;
            }
        }
        Ok(plan)
    }
    pub(super) fn dialog_animating(&self) -> bool {
        self.closing_dialog.is_some()
    }
    fn glyph(&mut self, byte: u8, scale: u32, color: u32) -> (u32, Arc<::image::RgbaImage>) {
        let key = (byte, scale, color);
        if let Some((_, id, image)) = self.glyphs.iter().find(|(k, _, _)| *k == key) {
            return (*id, Arc::clone(image));
        }
        // Cache a tiny coloured MicroFont mask once, never paint frame pixels.
        let bits = (microfont::font_pixels(byte) as u128) << 2;
        let image = ::image::RgbaImage::from_fn(
            microfont::FWIDTH as u32 * scale,
            microfont::FHEIGHT as u32 * scale,
            |x, y| {
                let bit = microfont::FWIDTH * microfont::FHEIGHT
                    - 1
                    - ((y / scale) as usize * microfont::FWIDTH + (x / scale) as usize);
                ::image::Rgba(if bits & (1u128 << bit) != 0 {
                    premultiply(color).to_le_bytes()
                } else {
                    [0; 4]
                })
            },
        );
        let image = Arc::new(image);
        let id = self.sprite_id();
        self.activity.glyphs += 1;
        self.activity.bytes += image.as_raw().len() as u64;
        self.glyphs.push((key, id, Arc::clone(&image)));
        (id, image)
    }
    fn draw(
        &mut self,
        primitive: &Primitive,
        offset: (f32, f32),
        opacity: f32,
        clip: Rect,
        plan: &mut FramePlan,
    ) -> Result<(), String> {
        match primitive {
            Primitive::Dialog { id, content } => {
                let mut dialog = FramePlan::default();
                self.draw(content, offset, opacity, clip, &mut dialog)?;
                self.dialog = Some((*id, dialog.foreground));
            }
            Primitive::Group { primitives } => {
                for p in primitives {
                    self.draw(p, offset, opacity, clip, plan)?;
                }
            }
            Primitive::Rectangle {
                bounds,
                linear_color,
            } => {
                let color = packed_color(*linear_color, opacity);
                let bounds = Rect::bounds(*bounds, offset);
                let backend = choose_backend(
                    &plan.foreground,
                    snap(bounds).intersect(clip),
                    color.to_le_bytes()[3] < 255,
                );
                append_quad(
                    &mut plan.foreground,
                    0,
                    snap(bounds),
                    [0., 0., 1., 1.],
                    premultiply(color),
                    backend,
                    clip,
                );
            }
            Primitive::Gradient {
                bounds,
                top_linear_color,
                bottom_linear_color,
            } => {
                let rect = snap(Rect::bounds(*bounds, offset));
                let w = rect.width().max(1.) as u32;
                let h = rect.height().max(1.) as u32;
                let top = packed_color(*top_linear_color, opacity);
                let bottom = packed_color(*bottom_linear_color, opacity);
                let (id, image) = if let Some((_, _, _, _, id, image)) = self
                    .gradients
                    .iter()
                    .find(|(a, b, c, d, _, _)| (*a, *b, *c, *d) == (w, h, top, bottom))
                {
                    (*id, Arc::clone(image))
                } else {
                    let image = Arc::new(::image::RgbaImage::from_fn(w, h, |_, y| {
                        let t = if h > 1 { y as f32 / (h - 1) as f32 } else { 0. };
                        let a = top.to_le_bytes();
                        let b = bottom.to_le_bytes();
                        let c = core::array::from_fn(|i| {
                            (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8
                        });
                        ::image::Rgba(premultiply(u32::from_le_bytes(c)).to_le_bytes())
                    }));
                    let id = self.sprite_id();
                    self.activity.gradients += 1;
                    self.activity.bytes += image.as_raw().len() as u64;
                    self.gradients
                        .push((w, h, top, bottom, id, Arc::clone(&image)));
                    (id, image)
                };
                retain(&mut plan.foreground, id, image);
                let backend = choose_backend(
                    &plan.foreground,
                    rect.intersect(clip),
                    top.to_le_bytes()[3] < 255 || bottom.to_le_bytes()[3] < 255,
                );
                append_quad(
                    &mut plan.foreground,
                    id,
                    rect,
                    [0., 0., 1., 1.],
                    u32::MAX,
                    backend,
                    clip,
                );
            }
            Primitive::Image {
                handle: (id, rotation),
                bounds,
                color,
                source_rect,
            } => {
                if self.unsupported.contains(id) {
                    return Err("native UI cannot draw voxel graphics".into());
                }
                if !matches!(rotation, graphic::Rotation::None) {
                    return Err("native UI rotated sprite support pending".into());
                }
                let asset = self
                    .images
                    .iter()
                    .find(|a| a.graphic == *id)
                    .ok_or("missing native UI asset")?;
                let (w, h) = asset.image.dimensions();
                if w == 0 || h == 0 {
                    return Ok(());
                }
                let source = source_rect.unwrap_or(Aabr {
                    min: vek::Vec2::zero(),
                    max: vek::Vec2::new(w as f32, h as f32),
                });
                let rect = snap(Rect::bounds(*bounds, offset));
                if rect.width() <= 0. || rect.height() <= 0. {
                    return Ok(());
                }
                let tint = u32::from_le_bytes([
                    color.r,
                    color.g,
                    color.b,
                    (color.a as f32 * opacity).round().clamp(0., 255.) as u8,
                ]);
                let key = PreparedKey {
                    source: asset.sprite,
                    crop: [
                        source.min.x.to_bits(),
                        source.min.y.to_bits(),
                        source.max.x.to_bits(),
                        source.max.y.to_bits(),
                    ],
                    width: rect.width() as u32,
                    height: rect.height() as u32,
                    tint,
                };
                let scene = asset.scene;
                let (id, image, partial) = if let Some((_, id, image, partial)) =
                    self.prepared.iter().find(|(k, _, _, _)| *k == key)
                {
                    (*id, Arc::clone(image), *partial)
                } else {
                    // Prepare an immutable asset at its displayed size once. This is
                    // never a composition of the UI frame, nor repeated on hover/ticks.
                    let source = Arc::clone(&asset.image);
                    let tint = tint.to_le_bytes();
                    let image = Arc::new(::image::RgbaImage::from_fn(
                        key.width,
                        key.height,
                        |x, y| {
                            let sx = f32::from_bits(key.crop[0])
                                + (x as f32 + 0.5)
                                    * (f32::from_bits(key.crop[2]) - f32::from_bits(key.crop[0]))
                                    / key.width as f32;
                            let sy = f32::from_bits(key.crop[1])
                                + (y as f32 + 0.5)
                                    * (f32::from_bits(key.crop[3]) - f32::from_bits(key.crop[1]))
                                    / key.height as f32;
                            let p = source
                                .get_pixel(
                                    sx.floor().clamp(0., (w - 1) as f32) as u32,
                                    sy.floor().clamp(0., (h - 1) as f32) as u32,
                                )
                                .0;
                            let tinted = core::array::from_fn(|i| {
                                ((p[i] as u32 * tint[i] as u32 + 127) / 255) as u8
                            });
                            ::image::Rgba(premultiply(u32::from_le_bytes(tinted)).to_le_bytes())
                        },
                    ));
                    let id = self.sprite_id();
                    let partial = image.pixels().any(|p| p[3] > 0 && p[3] < 255);
                    self.activity.images += 1;
                    self.activity.bytes += image.as_raw().len() as u64;
                    self.prepared.push((key, id, Arc::clone(&image), partial));
                    (id, image, partial)
                };
                let layer = if scene {
                    &mut plan.background
                } else {
                    &mut plan.foreground
                };
                retain(layer, id, image);
                let backend = choose_backend(layer, rect.intersect(clip), partial);
                append_quad(layer, id, rect, [0., 0., 1., 1.], u32::MAX, backend, clip);
            }
            Primitive::Text {
                glyphs,
                linear_color,
                ..
            } => {
                let color = packed_color(*linear_color, opacity);
                for glyph in glyphs {
                    let scale = (glyph.glyph.scale.y / microfont::FHEIGHT as f32)
                        .round()
                        .max(1.) as u32;
                    let (id, image) = self.glyph(glyph.glyph.id.0 as u8, scale, color);
                    let bounds = Rect::new(
                        (glyph.glyph.position.x - offset.0).floor(),
                        (glyph.glyph.position.y
                            - offset.1
                            - microfont::FHEIGHT as f32 * scale as f32)
                            .floor(),
                        image.width() as f32,
                        image.height() as f32,
                    );
                    retain(&mut plan.foreground, id, image);
                    let backend = choose_backend(
                        &plan.foreground,
                        bounds.intersect(clip),
                        color.to_le_bytes()[3] < 255,
                    );
                    append_quad(
                        &mut plan.foreground,
                        id,
                        bounds,
                        [0., 0., 1., 1.],
                        u32::MAX,
                        backend,
                        clip,
                    );
                }
            }
            Primitive::Clip {
                bounds,
                offset: child,
                content,
            } => {
                let clip = clip.intersect(Rect::bounds(*bounds, offset));
                self.draw(
                    content,
                    (offset.0 + child.x as f32, offset.1 + child.y as f32),
                    opacity,
                    clip,
                    plan,
                )?;
            }
            Primitive::Opacity { alpha, content } => {
                self.draw(content, offset, opacity * alpha, clip, plan)?
            }
            Primitive::Nothing => {}
        }
        Ok(())
    }
}
// Materialize the outgoing panel once. Reuse a single upload slot across
// transitions; animation then scales/blends one sprite instead of every glyph.
fn snapshot_dialog(layer: &LayerPlan, id: u32) -> LayerPlan {
    if layer.commands.is_empty() {
        return LayerPlan::default();
    }
    let left = layer
        .commands
        .iter()
        .map(|c| c.quad.c0.x.floor() as i32)
        .min()
        .unwrap();
    let top = layer
        .commands
        .iter()
        .map(|c| c.quad.c0.y.floor() as i32)
        .min()
        .unwrap();
    let right = layer
        .commands
        .iter()
        .map(|c| c.quad.c2.x.ceil() as i32)
        .max()
        .unwrap();
    let bottom = layer
        .commands
        .iter()
        .map(|c| c.quad.c2.y.ceil() as i32)
        .max()
        .unwrap();
    let mut image = ::image::RgbaImage::new((right - left) as u32, (bottom - top) as u32);
    for command in &layer.commands {
        let q = command.quad;
        let source = layer.uploads.iter().find(|u| u.id == q.sprite_id);
        let tint = q.color_rgba.to_le_bytes();
        for y in q.c0.y.floor() as i32..q.c2.y.ceil() as i32 {
            for x in q.c0.x.floor() as i32..q.c2.x.ceil() as i32 {
                let ink = if q.sprite_id == 0 {
                    tint
                } else {
                    let source = source.expect("retained dialog sprite upload");
                    let u =
                        q.c0.u + (q.c1.u - q.c0.u) * (x as f32 + 0.5 - q.c0.x) / (q.c1.x - q.c0.x);
                    let v =
                        q.c0.v + (q.c3.v - q.c0.v) * (y as f32 + 0.5 - q.c0.y) / (q.c3.y - q.c0.y);
                    let sx = (u * source.image.width() as f32)
                        .floor()
                        .clamp(0., (source.image.width() - 1) as f32)
                        as u32;
                    let sy = (v * source.image.height() as f32)
                        .floor()
                        .clamp(0., (source.image.height() - 1) as f32)
                        as u32;
                    let sample = source.image.get_pixel(sx, sy).0;
                    core::array::from_fn(|i| {
                        ((sample[i] as u16 * tint[i] as u16 + 127) / 255) as u8
                    })
                };
                if ink[3] == 0 {
                    continue;
                }
                let pixel = image.get_pixel_mut((x - left) as u32, (y - top) as u32);
                if command.backend == SpriteBackend::Bcs0 || !q.source_over {
                    pixel.0 = ink;
                } else {
                    pixel.0 = core::array::from_fn(|i| {
                        (ink[i] as u16 + (pixel.0[i] as u16 * (255 - ink[3]) as u16 + 127) / 255)
                            .min(255) as u8
                    });
                }
            }
        }
    }
    let mut result = LayerPlan::default();
    retain(&mut result, id, Arc::new(image));
    append_quad(
        &mut result,
        id,
        Rect::new(
            left as f32,
            top as f32,
            (right - left) as f32,
            (bottom - top) as f32,
        ),
        [0., 0., 1., 1.],
        u32::MAX,
        SpriteBackend::PremultipliedCompositor,
        Rect::new(
            left as f32,
            top as f32,
            (right - left) as f32,
            (bottom - top) as f32,
        ),
    );
    result
}

fn animate_dialog(layer: &mut LayerPlan, progress: f32) {
    let progress = progress.clamp(0.0, 1.0);
    let mut left = f32::INFINITY;
    let mut top = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for command in &layer.commands {
        for corner in [
            command.quad.c0,
            command.quad.c1,
            command.quad.c2,
            command.quad.c3,
        ] {
            left = left.min(corner.x);
            top = top.min(corner.y);
            right = right.max(corner.x);
            bottom = bottom.max(corner.y);
        }
    }
    let center = ((left + right) * 0.5, (top + bottom) * 0.5);
    let scale = 1.0 + 0.1 * progress;
    for command in &mut layer.commands {
        command.backend = SpriteBackend::PremultipliedCompositor;
        for corner in [
            &mut command.quad.c0,
            &mut command.quad.c1,
            &mut command.quad.c2,
            &mut command.quad.c3,
        ] {
            corner.x = center.0 + (corner.x - center.0) * scale;
            corner.y = center.1 + (corner.y - center.1) * scale;
        }
        // Both sprite texels and solid fills are premultiplied. Modulate all
        // four channels to preserve source-over blending throughout the fade.
        command.quad.color_rgba = u32::from_le_bytes(
            command
                .quad
                .color_rgba
                .to_le_bytes()
                .map(|channel| (channel as f32 * (1.0 - progress)).round() as u8),
        );
    }
}

fn retain(plan: &mut LayerPlan, id: u32, image: Arc<::image::RgbaImage>) {
    if plan.uploads.iter().all(|u| u.id != id) {
        plan.uploads.push(Upload { id, image });
    }
}
#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    right: f32,
    bottom: f32,
}
impl Rect {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            right: x + w,
            bottom: y + h,
        }
    }
    fn bounds(b: iced::Rectangle, offset: (f32, f32)) -> Self {
        Self::new(b.x - offset.0, b.y - offset.1, b.width, b.height)
    }
    fn width(self) -> f32 {
        self.right - self.x
    }
    fn height(self) -> f32 {
        self.bottom - self.y
    }
    fn integral(self) -> bool {
        [self.x, self.y, self.right, self.bottom]
            .iter()
            .all(|v| v.is_finite() && v.fract() == 0.)
    }
    fn intersect(self, other: Self) -> Self {
        Self {
            x: self.x.max(other.x),
            y: self.y.max(other.y),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }
}
fn append_quad(
    plan: &mut LayerPlan,
    id: u32,
    bounds: Rect,
    uv: [f32; 4],
    color: u32,
    backend: SpriteBackend,
    clip: Rect,
) {
    if bounds.width() <= 0. || bounds.height() <= 0. || color.to_le_bytes()[3] == 0 {
        return;
    }
    // Physical integer scissor edges keep glyph copies pixel-exact.
    let clip = Rect {
        x: clip.x.ceil(),
        y: clip.y.ceil(),
        right: clip.right.floor(),
        bottom: clip.bottom.floor(),
    };
    let r = bounds.intersect(clip);
    if r.width() <= 0. || r.height() <= 0. {
        return;
    }
    let u0 = uv[0] + (uv[2] - uv[0]) * (r.x - bounds.x) / bounds.width();
    let u1 = uv[0] + (uv[2] - uv[0]) * (r.right - bounds.x) / bounds.width();
    let v0 = uv[1] + (uv[3] - uv[1]) * (r.y - bounds.y) / bounds.height();
    let v1 = uv[1] + (uv[3] - uv[1]) * (r.bottom - bounds.y) / bounds.height();
    debug_assert!(r.integral());
    plan.commands.push(SpriteCommand {
        backend,
        quad: SpriteQuad {
            sprite_id: id,
            c0: SpriteCorner {
                x: r.x,
                y: r.y,
                u: u0,
                v: v0,
            },
            c1: SpriteCorner {
                x: r.right,
                y: r.y,
                u: u1,
                v: v0,
            },
            c2: SpriteCorner {
                x: r.right,
                y: r.bottom,
                u: u1,
                v: v1,
            },
            c3: SpriteCorner {
                x: r.x,
                y: r.bottom,
                u: u0,
                v: v1,
            },
            color_rgba: color,
            source_over: true,
        },
    });
}
fn packed_color(c: Rgba<f32>, opacity: f32) -> u32 {
    fn srgb(v: f32) -> u8 {
        let v = v.clamp(0., 1.);
        let s = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        };
        (s * 255. + 0.5) as u8
    }
    u32::from_le_bytes([
        srgb(c.r),
        srgb(c.g),
        srgb(c.b),
        (c.a * opacity * 255.).round().clamp(0., 255.) as u8,
    ])
}
pub(super) fn font_scale(size: u16) -> u32 {
    ((size as u32 + microfont::FHEIGHT as u32 / 2) / microfont::FHEIGHT as u32).max(1)
}
pub(super) fn native_text_lines(text: &str, max_columns: usize) -> Vec<Vec<(char, usize)>> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Vec::new()];
    for (offset, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(Vec::new());
            continue;
        }
        if lines.last().unwrap().len() >= max_columns.max(1) {
            lines.push(Vec::new());
        }
        lines.last_mut().unwrap().push((ch, offset));
    }
    lines
}
pub(super) fn native_line_metrics(text: &str, max_columns: usize) -> (usize, usize) {
    let lines = native_text_lines(text, max_columns);
    (lines.iter().map(Vec::len).max().unwrap_or(0), lines.len())
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PreparedKey {
    source: u32,
    crop: [u32; 4],
    width: u32,
    height: u32,
    tint: u32,
}
fn snap(r: Rect) -> Rect {
    Rect {
        x: r.x.round(),
        y: r.y.round(),
        right: r.right.round(),
        bottom: r.bottom.round(),
    }
}
fn premultiply(color: u32) -> u32 {
    let [r, g, b, a] = color.to_le_bytes();
    let m = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
    u32::from_le_bytes([m(r), m(g), m(b), a])
}

fn choose_backend(layer: &LayerPlan, bounds: Rect, partial: bool) -> SpriteBackend {
    // BCS preserves alpha against the scene plane. Source-over is still
    // necessary where a translucent UI sprite covers another UI primitive.
    if partial
        && layer.commands.iter().any(|c| {
            let q = &c.quad;
            bounds.x < q.c2.x
                && bounds.right > q.c0.x
                && bounds.y < q.c2.y
                && bounds.bottom > q.c0.y
        })
    {
        SpriteBackend::PremultipliedCompositor
    } else {
        SpriteBackend::Bcs0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds(x: f32, y: f32, w: f32, h: f32) -> iced::Rectangle {
        iced::Rectangle {
            x,
            y,
            width: w,
            height: h,
        }
    }
    fn sprite(id: graphic::Id, b: iced::Rectangle) -> Primitive {
        Primitive::Image {
            handle: (id, graphic::Rotation::None),
            bounds: b,
            color: Rgba::new(255, 255, 255, 255),
            source_rect: None,
        }
    }
    fn dialog(id: u8) -> Primitive {
        Primitive::Dialog {
            id,
            content: Box::new(Primitive::Rectangle {
                bounds: bounds(20., 20., 100., 80.),
                linear_color: Rgba::new(1., 1., 1., 1.),
            }),
        }
    }
    #[test]
    fn dense_dialog_transition_uses_one_snapshot_and_preserves_copy_path() {
        let mut renderer = Renderer::new(400, 300);
        let dense = Primitive::Dialog {
            id: 1,
            content: Box::new(Primitive::Group {
                primitives: (0..600)
                    .map(|i| Primitive::Rectangle {
                        bounds: bounds((i % 30) as f32 * 4., (i / 30) as f32 * 4., 3., 3.),
                        linear_color: Rgba::new(1., 0., 0., 1.),
                    })
                    .collect(),
            }),
        };
        renderer.prepare(&dense).unwrap();
        let replacement = renderer.prepare(&dialog(2)).unwrap();
        assert_eq!(replacement.foreground.commands.len(), 2);
        assert_eq!(
            replacement.foreground.commands[0].backend,
            SpriteBackend::Bcs0
        );
        let snapshot = &renderer.closing_dialog.as_ref().unwrap().0;
        assert_eq!(snapshot.commands.len(), 1);
        assert_eq!(snapshot.uploads.len(), 1);
        assert_eq!(
            snapshot.uploads[0].image.get_pixel(0, 0).0,
            [255, 0, 0, 255]
        );
        assert_eq!(snapshot.uploads[0].image.get_pixel(3, 0).0, [0; 4]);
        let id = snapshot.uploads[0].id;
        renderer.prepare(&dialog(3)).unwrap();
        assert_eq!(
            renderer.closing_dialog.as_ref().unwrap().0.uploads[0].id,
            id
        );
    }
    #[test]
    fn dialog_replacement_retains_only_one_outgoing_snapshot() {
        let mut renderer = Renderer::new(400, 300);
        assert_eq!(
            renderer
                .prepare(&dialog(1))
                .unwrap()
                .foreground
                .commands
                .len(),
            1
        );
        assert!(!renderer.dialog_animating());
        let replacement = renderer.prepare(&dialog(2)).unwrap();
        assert_eq!(replacement.foreground.commands.len(), 2);
        assert_eq!(renderer.dialog.as_ref().unwrap().0, 2);
        assert!(renderer.dialog_animating());
        // A third click replaces the ghost instead of accumulating dialogs.
        assert_eq!(
            renderer
                .prepare(&dialog(3))
                .unwrap()
                .foreground
                .commands
                .len(),
            2
        );
        renderer.closing_dialog.as_mut().unwrap().1 =
            std::time::Instant::now() - std::time::Duration::from_millis(501);
        assert_eq!(
            renderer
                .prepare(&dialog(3))
                .unwrap()
                .foreground
                .commands
                .len(),
            1
        );
        assert!(!renderer.dialog_animating());
        assert_eq!(
            renderer
                .prepare(&Primitive::Nothing)
                .unwrap()
                .foreground
                .commands
                .len(),
            1
        );
        assert!(renderer.dialog.is_none());
        renderer.closing_dialog.as_mut().unwrap().1 =
            std::time::Instant::now() - std::time::Duration::from_millis(501);
        assert!(
            renderer
                .prepare(&Primitive::Nothing)
                .unwrap()
                .foreground
                .commands
                .is_empty()
        );
        assert!(!renderer.dialog_animating());
    }
    #[test]
    fn dialog_fade_scales_about_center_and_modulates_premultiplied_color() {
        let mut renderer = Renderer::new(400, 300);
        renderer.prepare(&dialog(1)).unwrap();
        let original = renderer.dialog.as_ref().unwrap().1.clone();
        let mut half = original.clone();
        animate_dialog(&mut half, 0.5);
        let quad = half.commands[0].quad;
        assert!((quad.c0.x - 17.5).abs() < 0.001);
        assert!((quad.c0.y - 18.).abs() < 0.001);
        assert_eq!(quad.color_rgba.to_le_bytes(), [128; 4]);
        assert_eq!(
            half.commands[0].backend,
            SpriteBackend::PremultipliedCompositor
        );
        let mut end = original;
        animate_dialog(&mut end, 1.0);
        assert!((end.commands[0].quad.c1.x - end.commands[0].quad.c0.x - 110.).abs() < 0.001);
        assert_eq!(end.commands[0].quad.color_rgba, 0);
    }
    #[test]
    fn scene_and_foreground_keep_separate_sources_and_alpha() {
        let mut renderer = Renderer::new(8, 8);
        let bg = renderer.add_image(Arc::new(::image::RgbaImage::from_pixel(
            2,
            2,
            ::image::Rgba([30, 60, 90, 255]),
        )));
        renderer.mark_scene_image(bg);
        let ui = renderer.add_image(Arc::new(::image::RgbaImage::from_pixel(
            2,
            2,
            ::image::Rgba([200, 100, 50, 128]),
        )));
        let plan = renderer
            .prepare(&Primitive::Group {
                primitives: vec![
                    sprite(bg, bounds(0., 0., 8., 8.)),
                    sprite(ui, bounds(2., 2., 2., 2.)),
                ],
            })
            .unwrap();
        assert_eq!(plan.background.commands.len(), 1);
        assert_eq!(plan.foreground.commands.len(), 1);
        assert_eq!(plan.foreground.commands[0].backend, SpriteBackend::Bcs0);
        assert_eq!(
            plan.foreground.uploads[0].image.get_pixel(0, 0).0,
            [100, 50, 25, 128]
        );
        assert_eq!(plan.foreground.uploads[0].image.dimensions(), (2, 2));
    }
    #[test]
    fn static_assets_reuse_the_same_upload_allocation() {
        let mut renderer = Renderer::new(8, 8);
        let id = renderer.add_image(Arc::new(::image::RgbaImage::from_pixel(
            2,
            2,
            ::image::Rgba([255; 4]),
        )));
        let primitive = sprite(id, bounds(1., 1., 4., 4.));
        let first = renderer.prepare(&primitive).unwrap();
        let initial = renderer.take_activity();
        assert_eq!(initial.images, 1);
        assert_eq!(initial.bytes, 4 * 4 * 4);
        let second = renderer.prepare(&primitive).unwrap();
        let reused = renderer.take_activity();
        assert_eq!(
            (reused.images, reused.glyphs, reused.gradients, reused.bytes),
            (0, 0, 0, 0)
        );
        assert_eq!(
            first.foreground.uploads[0].id,
            second.foreground.uploads[0].id
        );
        assert!(Arc::ptr_eq(
            &first.foreground.uploads[0].image,
            &second.foreground.uploads[0].image
        ));
    }
    #[test]
    fn translucent_overlap_composes_only_inside_foreground() {
        let mut renderer = Renderer::new(8, 8);
        let id = renderer.add_image(Arc::new(::image::RgbaImage::from_pixel(
            2,
            2,
            ::image::Rgba([255, 0, 0, 128]),
        )));
        let plan = renderer
            .prepare(&Primitive::Group {
                primitives: vec![
                    Primitive::Rectangle {
                        bounds: bounds(0., 0., 8., 8.),
                        linear_color: Rgba::new(0., 0., 1., 1.),
                    },
                    sprite(id, bounds(1., 1., 2., 2.)),
                    sprite(id, bounds(6., 6., 2., 2.)),
                ],
            })
            .unwrap();
        assert_eq!(plan.foreground.commands[0].backend, SpriteBackend::Bcs0);
        assert_eq!(
            plan.foreground.commands[1].backend,
            SpriteBackend::PremultipliedCompositor
        );
        assert!(plan.background.commands.is_empty());
    }
    #[test]
    fn integer_scissor_crops_uv_without_scaling_the_sprite() {
        let mut renderer = Renderer::new(8, 8);
        let id = renderer.add_image(Arc::new(::image::RgbaImage::from_pixel(
            4,
            4,
            ::image::Rgba([255; 4]),
        )));
        let plan = renderer
            .prepare(&Primitive::Clip {
                bounds: bounds(2., 2., 2., 2.),
                offset: vek::Vec2::zero(),
                content: Box::new(sprite(id, bounds(1., 1., 4., 4.))),
            })
            .unwrap();
        let quad = plan.foreground.commands[0].quad;
        assert_eq!(
            (quad.c0.x, quad.c0.y, quad.c2.x, quad.c2.y),
            (2., 2., 4., 4.)
        );
        assert_eq!(
            (quad.c0.u, quad.c0.v, quad.c2.u, quad.c2.v),
            (0.25, 0.25, 0.75, 0.75)
        );
        assert_eq!(plan.foreground.commands[0].backend, SpriteBackend::Bcs0);
    }
    #[test]
    fn microfont_masks_retain_zero_holes_and_premultiplied_ink() {
        let mut renderer = Renderer::new(8, 8);
        let (id, image) = renderer.glyph(b'A', 1, u32::from_le_bytes([255, 200, 0, 128]));
        let (again, cached) = renderer.glyph(b'A', 1, u32::from_le_bytes([255, 200, 0, 128]));
        assert_eq!(id, again);
        assert!(Arc::ptr_eq(&image, &cached));
        assert!(image.pixels().any(|p| p.0 == [0; 4]));
        assert!(image.pixels().any(|p| p.0 == [128, 100, 0, 128]));
        assert_eq!(image.dimensions(), (6, 11));
    }
    #[test]
    fn empty_ui_plan_does_not_bake_an_opaque_frame() {
        let mut renderer = Renderer::new(1920, 1080);
        let plan = renderer.prepare(&Primitive::Nothing).unwrap();
        assert!(plan.foreground.commands.is_empty());
        assert!(plan.foreground.uploads.is_empty());
    }
}
