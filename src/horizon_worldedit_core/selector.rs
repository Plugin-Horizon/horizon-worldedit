//! The non-cuboid region selectors of WorldEdit (`//sel poly|polyextend|ellipsoid|sphere|cyl|convex`): the regions they
//! make and the membership tests, ported from `Polygonal2DRegion`, `EllipsoidRegion`, `CylinderRegion` and the
//! convex polyhedron. A shape always has a bounding box; the session keeps that box in `pos1`/`pos2`, so every command
//! that works on boxes sees the shape's box, and the commands that know shapes filter by `contains`.

use crate::horizon_worldedit_core::region::Cuboid;
use crate::horizon_worldedit_core::world::Pos;

/// Which selector a player has chosen (`//sel <name>`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelKind {
    #[default]
    Cuboid,
    Extend,
    Poly,
    PolyExtend,
    Ellipsoid,
    Sphere,
    Cyl,
    Convex,
}

impl SelKind {
    /// The name `//sel` takes, aliases included.
    pub fn parse(word: &str) -> Option<SelKind> {
        Some(match word.to_ascii_lowercase().as_str() {
            "cuboid" => SelKind::Cuboid,
            "extend" => SelKind::Extend,
            "poly" | "polygon" | "polygonal" | "polygonal2d" => SelKind::Poly,
            "polyextend" => SelKind::PolyExtend,
            "ellipsoid" => SelKind::Ellipsoid,
            "sphere" => SelKind::Sphere,
            "cyl" | "cylinder" => SelKind::Cyl,
            "convex" | "hull" | "polyhedron" => SelKind::Convex,
            _ => return None,
        })
    }
    /// `RegionSelectorType` name (for `worldedit.select.default-set`).
    pub fn type_name(self) -> &'static str {
        match self {
            SelKind::Cuboid => "CUBOID",
            SelKind::Extend => "EXTENDING_CUBOID",
            SelKind::Poly => "POLYGONAL",
            SelKind::PolyExtend => "EXTENDING_POLYGONAL",
            SelKind::Ellipsoid => "ELLIPSOID",
            SelKind::Sphere => "SPHERE",
            SelKind::Cyl => "CYLINDER",
            SelKind::Convex => "CONVEX_POLYHEDRON",
        }
    }
    /// The message after choosing the selector (`worldedit.select.<name>.message`).
    pub fn message(self) -> &'static str {
        match self {
            SelKind::Cuboid => "Cuboid: left click for point 1, right click for point 2",
            SelKind::Extend => "Cuboid: left click for a starting point, right click to extend",
            SelKind::Poly => "2D polygon selector: Left/right click to add a point.",
            SelKind::PolyExtend => "2D polygon selector: left click for a starting point, right click to extend",
            SelKind::Ellipsoid => "Ellipsoid selector: left click=center, right click to extend",
            SelKind::Sphere => "Sphere selector: left click=center, right click to set radius",
            SelKind::Cyl => "Cylindrical selector: Left click=center, right click to extend",
            SelKind::Convex => "Convex polyhedral selector: Left click=First vertex, right click to add more.",
        }
    }
    pub fn is_shape(self) -> bool {
        !matches!(self, SelKind::Cuboid | SelKind::Extend)
    }
}

/// A region that is not a box. Radii are the block radii as WorldEdit states them (the region itself counts half a block
/// more: a radius of 5 holds the blocks 5 away).
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// `Polygonal2DRegion`: points in x/z with a y range.
    Poly { points: Vec<[i32; 2]>, min_y: i32, max_y: i32 },
    /// `EllipsoidRegion` (a sphere has three equal radii).
    Ellipsoid { center: Pos, radius: [f64; 3] },
    /// `CylinderRegion`.
    Cyl { center: [i32; 2], radius: [f64; 2], min_y: i32, max_y: i32 },
    /// `ConvexPolyhedralRegion`: the hull of the vertices.
    Convex { points: Vec<Pos> },
}

fn floor_pos(v: [f64; 3]) -> Pos {
    [v[0].floor() as i32, v[1].floor() as i32, v[2].floor() as i32]
}

impl Shape {
    /// The smallest box holding the shape; `None` for a shape with no points.
    pub fn bounds(&self) -> Option<Cuboid> {
        match self {
            Shape::Poly { points, min_y, max_y } => {
                let first = points.first()?;
                let (mut lo, mut hi) = (*first, *first);
                for p in points {
                    lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                    hi = [hi[0].max(p[0]), hi[1].max(p[1])];
                }
                Some(Cuboid { min: [lo[0], *min_y, lo[1]], max: [hi[0], *max_y, hi[1]] })
            }
            Shape::Ellipsoid { center, radius } => {
                let c = [center[0] as f64, center[1] as f64, center[2] as f64];
                let lo = floor_pos([c[0] - radius[0], c[1] - radius[1], c[2] - radius[2]]);
                let hi = floor_pos([c[0] + radius[0], c[1] + radius[1], c[2] + radius[2]]);
                Some(Cuboid { min: lo, max: hi })
            }
            Shape::Cyl { center, radius, min_y, max_y } => {
                let lo = floor_pos([center[0] as f64 - radius[0], 0.0, center[1] as f64 - radius[1]]);
                let hi = floor_pos([center[0] as f64 + radius[0], 0.0, center[1] as f64 + radius[1]]);
                Some(Cuboid { min: [lo[0], *min_y, lo[2]], max: [hi[0], *max_y, hi[2]] })
            }
            Shape::Convex { points } => {
                let first = points.first()?;
                let (mut lo, mut hi) = (*first, *first);
                for p in points {
                    for a in 0..3 {
                        lo[a] = lo[a].min(p[a]);
                        hi[a] = hi[a].max(p[a]);
                    }
                }
                Some(Cuboid { min: lo, max: hi })
            }
        }
    }

    /// Whether the block `p` is in the shape.
    pub fn contains(&self, p: Pos) -> bool {
        match self {
            Shape::Poly { points, min_y, max_y } => poly_contains(points, *min_y, *max_y, p),
            Shape::Ellipsoid { center, radius } => {
                let d = |a: usize| (p[a] - center[a]) as f64 / (radius[a] + 0.5);
                d(0) * d(0) + d(1) * d(1) + d(2) * d(2) <= 1.0
            }
            Shape::Cyl { center, radius, min_y, max_y } => {
                if p[1] < *min_y || p[1] > *max_y {
                    return false;
                }
                let dx = (p[0] - center[0]) as f64 / (radius[0] + 0.5);
                let dz = (p[2] - center[1]) as f64 / (radius[1] + 0.5);
                dx * dx + dz * dz <= 1.0
            }
            Shape::Convex { points } => convex_contains(points, p),
        }
    }

    /// The blocks in the shape, as `getVolume` reports them (the formula for an ellipsoid and a cylinder, a count for the rest).
    pub fn volume(&self) -> u64 {
        match self {
            Shape::Ellipsoid { radius, .. } => ((4.0 / 3.0) * std::f64::consts::PI * (radius[0] + 0.5) * (radius[1] + 0.5) * (radius[2] + 0.5)).floor() as u64,
            Shape::Cyl { radius, min_y, max_y, .. } => (std::f64::consts::PI * (radius[0] + 0.5) * (radius[1] + 0.5) * (max_y - min_y + 1).max(0) as f64).floor() as u64,
            _ => match self.bounds() {
                Some(b) => b.positions().filter(|&p| self.contains(p)).count() as u64,
                None => 0,
            },
        }
    }

    /// The same kind of shape fitted to a box (an ellipsoid or a cylinder after the box was grown, shrunk or moved).
    pub fn fitted(&self, c: &Cuboid) -> Shape {
        match self {
            Shape::Ellipsoid { .. } => {
                let mid = |a: usize| (c.min[a] + c.max[a]).div_euclid(2);
                let rad = |a: usize| (c.max[a] - c.min[a]) as f64 / 2.0;
                Shape::Ellipsoid { center: [mid(0), mid(1), mid(2)], radius: [rad(0), rad(1), rad(2)] }
            }
            Shape::Cyl { .. } => Shape::Cyl {
                center: [(c.min[0] + c.max[0]).div_euclid(2), (c.min[2] + c.max[2]).div_euclid(2)],
                radius: [(c.max[0] - c.min[0]) as f64 / 2.0, (c.max[2] - c.min[2]) as f64 / 2.0],
                min_y: c.min[1],
                max_y: c.max[1],
            },
            other => other.clone(),
        }
    }

    /// Moved by `by`.
    pub fn shifted(&self, by: Pos) -> Shape {
        match self {
            Shape::Poly { points, min_y, max_y } => Shape::Poly { points: points.iter().map(|p| [p[0] + by[0], p[1] + by[2]]).collect(), min_y: min_y + by[1], max_y: max_y + by[1] },
            Shape::Ellipsoid { center, radius } => Shape::Ellipsoid { center: [center[0] + by[0], center[1] + by[1], center[2] + by[2]], radius: *radius },
            Shape::Cyl { center, radius, min_y, max_y } => Shape::Cyl { center: [center[0] + by[0], center[1] + by[2]], radius: *radius, min_y: min_y + by[1], max_y: max_y + by[1] },
            Shape::Convex { points } => Shape::Convex { points: points.iter().map(|p| [p[0] + by[0], p[1] + by[1], p[2] + by[2]]).collect() },
        }
    }
}

/// `Polygonal2DRegion.contains`: the border counts as inside.
fn poly_contains(points: &[[i32; 2]], min_y: i32, max_y: i32, p: Pos) -> bool {
    if points.len() < 3 || p[1] < min_y || p[1] > max_y {
        return false;
    }
    let (tx, tz) = (p[0] as i64, p[2] as i64);
    let mut inside = false;
    let last = points[points.len() - 1];
    let (mut x_old, mut z_old) = (last[0] as i64, last[1] as i64);
    for pt in points {
        let (x_new, z_new) = (pt[0] as i64, pt[1] as i64);
        if x_new == tx && z_new == tz {
            return true;
        }
        let (x1, z1, x2, z2) = if x_new > x_old { (x_old, z_old, x_new, z_new) } else { (x_new, z_new, x_old, z_old) };
        if x1 <= tx && tx <= x2 {
            let cross = (tz - z1) * (x2 - x1) - (z2 - z1) * (tx - x1);
            if cross == 0 {
                if (z1 <= tz) == (tz <= z2) {
                    return true;
                }
            } else if cross < 0 && x1 != tx {
                inside = !inside;
            }
        }
        x_old = x_new;
        z_old = z_new;
    }
    inside
}

fn sub3(a: Pos, b: Pos) -> [i64; 3] {
    [a[0] as i64 - b[0] as i64, a[1] as i64 - b[1] as i64, a[2] as i64 - b[2] as i64]
}
fn cross3(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot3(a: [i64; 3], b: [i64; 3]) -> i64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The outward-facing planes (normal, offset: points inside have `normal . p <= offset`) of the hull of `points`.
pub fn hull_planes(points: &[Pos]) -> Vec<([i64; 3], i64)> {
    let mut planes: Vec<([i64; 3], i64)> = Vec::new();
    let n = points.len();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let normal = cross3(sub3(points[j], points[i]), sub3(points[k], points[i]));
                if normal == [0, 0, 0] {
                    continue;
                }
                let d = dot3(normal, [points[i][0] as i64, points[i][1] as i64, points[i][2] as i64]);
                let (mut above, mut below) = (false, false);
                for q in points {
                    let side = dot3(normal, [q[0] as i64, q[1] as i64, q[2] as i64]) - d;
                    above |= side > 0;
                    below |= side < 0;
                }
                let plane = match (above, below) {
                    (false, true) => (normal, d),
                    (true, false) => ([-normal[0], -normal[1], -normal[2]], -d),
                    _ => continue,
                };
                if !planes.contains(&plane) {
                    planes.push(plane);
                }
            }
        }
    }
    planes
}

fn convex_contains(points: &[Pos], p: Pos) -> bool {
    if points.contains(&p) {
        return true;
    }
    let planes = hull_planes(points);
    if planes.is_empty() {
        return false;
    }
    let q = [p[0] as i64, p[1] as i64, p[2] as i64];
    planes.iter().all(|(n, d)| dot3(*n, q) <= *d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_counts_border_and_respects_height() {
        let square = Shape::Poly { points: vec![[0, 0], [4, 0], [4, 4], [0, 4]], min_y: 10, max_y: 12 };
        assert!(square.contains([2, 11, 2]));
        assert!(square.contains([0, 10, 0]));
        assert!(square.contains([4, 12, 2]));
        assert!(!square.contains([5, 11, 2]));
        assert!(!square.contains([2, 13, 2]));
        assert_eq!(square.volume(), 5 * 5 * 3);
        let tri = Shape::Poly { points: vec![[0, 0], [4, 0], [0, 4]], min_y: 0, max_y: 0 };
        assert!(tri.contains([1, 0, 1]));
        assert!(!tri.contains([3, 0, 3]));
        assert_eq!(tri.bounds().unwrap().size(), [5, 1, 5]);
        assert!(!Shape::Poly { points: vec![[0, 0], [4, 0]], min_y: 0, max_y: 0 }.contains([1, 0, 0]));
    }

    #[test]
    fn ellipsoid_and_cylinder_use_half_block_margin() {
        let s = Shape::Ellipsoid { center: [0, 64, 0], radius: [3.0, 3.0, 3.0] };
        assert!(s.contains([3, 64, 0]));
        assert!(!s.contains([4, 64, 0]));
        assert!(!s.contains([3, 66, 2]));
        assert_eq!(s.bounds().unwrap(), Cuboid::new([-3, 61, -3], [3, 67, 3]));
        assert_eq!(s.volume(), 179);
        let c = Shape::Cyl { center: [0, 0], radius: [2.0, 2.0], min_y: 5, max_y: 7 };
        assert!(c.contains([2, 6, 0]));
        assert!(!c.contains([2, 8, 0]));
        assert!(!c.contains([2, 6, 2]));
        assert_eq!(c.volume(), 58);
    }

    #[test]
    fn convex_hull_of_a_tetrahedron() {
        let t = Shape::Convex { points: vec![[0, 0, 0], [8, 0, 0], [0, 8, 0], [0, 0, 8]] };
        assert!(t.contains([1, 1, 1]));
        assert!(t.contains([0, 0, 0]));
        assert!(t.contains([4, 4, 0]));
        assert!(!t.contains([5, 5, 5]));
        assert!(!t.contains([-1, 0, 0]));
        assert_eq!(t.bounds().unwrap().size(), [9, 9, 9]);
    }

    #[test]
    fn fitting_keeps_the_kind() {
        let s = Shape::Ellipsoid { center: [0, 64, 0], radius: [3.0, 3.0, 3.0] };
        let grown = s.fitted(&Cuboid::new([-3, 61, -3], [3, 69, 3]));
        assert_eq!(grown, Shape::Ellipsoid { center: [0, 65, 0], radius: [3.0, 4.0, 3.0] });
        assert_eq!(s.shifted([1, 2, 3]).bounds().unwrap().min, [-2, 63, 0]);
    }
}
