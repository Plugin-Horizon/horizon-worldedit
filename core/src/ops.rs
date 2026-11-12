//! The region operations that need more than "every position of a shape": overlay, hollow,
//! naturalize, smooth, stack, move, and the ray a brush or `//hpos1` follows.

use crate::pattern::{Mask, Pattern};
use crate::region::{add, scale, Cuboid};
use std::collections::{HashSet, VecDeque};
use crate::world::{Pos, StateId, World};

/// `//overlay`: the pattern on top of the highest existing block of each column of the region
/// (WorldEdit's layer visitor: a column whose block above the region is already existing is skipped).
pub fn overlay(world: &dyn World, region: &Cuboid, pattern: &Pattern, salt: u64) -> Vec<(Pos, StateId)> {
    let mut out = Vec::new();
    for x in region.min[0]..=region.max[0] {
        for z in region.min[2]..=region.max[2] {
            if world.get([x, region.max[1] + 1, z]).is_some_and(|s| !world.is_air(s)) {
                continue;
            }
            for y in (region.min[1]..=region.max[1]).rev() {
                match world.get([x, y, z]) {
                    Some(s) if !world.is_air(s) => {
                        let p = [x, y + 1, z];
                        if p[1] <= world.max_y() {
                            out.push((p, pattern.pick(salt, p)));
                        }
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }
    out
}

/// `//hollow`, as `EditSession.hollowOutRegion`: the blocks reachable from the region's faces
/// through non-solid blocks are the outside; with `thickness` above 1 that many layers inside it
/// are kept too; everything not touching the outside becomes `pattern`.
pub fn hollow(world: &dyn World, region: &Cuboid, thickness: i32, pattern: &Pattern, salt: u64) -> Vec<(Pos, StateId)> {
    const DIRS: [Pos; 6] = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 1, 0], [0, -1, 0]];
    let solid = |p: Pos| world.get(p).is_some_and(|s| crate::pattern::is_solid_name(&world.describe(s).name));
    let mut outside: HashSet<Pos> = HashSet::new();
    let flood = |origin: Pos, outside: &mut HashSet<Pos>| {
        let mut queue = VecDeque::from([origin]);
        while let Some(cur) = queue.pop_front() {
            if solid(cur) || !outside.insert(cur) || !region.contains(cur) {
                continue;
            }
            for d in DIRS {
                queue.push_back(add(cur, d));
            }
        }
    };
    let (min, max) = (region.min, region.max);
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            flood([x, y, min[2]], &mut outside);
            flood([x, y, max[2]], &mut outside);
        }
    }
    for y in min[1]..=max[1] {
        for z in min[2]..=max[2] {
            flood([min[0], y, z], &mut outside);
            flood([max[0], y, z], &mut outside);
        }
    }
    for z in min[2]..=max[2] {
        for x in min[0]..=max[0] {
            flood([x, min[1], z], &mut outside);
            flood([x, max[1], z], &mut outside);
        }
    }
    for _ in 1..thickness {
        let layer: Vec<Pos> = region.positions().filter(|&p| DIRS.iter().any(|&d| outside.contains(&add(p, d)))).collect();
        outside.extend(layer);
    }
    region.positions().filter(|&p| !DIRS.iter().any(|&d| outside.contains(&add(p, d)))).map(|p| (p, pattern.pick(salt, p))).collect()
}

/// `//naturalize`, as WorldEdit's `Naturalizer`: in each column, from the first grass, dirt or
/// stone block down, grass on top, three dirt, stone below (blocks of other kinds stay).
pub fn naturalize(world: &dyn World, region: &Cuboid, [grass, dirt, stone]: [StateId; 3]) -> Vec<(Pos, StateId)> {
    let ground = |s: StateId| matches!(world.describe(s).name.as_str(), "minecraft:grass_block" | "minecraft:dirt" | "minecraft:stone");
    let mut out = Vec::new();
    for x in region.min[0]..=region.max[0] {
        for z in region.min[2]..=region.max[2] {
            // A column already underground at the top is left alone.
            if world.get([x, region.max[1] + 1, z]).is_some_and(ground) {
                continue;
            }
            let mut ground_y = None;
            for y in (region.min[1]..=region.max[1]).rev() {
                let Some(s) = world.get([x, y, z]) else { continue };
                if !ground(s) {
                    continue;
                }
                let top = *ground_y.get_or_insert(y);
                let (to, name) = match top - y {
                    0 => (grass, "minecraft:grass_block"),
                    1..=3 => (dirt, "minecraft:dirt"),
                    _ => (stone, "minecraft:stone"),
                };
                if world.describe(s).name != name {
                    out.push(([x, y, z], to));
                }
            }
        }
    }
    out
}

/// The 5x5 Gaussian kernel (sigma 1) WorldEdit's `//smooth` uses.
fn kernel() -> [[f64; 5]; 5] {
    let mut k = [[0.0; 5]; 5];
    let mut sum = 0.0;
    for (i, row) in k.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let (dx, dz) = (i as f64 - 2.0, j as f64 - 2.0);
            *v = (-(dx * dx + dz * dz) / 2.0).exp();
            sum += *v;
        }
    }
    for row in &mut k {
        for v in row {
            *v /= sum;
        }
    }
    k
}

/// `//smooth` and the smooth brush: the region's height map blurred `iterations` times, each column
/// raised (its top block on top, the block under it below) or cut down to the new height. Only
/// blocks `mask` accepts count as ground (`None`: every block but air).
pub fn smooth(world: &dyn World, region: &Cuboid, iterations: u32, mask: Option<&Mask>) -> Vec<(Pos, StateId)> {
    let [w, _, l] = region.size();
    let (w, l) = (w as usize, l as usize);
    let ground = |s: StateId| match mask {
        Some(m) => m.hits(world, s),
        None => !world.is_air(s),
    };
    // The highest ground block of each column, `min - 1` for none.
    let mut heights = vec![region.min[1] - 1; w * l];
    let mut loaded = vec![true; w * l];
    for i in 0..w {
        for j in 0..l {
            let (x, z) = (region.min[0] + i as i32, region.min[2] + j as i32);
            for y in (region.min[1]..=region.max[1]).rev() {
                match world.get([x, y, z]) {
                    Some(s) if ground(s) => {
                        heights[i * l + j] = y;
                        break;
                    }
                    None => {
                        loaded[i * l + j] = false;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    let k = kernel();
    let mut current: Vec<f64> = heights.iter().map(|&h| f64::from(h)).collect();
    for _ in 0..iterations.max(1) {
        let mut next = current.clone();
        for i in 0..w {
            for j in 0..l {
                let mut v = 0.0;
                for (di, row) in k.iter().enumerate() {
                    for (dj, weight) in row.iter().enumerate() {
                        let ii = (i as i64 + di as i64 - 2).clamp(0, w as i64 - 1) as usize;
                        let jj = (j as i64 + dj as i64 - 2).clamp(0, l as i64 - 1) as usize;
                        v += weight * current[ii * l + jj];
                    }
                }
                next[i * l + j] = v;
            }
        }
        current = next;
    }
    let mut out = Vec::new();
    let air = world.resolve(&crate::block::BlockState::air()).unwrap_or(0);
    for i in 0..w {
        for j in 0..l {
            if !loaded[i * l + j] {
                continue;
            }
            let (x, z) = (region.min[0] + i as i32, region.min[2] + j as i32);
            let old = heights[i * l + j];
            let new = (current[i * l + j] + 0.5).floor() as i32;
            let new = new.clamp(region.min[1] - 1, region.max[1]);
            if new == old || old < region.min[1] {
                continue;
            }
            let top = world.get([x, old, z]).unwrap_or(air);
            let below = world.get([x, old - 1, z]).filter(|&s| ground(s)).unwrap_or(top);
            if new > old {
                for y in old..new {
                    out.push(([x, y, z], below));
                }
                out.push(([x, new, z], top));
            } else {
                for y in (new + 1)..=old {
                    out.push(([x, y, z], air));
                }
                if new >= region.min[1] {
                    out.push(([x, new, z], top));
                }
            }
        }
    }
    out
}

/// `//stack`: the region copied `count` times next to itself toward `dir`.
pub fn stack(world: &dyn World, region: &Cuboid, step: Pos, count: i32, skip_air: bool, only: Option<&Mask>) -> Result<(Vec<(Pos, StateId)>, usize), String> {
    let mut source = Vec::with_capacity(region.volume() as usize);
    for p in region.positions() {
        let s = world.get(p).ok_or("Part of the selection is not loaded")?;
        if !(skip_air && world.is_air(s)) && only.is_none_or(|m| m.hits(world, s)) {
            source.push((p, s));
        }
    }
    let mut out = Vec::with_capacity(source.len() * count.max(0) as usize);
    for n in 1..=count.max(0) {
        let by = scale(step, n);
        out.extend(source.iter().map(|&(p, s)| (add(p, by), s)));
    }
    let copied = source.len() * count.max(0) as usize;
    Ok((out, copied))
}

/// `//move`: the region cleared to `leave` and placed `distance` blocks toward `dir` (one edit).
pub fn move_region(world: &dyn World, region: &Cuboid, by: Pos, leave: &Pattern, salt: u64, skip_air: bool, only: Option<&Mask>) -> Result<(Vec<(Pos, StateId)>, usize), String> {
    let mut placed = Vec::with_capacity(region.volume() as usize);
    let mut cleared = Vec::with_capacity(region.volume() as usize);
    for p in region.positions() {
        let s = world.get(p).ok_or("Part of the selection is not loaded")?;
        if !(skip_air && world.is_air(s)) && only.is_none_or(|m| m.hits(world, s)) {
            cleared.push((p, leave.pick(salt, p)));
            placed.push((add(p, by), s));
        }
    }
    let moved = placed.len();
    let mut out = cleared;
    out.extend(placed);
    Ok((out, moved))
}

/// The first block that isn't air along the ray from `eye` toward `dir`, at most `range` blocks
/// away; `None` past the range, at the build height's edge or at a chunk that isn't loaded.
pub fn raycast(world: &dyn World, eye: [f64; 3], dir: [f64; 3], range: f64) -> Option<Pos> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len == 0.0 {
        return None;
    }
    let d = dir.map(|v| v / len);
    let mut cell: Pos = eye.map(|v| v.floor() as i32);
    let step: Pos = d.map(|v| if v > 0.0 { 1 } else if v < 0.0 { -1 } else { 0 });
    let next_edge = |a: usize| {
        if step[a] > 0 {
            (f64::from(cell[a]) + 1.0 - eye[a]) / d[a]
        } else if step[a] < 0 {
            (eye[a] - f64::from(cell[a])) / -d[a]
        } else {
            f64::INFINITY
        }
    };
    let mut t_max = [next_edge(0), next_edge(1), next_edge(2)];
    let t_delta = d.map(|v| if v == 0.0 { f64::INFINITY } else { 1.0 / v.abs() });
    loop {
        let a = (0..3).min_by(|&i, &j| t_max[i].total_cmp(&t_max[j])).unwrap_or(0);
        if t_max[a] > range {
            return None;
        }
        cell[a] += step[a];
        t_max[a] += t_delta[a];
        if cell[1] < world.min_y() || cell[1] > world.max_y() {
            return None;
        }
        let s = world.get(cell)?;
        if !world.is_air(s) {
            return Some(cell);
        }
    }
}

/// The direction a player looks, from Minecraft's yaw and pitch (degrees).
pub fn look_vector(yaw: f32, pitch: f32) -> [f64; 3] {
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::fake::FakeWorld;

    #[test]
    fn overlay_goes_on_top() {
        let mut w = FakeWorld::new();
        w.fill([0, 60, 0], [1, 60, 0], "stone");
        w.fill([1, 61, 0], [1, 61, 0], "dirt");
        let p = Pattern::single(w.id("sand"), "sand");
        let mut out = overlay(&w, &Cuboid::new([0, 50, 0], [1, 70, 0]), &p, 0);
        out.sort();
        assert_eq!(out, vec![([0, 61, 0], w.id("sand")), ([1, 62, 0], w.id("sand"))]);
    }

    #[test]
    fn naturalize_layers() {
        let mut w = FakeWorld::new();
        w.fill([0, 0, 0], [0, 9, 0], "stone");
        let ids = [w.id("grass_block"), w.id("dirt"), w.id("stone")];
        let out = naturalize(&w, &Cuboid::new([0, 0, 0], [0, 20, 0]), ids);
        // Grass on top, three dirt, stone stays stone (nothing to write).
        assert!(out.contains(&([0, 9, 0], ids[0])) && out.contains(&([0, 8, 0], ids[1])) && out.contains(&([0, 6, 0], ids[1])));
        assert!(!out.iter().any(|(p, _)| p[1] <= 5));
    }

    #[test]
    fn hollow_empties_the_inside_of_a_solid_box() {
        // The region must reach air on every side, as WorldEdit's flood fill starts from its faces.
        let mut w = FakeWorld::new();
        w.fill([1, 1, 1], [6, 6, 6], "stone");
        let air = Pattern::single(0, "air");
        let r = Cuboid::new([0, 0, 0], [7, 7, 7]);
        let one = hollow(&w, &r, 1, &air, 0);
        assert_eq!(one.len(), 4 * 4 * 4);
        assert!(one.iter().all(|(p, _)| p.iter().all(|&c| (2..=5).contains(&c))));
        let two = hollow(&w, &r, 2, &air, 0);
        assert_eq!(two.len(), 2 * 2 * 2);
        // A region whose faces are solid has no outside: all of it goes.
        let full = hollow(&w, &Cuboid::new([1, 1, 1], [6, 6, 6]), 1, &air, 0);
        assert_eq!(full.len(), 6 * 6 * 6);
    }

    #[test]
    fn smooth_flattens_a_spike() {
        let mut w = FakeWorld::new();
        w.fill([0, 0, 0], [8, 0, 8], "stone");
        w.fill([4, 1, 4], [4, 6, 4], "stone");
        let out = smooth(&w, &Cuboid::new([0, 0, 0], [8, 10, 8]), 2, None);
        // The spike is cut down to about its surroundings.
        assert!(out.contains(&([4, 6, 4], 0)) && out.contains(&([4, 2, 4], 0)));
        assert!(out.iter().all(|(p, _)| p[1] <= 6));
    }

    #[test]
    fn stack_and_move() {
        let mut w = FakeWorld::new();
        w.fill([0, 0, 0], [1, 0, 0], "stone");
        let r = Cuboid::new([0, 0, 0], [1, 0, 0]);
        let (s, copied) = stack(&w, &r, [2, 0, 0], 2, false, None).unwrap();
        assert_eq!(copied, 4);
        assert_eq!(s.iter().map(|x| x.0).collect::<Vec<_>>(), vec![[2, 0, 0], [3, 0, 0], [4, 0, 0], [5, 0, 0]]);
        let air = Pattern::single(0, "air");
        let (m, moved) = move_region(&w, &r, [0, 3, 0], &air, 0, false, None).unwrap();
        assert_eq!(moved, 2);
        assert_eq!(m.len(), 4);
        assert!(m.contains(&([0, 3, 0], w.id("stone"))) && m.contains(&([0, 0, 0], 0)));
    }

    #[test]
    fn ray_hits_the_first_block() {
        let mut w = FakeWorld::new();
        w.fill([5, 64, 0], [5, 64, 0], "stone");
        assert_eq!(raycast(&w, [0.5, 64.5, 0.5], [1.0, 0.0, 0.0], 10.0), Some([5, 64, 0]));
        assert_eq!(raycast(&w, [0.5, 64.5, 0.5], [1.0, 0.0, 0.0], 3.0), None);
        assert_eq!(raycast(&w, [0.5, 64.5, 0.5], [-1.0, 0.0, 0.0], 10.0), None);
        let v = look_vector(-90.0, 0.0);
        assert!((v[0] - 1.0).abs() < 1e-9 && v[2].abs() < 1e-9);
    }
}
