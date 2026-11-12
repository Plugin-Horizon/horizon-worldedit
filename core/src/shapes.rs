//! The positions of generated shapes, as WorldEdit's `EditSession.makeSphere`, `makeCylinder` and
//! `makePyramid` lay them out (radii are measured with half a block added, so a radius of 3 is the
//! round shape builders expect, not a diamond).

use crate::world::Pos;
use std::collections::HashSet;

fn inside(x: f64, y: f64, z: f64) -> bool {
    x * x + y * y + z * z <= 1.0
}

/// A sphere or ellipsoid around `center`, `radii` per axis; `hollow` keeps the one-block surface.
pub fn sphere(center: Pos, radii: [f64; 3], hollow: bool) -> Vec<Pos> {
    let r = radii.map(|v| v.max(0.0) + 0.5);
    let ceil = r.map(|v| v.ceil() as i32);
    let mut out = Vec::new();
    for dx in 0..=ceil[0] {
        let (x, nx) = (dx as f64 / r[0], (dx + 1) as f64 / r[0]);
        for dy in 0..=ceil[1] {
            let (y, ny) = (dy as f64 / r[1], (dy + 1) as f64 / r[1]);
            for dz in 0..=ceil[2] {
                let (z, nz) = (dz as f64 / r[2], (dz + 1) as f64 / r[2]);
                if !inside(x, y, z) {
                    continue;
                }
                if hollow && inside(nx, y, z) && inside(x, ny, z) && inside(x, y, nz) {
                    continue;
                }
                mirror8(&mut out, center, [dx, dy, dz]);
            }
        }
    }
    out
}

/// A cylinder standing on `base`, `radii` (x, z), `height` blocks up (negative: down); `hollow`
/// keeps the one-block wall.
pub fn cylinder(base: Pos, radii: [f64; 2], height: i32, hollow: bool) -> Vec<Pos> {
    let r = radii.map(|v| v.max(0.0) + 0.5);
    let ceil = r.map(|v| v.ceil() as i32);
    // WorldEdit: height 0 makes nothing; a negative height starts that far below `base`.
    if height == 0 {
        return Vec::new();
    }
    let (from, to) = if height > 0 { (0, height - 1) } else { (height, -1) };
    let mut out = Vec::new();
    for dx in 0..=ceil[0] {
        let (x, nx) = (dx as f64 / r[0], (dx + 1) as f64 / r[0]);
        for dz in 0..=ceil[1] {
            let (z, nz) = (dz as f64 / r[1], (dz + 1) as f64 / r[1]);
            if x * x + z * z > 1.0 {
                continue;
            }
            if hollow && nx * nx + z * z <= 1.0 && x * x + nz * nz <= 1.0 {
                continue;
            }
            for dy in from..=to {
                mirror4(&mut out, [base[0], base[1] + dy, base[2]], dx, dz);
            }
        }
    }
    out
}

/// A pyramid on `base`, `size` layers; `hollow` keeps the edges of each layer.
pub fn pyramid(base: Pos, size: i32, hollow: bool) -> Vec<Pos> {
    let mut out = Vec::new();
    let mut r = size;
    for y in 0..=size {
        r -= 1;
        for x in 0..=r.max(-1) {
            for z in 0..=r.max(-1) {
                if (!hollow && x <= r && z <= r) || x == r || z == r {
                    mirror4(&mut out, [base[0], base[1] + y, base[2]], x, z);
                }
            }
        }
    }
    out
}

/// A cone on `base`, `radii` (x, z), `height` layers up (negative: down); `hollow` keeps a wall
/// `thickness` thick (`EditSession.makeCone`).
pub fn cone(base: Pos, radii: [f64; 2], height: i32, hollow: bool, thickness: f64) -> Vec<Pos> {
    let mut out = Vec::new();
    if height == 0 {
        return out;
    }
    let ceil = radii.map(|v| v.ceil() as i32);
    let (rx2, rz2) = (radii[0].powi(2), radii[1].powi(2));
    let height_sq = f64::from(height).powi(2);
    let layers = height.abs();
    for y in 0..layers {
        let y_term = f64::from(y - layers).powi(2) / height_sq;
        'x: for x in 0..=ceil[0] {
            let x_term = f64::from(x).powi(2) / rx2;
            for z in 0..=ceil[1] {
                let z_term = f64::from(z).powi(2) / rz2;
                let d = x_term + z_term - y_term;
                if d > 1.0 {
                    if z == 0 {
                        break 'x;
                    }
                    break;
                }
                if hollow {
                    let x_next = (f64::from(x) + thickness).powi(2) / rx2 + z_term - y_term;
                    let y_next = x_term + z_term - (f64::from(y) + thickness - f64::from(layers)).powi(2) / height_sq;
                    let z_next = x_term + (f64::from(z) + thickness).powi(2) / rz2 - y_term;
                    if x_next <= 0.0 && z_next <= 0.0 && (y_next <= 0.0 && f64::from(y) + thickness != f64::from(layers)) {
                        continue;
                    }
                }
                if d <= 0.0 {
                    let dy = if height < 0 { -y } else { y };
                    mirror4(&mut out, [base[0], base[1] + dy, base[2]], x, z);
                }
            }
        }
    }
    out
}

/// Java's `Math.round`.
fn round_half_up(v: f64) -> i32 {
    (v + 0.5).floor() as i32
}

/// `EditSession.drawLine`: the blocks on the straight line from `a` to `b` (stepping along the
/// longest axis), every block within `radius` of one of them, and with `filled` false only those
/// without all six neighbours in the set.
pub fn line(a: Pos, b: Pos, radius: f64, filled: bool) -> Vec<Pos> {
    let mut line: HashSet<Pos> = HashSet::new();
    let d = [(b[0] - a[0]).abs(), (b[1] - a[1]).abs(), (b[2] - a[2]).abs()];
    if d[0] + d[1] + d[2] == 0 {
        line.insert(a);
    } else {
        let dmax = d[0].max(d[1]).max(d[2]);
        let axis = if dmax == d[0] { 0 } else if dmax == d[1] { 1 } else { 2 };
        for step in 0..=dmax {
            let mut tip = a;
            for i in 0..3 {
                let sign = if b[i] - a[i] > 0 { 1.0 } else { -1.0 };
                tip[i] = if i == axis {
                    a[i] + step * if b[i] - a[i] > 0 { 1 } else { -1 }
                } else {
                    round_half_up(f64::from(a[i]) + f64::from(step) * f64::from(d[i]) / f64::from(dmax) * sign)
                };
            }
            line.insert(tip);
        }
    }
    let ceil = radius.ceil() as i32;
    let radius_sq = radius * radius;
    let mut ball: HashSet<Pos> = HashSet::new();
    for v in &line {
        for x in v[0] - ceil..=v[0] + ceil {
            for y in v[1] - ceil..=v[1] + ceil {
                for z in v[2] - ceil..=v[2] + ceil {
                    let (dx, dy, dz) = (f64::from(x - v[0]), f64::from(y - v[1]), f64::from(z - v[2]));
                    if dx * dx + dy * dy + dz * dz <= radius_sq {
                        ball.insert([x, y, z]);
                    }
                }
            }
        }
    }
    let mut out: Vec<Pos> = if filled {
        ball.iter().copied().collect()
    } else {
        ball.iter()
            .copied()
            .filter(|v| {
                ![[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]].iter().all(|o| ball.contains(&[v[0] + o[0], v[1] + o[1], v[2] + o[2]]))
            })
            .collect()
    };
    out.sort_unstable();
    out
}

/// The eight mirror images of an offset (each distinct position once).
fn mirror8(out: &mut Vec<Pos>, c: Pos, [x, y, z]: Pos) {
    for sx in signs(x) {
        for sy in signs(y) {
            for sz in signs(z) {
                out.push([c[0] + sx * x, c[1] + sy * y, c[2] + sz * z]);
            }
        }
    }
}

fn mirror4(out: &mut Vec<Pos>, c: Pos, x: i32, z: i32) {
    for sx in signs(x) {
        for sz in signs(z) {
            out.push([c[0] + sx * x, c[1], c[2] + sz * z]);
        }
    }
}

fn signs(v: i32) -> &'static [i32] {
    if v == 0 {
        &[1]
    } else {
        &[1, -1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique(v: &[Pos]) -> bool {
        v.iter().collect::<HashSet<_>>().len() == v.len()
    }

    #[test]
    fn spheres() {
        let s = sphere([0, 64, 0], [3.0; 3], false);
        assert!(unique(&s));
        // Radius 3 measured to 3.5, as makeSphere does (the upstream bundle measured to 3: 123).
        assert_eq!(s.len(), 179);
        assert!(s.contains(&[3, 64, 0]) && !s.contains(&[3, 65, 3]));
        let h = sphere([0, 64, 0], [3.0; 3], true);
        assert!(unique(&h) && h.len() < s.len() && !h.contains(&[0, 64, 0]));
        assert_eq!(sphere([5, 5, 5], [0.0; 3], false), vec![[5, 5, 5]]);
    }

    #[test]
    fn cylinders_and_pyramids() {
        let c = cylinder([0, 0, 0], [2.0, 2.0], 3, false);
        assert!(unique(&c));
        assert_eq!(c.len() % 3, 0);
        assert!(c.iter().all(|p| (0..3).contains(&p[1])));
        let down = cylinder([0, 10, 0], [1.0, 1.0], -2, false);
        assert!(down.iter().all(|p| p[1] == 8 || p[1] == 9));
        assert!(cylinder([0, 10, 0], [1.0, 1.0], 0, false).is_empty());
        let hc = cylinder([0, 0, 0], [3.0, 3.0], 1, true);
        assert!(!hc.contains(&[0, 0, 0]) && hc.contains(&[3, 0, 0]));
        let p = pyramid([0, 0, 0], 3, false);
        assert!(unique(&p));
        assert_eq!(p.len(), 25 + 9 + 1);
        let hp = pyramid([0, 0, 0], 3, true);
        assert_eq!(hp.len(), 16 + 8 + 1);
    }

    #[test]
    fn lines() {
        let l = line([0, 0, 0], [5, 2, 0], 0.0, true);
        assert_eq!(l.len(), 6);
        assert!(l.contains(&[0, 0, 0]) && l.contains(&[5, 2, 0]));
        assert!(line([0, 0, 0], [4, 0, 0], 1.0, true).len() > 5);
        // A thick line, hollowed: the middle of a 3-wide bar is gone.
        let solid = line([0, 0, 0], [8, 0, 0], 2.0, true);
        let shell = line([0, 0, 0], [8, 0, 0], 2.0, false);
        assert!(shell.len() < solid.len() && !shell.contains(&[4, 0, 0]));
    }

    #[test]
    fn cones() {
        let c = cone([0, 0, 0], [3.0, 3.0], 4, false, 1.0);
        assert!(unique(&c) && !c.is_empty());
        assert!(c.iter().all(|p| (0..4).contains(&p[1])));
        assert!(cone([0, 0, 0], [3.0, 3.0], 0, false, 1.0).is_empty());
    }
}
