//! WorldEdit's utility commands that read the world around a point: `//fill`, `//fillr`, `//drain`,
//! `//fixwater`, `//fixlava`, `//snow`, `//thaw`, `//green`, `//extinguish`; and the session options
//! `//limit`, `//gmask`, `//toggleplace`, `//searchitem`. The algorithms follow `EditSession`
//! (`fillXZ`, `drainArea`, `fixLiquid`, `simulateSnow`, `thaw`, `green`) and its visitors.

use super::*;
use crate::block::BlockState;
use crate::region::add;
use std::collections::{HashSet, VecDeque};

/// A breadth-first search's six steps, and the five `DownwardVisitor` and `NonRisingVisitor` take.
const SIX: [Pos; 6] = [[0, 1, 0], [0, -1, 0], [-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1]];
const NOT_UP: [Pos; 5] = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, -1, 0]];

/// WorldEdit's `BreadthFirstSearch`: the start positions are taken as they are, every other position
/// only when `visitable(from, to)` says so. The positions it applies its function to, in order.
fn search(starts: &[Pos], dirs: &[Pos], visitable: &dyn Fn(Pos, Pos) -> bool) -> Result<Vec<Pos>, String> {
    let mut seen: HashSet<Pos> = HashSet::new();
    let mut queue: VecDeque<Pos> = VecDeque::new();
    for &s in starts {
        if seen.insert(s) {
            queue.push_back(s);
        }
    }
    let mut out = Vec::new();
    while let Some(p) = queue.pop_front() {
        out.push(p);
        if out.len() as u64 > SCAN_LIMIT {
            return Err(format!("The area is more than {SCAN_LIMIT} blocks; one edit handles at most that many"));
        }
        for d in dirs {
            let to = add(p, *d);
            if seen.insert(to) && visitable(p, to) {
                queue.push_back(to);
            }
        }
    }
    Ok(out)
}

/// `EllipsoidRegion(center, radius).contains(p)`: WorldEdit widens the radius by one half.
fn in_sphere(center: Pos, radius: f64, p: Pos) -> bool {
    let r = radius + 0.5;
    let d = [f64::from(p[0] - center[0]), f64::from(p[1] - center[1]), f64::from(p[2] - center[2])];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) / (r * r) <= 1.0
}

fn name_at(world: &dyn World, p: Pos) -> Option<String> {
    world.get(p).map(|s| world.describe(s).name)
}

fn is_solid_name_block(name: &str) -> bool {
    crate::pattern::is_solid_name(name)
}

/// Whether a snow layer survives on top of this block (the game's rule needs the block's shape;
/// this is the full-block part of it).
fn holds_snow(name: &str) -> bool {
    is_solid_name_block(name) && !matches!(name, "minecraft:ice" | "minecraft:packed_ice" | "minecraft:blue_ice" | "minecraft:barrier") && !name.ends_with("_slab") && !name.ends_with("_stairs") && !name.ends_with("_fence")
}

impl Engine {
    fn number(args: &Args, at: usize, name: &str) -> Result<f64, String> {
        let w = args.get(at).ok_or_else(|| usage(name))?;
        w.parse::<f64>().map_err(|_| number_error(w))
    }

    /// `//fill <pattern> <radius> [depth]` and `//fillr <pattern> <radius> [depth]`.
    pub(super) fn fill_hole(&mut self, world: &mut dyn World, actor: &Actor, recursive: bool, args: &Args) -> R {
        let name = if recursive { "fillr" } else { "fill" };
        let pattern = self.need_pattern(world, actor, args, 0, name)?;
        let radius = Self::number(args, 1, name)?.max(1.0);
        self.check_radius(&[radius])?;
        let depth: i64 = match args.get(2) {
            Some(_) => i64::from(args.int(2, 1)?).max(1),
            None if recursive => i64::from(i32::MAX),
            None => 1,
        };
        let origin = self.placement(actor)?;
        let lowest = (i64::from(origin[1]) - depth + 1).max(i64::from(world.min_y())) as i32;
        let highest = world.max_y().min(origin[1]);
        let w: &dyn World = &*world;
        let open = |to: Pos| in_sphere(origin, radius, to) && (lowest..=highest).contains(&to[1]) && w.get(to).is_some_and(|s| w.is_air(s));
        let visitable = |from: Pos, to: Pos| (recursive || from[1] == origin[1] || to[1] < from[1]) && open(to);
        let found = search(&[origin], if recursive { &SIX } else { &NOT_UP }, &visitable)?;
        let salt = self.salt(actor);
        let ops = found.into_iter().map(|p| (p, pattern.pick(salt, p))).collect();
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been filled.{note}")])
    }

    /// `//drain <radius> [-w]`.
    pub(super) fn drain(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let radius = Self::number(args, 0, "drain")?.max(0.0);
        self.check_radius(&[radius])?;
        let waterlogged = args.flag('w');
        let origin = self.placement(actor)?;
        let w: &dyn World = &*world;
        let is_liquid = |p: Pos| name_at(w, p).is_some_and(|n| n == "minecraft:water" || n == "minecraft:lava");
        let is_logged = |p: Pos| w.get(p).is_some_and(|s| w.describe(s).prop("waterlogged") == Some("true"));
        let mask = |p: Pos| in_sphere(origin, radius, p) && (is_liquid(p) || (waterlogged && is_logged(p)));
        let starts: Vec<Pos> = (-1..=1).flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| [x, y, z]))).map(|d| add(origin, d)).filter(|&p| mask(p)).collect();
        let found = search(&starts, &SIX, &|_, to| mask(to))?;
        let air = world.resolve(&BlockState::air()).unwrap_or(0);
        let mut ops = Vec::with_capacity(found.len());
        for p in found {
            let mut to = air;
            if waterlogged {
                // `WaterloggedRemover`: a block that can be waterlogged stops being so, any other goes.
                let state = world.describe(world.get(p).unwrap_or(0));
                if state.properties.contains_key("waterlogged") {
                    let mut dry = state;
                    dry.properties.insert("waterlogged".into(), "false".into());
                    to = world.resolve(&dry).unwrap_or(air);
                }
            }
            ops.push((p, to));
        }
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been drained.{note}")])
    }

    /// `//fixwater <radius>` and `//fixlava <radius>`: the liquid becomes still source blocks, and
    /// spreads outward and downward through air within the radius.
    pub(super) fn fix_liquid(&mut self, world: &mut dyn World, actor: &Actor, lava: bool, args: &Args) -> R {
        let name = if lava { "fixlava" } else { "fixwater" };
        let radius = Self::number(args, 0, name)?.max(0.0);
        self.check_radius(&[radius])?;
        let fluid = if lava { "minecraft:lava" } else { "minecraft:water" };
        let origin = self.placement(actor)?;
        let source = world.resolve(&BlockState::new(fluid, &[])).ok_or_else(|| format!("Block name '{fluid}' was not recognized."))?;
        let w: &dyn World = &*world;
        let liquid = |p: Pos| name_at(w, p).is_some_and(|n| n == fluid);
        let open = |p: Pos| liquid(p) || w.get(p).is_some_and(|s| w.is_air(s));
        let highest = origin[1].min(w.max_y());
        let mask = |p: Pos| (w.min_y()..=highest).contains(&p[1]) && in_sphere(origin, radius, p) && open(p);
        let starts: Vec<Pos> = (-1..=1).flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| [x, y, z]))).map(|d| add(origin, d)).filter(|&p| liquid(p)).collect();
        let found = search(&starts, &NOT_UP, &|_, to| mask(to))?;
        let ops = found.into_iter().map(|p| (p, source)).collect();
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been fixed.{note}")])
    }

    /// The `[size] [height]` of `//snow`, `//thaw` and `//green`.
    fn size_and_height(&self, args: &Args, name: &str) -> Result<(f64, i32), String> {
        let size = match args.get(0) {
            Some(_) => Self::number(args, 0, name)?,
            None => 10.0,
        }
        .max(1.0);
        let height = args.int(1, self.config.default_vertical_height)?.max(1);
        self.check_radius(&[size])?;
        if (size * 2.0 + 1.0).powi(2) > SCAN_LIMIT as f64 {
            return Err(format!("The area is too big; one edit handles at most {SCAN_LIMIT} columns"));
        }
        Ok((size, height))
    }

    /// `//snow [size] [height] [-s]`: a snow layer on the ground in a cylinder round you, ice on still water.
    pub(super) fn snow(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let (size, height) = self.size_and_height(args, "snow")?;
        let stack = args.flag('s');
        let origin = self.placement(actor)?;
        let (lo, hi) = (world.min_y(), world.max_y());
        let (min_y, max_y) = ((origin[1].saturating_sub(height)).max(lo), (origin[1].saturating_add(height)).min(hi));
        let r = size + 0.5;
        let reach = r.ceil() as i32;
        let snow_layer = world.resolve(&BlockState::new("minecraft:snow", &[])).ok_or("Block name 'minecraft:snow' was not recognized.")?;
        let snow_block = world.resolve(&BlockState::new("minecraft:snow_block", &[]));
        let ice = world.resolve(&BlockState::new("minecraft:ice", &[]));
        let w: &dyn World = &*world;
        let is_ground = |p: Pos| -> bool {
            let Some(s) = w.get(p) else { return false };
            let state = w.describe(s);
            if state.is_air() || state.name == "minecraft:snow" {
                return false;
            }
            state.name == "minecraft:water" || is_solid_name_block(&state.name)
        };
        let mut ops: Vec<(Pos, StateId)> = Vec::new();
        for x in origin[0] - reach..=origin[0] + reach {
            for z in origin[2] - reach..=origin[2] + reach {
                let (dx, dz) = (f64::from(x - origin[0]), f64::from(z - origin[2]));
                if (dx * dx + dz * dz) / (r * r) > 1.0 {
                    continue;
                }
                // Underground: the block above the cylinder is already ground.
                if is_ground([x, max_y + 1, z]) {
                    continue;
                }
                let Some(y) = (min_y..=max_y).rev().find(|&y| is_ground([x, y, z])) else { continue };
                let here = w.describe(w.get([x, y, z]).unwrap_or(0));
                if here.name == "minecraft:water" {
                    if here.prop("level") == Some("0") {
                        if let Some(ice) = ice {
                            ops.push(([x, y, z], ice));
                        }
                    }
                    continue;
                }
                if y >= hi {
                    continue;
                }
                let above_pos = [x, y + 1, z];
                let Some(above) = w.get(above_pos).map(|s| w.describe(s)) else { continue };
                if stack && above.name == "minecraft:snow" {
                    let layers: u32 = above.prop("layers").and_then(|l| l.parse().ok()).unwrap_or(1);
                    if layers >= 7 {
                        if let Some(block) = snow_block {
                            ops.push((above_pos, block));
                        }
                    } else {
                        let mut more = above.clone();
                        more.properties.insert("layers".into(), (layers + 1).to_string());
                        if let Some(id) = w.resolve(&more) {
                            ops.push((above_pos, id));
                        }
                    }
                    continue;
                }
                if !above.is_air() || !holds_snow(&here.name) {
                    continue;
                }
                ops.push((above_pos, snow_layer));
            }
        }
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} surfaces have been covered.{note}")])
    }

    /// The columns of `//thaw` and `//green`: x and z within `size` of the origin (no half block
    /// added, as `thaw` and `green` do not use a region), and the y range up and down from it.
    fn columns(&self, world: &dyn World, origin: Pos, size: f64, height: i32) -> (Vec<(i32, i32)>, i32, i32) {
        let center = origin[1].clamp(world.min_y(), world.max_y());
        let min_y = (center.saturating_sub(height)).max(world.min_y());
        let max_y = (center.saturating_add(height)).min(world.max_y());
        let reach = size.ceil() as i32;
        let mut cols = Vec::new();
        for x in origin[0] - reach..=origin[0] + reach {
            for z in origin[2] - reach..=origin[2] + reach {
                let (dx, dz) = (f64::from(x - origin[0]), f64::from(z - origin[2]));
                if dx * dx + dz * dz <= size * size {
                    cols.push((x, z));
                }
            }
        }
        (cols, min_y, max_y)
    }

    /// `//thaw [size] [height]`: ice becomes water and snow goes, on the top block of each column.
    pub(super) fn thaw(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let (size, height) = self.size_and_height(args, "thaw")?;
        let origin = self.placement(actor)?;
        let (cols, min_y, max_y) = self.columns(world, origin, size, height);
        let air = world.resolve(&BlockState::air()).unwrap_or(0);
        let water = world.resolve(&BlockState::new("minecraft:water", &[])).ok_or("Block name 'minecraft:water' was not recognized.")?;
        let w: &dyn World = &*world;
        let mut ops = Vec::new();
        for (x, z) in cols {
            for y in ((min_y + 1)..=max_y).rev() {
                let Some(state) = w.get([x, y, z]).map(|s| w.describe(s)) else { break };
                match state.name.as_str() {
                    "minecraft:ice" => ops.push(([x, y, z], water)),
                    "minecraft:snow" => ops.push(([x, y, z], air)),
                    _ if state.is_air() => continue,
                    _ => {}
                }
                break;
            }
        }
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been thawed.{note}")])
    }

    /// `//green [size] [height] [-f]`: dirt (coarse dirt too with `-f`) under the open sky of the cylinder becomes grass.
    pub(super) fn green(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let (size, height) = self.size_and_height(args, "green")?;
        let origin = self.placement(actor)?;
        let coarse = args.flag('f');
        let (cols, min_y, max_y) = self.columns(world, origin, size, height);
        let grass = world.resolve(&BlockState::new("minecraft:grass_block", &[])).ok_or("Block name 'minecraft:grass_block' was not recognized.")?;
        let w: &dyn World = &*world;
        let mut ops = Vec::new();
        for (x, z) in cols {
            for y in ((min_y + 1)..=max_y).rev() {
                let Some(state) = w.get([x, y, z]).map(|s| w.describe(s)) else { break };
                let n = state.name.as_str();
                if n == "minecraft:dirt" || (coarse && n == "minecraft:coarse_dirt") {
                    ops.push(([x, y, z], grass));
                    break;
                } else if n == "minecraft:water" || n == "minecraft:lava" || is_solid_name_block(n) {
                    break;
                }
            }
        }
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been greened.{note}")])
    }

    /// `//extinguish [radius]`: the fire in a cube round you goes (40, or the largest radius, if none is given).
    pub(super) fn extinguish(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let default = if self.config.max_radius >= 0.0 { (self.config.max_radius as i32).min(40) } else { 40 };
        let size = match args.get(0) {
            Some(_) => args.int(0, default)?.max(1),
            None => default,
        };
        self.check_radius(&[f64::from(size)])?;
        let p = self.placement(actor)?;
        let r = size - 1;
        let cube = Cuboid::new([p[0] - r, p[1] - r, p[2] - r], [p[0] + r, p[1] + r, p[2] + r]).clamp_y(world.min_y(), world.max_y());
        let air = world.resolve(&BlockState::air()).unwrap_or(0);
        let mut ops = Vec::new();
        if let Some(cube) = cube {
            if cube.volume() > SCAN_LIMIT {
                return Err(format!("The area is {} blocks; one edit handles at most {SCAN_LIMIT}", cube.volume()));
            }
            ops = cube.positions().filter(|&q| name_at(world, q).is_some_and(|n| n == "minecraft:fire")).map(|q| (q, air)).collect();
        }
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} fires have been extinguished.{note}")])
    }

    /// `//limit [limit]`: this player's most blocks changed in one operation.
    pub(super) fn limit(&mut self, actor: &Actor, args: &Args) -> R {
        let default = self.config.max_blocks;
        let limit = match args.get(0) {
            Some(w) => w.parse::<i64>().map_err(|_| number_error(w))?.max(-1),
            None => default,
        };
        // Lifting the limit is for full operators, unless `node_levels` gives that node another level.
        let may_disable = actor.permission >= self.config.node_levels.get("worldedit.limit.unrestricted").copied().unwrap_or(4);
        // WorldEdit lets a `-1` (no limit) through its own maximum check; here it is above any maximum.
        if !may_disable && default > -1 && (limit > default || limit < 0) {
            return Err(format!("Your maximum allowable limit is {default}."));
        }
        self.sessions.get(actor.uuid).block_limit = Some(limit);
        let mut line = format!("Block change limit set to {limit}.");
        if limit != default {
            line.push_str(" (Use //limit to go back to the default.)");
        }
        Ok(vec![line])
    }

    /// `//gmask [mask]`: every edit changes only the blocks this accepts; none switches it off.
    pub(super) fn gmask(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let text = args.rest(0);
        if text.is_empty() {
            self.sessions.get(actor.uuid).global_mask = None;
            return Ok(vec!["Global mask disabled.".into()]);
        }
        Mask::parse(world, &text, actor.held.as_deref())?;
        self.sessions.get(actor.uuid).global_mask = Some(text);
        Ok(vec!["Global mask set.".into()])
    }

    /// `//toggleplace`: shapes and fills are made at position 1 instead of at your feet, and back.
    pub(super) fn toggle_place(&mut self, actor: &Actor) -> R {
        let s = self.sessions.get(actor.uuid);
        s.place_at_pos1 = !s.place_at_pos1;
        Ok(vec![if s.place_at_pos1 { "Now placing at pos #1." } else { "Now placing at the block you stand in." }.into()])
    }

    /// `//searchitem [-b] [-i] [-p page] <query>`. Only blocks can be searched: the game's item list
    /// is not part of what the editor sees.
    pub(super) fn search_item(&mut self, world: &mut dyn World, args: &Args) -> R {
        let search = args.rest(0);
        if search.len() <= 2 {
            return Err("Enter a longer search string (len > 2).".into());
        }
        if args.flag('b') && args.flag('i') {
            return Err("You cannot use both the 'b' and 'i' flags simultaneously.".into());
        }
        if args.flag('i') {
            return Err("Items that are not blocks cannot be searched on this server.".into());
        }
        let page: usize = match args.value('p') {
            Some(p) => p.parse().map_err(|_| number_error(p))?,
            None => 1,
        };
        let wanted = search.replace(' ', "_");
        let mut names = world.block_names();
        names.sort();
        let hits: Vec<String> = names.into_iter().filter(|id| id.contains(&wanted)).map(|id| format!("{} ({id})", block_title(&id))).collect();
        if hits.is_empty() {
            return Ok(vec!["No results found.".into()]);
        }
        Ok(help::paged(&format!("Search results for '{search}'"), &hits, page)?)
    }
}
