//! WorldEdit's utility commands that read the world around a point: `//fill`, `//fillr`, `//drain`,
//! `//fixwater`, `//fixlava`, `//snow`, `//thaw`, `//green`, `//extinguish`; and the session options
//! `//limit`, `//gmask`, `//toggleplace`, `//searchitem`. The algorithms follow `EditSession`
//! (`fillXZ`, `drainArea`, `fixLiquid`, `simulateSnow`, `thaw`, `green`) and its visitors.

use super::*;
use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::region::add;
use crate::horizon_worldedit_core::session::{SideState, SIDE_EFFECTS};
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
    crate::horizon_worldedit_core::pattern::is_solid_name(name)
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
        let may_disable = self.holds(actor, "worldedit.limit.unrestricted");
        let maximum = self.config.max_blocks_maximum;
        if !may_disable && maximum > -1 && limit > maximum {
            return Err(format!("Your maximum allowable limit is {maximum}."));
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

    /// `//trim [mask]`: the selection shrinks to the box round the blocks that match (non-air by default).
    pub(super) fn trim(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let selection = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
        let c = selection.clamp_y(world.min_y(), world.max_y()).ok_or("The selection is outside the build height")?;
        if c.volume() > SCAN_LIMIT {
            return Err(format!("The selection is {} blocks; one edit handles at most {SCAN_LIMIT}", c.volume()));
        }
        let text = args.rest(0);
        let mask = Mask::parse(world, if text.is_empty() { "#existing" } else { &text }, actor.held.as_deref())?;
        let mut found: Option<(Pos, Pos)> = None;
        for p in c.positions() {
            if world.get(p).is_some_and(|s| mask.hits(world, s)) {
                found = Some(match found {
                    None => (p, p),
                    Some((lo, hi)) => (std::array::from_fn(|a| lo[a].min(p[a])), std::array::from_fn(|a| hi[a].max(p[a]))),
                });
            }
        }
        let (lo, hi) = found.ok_or("No blocks matched the trim mask.")?;
        self.sessions.get(actor.uuid).set_selection(Cuboid::new(lo, hi), &actor.dimension);
        Ok(vec!["Region trimmed.".into()])
    }

    /// `/chunkinfo`: the chunk you stand in and the names of its files in the old save formats.
    pub(super) fn chunk_info(&mut self, actor: &Actor) -> R {
        let p = actor.block();
        let (x, z) = (p[0] >> 4, p[2] >> 4);
        let modulo = |n: i32| ((n % 64) + 64) % 64;
        Ok(vec![
            format!("Chunk: {x}, {z}"),
            format!("Old format: {}/{}/c.{}.{}.dat", base36(modulo(x)), base36(modulo(z)), base36(x), base36(z)),
            format!("McRegion: region/r.{}.{}.mca", x >> 5, z >> 5),
        ])
    }

    /// `/listchunks [-p page]`: the chunks the selection reaches.
    pub(super) fn list_chunks(&mut self, actor: &Actor, args: &Args) -> R {
        let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
        let page: usize = match args.value('p') {
            Some(p) => p.parse().map_err(|_| number_error(p))?,
            None => 1,
        };
        let mut chunks = Vec::new();
        for x in (c.min[0] >> 4)..=(c.max[0] >> 4) {
            for z in (c.min[2] >> 4)..=(c.max[2] >> 4) {
                chunks.push(format!("({x}, {z})"));
            }
        }
        let mut out = vec![format!("Listing chunks for: {}", actor.name)];
        out.extend(help::paged("Selected Chunks", &chunks, page)?);
        Ok(out)
    }

    /// `//toggleplace`: shapes and fills are made at position 1 instead of at your feet, and back.
    pub(super) fn toggle_place(&mut self, actor: &Actor) -> R {
        let s = self.sessions.get(actor.uuid);
        s.place_kind = if s.place_kind == PlaceKind::Pos1 { PlaceKind::Player } else { PlaceKind::Pos1 };
        s.place_offset = [0; 3];
        Ok(vec![place_info(s.place_kind, s.place_offset)])
    }

    /// `//placement <pos1|player|here|world|min|max> [multiplier] [offset]`: where shapes and pastes are made.
    pub(super) fn placement_type(&mut self, actor: &Actor, args: &Args) -> R {
        let kind = args.get(0).map(str::to_ascii_lowercase);
        let multiplier = args.int(1, 1)?;
        let offset = match args.get(2) {
            Some(w) => Self::offset_arg(actor, Some(w))?,
            None => [0; 3],
        };
        let mut offset = offset.map(|v| v.saturating_mul(multiplier));
        let kind = match kind.as_deref() {
            Some("world") => PlaceKind::World,
            Some("player") => PlaceKind::Player,
            Some("pos1") => PlaceKind::Pos1,
            Some("min") => PlaceKind::Min,
            Some("max") => PlaceKind::Max,
            // `here` is the world with the block the player stands in added to the offset.
            Some("here") => {
                let b = actor.block();
                offset = [offset[0] + b[0], offset[1] + b[1], offset[2] + b[2]];
                PlaceKind::World
            }
            _ => return Err(usage("placement")),
        };
        let s = self.sessions.get(actor.uuid);
        s.place_kind = kind;
        s.place_offset = offset;
        Ok(vec![place_info(kind, offset)])
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
        let page: usize = match args.value('p') {
            Some(p) => p.parse().map_err(|_| number_error(p))?,
            None => 1,
        };
        let wanted = search.replace(' ', "_");
        let blocks: HashSet<String> = world.block_names().into_iter().collect();
        let mut names = world.item_names();
        if names.is_empty() {
            // The game's item list is not offered: the blocks are what can be searched.
            if args.flag('i') {
                return Err("Items that are not blocks cannot be searched on this server.".into());
            }
            names = blocks.iter().cloned().collect();
        }
        names.sort();
        names.dedup();
        let hits: Vec<String> = names
            .into_iter()
            .filter(|id| id.contains(&wanted) && (!args.flag('b') || blocks.contains(id)) && (!args.flag('i') || !blocks.contains(id)))
            .map(|id| format!("{} ({id})", block_title(&id)))
            .collect();
        let mut out = vec!["(Please wait... searching items.)".to_owned()];
        if hits.is_empty() {
            out.push("No results found.".into());
            return Ok(out);
        }
        out.extend(help::paged(&format!("Search results for '{search}'"), &hits, page)?);
        Ok(out)
    }

    /// `//timeout [ms]`: the evaluation timeout (`calculation.timeout`, 100 ms). Recorded only: the calculator
    /// here has no timeout to apply it to.
    pub(super) fn timeout(&mut self, actor: &Actor, args: &Args) -> R {
        let limit = match args.get(0) {
            Some(w) => w.parse::<i64>().map_err(|_| number_error(w))?.max(-1),
            None => DEFAULT_TIMEOUT_MS,
        };
        self.sessions.get(actor.uuid).timeout_ms = Some(limit);
        let mut line = format!("Timeout time set to {limit}ms.");
        if limit != DEFAULT_TIMEOUT_MS {
            line.push_str(" (Use //timeout to go back to the default.)");
        }
        Ok(vec![line])
    }

    /// `//fast [on|off]`: WorldEdit's fast mode, a flag kept per player (see `Session::fast`).
    pub(super) fn fast(&mut self, actor: &Actor, args: &Args) -> R {
        let on = self.sessions.get(actor.uuid).fast;
        let wanted = match args.get(0) {
            Some(w) => Some(switch_arg(w).ok_or_else(|| usage("fast"))?),
            None => None,
        };
        if wanted == Some(on) {
            let text: &str = if on { "Fast mode already enabled." } else { "Fast mode already disabled." };
            return Err(text.into());
        }
        self.sessions.get(actor.uuid).fast = !on;
        let text: &str = if on {
            "Fast mode disabled."
        } else {
            "Fast mode enabled. Lighting in the affected chunks may be wrong and/or you may need to rejoin to see changes."
        };
        Ok(vec![text.to_owned()])
    }

    /// `//perf [sideeffect] [state] [-h]`: the side effects of a player's edits. The state is recorded; the
    /// writes here do not run side effects, so nothing else changes. Without a side effect, or with `-h`, the
    /// list of all of them is printed as text (WorldEdit prints an info box).
    pub(super) fn perf(&mut self, actor: &Actor, args: &Args) -> R {
        let show_all = args.flag('h');
        let mut out = Vec::new();
        if let Some(word) = args.get(0) {
            let (key, label, default) = side_effect(word).ok_or_else(|| usage("perf"))?;
            let state = match args.get(1) {
                Some(w) => Some(SideState::parse(w).ok_or_else(|| usage("perf"))?),
                None => None,
            };
            let current = self.sessions.get(actor.uuid).side_effects.get(key).copied().unwrap_or(default);
            match state {
                Some(new) if new == current => {
                    if !show_all {
                        return Err(format!("Side effect \"{label}\" is already {}", new.word()));
                    }
                }
                Some(new) => {
                    self.sessions.get(actor.uuid).side_effects.insert(key.to_owned(), new);
                    if !show_all {
                        out.push(format!("Side effect \"{label}\" set to {}", new.word()));
                    }
                }
                None => out.push(format!("Side effect \"{label}\" is set to {}", current.word())),
            }
        }
        if args.get(0).is_none() || show_all {
            let s = self.sessions.get(actor.uuid);
            for (key, label, default) in SIDE_EFFECTS {
                let state = s.side_effects.get(key).copied().unwrap_or(default);
                out.push(format!("{label}: {}", state.word()));
            }
        }
        Ok(out)
    }

    /// `//reorder [multi|fast|none]`: the reorder mode, recorded per player. Nothing reorders here.
    pub(super) fn reorder(&mut self, actor: &Actor, args: &Args) -> R {
        match args.get(0) {
            None => {
                let current = self.sessions.get(actor.uuid).reorder.unwrap_or("multi");
                Ok(vec![format!("The reorder mode is {current}")])
            }
            Some(w) => {
                let mode = match w.to_ascii_lowercase().as_str() {
                    "multi" => "multi",
                    "fast" => "fast",
                    "none" => "none",
                    _ => return Err(usage("reorder")),
                };
                self.sessions.get(actor.uuid).reorder = Some(mode);
                Ok(vec![format!("The reorder mode is now {mode}")])
            }
        }
    }

    /// `//drawsel [on|off]`: whether this player gets the selection outline (`server_side_cui`). Off puts the
    /// player in the same list as a `useServerCUI: false` session file.
    pub(super) fn drawsel(&mut self, actor: &Actor, args: &Args) -> R {
        if !self.config.server_side_cui {
            return Err("This functionality is disabled (see WorldEdit configuration).".into());
        }
        let drawing = !self.cui_off.contains(&actor.uuid);
        let wanted = match args.get(0) {
            Some(w) => Some(switch_arg(w).ok_or_else(|| usage("drawsel"))?),
            None => None,
        };
        if wanted == Some(drawing) {
            let text: &str = if drawing { "Server CUI already enabled." } else { "Server CUI already disabled." };
            return Err(text.into());
        }
        if drawing {
            self.cui_off.insert(actor.uuid);
            Ok(vec!["Server CUI disabled.".to_owned()])
        } else {
            self.cui_off.remove(&actor.uuid);
            Ok(vec!["Server CUI enabled. This only supports cuboid regions, with a maximum size of 48x48x48.".to_owned()])
        }
    }

    /// `//watchdog [active|inactive]`: WorldEdit's watchdog hook. Horizon has no watchdog to hook, so the answer
    /// is WorldEdit's own answer for such a platform.
    pub(super) fn watchdog(&mut self, args: &Args) -> R {
        if let Some(w) = args.get(0) {
            if !matches!(w.to_ascii_lowercase().as_str(), "active" | "inactive") {
                return Err(usage("watchdog"));
            }
        }
        Err("This platform has no watchdog hook.".into())
    }

    /// `//registry <registry> [-p page] [query]`: searches the block or item registry by id. A query without
    /// `*` or `?` matches anywhere in the id; `*` and `?` are the usual wildcards.
    pub(super) fn registry(&mut self, world: &mut dyn World, args: &Args) -> R {
        let name = args.get(0).ok_or_else(|| usage("registry"))?.to_ascii_lowercase();
        let page: usize = match args.value('p') {
            Some(p) => p.parse().map_err(|_| number_error(p))?,
            None => 1,
        };
        let mut query = args.words.iter().skip(1).copied().collect::<Vec<&str>>().join("_");
        if query.is_empty() {
            query = "*".to_owned();
        }
        if !query.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | ':' | '?' | '*' | '/')) {
            // WorldEdit prints its message with the placeholder unfilled; kept as it is.
            return Err("Invalid registry key: {0}".into());
        }
        let mut ids = match name.as_str() {
            "block" => world.block_names(),
            "item" => world.item_names(),
            _ => return Err(format!("Unknown registry: {name}")),
        };
        ids.sort();
        ids.dedup();
        let pattern: Vec<char> = if query.contains('*') || query.contains('?') { query.chars().collect() } else { format!("*{query}*").chars().collect() };
        let hits: Vec<String> = ids.into_iter().filter(|id| glob_match(&pattern, &id.chars().collect::<Vec<char>>())).collect();
        let title = if query == "*" { "Registry contents".to_owned() } else { format!("Search results for '{query}'") };
        let mut out = vec!["(Please wait... searching registry...)".to_owned()];
        if hits.is_empty() {
            out.push("No results found.".into());
            return Ok(out);
        }
        out.extend(help::paged(&title, &hits, page)?);
        Ok(out)
    }
}

/// `//timeout`'s default, `calculation.timeout`, in milliseconds.
const DEFAULT_TIMEOUT_MS: i64 = 100;

/// `on`/`true` and `off`/`false`, any case: the switch words of `//fast` and `//drawsel`.
fn switch_arg(word: &str) -> Option<bool> {
    match word.to_ascii_lowercase().as_str() {
        "on" | "true" => Some(true),
        "off" | "false" => Some(false),
        _ => None,
    }
}

/// The side effect a `//perf` argument names: its key, its printed name and its default.
fn side_effect(word: &str) -> Option<(&'static str, &'static str, SideState)> {
    let w = word.to_ascii_lowercase();
    SIDE_EFFECTS.iter().find(|e| e.0 == w.as_str()).copied()
}

/// `*` matches any run of characters and `?` one character, the rest matches itself.
pub(super) fn glob_match(pattern: &[char], text: &[char]) -> bool {
    let (p, t) = (pattern.first().copied(), text.first().copied());
    match (p, t) {
        (None, None) => true,
        (Some('*'), _) => glob_match(&pattern[1..], text) || (t.is_some() && glob_match(pattern, &text[1..])),
        (Some('?'), Some(_)) => glob_match(&pattern[1..], &text[1..]),
        (Some(a), Some(b)) if a == b => glob_match(&pattern[1..], &text[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    fn m(pattern: &str, text: &str) -> bool {
        let p: Vec<char> = pattern.chars().collect();
        let t: Vec<char> = text.chars().collect();
        glob_match(&p, &t)
    }

    #[test]
    fn globs_follow_the_registry_search() {
        assert!(m("*stone*", "minecraft:stone_bricks"));
        assert!(m("minecraft:st?ne", "minecraft:stone"));
        assert!(!m("minecraft:st?ne", "minecraft:stne"));
        assert!(m("*", ""));
        assert!(!m("a*", "b"));
        assert!(m("*_wall", "minecraft:oak_wall"));
    }
}

/// WorldEdit's `worldedit.toggleplace.*` lines.
fn place_info(kind: PlaceKind, offset: Pos) -> String {
    let (plain, from) = match kind {
        PlaceKind::World => ("Now placing at world origin.", ""),
        PlaceKind::Player => ("Now placing at the block you stand in.", "the block you stand in"),
        PlaceKind::Pos1 => ("Now placing at pos #1.", "pos #1"),
        PlaceKind::Min => ("Now placing at the minimum of the current selection.", "the minimum of the current selection"),
        PlaceKind::Max => ("Now placing at the maximum of the current selection.", "the maximum of the current selection"),
    };
    if offset == [0; 3] {
        plain.to_owned()
    } else if kind == PlaceKind::World {
        format!("Now placing at ({}, {}, {}).", offset[0], offset[1], offset[2])
    } else {
        format!("Now placing at an offset of ({}, {}, {}) from {from}.", offset[0], offset[1], offset[2])
    }
}

/// `Integer.toString(n, 36)`.
fn base36(n: i32) -> String {
    let mut v = i64::from(n).unsigned_abs();
    if v == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while v > 0 {
        digits.push(char::from_digit((v % 36) as u32, 36).unwrap_or('0'));
        v /= 36;
    }
    if n < 0 {
        digits.push('-');
    }
    digits.iter().rev().collect()
}
