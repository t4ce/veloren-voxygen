//! Conform greedy rectangle boundaries once, before publishing retained terrain.
use super::Vertex;
use std::collections::{BTreeMap, BTreeSet};

type Point = [i32; 3];
type Line = (usize, i32, i32);
fn line(point: Point, axis: usize) -> Line {
    (axis, point[(axis + 1) % 3], point[(axis + 2) % 3])
}
fn edge_axis(a: Point, b: Point) -> usize {
    (0..3).find(|&axis| a[axis] != b[axis]).unwrap()
}
fn rectangle(vertices: &[Vertex]) -> [Point; 4] {
    [0, 1, 2, 5].map(|i| std::array::from_fn(|axis| vertices[i].position[axis] as i32))
}

pub(super) fn stitch(vertices: &[Vertex]) -> Result<Vec<Vertex>, String> {
    if vertices.len() % 6 != 0 {
        return Err("terrain rectangles are incomplete".into());
    }
    // Only actual edge lines need storage. Include endpoints from all face
    // orientations: perpendicular faces and chunk boundaries must agree too.
    let mut lines = BTreeMap::<Line, BTreeSet<i32>>::new();
    for quad in vertices.chunks_exact(6) {
        let points = rectangle(quad);
        for i in 0..4 {
            let (a, b) = (points[i], points[(i + 1) % 4]);
            let axis = edge_axis(a, b);
            let values = lines.entry(line(a, axis)).or_default();
            values.insert(a[axis]);
            values.insert(b[axis]);
        }
    }
    for quad in vertices.chunks_exact(6) {
        for point in rectangle(quad) {
            for axis in 0..3 {
                if let Some(values) = lines.get_mut(&line(point, axis)) {
                    values.insert(point[axis]);
                }
            }
        }
    }
    let mut output = Vec::with_capacity(vertices.len());
    for quad in vertices.chunks_exact(6) {
        let points = rectangle(quad);
        let mut boundary = Vec::new();
        for i in 0..4 {
            let (a, b) = (points[i], points[(i + 1) % 4]);
            let axis = edge_axis(a, b);
            boundary.push(a);
            let values = &lines[&line(a, axis)];
            let interior = values.range((
                std::ops::Bound::Excluded(a[axis].min(b[axis])),
                std::ops::Bound::Excluded(a[axis].max(b[axis])),
            ));
            let mut append = |&value: &i32| {
                let mut point = a;
                point[axis] = value;
                boundary.push(point);
            };
            if a[axis] < b[axis] {
                interior.for_each(&mut append);
            } else {
                interior.rev().for_each(&mut append);
            }
        }
        if boundary.len() == 4 {
            output.extend_from_slice(quad);
            continue;
        }
        triangulate(&mut output, boundary, quad[0].atlas_uv)?;
    }
    Ok(output)
}

fn cross(a: Point, b: Point, c: Point, u: usize, v: usize) -> i64 {
    (i64::from(b[u]) - i64::from(a[u])) * (i64::from(c[v]) - i64::from(a[v]))
        - (i64::from(b[v]) - i64::from(a[v])) * (i64::from(c[u]) - i64::from(a[u]))
}
fn triangulate(
    output: &mut Vec<Vertex>,
    mut polygon: Vec<Point>,
    color: [f32; 4],
) -> Result<(), String> {
    let axis = (0..3)
        .find(|&axis| polygon.iter().all(|p| p[axis] == polygon[0][axis]))
        .ok_or("terrain rectangle is not planar")?;
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let winding = (0..polygon.len())
        .map(|i| {
            cross(
                polygon[0],
                polygon[i],
                polygon[(i + 1) % polygon.len()],
                u,
                v,
            )
        })
        .sum::<i64>()
        .signum();
    if winding == 0 {
        return Err("terrain rectangle is degenerate".into());
    }
    let mut emit = |points: [Point; 3]| {
        output.extend(points.map(|p| Vertex {
            position: [p[0] as f32, p[1] as f32, p[2] as f32, 1.],
            atlas_uv: color,
        }));
    };
    while polygon.len() > 3 {
        let ear = (0..polygon.len())
            .find(|&i| {
                let (prev, next) = (
                    (i + polygon.len() - 1) % polygon.len(),
                    (i + 1) % polygon.len(),
                );
                let (a, b, c) = (polygon[prev], polygon[i], polygon[next]);
                if cross(a, b, c, u, v) * winding <= 0 {
                    return false;
                }
                // A vertex on the new diagonal must block this ear too; otherwise
                // triangulation could reintroduce the very T-junction we removed.
                !polygon.iter().enumerate().any(|(j, &p)| {
                    j != prev
                        && j != i
                        && j != next
                        && cross(a, b, p, u, v) * winding >= 0
                        && cross(b, c, p, u, v) * winding >= 0
                        && cross(c, a, p, u, v) * winding >= 0
                })
            })
            .ok_or("terrain boundary could not be triangulated")?;
        emit([
            polygon[(ear + polygon.len() - 1) % polygon.len()],
            polygon[ear],
            polygon[(ear + 1) % polygon.len()],
        ]);
        polygon.remove(ear);
    }
    if cross(polygon[0], polygon[1], polygon[2], u, v) * winding <= 0 {
        return Err("terrain boundary ended in a degenerate triangle".into());
    }
    emit([polygon[0], polygon[1], polygon[2]]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn has_t_junction(vertices: &[Vertex]) -> bool {
        let points = vertices
            .iter()
            .map(|v| {
                [
                    v.position[0] as i32,
                    v.position[1] as i32,
                    v.position[2] as i32,
                ]
            })
            .collect::<BTreeSet<_>>();
        vertices.chunks_exact(3).any(|triangle| {
            (0..3).any(|i| {
                let a = triangle[i].position;
                let b = triangle[(i + 1) % 3].position;
                let a = [a[0] as i32, a[1] as i32, a[2] as i32];
                let b = [b[0] as i32, b[1] as i32, b[2] as i32];
                points.iter().any(|&p| {
                    p != a
                        && p != b
                        && (0..3).all(|axis| {
                            p[axis] >= a[axis].min(b[axis]) && p[axis] <= a[axis].max(b[axis])
                        })
                        && cross(a, b, p, 0, 1) == 0
                        && cross(a, b, p, 1, 2) == 0
                        && cross(a, b, p, 2, 0) == 0
                })
            })
        })
    }
    #[test]
    fn l_shaped_greedy_surface_has_a_real_junction_and_is_repaired() {
        let mut original = Vec::new();
        super::super::merge_plane(
            &mut original,
            5,
            7,
            vec![(0, 0, [20; 3]), (1, 0, [20; 3]), (0, 1, [20; 3])],
        )
        .unwrap();
        assert!(has_t_junction(&original));
        let result = stitch(&original).unwrap();
        assert!(!has_t_junction(&result));
        assert_eq!(result.len(), 15); // one extra triangle, no center fan
    }
    #[test]
    fn every_small_face_mask_preserves_coverage_winding_color_and_edges() {
        for side in 0..6 {
            for mask in 1..512 {
                let cells = (0..9)
                    .filter(|i| mask & (1 << i) != 0)
                    .map(|i| (i % 3 - 33, i / 3 - 33, [31, 140, 47]))
                    .collect::<Vec<_>>();
                let mut original = Vec::new();
                super::super::merge_plane(&mut original, side, 7, cells.clone()).unwrap();
                let result = stitch(&original).unwrap();
                assert!(!has_t_junction(&result), "side={side} mask={mask}");
                let axis = side / 2;
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let mut area = 0i64;
                for triangle in result.chunks_exact(3) {
                    let points = triangle
                        .iter()
                        .map(|v| {
                            [
                                v.position[0] as i32,
                                v.position[1] as i32,
                                v.position[2] as i32,
                            ]
                        })
                        .collect::<Vec<_>>();
                    let signed = cross(points[0], points[1], points[2], u, v);
                    assert!(signed * (if side % 2 == 0 { -1 } else { 1 }) > 0);
                    area += signed.abs();
                    assert!(
                        triangle
                            .iter()
                            .all(|v| v.position[axis] == 7. && v.atlas_uv == original[0].atlas_uv)
                    );
                }
                assert_eq!(area, cells.len() as i64 * 2);
            }
        }
    }
    #[test]
    fn matching_rectangles_are_unchanged_and_perpendicular_join_is_split() {
        let mut original = Vec::new();
        super::super::merge_plane(&mut original, 5, 0, vec![(0, 0, [20; 3]), (1, 0, [20; 3])])
            .unwrap();
        let unchanged = stitch(&original).unwrap();
        assert_eq!(unchanged.len(), original.len());
        for (a, b) in original.iter().zip(&unchanged) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.atlas_uv, b.atlas_uv);
        }
        // A perpendicular face ends at the middle of the first rectangle's edge.
        super::super::merge_plane(&mut original, 1, 1, vec![(0, 0, [40; 3])]).unwrap();
        assert!(has_t_junction(&original));
        assert!(!has_t_junction(&stitch(&original).unwrap()));
    }
}
