//! The region operations that need more than "every position of a shape": overlay, hollow,
//! naturalize, smooth, stack, move, and the ray a brush or `//hpos1` follows.

use crate::horizon_worldedit_core::pattern::{Mask, Pattern};
use crate::horizon_worldedit_core::region::{add, scale, Cuboid};
use crate::horizon_worldedit_core::nbt::Tag;
use std::collections::{HashMap, HashSet, VecDeque};
use crate::horizon_worldedit_core::world::{Pos, StateId, World};

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
    let solid = |p: Pos| world.get(p).is_some_and(|s| crate::horizon_worldedit_core::pattern::is_solid_name(&world.describe(s).name));
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
    let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
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

/// `//snowsmooth` (WorldEdit's `SnowHeightMap`): the height map of the region where a snow layer counts as
/// a fraction of a block, blurred `iterations` times with the 5x5 kernel and written back. A column that
/// grows gets snow layers on top and copies the blocks below it (`layer_blocks` snow blocks at its foot);
/// a column that shrinks is cut down the same way. Columns with liquid at their top do not grow.
pub fn snow_smooth(world: &dyn World, region: &Cuboid, iterations: u32, layer_blocks: i32, mask: Option<&Mask>) -> Vec<(Pos, StateId)> {
    let [w, _, l] = region.size();
    let (w, l) = (w as usize, l as usize);
    let (origin, top_y) = (region.min[1], region.max[1]);
    let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
    let state = |name: &str, props: &[(&str, &str)]| world.resolve(&crate::horizon_worldedit_core::block::BlockState::new(name, props)).unwrap_or(air);
    let ground = |s: StateId| match mask {
        Some(m) => m.hits(world, s),
        None => !world.is_air(s),
    };
    let snow_layers = |n: i32| {
        let n = n.to_string();
        state("minecraft:snow", &[("layers", n.as_str())])
    };
    let snow_block = state("minecraft:snow_block", &[]);
    // SnowHeightMap's constructor: the top of each column, in blocks, a snow layer as a fraction.
    let mut data = vec![0.0f64; w * l];
    let mut loaded = vec![true; w * l];
    for i in 0..w {
        for j in 0..l {
            let (x, z) = (region.min[0] + i as i32, region.min[2] + j as i32);
            let idx = i * l + j;
            // getHighestTerrainBlock: the highest ground block, or the floor when there is none.
            let mut top = origin;
            for y in (origin..=top_y).rev() {
                match world.get([x, y, z]) {
                    Some(s) if ground(s) => {
                        top = y;
                        break;
                    }
                    None => {
                        loaded[idx] = false;
                        break;
                    }
                    _ => {}
                }
            }
            if !loaded[idx] {
                continue;
            }
            let upper = world.describe(world.get([x, top + 1, z]).unwrap_or(air));
            data[idx] = if upper.name == "minecraft:snow" {
                let layers = upper.prop("layers").and_then(|v| v.parse::<i32>().ok()).unwrap_or(1);
                f64::from(top + 1) + f64::from(layers - 1) / 8.0
            } else if world.is_air(world.get([x, top, z]).unwrap_or(air)) {
                f64::from(top)
            } else {
                f64::from(top + 1)
            };
        }
    }
    // applyFilter: the blur `iterations` times (clamped edges, as `smooth`), then half a snow layer more.
    let k = kernel();
    let mut current = data.clone();
    for _ in 0..iterations {
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
    let target: Vec<f64> = current.iter().map(|h| h + 0.0625).collect();
    // applyChanges. Reads come from the world as it is: a column only reads cells it has not written yet.
    let mut out = Vec::new();
    let o = f64::from(origin);
    for i in 0..w {
        for j in 0..l {
            let idx = i * l + j;
            if !loaded[idx] {
                continue;
            }
            let cur = data[idx];
            if cur == o {
                continue;
            }
            let (x, z) = (region.min[0] + i as i32, region.min[2] + j as i32);
            let new = target[idx].min(f64::from(top_y));
            let scale = (cur - o) / (new - o);
            if new >= cur {
                let existing = world.get([x, cur.floor() as i32, z]).unwrap_or(air);
                if matches!(world.describe(existing).name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column") {
                    continue;
                }
                let snow_y = new.floor() as i32;
                out.push(([x, snow_y, z], snow_layers(((new - f64::from(snow_y)) * 8.0) as i32 + 1)));
                let snow_from = (new - 1.0 - o - f64::from(layer_blocks)).floor() as i64;
                let mut y = (new - 1.0 - o).floor() as i64;
                while y >= 0 {
                    let p = [x, origin + y as i32, z];
                    if y >= snow_from {
                        out.push((p, snow_block));
                    } else {
                        let from = (y as f64 * scale).floor() as i32;
                        out.push((p, world.get([x, origin + from, z]).unwrap_or(air)));
                    }
                    y -= 1;
                }
            } else {
                let rel_top = (new - o).floor() as i64;
                let snow_from = (new - o - f64::from(layer_blocks)).floor() as i64;
                for y in 0..rel_top {
                    let p = [x, origin + y as i32, z];
                    if y >= snow_from {
                        out.push((p, snow_block));
                    } else {
                        let from = (y as f64 * scale).floor() as i32;
                        out.push((p, world.get([x, origin + from, z]).unwrap_or(air)));
                    }
                }
                let snow_y = new.floor() as i32;
                out.push(([x, snow_y, z], snow_layers(((new - f64::from(snow_y)) * 8.0) as i32 + 1)));
                for y in (new + 1.0).floor() as i32..=cur.floor() as i32 {
                    out.push(([x, y, z], air));
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

/// The block entity data of the blocks `//stack` or `//move` copy (the same blocks they pick), by each place the
/// block goes to: `shifts` are the offsets of the copies.
pub fn carried_entities(world: &dyn World, region: &Cuboid, shifts: &[Pos], only: Option<&Mask>) -> HashMap<Pos, Tag> {
    let mut out = HashMap::new();
    for p in region.positions() {
        let Some(s) = world.get(p) else { continue };
        if world.is_air(s) || !only.is_none_or(|m| m.hits(world, s)) {
            continue;
        }
        if let Some(tag) = world.block_entity_at(p) {
            for &by in shifts {
                out.insert(add(p, by), tag.clone());
            }
        }
    }
    out
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
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

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
    fn snow_smooth_puts_a_snow_layer_on_flat_ground() {
        let mut w = FakeWorld::new();
        w.fill([0, 0, 0], [8, 4, 8], "stone");
        let out = snow_smooth(&w, &Cuboid::new([0, 0, 0], [8, 10, 8]), 0, 1, None);
        // No blur: the column stands at 5 (the top stone is at 4) plus 1/16, so one snow layer at y 5.
        assert!(out.contains(&([4, 5, 4], w.id("minecraft:snow[layers=1]"))));
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
    fn stack_and_move_carry_block_entities() {
        let mut w = FakeWorld::new();
        w.fill([0, 0, 0], [1, 0, 0], "stone");
        let tag = Tag::Byte(7);
        w.entities.insert([1, 0, 0], tag.clone());
        let r = Cuboid::new([0, 0, 0], [1, 0, 0]);
        let got = carried_entities(&w, &r, &[[2, 0, 0], [4, 0, 0]], None);
        assert_eq!(got, HashMap::from([([3, 0, 0], tag.clone()), ([5, 0, 0], tag)]));
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
