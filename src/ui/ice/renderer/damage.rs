//! Menu damage: edits/carets, hover, server rows, scroll and dialog changes.
//! Two foreground buffers alternate: repaint the preceding publication's
//! damage too, since the newly leased buffer still contains the older UI.
use super::bcs::LayerPlan;
use trueos::ui4_solara_text::{Damage, SpriteCommand};

pub(crate) struct Repaint {
    pub changed: Damage,
    pub region: Damage,
    pub commands: Vec<SpriteCommand>,
}

pub(crate) fn prepare(
    previous: Option<((u32, u32), &LayerPlan)>,
    current: &LayerPlan,
    size: (u32, u32),
    preceding_damage: Option<Damage>,
) -> Repaint {
    let full = Damage::full(size.0, size.1);
    let changed = if current.commands.iter().any(|c| !axis_aligned(c)) {
        full
    } else if let Some((_old_size, old)) = previous.filter(|(s, _)| *s == size) {
        changed_bounds(old, current, size).unwrap_or(full)
    } else {
        full
    };
    let region = union(changed, preceding_damage.unwrap_or(changed));
    let commands = current
        .commands
        .iter()
        .filter_map(|command| clip(*command, region))
        .collect();
    Repaint {
        changed,
        region,
        commands,
    }
}

pub(crate) fn union(a: Damage, b: Damage) -> Damage {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Damage {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

fn changed_bounds(old: &LayerPlan, new: &LayerPlan, size: (u32, u32)) -> Option<Damage> {
    // Retain matching command prefixes/suffixes, preserving paint order. A
    // typed glyph or appended row doesn't invalidate static dialog furniture.
    let prefix = old
        .commands
        .iter()
        .zip(&new.commands)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old.commands[prefix..]
        .iter()
        .rev()
        .zip(new.commands[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let changed = old.commands[prefix..old.commands.len() - suffix]
        .iter()
        .chain(&new.commands[prefix..new.commands.len() - suffix]);
    let mut result = None;
    for command in changed {
        let rect = bounds(command, size)?;
        if rect.width > 0 && rect.height > 0 {
            result = Some(result.map_or(rect, |prior| union(prior, rect)));
        }
    }
    // A reused upload ID can change its pixels without changing geometry.
    for upload in &new.uploads {
        if old.uploads.iter().any(|previous| {
            previous.id == upload.id && std::sync::Arc::ptr_eq(&previous.image, &upload.image)
        }) {
            continue;
        }
        for command in old
            .commands
            .iter()
            .chain(&new.commands)
            .filter(|command| command.quad.sprite_id == upload.id)
        {
            let rect = bounds(command, size)?;
            if rect.width > 0 && rect.height > 0 {
                result = Some(result.map_or(rect, |prior| union(prior, rect)));
            }
        }
    }
    result
}

fn axis_aligned(command: &SpriteCommand) -> bool {
    let q = &command.quad;
    [q.c0, q.c1, q.c2, q.c3]
        .iter()
        .all(|c| c.x.is_finite() && c.y.is_finite())
        && q.c0.y == q.c1.y
        && q.c1.x == q.c2.x
        && q.c2.y == q.c3.y
        && q.c3.x == q.c0.x
        && q.c2.x > q.c0.x
        && q.c2.y > q.c0.y
        && q.c0.u == q.c3.u
        && q.c1.u == q.c2.u
        && q.c0.v == q.c1.v
        && q.c2.v == q.c3.v
}
fn bounds(command: &SpriteCommand, size: (u32, u32)) -> Option<Damage> {
    if !axis_aligned(command) {
        return None;
    }
    let q = &command.quad;
    let x = q.c0.x.floor().clamp(0., size.0 as f32) as u32;
    let y = q.c0.y.floor().clamp(0., size.1 as f32) as u32;
    let right = q.c2.x.ceil().clamp(0., size.0 as f32) as u32;
    let bottom = q.c2.y.ceil().clamp(0., size.1 as f32) as u32;
    Some(Damage {
        x,
        y,
        width: right.saturating_sub(x),
        height: bottom.saturating_sub(y),
    })
}
fn clip(mut command: SpriteCommand, region: Damage) -> Option<SpriteCommand> {
    // Unsupported geometry forces full damage; preserve the original quad.
    if !axis_aligned(&command) {
        return Some(command);
    }
    let q = &mut command.quad;
    let left = q.c0.x.max(region.x as f32);
    let top = q.c0.y.max(region.y as f32);
    let right = q.c2.x.min((region.x + region.width) as f32);
    let bottom = q.c2.y.min((region.y + region.height) as f32);
    if right <= left || bottom <= top {
        return None;
    }
    let u = |x| q.c0.u + (q.c1.u - q.c0.u) * (x - q.c0.x) / (q.c1.x - q.c0.x);
    let v = |y| q.c0.v + (q.c3.v - q.c0.v) * (y - q.c0.y) / (q.c3.y - q.c0.y);
    let (u0, u1, v0, v1) = (u(left), u(right), v(top), v(bottom));
    q.c0.x = left;
    q.c0.y = top;
    q.c0.u = u0;
    q.c0.v = v0;
    q.c1.x = right;
    q.c1.y = top;
    q.c1.u = u1;
    q.c1.v = v0;
    q.c2.x = right;
    q.c2.y = bottom;
    q.c2.u = u1;
    q.c2.v = v1;
    q.c3.x = left;
    q.c3.y = bottom;
    q.c3.u = u0;
    q.c3.v = v1;
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos::ui4_solara_text::{SpriteBackend, SpriteCorner, SpriteQuad};
    fn rect(x: u32, y: u32, width: u32, height: u32) -> Damage {
        Damage {
            x,
            y,
            width,
            height,
        }
    }
    fn command(id: u32, x: f32, y: f32, w: f32, h: f32) -> SpriteCommand {
        let corner = |x, y, u, v| SpriteCorner { x, y, u, v };
        SpriteCommand {
            backend: SpriteBackend::Bcs0,
            quad: SpriteQuad {
                sprite_id: id,
                c0: corner(x, y, 0., 0.),
                c1: corner(x + w, y, 1., 0.),
                c2: corner(x + w, y + h, 1., 1.),
                c3: corner(x, y + h, 0., 1.),
                color_rgba: u32::MAX,
                source_over: true,
            },
        }
    }
    fn plan(commands: Vec<SpriteCommand>) -> LayerPlan {
        LayerPlan {
            commands,
            uploads: vec![],
        }
    }
    fn diff(old: &LayerPlan, new: &LayerPlan) -> Repaint {
        prepare(Some(((400, 300), old)), new, (400, 300), None)
    }
    #[test]
    fn typing_only_repaints_changed_glyph_and_replays_clipped_editbox() {
        let background = command(0, 10., 20., 200., 30.);
        let old = plan(vec![
            background,
            command(1, 20., 25., 6., 11.),
            command(2, 26., 25., 6., 11.),
        ]);
        let new = plan(vec![
            background,
            command(1, 20., 25., 6., 11.),
            command(3, 26., 25., 6., 11.),
        ]);
        let repaint = diff(&old, &new);
        assert_eq!(repaint.changed, rect(26, 25, 6, 11));
        assert_eq!(repaint.commands.len(), 2);
        let clipped = repaint.commands[0].quad;
        assert_eq!(
            (clipped.c0.x, clipped.c0.y, clipped.c2.x, clipped.c2.y),
            (26., 25., 32., 36.)
        );
        assert!((clipped.c0.u - 0.08).abs() < 0.00001);
    }
    #[test]
    fn appended_server_row_leaves_existing_rows_untouched() {
        let old = plan(vec![
            command(0, 10., 10., 200., 200.),
            command(1, 20., 20., 180., 30.),
        ]);
        let mut new = old.clone();
        new.commands.push(command(2, 20., 50., 180., 30.));
        let repaint = diff(&old, &new);
        assert_eq!(repaint.changed, rect(20, 50, 180, 30));
        assert_eq!(repaint.commands.len(), 2);
    }
    #[test]
    fn scroll_repaints_viewport_without_static_header() {
        let old = plan(vec![
            command(1, 0., 0., 200., 20.),
            command(2, 10., 30., 180., 100.),
        ]);
        let new = plan(vec![old.commands[0], command(3, 10., 30., 180., 100.)]);
        assert_eq!(diff(&old, &new).changed, rect(10, 30, 180, 100));
    }
    #[test]
    fn hover_and_dialog_switch_cover_affected_panels() {
        let old = plan(vec![
            command(0, 20., 20., 200., 200.),
            command(1, 30., 40., 80., 20.),
        ]);
        let new = plan(vec![old.commands[0], command(2, 30., 40., 80., 20.)]);
        assert_eq!(diff(&old, &new).changed, rect(30, 40, 80, 20));
        let switched = plan(vec![command(3, 40., 40., 200., 200.)]);
        assert_eq!(diff(&old, &switched).changed, rect(20, 20, 220, 220));
    }
    #[test]
    fn alternating_buffer_debt_repaints_previous_change_but_publishes_current_damage() {
        let old = plan(vec![
            command(1, 20., 20., 6., 11.),
            command(2, 100., 20., 6., 11.),
        ]);
        let new = plan(vec![old.commands[0], command(3, 100., 20., 6., 11.)]);
        let repaint = prepare(
            Some(((400, 300), &old)),
            &new,
            (400, 300),
            Some(rect(20, 20, 6, 11)),
        );
        assert_eq!(repaint.changed, rect(100, 20, 6, 11));
        assert_eq!(repaint.region, rect(20, 20, 86, 11));
        assert_eq!(repaint.commands.len(), 2);
    }
    #[test]
    fn startup_and_resize_initialize_entire_alternating_pair() {
        let new = plan(vec![command(1, 20., 20., 6., 11.)]);
        let first = prepare(None, &new, (400, 300), None);
        assert_eq!(first.region, Damage::full(400, 300));
        let changed = plan(vec![command(2, 20., 20., 6., 11.)]);
        let second = prepare(
            Some(((400, 300), &new)),
            &changed,
            (400, 300),
            Some(first.changed),
        );
        assert_eq!(second.changed, rect(20, 20, 6, 11));
        assert_eq!(second.region, Damage::full(400, 300));
        let resized = prepare(
            Some(((400, 300), &new)),
            &new,
            (800, 600),
            Some(first.changed),
        );
        assert_eq!(resized.region, Damage::full(800, 600));
        let after_resize = prepare(
            Some(((800, 600), &new)),
            &changed,
            (800, 600),
            Some(resized.changed),
        );
        assert_eq!(after_resize.changed, rect(20, 20, 6, 11));
        assert_eq!(after_resize.region, Damage::full(800, 600));
    }
    #[test]
    fn replaced_upload_at_same_geometry_damages_its_rect() {
        use std::sync::Arc;
        let mut old = plan(vec![command(1, 20., 20., 60., 40.)]);
        old.uploads.push(super::super::bcs::Upload {
            id: 1,
            image: Arc::new(image::RgbaImage::new(60, 40)),
        });
        let mut new = old.clone();
        new.uploads[0].image = Arc::new(image::RgbaImage::new(60, 40));
        assert_eq!(diff(&old, &new).changed, rect(20, 20, 60, 40));
    }
    #[test]
    fn non_axis_geometry_uses_full_frame_even_if_unchanged_overlap() {
        let mut rotated = command(1, 20., 20., 60., 40.);
        rotated.quad.c1.y += 1.;
        let old = plan(vec![rotated, command(2, 100., 100., 6., 11.)]);
        let new = plan(vec![rotated, command(3, 100., 100., 6., 11.)]);
        assert_eq!(diff(&old, &new).region, Damage::full(400, 300));
    }
}
