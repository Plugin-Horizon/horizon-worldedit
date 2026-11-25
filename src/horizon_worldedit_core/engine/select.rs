//! The non-cuboid selectors: `//sel <type>` and the clicks that make polygon, ellipsoid, sphere, cylinder and convex
//! selections (`Polygonal2DRegionSelector`, `ExtendingPolygonal2DRegionSelector`, `EllipsoidRegionSelector`,
//! `SphereRegionSelector`, `CylinderRegionSelector`, `ConvexPolyhedralRegionSelector`).

use super::*;
use crate::horizon_worldedit_core::selector::{SelKind, Shape};

fn already_set() -> String {
    "Position already set.".to_owned()
}

fn vec_string(v: [f64; 3]) -> String {
    format!("({}, {}, {})", java_double(v[0]), java_double(v[1]), java_double(v[2]))
}

/// The distance from `p` to the segment `a`-`b` (`ExtendingPolygonal2DRegionSelector.distanceFromLine`).
fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if len == 0.0 {
        return ((p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2)).sqrt();
    }
    let n = [d[0] / len, d[1] / len];
    let t = ((p[0] - a[0]) * n[0] + (p[1] - a[1]) * n[1]).clamp(0.0, len);
    let c = [a[0] + n[0] * t, a[1] + n[1] * t];
    ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt()
}

impl Engine {
    /// `ActorSelectorLimits.getPolygonVertexLimit` / `getPolyhedronVertexLimit`: how many points a polygon (`poly`) or a
    /// convex polyhedron takes; `None` for no limit.
    pub(super) fn point_limit(&mut self, actor: &Actor, poly: bool) -> Option<usize> {
        let (default, max) = if poly { (self.config.max_polygonal_default, self.config.max_polygonal_max) } else { (self.config.max_polyhedron_default, self.config.max_polyhedron_max) };
        let limit = if self.holds(actor, "worldedit.limit.unrestricted") || max < 0 {
            default
        } else if default < 0 {
            max
        } else {
            default.min(max)
        };
        (limit > 0).then_some(limit as usize)
    }

    /// `//sel [-d] [type]`: a chosen selector takes over what is selected, as far as it fits.
    pub(super) fn sel_type(&mut self, actor: &Actor, kind: SelKind, set_default: bool) -> R {
        let s = self.sessions.get(actor.uuid);
        let old = if s.dimension == actor.dimension { s.pos1.zip(s.pos2).map(|(a, b)| Cuboid::new(a, b)) } else { None };
        let old_shape = s.shape.clone();
        s.kind = kind;
        s.extend = kind == SelKind::Extend;
        s.shape = None;
        match kind {
            SelKind::Cuboid | SelKind::Extend => {
                if let Some(b) = old_shape.as_ref().and_then(Shape::bounds) {
                    s.pos1 = Some(b.min);
                    s.pos2 = Some(b.max);
                } else if let (Some(a), Some(b), true) = (s.pos1, s.pos2, kind == SelKind::Extend) {
                    let c = Cuboid::new(a, b);
                    s.pos1 = Some(c.min);
                    s.pos2 = Some(c.max);
                }
            }
            _ => {
                // The old box, when there is one, becomes the new shape as WorldEdit's converting constructors do.
                s.shape = old.map(|c| match kind {
                    SelKind::Poly | SelKind::PolyExtend => Shape::Poly { points: vec![[c.min[0], c.min[2]], [c.max[0], c.min[2]], [c.max[0], c.max[2]], [c.min[0], c.max[2]]], min_y: c.min[1], max_y: c.max[1] },
                    SelKind::Ellipsoid => Shape::Ellipsoid { center: [0; 3], radius: [0.0; 3] }.fitted(&c),
                    SelKind::Sphere => {
                        let r = [0, 1, 2].map(|a| (c.max[a] - c.min[a]) as f64 / 2.0).into_iter().fold(0.0, f64::max);
                        Shape::Ellipsoid { center: [(c.min[0] + c.max[0]).div_euclid(2), (c.min[1] + c.max[1]).div_euclid(2), (c.min[2] + c.max[2]).div_euclid(2)], radius: [r; 3] }
                    }
                    SelKind::Cyl => Shape::Cyl { center: [0, 0], radius: [0.0, 0.0], min_y: c.min[1], max_y: c.max[1] }.fitted(&c),
                    _ => Shape::Convex { points: (0..8).map(|i| [if i & 1 == 0 { c.min[0] } else { c.max[0] }, if i & 2 == 0 { c.min[1] } else { c.max[1] }, if i & 4 == 0 { c.min[2] } else { c.max[2] }]).collect() },
                });
                match s.shape.as_ref().and_then(Shape::bounds) {
                    Some(b) => {
                        s.pos1 = Some(b.min);
                        s.pos2 = Some(b.max);
                    }
                    None => {
                        s.pos1 = None;
                        s.pos2 = None;
                    }
                }
            }
        }
        let mut out = vec![kind.message().to_owned()];
        // `SelectorChoice.explainNewSelector`: the polygon and polyhedron selectors say how many points they take.
        match kind {
            SelKind::Poly | SelKind::PolyExtend => out.extend(self.point_limit(actor, true).map(|l| format!("{l} points maximum."))),
            SelKind::Convex => out.extend(self.point_limit(actor, false).map(|l| format!("{l} points maximum."))),
            _ => {}
        }
        let s = self.sessions.get(actor.uuid);
        if set_default {
            s.default_kind = kind;
            out.push(format!("Your default region selector is now {}.", kind.type_name()));
        }
        Ok(out)
    }

    /// A click with a shape selector: sets or adds a point and says what happened. `Err` when nothing changed.
    pub(super) fn shape_corner(&mut self, actor: &Actor, first: bool, p: Pos) -> Result<Vec<String>, String> {
        let at = pt(p);
        let limit_poly = self.point_limit(actor, true);
        let limit_convex = self.point_limit(actor, false);
        let s = self.sessions.get(actor.uuid);
        let kind = s.kind;
        let current = s.shape.take();
        let mut line = String::new();
        let next: Shape = match kind {
            SelKind::Poly | SelKind::PolyExtend => {
                let (mut points, mut min_y, mut max_y) = match current {
                    Some(Shape::Poly { points, min_y, max_y }) => (points, min_y, max_y),
                    _ => (Vec::new(), p[1], p[1]),
                };
                if first {
                    if points.len() == 1 && points[0] == [p[0], p[2]] && min_y == p[1] && max_y == p[1] {
                        s.shape = Some(Shape::Poly { points, min_y, max_y });
                        return Err(already_set());
                    }
                    points = vec![[p[0], p[2]]];
                    min_y = p[1];
                    max_y = p[1];
                    line = format!("Starting a new polygon at {at}.");
                } else {
                    if points.last() == Some(&[p[0], p[2]]) {
                        s.shape = Some(Shape::Poly { points, min_y, max_y });
                        return Err(already_set());
                    }
                    // `Polygonal2DRegionSelector.selectSecondary`: past the vertex limit nothing is added (the extending
                    // selector only asks it for its first three points).
                    if limit_poly.is_some_and(|l| points.len() > l) && (kind == SelKind::Poly || points.len() < 3) {
                        s.shape = Some(Shape::Poly { points, min_y, max_y });
                        return Ok(Vec::new());
                    }
                    if kind == SelKind::PolyExtend && points.len() >= 3 {
                        if points.contains(&[p[0], p[2]]) {
                            s.shape = Some(Shape::Poly { points, min_y, max_y });
                            return Err(already_set());
                        }
                        let target = [f64::from(p[0]), f64::from(p[2])];
                        let mut prev = points[points.len() - 1];
                        let (mut best, mut at_index) = (f64::MAX, 0);
                        for (i, q) in points.iter().enumerate() {
                            let d = segment_distance(target, [f64::from(prev[0]), f64::from(prev[1])], [f64::from(q[0]), f64::from(q[1])]);
                            if d < best {
                                best = d;
                                at_index = i;
                            }
                            prev = *q;
                        }
                        points.insert(at_index, [p[0], p[2]]);
                    } else {
                        points.push([p[0], p[2]]);
                    }
                    min_y = min_y.min(p[1]);
                    max_y = max_y.max(p[1]);
                    line = format!("Added point #{} at {at}.", points.len());
                }
                min_y = min_y.min(p[1]);
                max_y = max_y.max(p[1]);
                Shape::Poly { points, min_y, max_y }
            }
            SelKind::Ellipsoid | SelKind::Sphere => {
                let sphere = kind == SelKind::Sphere;
                let (center, radius) = match current {
                    Some(Shape::Ellipsoid { center, radius }) => (Some(center), radius),
                    _ => (None, [0.0; 3]),
                };
                if first {
                    if center == Some(p) && radius == [0.0; 3] {
                        s.shape = Some(Shape::Ellipsoid { center: p, radius });
                        return Err(already_set());
                    }
                    let shape = Shape::Ellipsoid { center: p, radius: [0.0; 3] };
                    line = format!("Center position set to {at}.");
                    shape
                } else {
                    let Some(c) = center else { return Err(already_set()) };
                    let radius = if sphere {
                        let d = [0, 1, 2].map(|a| f64::from(p[a] - c[a]));
                        let r = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().ceil();
                        [r; 3]
                    } else {
                        [0, 1, 2].map(|a| radius[a].max(f64::from((p[a] - c[a]).abs())))
                    };
                    let shape = Shape::Ellipsoid { center: c, radius };
                    line = if sphere { format!("Radius set to {} ({}).", java_double(radius[0]), shape.volume()) } else { format!("Radius set to {} ({}).", vec_string(radius), shape.volume()) };
                    shape
                }
            }
            SelKind::Cyl => {
                if first {
                    line = format!("Starting a new cylindrical selection at {at}.");
                    Shape::Cyl { center: [p[0], p[2]], radius: [0.0, 0.0], min_y: p[1], max_y: p[1] }
                } else {
                    let Some(Shape::Cyl { center, radius, min_y, max_y }) = current else {
                        return Err("You must select the center point before setting the radius.".into());
                    };
                    let radius = [radius[0].max(f64::from((p[0] - center[0]).abs())), radius[1].max(f64::from((p[2] - center[1]).abs()))];
                    let shape = Shape::Cyl { center, radius, min_y: min_y.min(p[1]), max_y: max_y.max(p[1]) };
                    line = format!("Radius set to {}/{} blocks. ({})", calc::format_number(radius[0]), calc::format_number(radius[1]), shape.volume());
                    shape
                }
            }
            _ => {
                let mut points = match current {
                    Some(Shape::Convex { points }) => points,
                    _ => Vec::new(),
                };
                if first {
                    points = vec![p];
                    line = format!("Started new selection with vertex {at}.");
                } else {
                    if limit_convex.is_some_and(|l| points.len() > l) {
                        s.shape = Some(Shape::Convex { points });
                        return Ok(Vec::new());
                    }
                    if points.contains(&p) {
                        s.shape = Some(Shape::Convex { points });
                        return Err(already_set());
                    }
                    points.push(p);
                    line = format!("Added vertex {at} to the selection.");
                }
                Shape::Convex { points }
            }
        };
        if let Some(b) = next.bounds() {
            s.pos1 = Some(b.min);
            s.pos2 = Some(b.max);
        }
        s.shape = Some(next);
        Ok(vec![line])
    }

    /// What `//expand`, `//contract` and the others may do to a shape; `Err` is WorldEdit's refusal.
    pub(super) fn shape_resizable(shape: Option<&Shape>, name: &str, dir: Option<Dir>) -> Result<(), String> {
        let vertical = dir.is_some_and(|d| matches!(d, Dir::Up | Dir::Down));
        match (shape, name) {
            (Some(Shape::Convex { .. }), "expand") => Err("Cannot expand a convex polyhedral region.".into()),
            (Some(Shape::Convex { .. }), "contract") => Err("Cannot contract a convex polyhedral region.".into()),
            (Some(Shape::Poly { .. }), "expand") if !vertical => Err("Polygons can only be expanded vertically.".into()),
            (Some(Shape::Poly { .. }), "contract") if !vertical => Err("Polygons can only be contracted vertically.".into()),
            _ => Ok(()),
        }
    }

    /// After a resize of the box `next`: the shape follows (an ellipsoid or a cylinder is fitted to the box, a polygon
    /// takes the new height, a moved shape is moved).
    pub(super) fn keep_shape(&mut self, actor: &Actor, shape: Option<Shape>, next: Cuboid, moved: Option<Pos>) {
        let Some(shape) = shape else { return };
        let new = match (&shape, moved) {
            (_, Some(by)) => shape.shifted(by),
            (Shape::Poly { points, .. }, None) => Shape::Poly { points: points.clone(), min_y: next.min[1], max_y: next.max[1] },
            (_, None) => shape.fitted(&next),
        };
        let s = self.sessions.get(actor.uuid);
        if let Some(b) = new.bounds() {
            s.pos1 = Some(b.min);
            s.pos2 = Some(b.max);
        }
        s.shape = Some(new);
    }

    /// The player's shape selection, if the selector is not a plain box.
    pub(super) fn shape_of(&mut self, actor: &Actor) -> Option<Shape> {
        let s = self.sessions.get(actor.uuid);
        if s.dimension == actor.dimension {
            s.shape.clone()
        } else {
            None
        }
    }

    /// The positions of `c` that are in `shape` (all of them for a box).
    pub(super) fn shape_positions(shape: &Option<Shape>, c: &Cuboid) -> Vec<Pos> {
        match shape {
            Some(sh) => c.positions().filter(|&p| sh.contains(p)).collect(),
            None => c.positions().collect(),
        }
    }

    /// `//size` for a shape selection.
    pub(super) fn shape_info(shape: &Shape, kind: SelKind) -> Vec<String> {
        let mut out = vec![format!(
            "Type: {}",
            match kind {
                SelKind::Poly => "Polygonal2DRegionSelector",
                SelKind::PolyExtend => "ExtendingPolygonal2DRegionSelector",
                SelKind::Ellipsoid => "EllipsoidRegionSelector",
                SelKind::Sphere => "SphereRegionSelector",
                SelKind::Cyl => "CylinderRegionSelector",
                _ => "ConvexPolyhedralRegionSelector",
            }
        )];
        match shape {
            Shape::Poly { points, .. } => out.push(format!("# points: {}", points.len())),
            Shape::Ellipsoid { center, radius } => {
                out.push(format!("Center: {}", pt(*center)));
                out.push(format!("X/Y/Z Radius: {}", vec_string(*radius)));
            }
            Shape::Cyl { center, radius, .. } => {
                out.push(format!("Center: {}", pt([center[0], 0, center[1]])));
                out.push(format!("Radius: {}", calc::format_number(radius[0])));
            }
            Shape::Convex { points } => {
                out.push(format!("Vertices: {}", points.len()));
                // The hull is triangulated: with V corner points it has 2V - 4 triangles (a point inside the hull of the others
                // is no corner).
                let corners = (0..points.len()).filter(|&i| {
                    let others: Vec<Pos> = points.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, p)| *p).collect();
                    others.len() < 4 || !Shape::Convex { points: others }.contains(points[i])
                });
                out.push(format!("Triangles: {}", (2 * corners.count()).saturating_sub(4)));
            }
        }
        out
    }
}
