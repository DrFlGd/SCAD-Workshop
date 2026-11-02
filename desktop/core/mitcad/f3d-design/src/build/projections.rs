// SPDX-License-Identifier: MIT
//! What a sketch's linked curves follow, where it is a curve of an earlier
//! sketch (a projection of another sketch's geometry).
//!
//! A sketch feature ([`SKETCH_FEATURE`]) lists one projection record
//! ([`SKETCH_PROJECTION`]) for each entity projected into its sketch. The
//! record refers to an input slot ([`INPUT_SLOT`]) whose entity input
//! ([`ENTITY_REF`]) names the source as sweeps' paths do: a sketch curve by
//! its ids ([`SKETCH_CURVE_ID`]), the curve as it was stored next to it.
//! The record does not name the curve it made, so each source is matched to
//! the sketch's linked curve that its projection onto the sketch's plane
//! is: the same kind, the same points. On a design with 233 linked curves
//! in 12 sketches (projections of two earlier sketches' profiles) all 250
//! records resolve and each lies on one linked curve; 228 are in the order
//! of the curves, so the order is not used.
//!
//! The linked curve gets the source as its `referencedEntity` (a
//! `sketch_entity` reference, as the API gives it). A record whose input
//! names a face or an edge ([`FACE_REF`]) keeps that face's boundary or
//! that edge's curve, in the source's component's coordinates: each
//! stored curve is matched the same way, and the linked curve gets the face
//! or the edge with that one curve. A source in a component that an item
//! of the timeline has moved (a captured position: the latching box's
//! latch, projected into the box's sketches) is matched where that item
//! put it. Records whose input names a point are left alone.

use super::super::classes::*;
use super::super::ir::*;
use super::super::sketch::SketchInfo;
use super::super::stream::Segment;
use super::Builder;

/// How far (cm) a projected source point may lie from the linked curve's
/// to be that curve: both are the file's numbers, so only rounding.
const SAME: f64 = 1e-6;

/// A curve's kind and characteristic points in model space (cm): a line's
/// ends, a circle's centre, an arc's centre and ends; a circle's or an
/// arc's radius.
struct Shape {
    kind: String,
    points: Vec<[f64; 3]>,
    radius: Option<f64>,
    /// Points whose order does not matter (a line's or an arc's ends).
    unordered: bool,
    /// A circle's or an arc's centre, axes and range, for its projection
    /// seen edge on.
    arc: Option<Arc>,
}

/// A circle or an arc in model space (cm): `centre + radius (cos t u + sin t
/// v)` for `t` from `start` to `end`.
struct Arc {
    centre: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    radius: f64,
    start: f64,
    end: f64,
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|x| x * s)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A sketch's frame in model space: origin, x and y axis.
struct Frame {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
}

impl Frame {
    fn of(s: &SketchInfo) -> Option<Frame> {
        let f = s.detail.model_frame.as_ref()?;
        Some(Frame {
            origin: f.origin?,
            x: f.x_axis?,
            y: f.y_axis?,
        })
    }

    fn to_model(&self, p: [f64; 3]) -> [f64; 3] {
        add(self.origin, self.direction(p))
    }

    /// A sketch direction in model space.
    fn direction(&self, d: [f64; 3]) -> [f64; 3] {
        add(scale(self.x, d[0]), scale(self.y, d[1]))
    }

    fn normal(&self) -> [f64; 3] {
        cross(self.x, self.y)
    }

    /// A model point projected along the frame's normal onto its plane.
    fn onto(&self, p: [f64; 3]) -> [f64; 3] {
        let d = sub(p, self.origin);
        self.to_model([dot(d, self.x), dot(d, self.y), 0.0])
    }
}

/// A curve of a sketch with `frame` as a [`Shape`] in model space.
fn shape(frame: &Frame, c: &SketchCurve) -> Option<Shape> {
    let kind = c.curve_type.clone()?;
    let g = c.geometry.as_ref()?;
    let at = |p: [f64; 3]| frame.to_model(p);
    match kind.as_str() {
        "SketchLine" => Some(Shape {
            points: vec![at(g.start_point?), at(g.end_point?)],
            radius: None,
            kind,
            unordered: true,
            arc: None,
        }),
        "SketchCircle" => {
            let (c, r) = (g.center?, g.radius?);
            Some(Shape {
                points: vec![at(c)],
                radius: Some(r),
                kind,
                unordered: false,
                arc: Some(Arc {
                    centre: at(c),
                    u: frame.x,
                    v: frame.y,
                    radius: r,
                    start: 0.0,
                    end: std::f64::consts::TAU,
                }),
            })
        }
        "SketchArc" => {
            let (c, r) = (g.center?, g.radius?);
            let (u, n) = (g.reference_vector?, g.normal?);
            let v = cross(n, u);
            let end = |t: f64| add(c, add(scale(u, r * t.cos()), scale(v, r * t.sin())));
            Some(Shape {
                points: vec![at(c), at(end(g.start_angle?)), at(end(g.end_angle?))],
                radius: Some(r),
                kind,
                unordered: true,
                arc: Some(Arc {
                    centre: at(c),
                    u: frame.direction(u),
                    v: frame.direction(v),
                    radius: r,
                    start: g.start_angle?,
                    end: g.end_angle?,
                }),
            })
        }
        _ => None,
    }
}

/// A stored curve (model space, cm: a face's or an edge's input keeps
/// them, `super::super::curve`) as a [`Shape`]: a line, or an arc or a whole
/// circle.
fn stored_shape(g: &Geometry) -> Option<Shape> {
    match g.geometry_type.as_deref()? {
        "Line3D" => Some(Shape {
            kind: "SketchLine".into(),
            points: vec![g.start_point?, g.end_point?],
            radius: None,
            unordered: true,
            arc: None,
        }),
        "Arc3D" => {
            let (c, r, u, n) = (g.center?, g.radius?, g.reference_vector?, g.normal?);
            let (t0, t1) = (g.start_angle?, g.end_angle?);
            let v = cross(n, u);
            let at = |t: f64| add(c, add(scale(u, r * t.cos()), scale(v, r * t.sin())));
            let arc = Arc {
                centre: c,
                u,
                v,
                radius: r,
                start: t0,
                end: t1,
            };
            if (t1 - t0 - std::f64::consts::TAU).abs() < 1e-9 {
                Some(Shape {
                    kind: "SketchCircle".into(),
                    points: vec![c],
                    radius: Some(r),
                    unordered: false,
                    arc: Some(arc),
                })
            } else {
                Some(Shape {
                    kind: "SketchArc".into(),
                    points: vec![c, at(t0), at(t1)],
                    radius: Some(r),
                    unordered: true,
                    arc: Some(arc),
                })
            }
        }
        _ => None,
    }
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    let d = sub(a, b);
    dot(d, d).sqrt() <= SAME
}

/// Whether `arc`, seen edge on from `frame` (its plane holds the frame's
/// normal), projects onto the line from `a` to `b` (on the frame's plane):
/// its centre on the line, its extremes along the line the line's ends.
fn edge_on(frame: &Frame, arc: &Arc, a: [f64; 3], b: [f64; 3]) -> bool {
    let n = frame.normal();
    let axis = cross(arc.u, arc.v);
    if dot(axis, n).abs() > 1e-9 {
        return false;
    }
    let len = dot(sub(b, a), sub(b, a)).sqrt();
    if len <= SAME {
        return false;
    }
    let d = scale(sub(b, a), 1.0 / len);
    // The arc's plane is the one through the line along the normal.
    if dot(axis, d).abs() > 1e-9 {
        return false;
    }
    let c = sub(frame.onto(arc.centre), a);
    let off = sub(c, scale(d, dot(c, d)));
    if dot(off, off).sqrt() > SAME {
        return false;
    }
    // Along the line: dot(c, d) + r (cos t (u.d) + sin t (v.d)), whose
    // extremes are at the range's ends or where the derivative vanishes.
    let (p, q) = (dot(arc.u, d), dot(arc.v, d));
    let at = |t: f64| dot(c, d) + arc.radius * (t.cos() * p + t.sin() * q);
    let peak = q.atan2(p);
    let mut ts = vec![arc.start, arc.end];
    for k in -2..=2 {
        for t in [peak, peak + std::f64::consts::PI] {
            let t = t + k as f64 * std::f64::consts::TAU;
            if t > arc.start && t < arc.end {
                ts.push(t);
            }
        }
    }
    let along: Vec<f64> = ts.into_iter().map(at).collect();
    let lo = along.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = along.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    lo.abs() <= SAME && (hi - len).abs() <= SAME
}

/// Whether `source`, projected onto `frame`'s plane, is `curve`: the same
/// kind with the same points, or a circle or an arc seen edge on as a
/// line.
fn projects_onto(frame: &Frame, source: &Shape, curve: &Shape) -> bool {
    if let (Some(arc), "SketchLine") = (&source.arc, curve.kind.as_str()) {
        let (a, b) = (curve.points[0], curve.points[1]);
        return edge_on(frame, arc, a, b) || edge_on(frame, arc, b, a);
    }
    if source.kind != curve.kind || source.points.len() != curve.points.len() {
        return false;
    }
    match (source.radius, curve.radius) {
        (Some(a), Some(b)) if (a - b).abs() > SAME => return false,
        (Some(_), None) | (None, Some(_)) => return false,
        _ => {}
    }
    let s: Vec<[f64; 3]> = source.points.iter().map(|&p| frame.onto(p)).collect();
    let same = |a: &[[f64; 3]], b: &[[f64; 3]]| a.iter().zip(b).all(|(p, q)| near(*p, *q));
    if same(&s, &curve.points) {
        return true;
    }
    // A line's or an arc's ends either way round (the centre stays first).
    if source.unordered {
        let keep = if source.kind == "SketchArc" { 1 } else { 0 };
        let mut r = s.clone();
        r[keep..].reverse();
        return same(&r, &curve.points);
    }
    false
}

/// `p` (cm) placed by the rigid placement `m`.
fn place(m: &Mat4, p: [f64; 3]) -> [f64; 3] {
    add(turn(m, p), [m[0][3], m[1][3], m[2][3]])
}

/// The direction `d` turned by the rigid placement `m`.
fn turn(m: &Mat4, d: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| dot([m[r][0], m[r][1], m[r][2]], d))
}

/// A stored line or arc placed by `m`.
fn placed(g: &Geometry, m: &Mat4) -> Geometry {
    let mut g = g.clone();
    for p in [&mut g.start_point, &mut g.end_point, &mut g.center] {
        *p = p.map(|p| place(m, p));
    }
    for d in [&mut g.normal, &mut g.reference_vector] {
        *d = d.map(|d| turn(m, d));
    }
    g
}

/// The placements the timeline's items put occurrences at, other than
/// where the file has them (the identity): a face's or an edge's stored
/// curves are in its component's coordinates, and a sketch of another
/// component projects them from where an item (a captured position) has
/// put that component.
fn placement_moves(seg: &Segment) -> Vec<Mat4> {
    let mut out: Vec<Mat4> = Vec::new();
    for o in seg.objects_of(OCCURRENCE_PLACEMENTS) {
        for (_, m) in super::placements::history(seg, o.id).map_or_else(Vec::new, |h| h.steps) {
            let same = |a: &Mat4, b: &Mat4| {
                a.iter()
                    .flatten()
                    .zip(b.iter().flatten())
                    .all(|(x, y)| (x - y).abs() <= SAME)
            };
            if !same(&m, &IDENTITY) && !out.iter().any(|o| same(o, &m)) {
                out.push(m);
            }
        }
    }
    out
}

impl Builder<'_> {
    /// The sketch curve object a projection record's entity input names.
    fn projection_source(&self, record: u64) -> Option<u64> {
        let seg = self.seg;
        let slot = seg
            .ref_ids(seg.data_of(record))
            .into_iter()
            .find(|&r| seg.guid_of(r) == Some(INPUT_SLOT))?;
        let input = seg
            .ref_ids(seg.data_of(slot))
            .into_iter()
            .find(|&r| seg.guid_of(r) == Some(ENTITY_REF))?;
        self.input_entity_object(input)
            .filter(|&e| !is_point_class(seg.guid_of(e)))
    }

    /// The face or edge a projection record's input names, as a reference
    /// without curves, and the curves the input stored (model space, cm):
    /// a face's boundary edges, an edge's curve.
    fn projection_geometry(&self, record: u64) -> Option<(Reference, Vec<Geometry>)> {
        let seg = self.seg;
        let slot = seg
            .ref_ids(seg.data_of(record))
            .into_iter()
            .find(|&r| seg.guid_of(r) == Some(INPUT_SLOT))?;
        let input = seg
            .ref_ids(seg.data_of(slot))
            .into_iter()
            .find(|&r| seg.guid_of(r) == Some(FACE_REF))?;
        let curves: Vec<Geometry> = super::super::curve::of_input(seg, input)?
            .iter()
            .map(super::super::curve::StoredCurve::to_geometry)
            .collect();
        let mut r = super::face_input(seg, input).or_else(|| super::edge_input(seg, input))?;
        if let Reference::Face(fp) | Reference::Edge(fp) = &mut r
            && let Some(f) = fp.f3d.as_mut()
        {
            f.curves = None;
        }
        Some((r, curves))
    }

    /// Gives the linked curves of `sketches` that are projections of
    /// earlier sketches' curves those curves as their `referencedEntity`,
    /// and those that are projections of a face's or an edge's the face or
    /// the edge, with the one stored curve they are the projection of
    /// (`_f3d.curves`).
    pub(super) fn projection_sources(&self, sketches: &mut [SketchInfo]) {
        let seg = self.seg;
        let moves = placement_moves(seg);
        let mut found: Vec<(usize, usize, Reference)> = Vec::new();
        for (si, s) in sketches.iter().enumerate() {
            let linked: Vec<usize> = s
                .detail
                .curves
                .iter()
                .flatten()
                .enumerate()
                .filter(|(_, c)| c.is_linked == Some(true))
                .map(|(i, _)| i)
                .collect();
            if linked.is_empty() {
                continue;
            }
            let (Some(&feature), Some(frame)) = (self.feat_of_sk.get(&s.id), Frame::of(s)) else {
                continue;
            };
            let curves = s.detail.curves.as_deref().unwrap_or(&[]);
            let mut records = seg.ref_ids(seg.data_of(feature));
            records.retain(|&r| seg.guid_of(r) == Some(SKETCH_PROJECTION));
            let mut seen = std::collections::HashSet::new();
            records.retain(|r| seen.insert(*r));
            let mut taken = vec![false; curves.len()];
            for record in records {
                if let Some((r, stored)) = self.projection_geometry(record) {
                    for g in stored {
                        // Where the file has the source's component, or
                        // where an item of the timeline puts it.
                        let hit = std::iter::once(None)
                            .chain(moves.iter().map(Some))
                            .filter_map(|m| {
                                stored_shape(&m.map_or_else(|| g.clone(), |m| placed(&g, m)))
                            })
                            .find_map(|source| {
                                linked.iter().copied().find(|&i| {
                                    !taken[i]
                                        && shape(&frame, &curves[i])
                                            .is_some_and(|c| projects_onto(&frame, &source, &c))
                                })
                            });
                        let Some(i) = hit else {
                            continue;
                        };
                        taken[i] = true;
                        let mut r = r.clone();
                        if let Reference::Face(fp) | Reference::Edge(fp) = &mut r
                            && let Some(f) = fp.f3d.as_mut()
                        {
                            f.curves = Some(vec![g]);
                        }
                        found.push((si, i, r));
                    }
                    continue;
                }
                let Some(e) = self.projection_source(record) else {
                    continue;
                };
                let Some((sid, lid)) = self.local_ids.get(&e) else {
                    continue;
                };
                let Some(source) = sketches.iter().find(|o| o.id == *sid).and_then(|o| {
                    let c = o
                        .detail
                        .curves
                        .iter()
                        .flatten()
                        .find(|c| c.id.as_ref() == Some(lid))?;
                    shape(&Frame::of(o)?, c)
                }) else {
                    continue;
                };
                let hit = linked.iter().copied().find(|&i| {
                    !taken[i]
                        && shape(&frame, &curves[i])
                            .is_some_and(|c| projects_onto(&frame, &source, &c))
                });
                let (Some(i), Some(r)) = (hit, self.sketch_entity(e)) else {
                    continue;
                };
                taken[i] = true;
                found.push((si, i, r));
            }
        }
        for (si, i, r) in found {
            if let Some(c) = sketches[si]
                .detail
                .curves
                .as_mut()
                .and_then(|c| c.get_mut(i))
            {
                c.referenced_entity = Some(r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: [f64; 3], b: [f64; 3]) -> Shape {
        Shape {
            kind: "SketchLine".into(),
            points: vec![a, b],
            radius: None,
            unordered: true,
            arc: None,
        }
    }

    /// A plane at x = 1.7 cm looking along x, as the ribs' sketches of the
    /// design in the module documentation.
    fn rib_plane() -> Frame {
        Frame {
            origin: [1.7, 0.0, 0.0],
            x: [0.0, 0.0, -1.0],
            y: [0.0, 1.0, 0.0],
        }
    }

    #[test]
    fn a_line_of_a_parallel_sketch_projects_onto_its_copy() {
        let f = rib_plane();
        let source = line([-1.7, 0.24, 6.0], [-1.7, 2.94, 6.0]);
        assert!(projects_onto(
            &f,
            &source,
            &line([1.7, 0.24, 6.0], [1.7, 2.94, 6.0])
        ));
        // Either way round.
        assert!(projects_onto(
            &f,
            &source,
            &line([1.7, 2.94, 6.0], [1.7, 0.24, 6.0])
        ));
        // Not onto the wall's other side.
        assert!(!projects_onto(
            &f,
            &source,
            &line([1.7, 0.24, -6.0], [1.7, 2.94, -6.0])
        ));
    }

    #[test]
    fn circles_and_arcs_need_their_radius() {
        let f = rib_plane();
        let circle = |c: [f64; 3], r: f64| Shape {
            kind: "SketchCircle".into(),
            points: vec![c],
            radius: Some(r),
            unordered: false,
            arc: None,
        };
        assert!(projects_onto(
            &f,
            &circle([-3.0, 1.0, 2.0], 0.5),
            &circle([1.7, 1.0, 2.0], 0.5)
        ));
        assert!(!projects_onto(
            &f,
            &circle([-3.0, 1.0, 2.0], 0.5),
            &circle([1.7, 1.0, 2.0], 0.6)
        ));
        let arc = |ends: [[f64; 3]; 2]| Shape {
            kind: "SketchArc".into(),
            points: vec![[1.7, 0.0, 0.0], ends[0], ends[1]],
            radius: Some(1.0),
            unordered: true,
            arc: None,
        };
        let a = arc([[1.7, 1.0, 0.0], [1.7, 0.0, 1.0]]);
        // The ends either way round, the centre first.
        assert!(projects_onto(
            &f,
            &a,
            &arc([[1.7, 0.0, 1.0], [1.7, 1.0, 0.0]])
        ));
        assert!(!projects_onto(
            &f,
            &line([1.7, 1.0, 0.0], [1.7, 0.0, 1.0]),
            &a
        ));
    }

    #[test]
    fn an_arc_seen_edge_on_projects_onto_a_line() {
        // A quarter circle of radius 1 about (0, 0, 0) in the plane z = 0,
        // from +x to +y, seen from the plane y = 5 (looking along y): the
        // line from x = 0 to x = 1 at z = 0.
        let f = Frame {
            origin: [0.0, 5.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 0.0, 1.0],
        };
        let quarter = Shape {
            kind: "SketchArc".into(),
            points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            radius: Some(1.0),
            unordered: true,
            arc: Some(Arc {
                centre: [0.0; 3],
                u: [1.0, 0.0, 0.0],
                v: [0.0, 1.0, 0.0],
                radius: 1.0,
                start: 0.0,
                end: std::f64::consts::FRAC_PI_2,
            }),
        };
        let on = |a: [f64; 3], b: [f64; 3]| projects_onto(&f, &quarter, &line(a, b));
        assert!(on([0.0, 5.0, 0.0], [1.0, 5.0, 0.0]));
        assert!(on([1.0, 5.0, 0.0], [0.0, 5.0, 0.0]));
        assert!(!on([-1.0, 5.0, 0.0], [1.0, 5.0, 0.0]));
    }

    /// A latch's socket edge, stored in the latch's coordinates, projects
    /// onto a box's sketch where a captured position puts the latch
    /// (13.16 cm along -y, 1.1667 cm up), as in the latching box design.
    #[test]
    fn a_stored_curve_of_a_moved_component_projects_where_it_was_put() {
        let mut m = IDENTITY;
        m[1][3] = -13.16;
        m[2][3] = 1.1667;
        let stored = Geometry {
            geometry_type: Some("Line3D".into()),
            start_point: Some([-0.5, 9.96, 0.38]),
            end_point: Some([-0.5, 10.08, 0.3]),
            ..Geometry::default()
        };
        // The box's sketch at x = 0.53 cm, looking along x.
        let f = Frame {
            origin: [0.53, 0.0, 0.0],
            x: [0.0, -1.0, 0.0],
            y: [0.0, 0.0, 1.0],
        };
        let curve = line([0.53, -3.2, 1.5467], [0.53, -3.08, 1.4667]);
        let at = |g: &Geometry| stored_shape(g).is_some_and(|s| projects_onto(&f, &s, &curve));
        assert!(!at(&stored));
        assert!(at(&placed(&stored, &m)));
    }
}
