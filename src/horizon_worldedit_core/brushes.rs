//! The brushes that need more than a shape and a pattern: morph (`EditSession.morph`), the snow brush (`ApplyLayer`
//! with `SnowSimulator`), the heightmap brush (`ImageHeightmapBrush`), the butcher brush's reach and the gravity brush
//! (`GravityBrush`). Each returns the writes; the engine applies the brush mask and commits them as one edit.

use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::heightmap::HeightImage;
use crate::horizon_worldedit_core::pattern::{is_solid_name, Mask};
use crate::horizon_worldedit_core::shapes;
use crate::horizon_worldedit_core::world::{EntityInfo, Pos, StateId, World};
use std::collections::HashMap;

/// `EditSession.recurseDirections`: north, east, south, west, up, down.
const RECURSE: [[i32; 3]; 6] = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 1, 0], [0, -1, 0]];

fn air_id(world: &dyn World) -> StateId {
    world.resolve(&BlockState::air()).unwrap_or(0)
}

/// `Material.isAir() || Material.isLiquid()`: what morph treats as empty.
fn empty(world: &dyn World, cache: &mut HashMap<StateId, bool>, s: StateId) -> bool {
    *cache.entry(s).or_insert_with(|| {
        let d = world.describe(s);
        d.is_air() || matches!(d.name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column")
    })
}

/// `EditSession.morph`: erodes, then dilates, on a copy of the cube round `pos` and writes back what changed. An erode
/// pass turns a solid block that touches at least `min_erode` air or liquid faces into the most common of those
/// neighbours; a dilate pass turns an empty block that touches at least `min_dilate` solid faces into the most common
/// of those. Ties go to the first neighbour counted (north, east, south, west, up, down).
pub fn morph(world: &dyn World, pos: Pos, size: f64, min_erode: i32, erode_iter: i32, min_dilate: i32, dilate_iter: i32, limit: u64) -> Result<Vec<(Pos, StateId)>, String> {
    let c = size.ceil() as i64;
    let n = c * 2 + 3;
    if n < 3 || (n as u64).saturating_pow(3) > limit {
        return Err(format!("The morph brush works on at most {limit} blocks at once; use a smaller size"));
    }
    let (c, n) = (c as i32, n as usize);
    let idx = |x: usize, y: usize, z: usize| (x * n + y) * n + z;
    let air = air_id(world);
    let mut cur = vec![air; n * n * n];
    for x in 0..n {
        for y in 0..n {
            for z in 0..n {
                let p = [pos[0] + x as i32 - c - 1, pos[1] + y as i32 - c - 1, pos[2] + z as i32 - c - 1];
                cur[idx(x, y, z)] = world.get(p).unwrap_or(air);
            }
        }
    }
    let original = cur.clone();
    let mut next = cur.clone();
    let size_sq = size * size;
    let mut cache: HashMap<StateId, bool> = HashMap::new();
    let reach = (c * 2) as usize;
    for pass in 0..2 {
        let (iterations, min_faces) = if pass == 0 { (erode_iter, min_erode) } else { (dilate_iter, min_dilate) };
        for _ in 0..iterations.max(0) {
            for x in 0..=reach {
                for y in 0..=reach {
                    for z in 0..=reach {
                        let real = [x as i32 - c, y as i32 - c, z as i32 - c];
                        if f64::from(real[0]).powi(2) + f64::from(real[1]).powi(2) + f64::from(real[2]).powi(2) > size_sq {
                            continue;
                        }
                        let here = cur[idx(x + 1, y + 1, z + 1)];
                        next[idx(x + 1, y + 1, z + 1)] = here;
                        // Erosion needs a solid block, dilation an empty one.
                        let is_empty = empty(world, &mut cache, here);
                        if pass == 0 && is_empty || pass == 1 && !is_empty {
                            continue;
                        }
                        let mut counts: Vec<(StateId, i32)> = Vec::new();
                        let (mut faces, mut best_count, mut best) = (0, 0, here);
                        for d in RECURSE {
                            let adj = cur[idx((x as i32 + 1 + d[0]) as usize, (y as i32 + 1 + d[1]) as usize, (z as i32 + 1 + d[2]) as usize)];
                            // Erosion counts empty neighbours, dilation solid ones.
                            if empty(world, &mut cache, adj) != (pass == 0) {
                                continue;
                            }
                            faces += 1;
                            let count = match counts.iter_mut().find(|(s, _)| *s == adj) {
                                Some(e) => {
                                    e.1 += 1;
                                    e.1
                                }
                                None => {
                                    counts.push((adj, 1));
                                    1
                                }
                            };
                            if count > best_count {
                                best_count = count;
                                best = adj;
                            }
                        }
                        if faces >= min_faces {
                            next[idx(x + 1, y + 1, z + 1)] = best;
                        }
                    }
                }
            }
            std::mem::swap(&mut cur, &mut next);
        }
    }
    let mut out = Vec::new();
    for x in 0..n {
        for y in 0..n {
            for z in 0..n {
                let (was, now) = (original[idx(x, y, z)], cur[idx(x, y, z)]);
                if was != now {
                    out.push(([pos[0] + x as i32 - c - 1, pos[1] + y as i32 - c - 1, pos[2] + z as i32 - c - 1], now));
                }
            }
        }
    }
    Ok(out)
}

/// What `LayerVisitor` walks for the snow brush: the columns and the height range of the brush's region.
/// `shape`: 0 sphere, 1 cylinder, 2 cuboid. A sphere is cut to its bounding box (`Regions.asFlatRegion`), a cylinder
/// is `CylinderRegionFactory(radius)`: as high as it is wide, its columns round.
pub fn snow_columns(pos: Pos, size: f64, shape: u8) -> (Vec<(i32, i32)>, i32, i32) {
    match shape {
        0 => {
            // `EllipsoidRegion` bounds: the centre plus and minus the radius, floored.
            let (lo, hi) = ([0, 1, 2].map(|a| (f64::from(pos[a]) - size).floor() as i32), [0, 1, 2].map(|a| (f64::from(pos[a]) + size).floor() as i32));
            let cols = (lo[0]..=hi[0]).flat_map(|x| (lo[2]..=hi[2]).map(move |z| (x, z))).collect();
            (cols, lo[1], hi[1])
        }
        1 => {
            let half = (size / 2.0) as i32;
            let cols = shapes::cylinder([pos[0], 0, pos[2]], [size; 2], 1, false).into_iter().map(|p| (p[0], p[2])).collect();
            (cols, pos[1] - half, pos[1] + half)
        }
        _ => {
            let r = size as i32;
            let cols = (pos[0] - r..=pos[0] + r).flat_map(|x| (pos[2] - r..=pos[2] + r).map(move |z| (x, z))).collect();
            (cols, pos[1] - r, pos[1] + r)
        }
    }
}

/// Whether a snow layer survives on top of this block (as `//snow` decides it).
fn holds_snow(name: &str) -> bool {
    is_solid_name(name) && !matches!(name, "minecraft:snow" | "minecraft:ice" | "minecraft:packed_ice" | "minecraft:blue_ice" | "minecraft:barrier") && !name.ends_with("_slab") && !name.ends_with("_stairs") && !name.ends_with("_fence")
}

/// `LayerVisitor` with `SnowSimulator`: in each column, the first ground block from `max_y` down to `min_y` gets a
/// snow layer on top (one more layer on existing snow with `stack`, a snow block when full); still water becomes ice.
pub fn snow(world: &dyn World, columns: &[(i32, i32)], min_y: i32, max_y: i32, stack: bool) -> Vec<(Pos, StateId)> {
    let (Some(layer), snow_block, ice) = (world.resolve(&BlockState::new("minecraft:snow", &[])), world.resolve(&BlockState::new("minecraft:snow_block", &[])), world.resolve(&BlockState::new("minecraft:ice", &[]))) else {
        return Vec::new();
    };
    let is_ground = |p: Pos| -> bool {
        let Some(s) = world.get(p) else { return false };
        let b = world.describe(s);
        if b.is_air() || (stack && b.name == "minecraft:snow") {
            return false;
        }
        b.name == "minecraft:water" || is_solid_name(&b.name)
    };
    let top = world.max_y();
    let mut out = Vec::new();
    for &(x, z) in columns {
        // Abort if we are underground.
        if is_ground([x, max_y.saturating_add(1), z]) {
            continue;
        }
        let Some(y) = (min_y..=max_y).rev().find(|&y| is_ground([x, y, z])) else { continue };
        let here = world.describe(world.get([x, y, z]).unwrap_or(0));
        if here.name == "minecraft:water" {
            if here.prop("level") == Some("0") {
                if let Some(ice) = ice {
                    out.push(([x, y, z], ice));
                }
            }
            continue;
        }
        // Can't put snow this far up.
        if y == top {
            continue;
        }
        let up = [x, y + 1, z];
        let Some(above) = world.get(up).map(|s| world.describe(s)) else { continue };
        if stack && above.name == "minecraft:snow" {
            let layers: u32 = above.prop("layers").and_then(|l| l.parse().ok()).unwrap_or(1);
            if layers >= 7 {
                if let Some(b) = snow_block {
                    out.push((up, b));
                }
            } else {
                let mut more = above.clone();
                more.properties.insert("layers".into(), (layers + 1).to_string());
                if let Some(id) = world.resolve(&more) {
                    out.push((up, id));
                }
            }
            continue;
        }
        if !above.is_air() || !holds_snow(&here.name) {
            continue;
        }
        out.push((up, layer));
    }
    out
}

/// `ImageHeightmapBrush.build`: for each column within `ceil(size)` of `pos`, the top terrain block (solid, or what the
/// brush mask accepts) grows up by `height` blocks of itself, or with `erase` loses `height` blocks; `flatten` stops
/// at the height of the click. `random` (0..1) is the one random draw of a `-r` brush.
#[allow(clippy::too_many_arguments)]
pub fn heightmap(world: &dyn World, mask: Option<&Mask>, image: &HeightImage, intensity: f64, erase: bool, flatten: bool, randomize: bool, pos: Pos, size: f64, random: f64) -> Vec<(Pos, StateId)> {
    let size = size.ceil() as i32;
    let (lo, hi) = (world.min_y(), world.max_y());
    let air = air_id(world);
    let mut out: Vec<(Pos, StateId)> = Vec::new();
    for off_x in -size..=size {
        for off_z in -size..=size {
            let (x, z) = (pos[0] + off_x, pos[2] + off_z);
            // `getHighestTerrainBlock(x, z, min, max, mask)`: minY when nothing matches.
            let top = (lo..=hi).rev().find(|&y| world.get([x, y, z]).is_some_and(|s| match mask {
                Some(m) => m.hits(world, s),
                None => is_solid_name(&world.describe(s).name),
            }));
            let y0 = top.unwrap_or(lo);
            let Some(base) = world.get([x, y0, z]) else { continue };
            if mask.is_some_and(|m| !m.hits(world, base)) {
                continue;
            }
            let mut height = image.height_at(off_x + size, off_z + size, size * 2 + 1) * intensity;
            // Add a bit of variation.
            if randomize && random > 1.0 - height % 1.0 {
                height += 1.0;
            }
            let mut i = 0;
            while f64::from(i) < height {
                if erase {
                    // Remove blocks if using the erase flag.
                    if !flatten || y0 - i > pos[1] {
                        out.push(([x, y0 - i, z], air));
                    }
                } else if !flatten || y0 + i <= pos[1] {
                    out.push(([x, y0 + i, z], base));
                }
                i += 1;
            }
        }
    }
    out
}

/// `GravityBrush.build`: in each column within `size` of `pos`, every block between `pos.y - offset` and
/// `pos.y + offset` falls to the lowest air below it. The loops step in whole blocks from `pos - size`, as the
/// original's doubles do, and floor each coordinate.
pub fn gravity(world: &dyn World, pos: Pos, size: f64, offset: f64) -> Vec<(Pos, StateId)> {
    let (lo, hi) = (world.min_y(), world.max_y());
    let y_max = (f64::from(pos[1]) + offset).min(f64::from(hi));
    let y_min = (f64::from(pos[1]) - offset).max(f64::from(lo));
    let air = air_id(world);
    let mut out = Vec::new();
    let mut x = f64::from(pos[0]) - size;
    while x <= f64::from(pos[0]) + size {
        let mut z = f64::from(pos[2]) - size;
        while z <= f64::from(pos[2]) + size {
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let mut lowest: Option<i32> = None;
            let (mut put, mut gone): (Vec<(Pos, StateId)>, Vec<Pos>) = (Vec::new(), Vec::new());
            let mut y = y_min;
            while y <= y_max {
                let by = y.floor() as i32;
                y += 1.0;
                let Some(s) = world.get([bx, by, bz]) else { continue };
                if world.is_air(s) {
                    lowest.get_or_insert(by);
                    continue;
                }
                let Some(l) = lowest else { continue };
                lowest = Some(l + 1);
                gone.retain(|g| *g != [bx, l, bz]);
                put.push(([bx, l, bz], s));
                gone.push([bx, by, bz]);
            }
            out.extend(put);
            out.extend(gone.into_iter().map(|g| (g, air)));
            z += 1.0;
        }
        x += 1.0;
    }
    out
}

/// The flags of a butcher brush (`CreatureButcher.Flags`).
pub mod butcher_flags {
    pub const PETS: u8 = 1;
    pub const NPCS: u8 = 1 << 1;
    pub const GOLEMS: u8 = 1 << 2;
    pub const ANIMALS: u8 = 1 << 3;
    pub const AMBIENT: u8 = 1 << 4;
    pub const TAGGED: u8 = 1 << 5;
    pub const ARMOR_STAND: u8 = 1 << 6;
    pub const WATER: u8 = 1 << 7;
    /// `-f`: everything friendly (the armor stand is not part of it).
    pub const FRIENDLY: u8 = PETS | NPCS | ANIMALS | GOLEMS | AMBIENT | TAGGED | WATER;
}

/// The entities a butcher brush at `pos` reaches: `CylinderRegion.createRadius` (full height, `size` wide) contains a
/// block when its column is within `size` of the centre (no half block added, unlike the `/butcher` command).
pub fn in_butcher_cylinder(entities: Vec<EntityInfo>, pos: Pos, size: f64) -> Vec<EntityInfo> {
    entities
        .into_iter()
        .filter(|e| {
            let (dx, dz) = (f64::from(e.pos[0].floor() as i32 - pos[0]), f64::from(e.pos[2].floor() as i32 - pos[2]));
            (dx / size).powi(2) + (dz / size).powi(2) <= 1.0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    #[test]
    fn morph_erodes_a_pillar_and_dilates_a_hole() {
        let mut w = FakeWorld::new();
        // A one-block pillar of stone in the air: every face but the bottom one is open, so erosion (min 2) takes it.
        w.fill([0, 64, 0], [0, 64, 0], "stone");
        let out = morph(&w, [0, 64, 0], 2.0, 2, 1, 5, 0, 1_000_000).unwrap();
        assert_eq!(out, vec![([0, 64, 0], 0)]);
        // A hole in a solid floor: five solid faces, so dilation (min 5) fills it with the floor's block.
        let mut w = FakeWorld::new();
        w.fill([-3, 60, -3], [3, 64, 3], "stone");
        w.blocks.remove(&[0, 64, 0]);
        let out = morph(&w, [0, 64, 0], 2.0, 9, 0, 5, 1, 1_000_000).unwrap();
        let stone = w.id("stone");
        assert!(out.contains(&([0, 64, 0], stone)), "{out:?}");
        // Nothing changes when the thresholds cannot be met.
        assert!(morph(&w, [0, 64, 0], 2.0, 7, 1, 7, 1, 1_000_000).unwrap().is_empty());
    }

    #[test]
    fn morph_leaves_liquid_alone_when_eroding_and_fills_it_when_dilating() {
        let mut w = FakeWorld::new();
        w.fill([-2, 60, -2], [2, 64, 2], "stone");
        w.fill([0, 64, 0], [0, 64, 0], "water");
        // The water block is "empty": erosion skips it, dilation (min 5) turns it into the stone round it.
        let eroded = morph(&w, [0, 64, 0], 1.0, 1, 1, 7, 0, 1_000_000).unwrap();
        assert!(!eroded.iter().any(|(p, _)| *p == [0, 64, 0]));
        let dilated = morph(&w, [0, 64, 0], 1.0, 7, 0, 5, 1, 1_000_000).unwrap();
        assert!(dilated.contains(&([0, 64, 0], w.id("stone"))));
    }

    #[test]
    fn morph_refuses_a_cube_over_the_limit() {
        let w = FakeWorld::new();
        assert!(morph(&w, [0, 64, 0], 500.0, 1, 1, 1, 1, 1_000_000).is_err());
    }

    #[test]
    fn snow_layers_the_first_ground_of_each_column() {
        let mut w = FakeWorld::new();
        w.fill([-3, 60, -3], [3, 60, 3], "stone");
        let (cols, lo, hi) = snow_columns([0, 60, 0], 2.0, 2);
        assert_eq!(cols.len(), 25);
        let out = snow(&w, &cols, lo, hi, false);
        assert_eq!(out.len(), 25);
        assert!(out.iter().all(|(p, s)| p[1] == 61 && w.describe(*s).name == "minecraft:snow"));
        // Underground (ground above the range): nothing.
        w.fill([0, 63, 0], [0, 63, 0], "stone");
        assert_eq!(snow(&w, &cols, lo, hi, false).len(), 24);
    }

    #[test]
    fn snow_stacks_and_turns_water_to_ice() {
        let mut w = FakeWorld::new();
        w.fill([0, 60, 0], [0, 60, 0], "stone");
        w.blocks.insert([0, 61, 0], w.id("snow[layers=3]"));
        w.fill([1, 60, 1], [1, 60, 1], "water");
        let cols = [(0, 0), (1, 1)];
        // Without `-s` the snow layer is no ground and the stone below has no air above it.
        let plain = snow(&w, &cols, 58, 62, false);
        assert_eq!(plain, vec![([1, 60, 1], w.id("ice"))]);
        let stacked = snow(&w, &cols, 58, 62, true);
        assert!(stacked.contains(&([0, 61, 0], w.id("snow[layers=4]"))));
        w.blocks.insert([0, 61, 0], w.id("snow[layers=7]"));
        assert!(snow(&w, &cols, 58, 62, true).contains(&([0, 61, 0], w.id("snow_block"))));
    }

    #[test]
    fn snow_columns_per_region_shape() {
        // Sphere: the box round it, floored (2.5 reaches three blocks down and two up).
        let (cols, lo, hi) = snow_columns([10, 70, 10], 2.5, 0);
        assert_eq!((lo, hi, cols.len()), (67, 72, 36));
        // Cuboid: (int) size each way.
        let (cols, lo, hi) = snow_columns([10, 70, 10], 2.9, 2);
        assert_eq!((lo, hi, cols.len()), (68, 72, 25));
        // Cylinder: round columns, half the radius up and down.
        let (cols, lo, hi) = snow_columns([10, 70, 10], 4.0, 1);
        assert_eq!((lo, hi), (68, 72));
        assert!(cols.contains(&(10, 10)) && !cols.contains(&(14, 14)));
    }

    #[test]
    fn heightmap_raises_and_erases_columns() {
        let mut w = FakeWorld::new();
        w.fill([-4, 60, -4], [4, 60, 4], "stone");
        let white = HeightImage { width: 1, height: 1, rgba: vec![[255, 255, 255, 255]] };
        // Intensity 3 on a white image: three blocks (the top one and two above) in every column of the 3 x 3.
        let out = heightmap(&w, None, &white, 3.0, false, false, false, [0, 60, 0], 1.0, 0.0);
        assert_eq!(out.len(), 27);
        assert!(out.contains(&([1, 62, -1], w.id("stone"))));
        // `-f` stops at the click's height: nothing above y 60.
        let out = heightmap(&w, None, &white, 3.0, false, true, false, [0, 60, 0], 1.0, 0.0);
        assert_eq!(out.len(), 9);
        // Erase takes the top block and two below it.
        let out = heightmap(&w, None, &white, 3.0, true, false, false, [0, 60, 0], 1.0, 0.0);
        assert_eq!(out.len(), 27);
        assert!(out.iter().all(|(_, s)| *s == 0));
        // A black image does nothing.
        let black = HeightImage { width: 1, height: 1, rgba: vec![[0, 0, 0, 255]] };
        assert!(heightmap(&w, None, &black, 5.0, false, false, false, [0, 60, 0], 2.0, 0.0).is_empty());
        // `-r`: a fraction of 0.5 grows one more block when the draw is above 0.5.
        let grey = HeightImage { width: 1, height: 1, rgba: vec![[128, 128, 128, 255]] };
        let low = heightmap(&w, None, &grey, 1.0, false, false, false, [0, 60, 0], 0.0, 0.0).len();
        let high = heightmap(&w, None, &grey, 1.0, false, false, true, [0, 60, 0], 0.0, 0.9).len();
        assert_eq!((low, high), (1, 2));
    }

    #[test]
    fn gravity_drops_floating_blocks_to_the_lowest_air() {
        let mut w = FakeWorld::new();
        w.fill([0, 57, 0], [0, 57, 0], "stone");
        w.fill([0, 63, 0], [0, 63, 0], "gold_block");
        let out = gravity(&w, [0, 62, 0], 0.0, 5.0);
        // The stone at the bottom of the range has nothing to fall through; the gold falls onto it.
        assert_eq!(out, vec![([0, 58, 0], w.id("gold_block")), ([0, 63, 0], 0)]);
        // A fractional radius steps from `pos - size` in whole blocks: 2.5 covers x-3 .. x+2.
        let mut w = FakeWorld::new();
        w.fill([-3, 62, 0], [-3, 62, 0], "gold_block");
        w.fill([3, 62, 0], [3, 62, 0], "gold_block");
        let out = gravity(&w, [0, 60, 0], 2.5, 5.0);
        assert!(out.contains(&([-3, 55, 0], w.id("gold_block"))), "{out:?}");
        assert!(out.iter().all(|(p, _)| p[0] != 3), "x+3 is outside: {out:?}");
    }

    #[test]
    fn the_butcher_cylinder_has_no_half_block() {
        let mob = |x: f64| EntityInfo { id: 1, kind: "minecraft:zombie".into(), pos: [x, 64.0, 0.5], living: true, named: false, tamed: false };
        let keep = in_butcher_cylinder(vec![mob(5.5), mob(6.5)], [0, 64, 0], 5.0);
        assert_eq!(keep.len(), 1, "a block 5 away is in, 6 is out");
    }
}

/// The brush commands end to end: what `/brush ...` says, what a click does.
#[cfg(test)]
mod command_tests {
    use super::*;
    use crate::horizon_worldedit_core::engine::{Actor, Engine, Env};
    use crate::horizon_worldedit_core::world::fake::FakeWorld;
    use crate::horizon_worldedit_core::Config;
    use std::path::PathBuf;

    const HINT: &str = "Run /brush unbind while holding the item to unbind it.";

    fn actor() -> Actor {
        Actor {
            uuid: 7,
            name: "builder".into(),
            permission: 2,
            dimension: "minecraft:overworld".into(),
            position: [0.5, 61.0, 0.5],
            yaw: 0.0,
            pitch: 90.0,
            game_mode: 1,
            held: Some("minecraft:blaze_rod".into()),
            offhand: None,
            granted: Default::default(),
            inventory: None,
        }
    }

    fn env(tag: &str) -> Env {
        let base = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
        let dir = base.join("we-tests").join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Env { schematics_dir: dir.clone(), config_file: dir.join("worldedit.toml"), data_version: 4440, now_ms: 0 }
    }

    /// For commands that touch no file.
    fn bare_env() -> Env {
        Env { schematics_dir: PathBuf::from("none"), config_file: PathBuf::from("none/worldedit.toml"), data_version: 4440, now_ms: 0 }
    }

    fn say(e: &mut Engine, w: &mut FakeWorld, a: &Actor, line: &str) -> String {
        let r = e.command(w, a, line, &bare_env());
        let text = r.text();
        assert!(r.lines.iter().all(|l| !l.error), "`{line}` failed: {text}");
        text
    }

    fn refused(e: &mut Engine, w: &mut FakeWorld, a: &Actor, line: &str) -> String {
        let r = e.command(w, a, line, &bare_env());
        assert!(r.lines.iter().any(|l| l.error), "`{line}` should fail: {}", r.text());
        r.text()
    }

    fn floor() -> FakeWorld {
        let mut w = FakeWorld::new();
        w.fill([-8, 60, -8], [8, 60, 8], "stone");
        w
    }

    #[test]
    fn snow_brush_equips_by_the_original_wording_and_covers_the_ground() {
        let (mut e, mut w, a) = (Engine::default(), floor(), actor());
        assert_eq!(say(&mut e, &mut w, &a, "/brush snow cuboid 2"), format!("Set brush to set snow.\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush snow cyl 3 -s"), format!("Set brush to set snow stacker.\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush snow sphere"), format!("Set brush to set snow.\n{HINT}"));
        assert_eq!(refused(&mut e, &mut w, &a, "/brush snow blob 2"), "Not a known region type: blob");
        // WorldEdit's converter knows cuboid, sphere, cyl and cylinder, not the one-letter forms.
        assert_eq!(refused(&mut e, &mut w, &a, "/brush snow s 2"), "Not a known region type: s");
        assert_eq!(refused(&mut e, &mut w, &a, "/brush snow cuboid 9"), "Maximum brush radius (in configuration): 5");
        say(&mut e, &mut w, &a, "/brush snow cuboid 2");
        assert!(e.brush_use(&mut w, &a, Some([0, 60, 0]), 0).unwrap().lines.is_empty());
        assert_eq!(w.count("snow[layers=1]"), 25);
        say(&mut e, &mut w, &a, "//undo");
        assert_eq!(w.count("snow[layers=1]"), 0);
        // The node.
        let mut b = actor();
        b.permission = 0;
        assert!(refused(&mut e, &mut w, &b, "/brush snow cuboid").contains("not permitted"));
    }

    #[test]
    fn morph_erode_and_dilate_equip_and_reshape() {
        let (mut e, mut w, a) = (Engine::default(), floor(), actor());
        assert_eq!(say(&mut e, &mut w, &a, "/brush morph"), format!("Morph brush shape equipped: 5.\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush morph 3 2 1 4 2"), format!("Morph brush shape equipped: 3.\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush erode 2"), format!("Morph brush shape equipped: 2.\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush dilate"), format!("Morph brush shape equipped: 5.\n{HINT}"));
        assert_eq!(refused(&mut e, &mut w, &a, "/brush erode 9"), "Maximum brush radius (in configuration): 5");
        assert!(refused(&mut e, &mut w, &a, "/brush morph 3 x").contains("x"));
        // erode = morph(2, 1, 5, 1): a lone block on the floor loses its top, sides and goes (open faces >= 2).
        w.fill([0, 61, 0], [0, 61, 0], "dirt");
        say(&mut e, &mut w, &a, "/brush erode 2");
        e.brush_use(&mut w, &a, Some([0, 61, 0]), 0).unwrap();
        assert_eq!(w.name_at([0, 61, 0]), "minecraft:air");
        // dilate = morph(5, 1, 2, 1): an air block touching two solid faces fills in.
        say(&mut e, &mut w, &a, "/brush dilate 1");
        e.brush_use(&mut w, &a, Some([0, 61, 0]), 1000).unwrap();
        assert_eq!(w.name_at([0, 61, 0]), "minecraft:air", "one solid face is not enough");
        // The node of all three is worldedit.brush.morph.
        let mut b = actor();
        b.permission = 0;
        b.granted.insert("worldedit.brush.morph".into());
        assert!(say(&mut e, &mut w, &b, "/brush erode 1").starts_with("Morph brush"));
        assert!(refused(&mut e, &mut w, &b, "/brush butcher").contains("not permitted"));
    }

    #[test]
    fn butcher_brush_cuts_its_flags_by_permission_and_kills_in_its_cylinder() {
        let mob = |id: u64, kind: &str, x: f64| EntityInfo { id, kind: kind.into(), pos: [x, 61.0, 0.5], living: true, named: false, tamed: false };
        let (mut e, mut w, a) = (Engine::default(), floor(), actor());
        assert_eq!(say(&mut e, &mut w, &a, "/brush butcher"), format!("Butcher brush equipped (5).\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush kill 3 -a"), format!("Butcher brush equipped (3).\n{HINT}"));
        // The brush limit, or the butcher limit for those who may butcher (-1 there: the brush limit stays).
        assert_eq!(refused(&mut e, &mut w, &a, "/brush butcher 6"), "Maximum allowed brush radius: 5.0");
        let mut cfg = Config::default();
        cfg.butcher_max_radius = 20;
        let mut big = Engine::new(cfg);
        assert_eq!(say(&mut big, &mut w, &a, "/brush butcher 20"), format!("Butcher brush equipped (20).\n{HINT}"));
        assert_eq!(refused(&mut big, &mut w, &a, "/brush butcher 21"), "Maximum allowed brush radius: 20.0");
        // Without -a the cow lives; with it the cow dies; the player is never touched.
        w.mobs = vec![mob(1, "minecraft:zombie", 1.5), mob(2, "minecraft:cow", 2.5), mob(3, "minecraft:player", 0.5), mob(4, "minecraft:zombie", 9.5)];
        say(&mut e, &mut w, &a, "/brush butcher 3");
        assert!(e.brush_use(&mut w, &a, Some([0, 60, 0]), 0).unwrap().lines.is_empty());
        assert_eq!(w.mobs.iter().map(|m| m.id).collect::<Vec<_>>(), vec![2, 3, 4]);
        say(&mut e, &mut w, &a, "/brush butcher 3 -a");
        e.brush_use(&mut w, &a, Some([0, 60, 0]), 1000).unwrap();
        assert_eq!(w.mobs.iter().map(|m| m.id).collect::<Vec<_>>(), vec![3, 4]);
        // A flag without its node is dropped when the brush is equipped; -f is cut the same way.
        let mut b = actor();
        b.permission = 0;
        b.granted.insert("worldedit.brush.butcher".into());
        b.granted.insert("worldedit.butcher.animals".into());
        w.mobs = vec![mob(1, "minecraft:cow", 0.5), EntityInfo { id: 2, kind: "minecraft:wolf".into(), pos: [0.5, 61.0, 0.5], living: true, named: false, tamed: true }, mob(5, "minecraft:villager", 0.5)];
        say(&mut e, &mut w, &b, "/brush butcher 3 -f");
        e.brush_use(&mut w, &b, Some([0, 60, 0]), 2000).unwrap();
        // Only the animals node is held: the cow goes; the tamed wolf is an animal and a pet (pets need their node), the villager is an NPC.
        assert_eq!(w.mobs.iter().map(|m| m.id).collect::<Vec<_>>(), vec![2, 5]);
    }

    #[test]
    fn heightmap_brush_reads_the_assets_folder() {
        let (mut e, mut w, a) = (Engine::default(), floor(), actor());
        let env = env("brush-heightmap");
        let assets = env.config_file.parent().unwrap().join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        let white = crate::horizon_worldedit_core::heightmap::tests::png_gray(1, 1, &[255]);
        std::fs::write(assets.join("hill.png"), white).unwrap();
        std::fs::write(assets.join("broken.png"), b"not a png").unwrap();
        let r = e.command(&mut w, &a, "/brush heightmap nope", &env);
        assert_eq!(r.text(), "Unknown heightmap brush: nope.");
        let r = e.command(&mut w, &a, "/brush heightmap broken", &env);
        assert_eq!(r.text(), "Failed to load asset");
        let r = e.command(&mut w, &a, "/brush heightmap hill 9", &env);
        assert_eq!(r.text(), "Maximum brush radius (in configuration): 5");
        let r = e.command(&mut w, &a, "/brush heightmap hill 1 2", &env);
        assert_eq!(r.text(), format!("Heightmap brush equipped (1).\n{HINT}"));
        e.brush_use(&mut w, &a, Some([0, 60, 0]), 0).unwrap();
        // Intensity 2 on a white image: two blocks of stone per column, the top one and one above, in the 3 x 3.
        assert_eq!(w.name_at([1, 61, -1]), "minecraft:stone");
        assert_eq!(w.name_at([1, 62, -1]), "minecraft:air");
        assert_eq!(w.name_at([2, 61, 0]), "minecraft:air");
        // -e erases down instead.
        e.command(&mut w, &a, "/brush heightmap hill.png 1 2 -e", &env);
        e.brush_use(&mut w, &a, Some([0, 61, 0]), 1000).unwrap();
        assert_eq!(w.name_at([0, 61, 0]), "minecraft:air");
        assert_eq!(w.name_at([0, 60, 0]), "minecraft:air");
        // The node.
        let mut b = actor();
        b.permission = 0;
        let r = e.command(&mut w, &b, "/brush heightmap hill", &env);
        assert!(r.text().contains("not permitted"));
    }

    #[test]
    fn gravity_height_flag_takes_a_value_and_the_default_is_the_vertical_height() {
        let (mut e, mut w, a) = (Engine::default(), floor(), actor());
        assert_eq!(say(&mut e, &mut w, &a, "/brush gravity 0 -h 2"), format!("Gravity brush equipped (0).\n{HINT}"));
        // A column of stone from the bottom, so nothing under the floor can fall; gold floats at 70.
        w.fill([0, -64, 0], [0, 60, 0], "stone");
        w.fill([0, 70, 0], [0, 70, 0], "gold_block");
        e.brush_use(&mut w, &a, Some([0, 61, 0]), 0).unwrap();
        assert_eq!(w.name_at([0, 70, 0]), "minecraft:gold_block", "ten blocks up is outside -h 2");
        // Without -h: default-vertical-height (256) up and down.
        assert_eq!(say(&mut e, &mut w, &a, "/brush grav 0"), format!("Gravity brush equipped (0).\n{HINT}"));
        e.brush_use(&mut w, &a, Some([0, 61, 0]), 1000).unwrap();
        assert_eq!(w.name_at([0, 70, 0]), "minecraft:air");
        assert_eq!(w.name_at([0, 61, 0]), "minecraft:gold_block");
        assert!(refused(&mut e, &mut w, &a, "/brush gravity 3 -h x").contains("x"));
    }

    #[test]
    fn set_raise_and_lower_say_what_the_operation_does() {
        let (mut e, mut w, a) = (Engine::default(), floor(), actor());
        assert_eq!(say(&mut e, &mut w, &a, "/brush set cuboid 1 dirt"), format!("Set brush to set replace blocks.\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush raise sphere"), format!("Set brush to deformation of y-=1.\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush lower cylinder 2"), format!("Set brush to deformation of y+=1.\n{HINT}"));
        assert_eq!(refused(&mut e, &mut w, &a, "/brush set c 2 dirt"), "Not a known region type: c");
        assert!(refused(&mut e, &mut w, &a, "/brush set cuboid 2 nonsense_block").contains("nonsense_block"));
        // `/material` belongs to the sphere and cylinder brushes: the operation keeps its own pattern.
        say(&mut e, &mut w, &a, "/brush set cuboid 1 dirt");
        e.brush_use(&mut w, &a, Some([0, 60, 0]), 0).unwrap();
        assert_eq!(w.count("dirt"), 27);
    }

    #[test]
    fn an_unlimited_brush_radius_limits_nothing() {
        let mut e = Engine::new(Config { max_brush_radius: -1.0, ..Config::default() });
        let (mut w, a) = (floor(), actor());
        assert_eq!(say(&mut e, &mut w, &a, "/brush sphere stone 9"), format!("Sphere brush shape equipped (9).\n{HINT}"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush snow sphere 40"), format!("Set brush to set snow.\n{HINT}"));
    }

    #[test]
    fn flags_belong_to_their_sub_command() {
        let (mut e, mut w, a) = (Engine::default(), floor(), actor());
        // -s is the snow brush's; the sphere brush does not know it.
        assert!(refused(&mut e, &mut w, &a, "/brush sphere stone 2 -s").contains("Usage"));
        assert_eq!(say(&mut e, &mut w, &a, "/brush sphere stone 2 -h"), format!("Sphere brush shape equipped (2).\n{HINT}"));
        // The clipboard brush's entity and biome flags are supported now: -e gets as far as the empty clipboard.
        assert_eq!(refused(&mut e, &mut w, &a, "/brush clipboard -e"), "Your clipboard is empty. Use //copy first.");
    }
}
