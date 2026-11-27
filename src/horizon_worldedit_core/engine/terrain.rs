//! The commands that need the game's terrain: `//regen`, `//forest`, `//flora`, `/delchunks`, the biome commands
//! (`/biomelist`, `/biomeinfo`, `//setbiome`, `//replacebiome`), `//world`, and the forest, feature and biome brushes.
//! They follow `RegionCommands`, `BiomeCommands`, `ChunkCommands`, `GeneralCommands.world`, `ForestGenerator`,
//! `FloraGenerator`, `TreeGenerator` and the paint/operation brushes of WorldEdit.

use super::*;
use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::clipboard::biome_cell;
use crate::horizon_worldedit_core::edit::{Change, Extras};
use crate::horizon_worldedit_core::pattern::is_solid_name;
use crate::horizon_worldedit_core::region::add;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

/// The most chunks one `//regen` takes (the engine's own cap for one call).
pub const REGEN_CHUNKS: usize = 64;
/// The most chunks one engine call to delete takes.
const DELETE_CHUNKS_PER_CALL: usize = 4096;
/// The most cells one biome box call takes (the engine's cap).
const FILL_CELLS: u64 = 16_384;
/// How far a tree reaches from where it is planted: sideways, and up.
const TREE_REACH: [i32; 2] = [8, 40];

/// The `//world` override of each player: the level their commands act on instead of the one they stand in.
static WORLD_OVERRIDE: Mutex<Option<HashMap<u128, String>>> = Mutex::new(None);

/// The level `player` set with `//world`, if any.
pub fn world_override(player: u128) -> Option<String> {
    WORLD_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&player).cloned())
}

fn set_world_override(player: u128, level: Option<String>) {
    let mut guard = WORLD_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    match level {
        Some(l) => {
            map.insert(player, l);
        }
        None => {
            map.remove(&player);
        }
    }
}

/// A small generator (splitmix64) for the random choices: a salt from the session gives each command its own run.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// A double in `[0, 1)`.
    pub(super) fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub(super) fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

// ---- biomes ----

/// `minecraft:old_growth_pine_taiga` as `Old Growth Pine Taiga`: the name the game shows for a biome.
pub(super) fn rich_name(id: &str) -> String {
    id.rsplit(':')
        .next()
        .unwrap_or(id)
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A name with its namespace: `plains` is `minecraft:plains`.
pub(super) fn namespaced(name: &str) -> String {
    let name = name.trim().to_ascii_lowercase();
    if name.contains(':') { name } else { format!("minecraft:{name}") }
}

/// The 4x4x4 cells (their corner blocks) that hold a block of `c`.
pub(super) fn cells_of(c: &Cuboid) -> impl Iterator<Item = Pos> {
    let (lo, hi) = (c.min, c.max);
    ((lo[0] >> 2)..=(hi[0] >> 2)).flat_map(move |x| ((lo[2] >> 2)..=(hi[2] >> 2)).flat_map(move |z| ((lo[1] >> 2)..=(hi[1] >> 2)).map(move |y| [x << 2, y << 2, z << 2])))
}

/// The biome `name` names, checked against the level's table when it gives one.
pub(super) fn biome_arg(world: &dyn World, name: &str) -> Result<String, String> {
    let id = namespaced(name);
    let known = world.biome_names();
    if !known.is_empty() && !known.iter().any(|b| *b == id) {
        return Err(format!("Biome name '{name}' was not recognized."));
    }
    Ok(id)
}

impl Engine {
    /// `/biomelist [-p page]`: every biome of the game.
    pub(super) fn biome_list(&mut self, world: &dyn World, args: &Args) -> R {
        let page: usize = match args.value('p') {
            Some(p) => p.parse().map_err(|_| number_error(p))?,
            None => 1,
        };
        let mut names = world.biome_names();
        names.sort();
        let lines: Vec<String> = names.iter().map(|id| format!("{id} ({})", rich_name(id))).collect();
        help::paged("Available Biomes", &lines, page)
    }

    /// `/biomeinfo [-t] [-p]`: the biomes of the block in sight, of your block, or of the selection.
    pub(super) fn biome_info(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        if actor.uuid == CONSOLE_UUID && (args.flag('p') || args.flag('t')) {
            return Err(if args.flag('t') { "Raytracing commands require a player!" } else { "Command sender must be present in the world to use the -p flag." }.into());
        }
        let (message, cells): (&str, Vec<Pos>) = if args.flag('t') {
            let Some(target) = ops::raycast(world, actor.eye(), ops::look_vector(actor.yaw, actor.pitch), 300.0) else {
                return Err("No block in sight!".into());
            };
            ("Biomes at line of sight point", vec![target])
        } else if args.flag('p') {
            ("Biomes at your position", vec![actor.block()])
        } else {
            let c = self.region(world, actor)?;
            ("Biomes in your selection", cells_of(&c).collect())
        };
        let biomes: BTreeSet<String> = cells.into_iter().filter_map(|p| world.biome_at(p)).collect();
        let names: Vec<String> = biomes.iter().map(|b| rich_name(b)).collect();
        Ok(vec![format!("{message}: {}", names.join(", "))])
    }

    /// `//setbiome <biome> [-p]` and `//replacebiome <mask> <biome> [-p]`: the biome of every cell of the selection (or of
    /// your block), where `mask` (when given) accepts the block.
    pub(super) fn set_biome_cmd(&mut self, world: &mut dyn World, actor: &Actor, args: &Args, replace: bool) -> R {
        let name = if replace { "replacebiome" } else { "setbiome" };
        let (mask_text, biome_word) = if replace { (args.get(0), args.get(1)) } else { (None, args.get(0)) };
        if replace && mask_text.is_none() {
            return Err(usage(name));
        }
        let biome = biome_arg(world, biome_word.ok_or_else(|| usage(name))?)?;
        let mask = match mask_text {
            Some(m) => Some(Mask::parse(world, m, actor.held.as_deref())?),
            None => None,
        };
        if args.flag('p') && actor.uuid == CONSOLE_UUID {
            return Err("Command sender must be present in the world to use the -p flag.".into());
        }
        let c = if args.flag('p') {
            let b = actor.block();
            Cuboid::new(b, b)
        } else {
            self.region(world, actor)?
        };
        let (affected, extras) = apply_biome(world, &c, &biome, mask.as_ref())?;
        self.remember_biomes(actor, &world_dimension(actor), extras);
        Ok(vec![format!("Biomes were changed for approximately {affected} blocks.")])
    }

    /// Keeps the cells a biome edit set for `//undo`.
    pub(super) fn remember_biomes(&mut self, actor: &Actor, dimension: &str, extras: Extras) {
        if extras.is_empty() {
            return;
        }
        let size = self.config.history_size;
        let change = Change { dimension: dimension.to_owned(), at_ms: self.now_ms, extras, ..Default::default() };
        self.sessions.get(actor.uuid).remember(change, size);
    }

    // ---- regeneration and chunks ----

    /// `//regen [-b] [-c] [seed]`: the selection generated again. Without a seed or `-c` the engine regenerates the
    /// selection's chunks from the world's seed, and the part outside the selection (in the chunks it only partly
    /// covers) is put back as it was; biomes are then always regenerated with the blocks. With a seed, or with `-c`
    /// (into the clipboard instead of the world), the level's generator is run on the side under that seed and only the
    /// selection is taken from the result; `-b` takes the biomes with the blocks there.
    pub(super) fn regen(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let seed = match args.get(0) {
            Some(w) => Some(w.parse::<i64>().map_err(|_| number_error(w))?),
            None => None,
        };
        let c = self.region(world, actor)?;
        let chunks = chunks_of(&c);
        if chunks.len() > REGEN_CHUNKS {
            return Err(format!("The selection reaches {} chunks; //regen takes at most {REGEN_CHUNKS} at once", chunks.len()));
        }
        if seed.is_some() || args.flag('c') {
            return self.regen_aside(world, actor, &c, seed, args.flag('b'), args.flag('c'));
        }
        if let Some(missing) = chunks.iter().find(|ch| world.get([ch[0] * 16, c.min[1], ch[1] * 16]).is_none()) {
            return Err(format!("The chunk ({}, {}) is not loaded; stand near it or load it first", missing[0], missing[1]));
        }
        // What stands in the region now, for //undo, and what stands round it in the chunks it covers only in part.
        let before: Vec<(Pos, StateId)> = c.positions().filter_map(|p| world.get(p).map(|s| (p, s))).collect();
        let entities_before: HashMap<Pos, Tag> = before.iter().filter(|&&(_, s)| !world.is_air(s)).filter_map(|&(p, _)| world.block_entity_at(p).map(|t| (p, t))).collect();
        let outside = outside_of(world, &c, &chunks);
        world.regenerate_chunks(&chunks).map_err(|e| format!("Unable to regenerate chunks: {e}"))?;
        // Put back what the selection does not own.
        let now: Vec<(Pos, StateId)> = outside.blocks.iter().filter(|&&(p, s)| world.get(p) != Some(s)).copied().collect();
        if !now.is_empty() {
            world.write(&now);
        }
        let restore: Vec<(Pos, Tag)> = outside.entities.iter().map(|(p, t)| (*p, t.clone())).collect();
        if !restore.is_empty() {
            world.set_block_entities(&restore);
        }
        let blocks: Vec<(Pos, StateId, StateId)> = before
            .iter()
            .filter_map(|&(p, old)| {
                let new = world.get(p)?;
                (new != old).then_some((p, old, new))
            })
            .collect();
        let entities_after: HashMap<Pos, Tag> = blocks.iter().filter(|&&(_, _, s)| !world.is_air(s)).filter_map(|&(p, _, _)| world.block_entity_at(p).map(|t| (p, t))).collect();
        let change = Change { dimension: actor.dimension.clone(), blocks, at_ms: self.now_ms, entities_before, entities_after, extras: Extras::default() };
        let size = self.config.history_size;
        self.sessions.get(actor.uuid).remember(change, size);
        Ok(vec!["Region regenerated.".into()])
    }

    /// `//regen` under a seed, or into the clipboard: the generator's blocks over the selection (the part of it the
    /// selection shape holds), written to the world with an undo entry, or kept as the clipboard around the placement.
    fn regen_aside(&mut self, world: &mut dyn World, actor: &Actor, c: &Cuboid, seed: Option<i64>, biomes: bool, to_clipboard: bool) -> R {
        let shape = self.shape_of(actor);
        let generated = world.generate_region(c.min, c.max, seed, biomes).map_err(|e| format!("Unable to regenerate chunks: {e}"))?;
        let inside = |p: Pos| shape.as_ref().is_none_or(|s| s.contains(p)) && (c.min[0]..=c.max[0]).contains(&p[0]) && (c.min[1]..=c.max[1]).contains(&p[1]) && (c.min[2]..=c.max[2]).contains(&p[2]);
        let blocks: Vec<(Pos, StateId)> = generated.blocks.iter().copied().filter(|&(p, _)| inside(p)).collect();
        if to_clipboard {
            let origin = self.placement(actor)?;
            let rel = |p: Pos| [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
            let mut clip = Clipboard::from_blocks(blocks.iter().map(|&(p, s)| (rel(p), s)).collect(), "the regenerated region".into());
            clip.bounds = c.shifted([-origin[0], -origin[1], -origin[2]]);
            clip.origin = Some((actor.dimension.clone(), origin));
            if biomes {
                for &(p, _) in &blocks {
                    if let Some(b) = generated.biome_at(p) {
                        clip.biomes.entry(biome_cell(rel(p))).or_insert_with(|| b.clone());
                    }
                }
            }
            self.sessions.get(actor.uuid).clipboard = Some(clip);
            return Ok(vec!["Region regenerated.".into()]);
        }
        if let Some(missing) = chunks_of(c).iter().find(|ch| world.get([ch[0] * 16, c.min[1], ch[1] * 16]).is_none()) {
            return Err(format!("The chunk ({}, {}) is not loaded; stand near it or load it first", missing[0], missing[1]));
        }
        let before: Vec<(Pos, StateId)> = blocks.iter().filter_map(|&(p, _)| world.get(p).map(|s| (p, s))).collect();
        let entities_before: HashMap<Pos, Tag> = before.iter().filter(|&&(_, s)| !world.is_air(s)).filter_map(|&(p, _)| world.block_entity_at(p).map(|t| (p, t))).collect();
        world.write(&blocks);
        let mut extras = Extras::default();
        if biomes {
            let mut cells: Vec<(&Pos, &String)> = generated.biomes.iter().collect();
            cells.sort();
            for (&cell, biome) in cells {
                let reaches = (0..3).all(|a| cell[a] + 3 >= c.min[a] && cell[a] <= c.max[a]);
                if !reaches {
                    continue;
                }
                let was = world.biome_at(cell);
                if was.as_deref() != Some(biome.as_str()) && world.set_biome(cell, biome) {
                    extras.biomes.push((cell, was.unwrap_or_else(|| "minecraft:plains".to_owned()), biome.clone()));
                }
            }
        }
        let changed: Vec<(Pos, StateId, StateId)> = before
            .iter()
            .filter_map(|&(p, old)| {
                let new = world.get(p)?;
                (new != old).then_some((p, old, new))
            })
            .collect();
        let entities_after: HashMap<Pos, Tag> = changed.iter().filter(|&&(_, _, s)| !world.is_air(s)).filter_map(|&(p, _, _)| world.block_entity_at(p).map(|t| (p, t))).collect();
        let change = Change { dimension: actor.dimension.clone(), blocks: changed, at_ms: self.now_ms, entities_before, entities_after, extras };
        let size = self.config.history_size;
        self.sessions.get(actor.uuid).remember(change, size);
        Ok(vec!["Region regenerated.".into()])
    }

    /// `/delchunks [-o <time>]`: the chunks the selection reaches erased from the saved world, after the region files that
    /// hold them are copied to the level's `backups` folder. With `-o` only chunks last saved before that time go: a date
    /// (`2026-01-01`, `2026-01-01T12:30`, UTC) or an age (`30d`, `1y2w`, `12h30m`). Chunks that are loaded stay (the game
    /// cannot erase what it holds); the answer says how many.
    pub(super) fn del_chunks(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
        let mut chunks = chunks_of(&c);
        let mut out = Vec::new();
        if let Some(text) = args.value('o') {
            let before = parse_cutoff(text, self.now_ms / 1000)?;
            let all = chunks.len();
            // A chunk the save does not hold has nothing to age: it is left out.
            chunks.retain(|&ch| world.saved_chunk_modified(ch).is_some_and(|t| t < before));
            if chunks.len() < all {
                out.push(format!("{} chunk(s) of the selection are newer than that or not saved and were kept.", all - chunks.len()));
            }
        }
        let (mut erased, mut asked) = (0u32, 0usize);
        for part in chunks.chunks(DELETE_CHUNKS_PER_CALL) {
            erased += world.delete_chunks_with_backup(part).map_err(|e| format!("Failed to erase the chunks: {e}"))?;
            asked += part.len();
        }
        out.insert(0, format!("{erased} chunk(s) have been erased from the saved world; the region files were copied to the level's backups folder first."));
        if (erased as usize) < asked {
            out.push(format!("{} chunk(s) are loaded or held and were left alone; run /delchunks again once nobody is near them.", asked - erased as usize));
        }
        Ok(out)
    }

    // ---- //world ----

    /// `//world [level]`: the level your commands act on; no argument removes the override.
    pub(super) fn world_cmd(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        let Some(word) = args.get(0) else {
            set_world_override(actor.uuid, None);
            return Ok(vec!["Removed world override.".into()]);
        };
        let id = namespaced(word);
        let known = world.level_names();
        let id = if known.is_empty() || known.contains(&id) {
            id
        } else if let Some(k) = known.iter().find(|k| k.rsplit(':').next() == Some(word.to_ascii_lowercase().as_str())) {
            k.clone()
        } else {
            return Err(format!("World '{word}' was not recognized. Levels: {}", known.join(", ")));
        };
        set_world_override(actor.uuid, Some(id.clone()));
        Ok(vec![format!("Set the world override to {id}. (Use //world to go back to default)")])
    }

    // ---- trees and flora ----

    /// `//forest [type] [density]`: trees on the ground of the selection, in a share of its columns.
    pub(super) fn forest(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let tree = tree_lookup(args.get(0).unwrap_or("tree")).ok_or_else(|| format!("Tree type '{}' is unknown.", args.get(0).unwrap_or("tree")))?;
        let density = density_arg(args.get(1), 5.0)?;
        let c = self.region(world, actor)?;
        let mut rng = Rng(self.salt(actor) ^ (self.now_ms as u64));
        let columns: Vec<(i32, i32)> = (c.min[0]..=c.max[0]).flat_map(|x| (c.min[2]..=c.max[2]).map(move |z| (x, z))).collect();
        let planted = self.plant_with_undo(world, actor, &c, |world| plant_forest(world, &columns, c.min[1], c.max[1], density, tree, &mut rng))?;
        Ok(vec![format!("{planted} trees created.")])
    }

    /// `//flora [density]`: grass and flowers on grass, dead bushes and cacti on sand, in a share of the selection's columns.
    pub(super) fn flora(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let density = density_arg(args.get(0), 5.0)?;
        let c = self.region(world, actor)?;
        let mut rng = Rng(self.salt(actor) ^ (self.now_ms as u64));
        let columns: Vec<(i32, i32)> = (c.min[0]..=c.max[0]).flat_map(|x| (c.min[2]..=c.max[2]).map(move |z| (x, z))).collect();
        let (ops, affected) = flora_ops(world, &columns, c.min[1], c.max[1], density, &mut rng)?;
        let (_, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{affected} flora created.{note}")])
    }

    /// Runs `place` over `area` (the blocks it may touch, trees reach past it) and remembers what changed for `//undo`.
    pub(super) fn plant_with_undo(&mut self, world: &mut dyn World, actor: &Actor, area: &Cuboid, place: impl FnOnce(&mut dyn World) -> usize) -> Result<usize, String> {
        let reach = Cuboid::new([area.min[0] - TREE_REACH[0], area.min[1], area.min[2] - TREE_REACH[0]], [area.max[0] + TREE_REACH[0], (area.max[1] + TREE_REACH[1]).min(world.max_y()), area.max[2] + TREE_REACH[0]]);
        if reach.volume() > SCAN_LIMIT {
            return Err(format!("The area is {} blocks with the room trees need; one edit handles at most {SCAN_LIMIT}", reach.volume()));
        }
        let before: Vec<(Pos, StateId)> = reach.positions().filter_map(|p| world.get(p).map(|s| (p, s))).collect();
        let planted = place(world);
        let blocks: Vec<(Pos, StateId, StateId)> = before
            .iter()
            .filter_map(|&(p, old)| {
                let new = world.get(p)?;
                (new != old).then_some((p, old, new))
            })
            .collect();
        let entities_after: HashMap<Pos, Tag> = blocks.iter().filter(|&&(_, _, s)| !world.is_air(s)).filter_map(|&(p, _, _)| world.block_entity_at(p).map(|t| (p, t))).collect();
        let change = Change { dimension: actor.dimension.clone(), blocks, at_ms: self.now_ms, entities_after, ..Default::default() };
        let size = self.config.history_size;
        self.sessions.get(actor.uuid).remember(change, size);
        Ok(planted)
    }

    // ---- brushes ----

    /// `/brush forest|feature|biome <shape> [radius] [density] <type> [-c]`: the brush the item will carry.
    pub(super) fn terrain_brush_shape(&mut self, world: &dyn World, kind: &str, args: &Args) -> Result<(BrushShape, String), String> {
        let words = &args.words;
        let usage_line = || usage(&format!("brush {kind}"));
        let shape = Self::region_shape(words.get(1).ok_or_else(usage_line)?)?;
        // `<shape> [radius] [density] <type>`: the last word is the type, the ones between are optional.
        let last = words.len() - 1;
        if last < 2 {
            return Err(usage_line());
        }
        let ty = words[last];
        let optional = &words[2..last];
        let number = |w: &str| w.parse::<f64>().map_err(|_| number_error(w));
        let radius = match optional.first() {
            Some(w) => number(w)?,
            None => 5.0,
        };
        self.check_brush_radius(radius)?;
        let (id, density, message) = match kind {
            "forest" => {
                let density = match optional.get(1) {
                    Some(w) => number(w)?,
                    None => 20.0,
                };
                tree_lookup(ty).ok_or_else(|| format!("Tree type '{ty}' is unknown."))?;
                (ty.to_ascii_lowercase(), density, format!("Set brush to scatter tree of type {}.", ty.to_ascii_uppercase()))
            }
            "feature" => {
                let density = match optional.get(1) {
                    Some(w) => number(w)?,
                    None => 5.0,
                };
                let id = namespaced(ty);
                (id.clone(), density, format!("Set brush to set feature of type {id}."))
            }
            _ => {
                let id = biome_arg(world, ty)?;
                (id.clone(), 100.0, format!("Set brush to set biome of type {id}."))
            }
        };
        if !(0.0..=100.0).contains(&density) {
            return Err("Density must be in [0, 100]".into());
        }
        let kind = match kind {
            "forest" => 0,
            "feature" => 1,
            _ => 2,
        };
        Ok((BrushShape::Terrain { kind, shape, radius, density, arg: id, column: args.flag('c') }, message))
    }

    /// A click with a forest, feature or biome brush at `target`.
    pub(super) fn terrain_brush_use(&mut self, world: &mut dyn World, actor: &Actor, shape: &BrushShape, target: Pos) -> Reply {
        let BrushShape::Terrain { kind, shape, radius, density, arg, column } = shape else { return Reply::default() };
        let (lo, hi) = (world.min_y(), world.max_y());
        let cells: Vec<Pos> = {
            let raw = match shape {
                0 => shapes::sphere(target, [*radius; 3], false),
                1 => shapes::cylinder(target, [*radius; 2], 1, false),
                _ => {
                    let r = *radius as i32;
                    let mut v = Vec::new();
                    for x in -r..=r {
                        for y in -r..=r {
                            for z in -r..=r {
                                v.push(add(target, [x, y, z]));
                            }
                        }
                    }
                    v
                }
            };
            raw.into_iter().filter(|p| (lo..=hi).contains(&p[1])).collect()
        };
        let mut rng = Rng(self.salt(actor) ^ (self.now_ms as u64));
        let reply = match kind {
            0 => {
                // The paint brush: a share of the shape's columns gets a tree on its ground.
                let columns: Vec<(i32, i32)> = cells.iter().map(|p| (p[0], p[2])).collect::<BTreeSet<_>>().into_iter().collect();
                let (y0, y1) = (cells.iter().map(|p| p[1]).min().unwrap_or(target[1]), cells.iter().map(|p| p[1]).max().unwrap_or(target[1]));
                let Some(tree) = tree_lookup(arg) else { return Reply::err(format!("Tree type '{arg}' is unknown.")) };
                let area = Cuboid::new([target[0] - *radius as i32, y0, target[2] - *radius as i32], [target[0] + *radius as i32, y1, target[2] + *radius as i32]);
                self.plant_with_undo(world, actor, &area, |world| plant_forest(world, &columns, y0, y1, density / 100.0, tree, &mut rng)).map(|_| ())
            }
            1 => {
                let area = Cuboid::new(sub(target, [*radius as i32; 3]), add(target, [*radius as i32; 3]));
                self.plant_with_undo(world, actor, &area, |world| {
                    let mut placed = 0;
                    for p in &cells {
                        if rng.unit() <= density / 100.0 && world.generate_tree(*p, arg) {
                            placed += 1;
                        }
                    }
                    placed
                })
                .map(|_| ())
            }
            _ => {
                let cells: Vec<Pos> = if *column { cells.iter().map(|p| [p[0], lo, p[2]]).collect::<BTreeSet<_>>().into_iter().flat_map(|p| (lo..=hi).map(move |y| [p[0], y, p[2]])).collect() } else { cells };
                let mut extras = Extras::default();
                for cell in cells.iter().map(|p| [p[0] >> 2, p[1] >> 2, p[2] >> 2]).collect::<BTreeSet<_>>() {
                    let p = [cell[0] << 2, cell[1] << 2, cell[2] << 2];
                    let before = world.biome_at(p);
                    if world.set_biome(p, arg) {
                        extras.biomes.push((p, before.unwrap_or_else(|| "minecraft:plains".to_owned()), arg.clone()));
                    }
                }
                self.remember_biomes(actor, &actor.dimension, extras);
                Ok(())
            }
        };
        match reply {
            Ok(()) => Reply::default(),
            Err(e) => Reply::err(e),
        }
    }
}

/// `density` percent as a share, `[0, 100]`.
pub(super) fn density_arg(word: Option<&str>, default: f64) -> Result<f64, String> {
    let d = match word {
        Some(w) => w.parse::<f64>().map_err(|_| number_error(w))?,
        None => default,
    };
    if !(0.0..=100.0).contains(&d) {
        return Err("Density must be in [0, 100]".into());
    }
    Ok(d / 100.0)
}

pub(super) fn world_dimension(actor: &Actor) -> String {
    actor.dimension.clone()
}

/// The chunks (x, z) a cuboid reaches.
/// `-o`'s time as seconds since the Unix epoch: a UTC date or date and time, or an age back from `now` (`1y2w3d4h5m6s`).
fn parse_cutoff(text: &str, now: i64) -> Result<i64, String> {
    let bad = || format!("'{text}' is not a time: use a date like 2026-01-01 or an age like 30d");
    let t = text.trim();
    if t.contains('-') {
        let (date, clock) = match t.split_once(['T', ' ']) {
            Some((d, c)) => (d, Some(c)),
            None => (t, None),
        };
        let mut d = date.split('-').map(|v| v.parse::<i64>());
        let (Some(Ok(y)), Some(Ok(m)), Some(Ok(day)), None) = (d.next(), d.next(), d.next(), d.next()) else { return Err(bad()) };
        if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
            return Err(bad());
        }
        let mut secs = 0;
        if let Some(clock) = clock {
            let mut p = clock.trim_end_matches('Z').split(':').map(|v| v.parse::<i64>());
            let (Some(Ok(h)), Some(Ok(mi))) = (p.next(), p.next()) else { return Err(bad()) };
            let s = match p.next() {
                Some(Ok(s)) => s,
                Some(Err(_)) => return Err(bad()),
                None => 0,
            };
            if !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..60).contains(&s) {
                return Err(bad());
            }
            secs = h * 3600 + mi * 60 + s;
        }
        // Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's civil-from-days, inverted).
        let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * m + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        return Ok((era * 146_097 + doe - 719_468) * 86_400 + secs);
    }
    let mut total = 0i64;
    let mut number = String::new();
    for ch in t.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        let n: i64 = number.parse().map_err(|_| bad())?;
        number.clear();
        total += n * match ch.to_ascii_lowercase() {
            'y' => 365 * 86_400,
            'w' => 7 * 86_400,
            'd' => 86_400,
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => return Err(bad()),
        };
    }
    if !number.is_empty() || total == 0 {
        return Err(bad());
    }
    Ok(now - total)
}

fn chunks_of(c: &Cuboid) -> Vec<[i32; 2]> {
    ((c.min[0] >> 4)..=(c.max[0] >> 4)).flat_map(|x| ((c.min[2] >> 4)..=(c.max[2] >> 4)).map(move |z| [x, z])).collect()
}

/// What stands in `chunks` outside `c`: blocks, and the data of the block entities among them.
struct Outside {
    blocks: Vec<(Pos, StateId)>,
    entities: HashMap<Pos, Tag>,
}

/// The blocks of `chunks` that `c` does not own, read before a regeneration: none for a chunk it covers whole.
fn outside_of(world: &dyn World, c: &Cuboid, chunks: &[[i32; 2]]) -> Outside {
    let mut out = Outside { blocks: Vec::new(), entities: HashMap::new() };
    let (lo, hi) = (world.min_y(), world.max_y());
    for &[cx, cz] in chunks {
        let whole = c.min[0] <= cx * 16 && c.max[0] >= cx * 16 + 15 && c.min[2] <= cz * 16 && c.max[2] >= cz * 16 + 15 && c.min[1] <= lo && c.max[1] >= hi;
        if whole {
            continue;
        }
        for x in cx * 16..cx * 16 + 16 {
            for z in cz * 16..cz * 16 + 16 {
                for y in lo..=hi {
                    let p = [x, y, z];
                    let inside = (c.min[0]..=c.max[0]).contains(&x) && (c.min[1]..=c.max[1]).contains(&y) && (c.min[2]..=c.max[2]).contains(&z);
                    if inside {
                        continue;
                    }
                    if let Some(s) = world.get(p) {
                        out.blocks.push((p, s));
                        if !world.is_air(s) {
                            if let Some(t) = world.block_entity_at(p) {
                                out.entities.insert(p, t);
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Sets `biome` over `c` (only where `mask` accepts the block, when given), cell by cell; the blocks affected and what
/// `//undo` needs. A box without a mask goes to the game in as few calls as the cell cap allows.
pub(super) fn apply_biome(world: &mut dyn World, c: &Cuboid, biome: &str, mask: Option<&Mask>) -> Result<(u64, Extras), String> {
    let cells: Vec<Pos> = cells_of(c).collect();
    let mut extras = Extras::default();
    if mask.is_none() {
        for &p in &cells {
            let before = world.biome_at(p);
            if before.as_deref() != Some(biome) {
                extras.biomes.push((p, before.unwrap_or_else(|| "minecraft:plains".to_owned()), biome.to_owned()));
            }
        }
        // A tall box in slabs of whole cell rows, each at most the engine's cell cap.
        let across = ((c.max[0] >> 2) - (c.min[0] >> 2) + 1) as u64 * ((c.max[2] >> 2) - (c.min[2] >> 2) + 1) as u64;
        if across > FILL_CELLS {
            return Err(format!("The selection is {across} biome cells wide; one edit handles at most {FILL_CELLS} per layer"));
        }
        let rows = (FILL_CELLS / across).max(1) as i32;
        let mut row = c.min[1] >> 2;
        while row <= c.max[1] >> 2 {
            let last = (row + rows - 1).min(c.max[1] >> 2);
            let (y0, y1) = ((row << 2).max(c.min[1]), (((last + 1) << 2) - 1).min(c.max[1]));
            world.fill_biome([c.min[0], y0, c.min[2]], [c.max[0], y1, c.max[2]], biome)?;
            row = last + 1;
        }
        return Ok((c.volume(), extras));
    }
    let mask = mask.expect("checked");
    let mut affected = 0u64;
    let mut done: HashSet<Pos> = HashSet::new();
    for p in c.positions() {
        let Some(s) = world.get(p) else { continue };
        if !mask.hits_at(&*world, p, s) {
            continue;
        }
        affected += 1;
        let cell = [p[0] >> 2 << 2, p[1] >> 2 << 2, p[2] >> 2 << 2];
        if done.insert(cell) {
            let before = world.biome_at(cell);
            if world.set_biome(cell, biome) {
                extras.biomes.push((cell, before.unwrap_or_else(|| "minecraft:plains".to_owned()), biome.to_owned()));
            }
        }
    }
    Ok((affected, extras))
}

// ---- trees ----

#[derive(Clone, Copy, Debug)]
pub(super) enum Tree {
    /// A configured feature of the game.
    Feature(&'static str),
    /// WorldEdit's own pine, made of logs and leaves.
    Pine,
    /// A chorus plant, which grows from the end stone, one block lower.
    Chorus,
    /// One of these kinds at random (their first names).
    Pick(&'static [&'static str]),
    /// Any kind at random.
    Any,
}

/// WorldEdit's `TreeType`s: every name it answers to, and what it plants (`Bukkit TreeType` to the configured feature).
const TREES: &[(&[&str], Tree)] = &[
    (&["oak", "tree", "regular"], Tree::Feature("minecraft:oak")),
    (&["largeoak", "bigoak", "big", "bigtree"], Tree::Feature("minecraft:fancy_oak")),
    (&["spruce", "redwood", "sequoia", "sequoioideae"], Tree::Feature("minecraft:spruce")),
    (&["tallspruce", "bigspruce", "tallredwood", "tallsequoia", "tallsequoioideae"], Tree::Feature("minecraft:pine")),
    (&["largespruce", "megaredwood"], Tree::Feature("minecraft:mega_spruce")),
    (&["randspruce", "randredwood", "randomredwood", "anyredwood"], Tree::Pick(&["spruce", "tallspruce", "largespruce"])),
    (&["birch", "white", "whitebark"], Tree::Feature("minecraft:birch")),
    (&["tallbirch"], Tree::Feature("minecraft:super_birch_bees_0002")),
    (&["randbirch", "randombirch"], Tree::Pick(&["birch", "tallbirch"])),
    (&["jungle"], Tree::Feature("minecraft:mega_jungle_tree")),
    (&["smalljungle", "shortjungle"], Tree::Feature("minecraft:jungle_tree")),
    (&["randjungle", "randomjungle"], Tree::Pick(&["jungle", "smalljungle"])),
    (&["junglebush", "jungleshrub"], Tree::Feature("minecraft:jungle_bush")),
    (&["redmushroom", "redgiantmushroom"], Tree::Feature("minecraft:huge_red_mushroom")),
    (&["brownmushroom", "browngiantmushroom"], Tree::Feature("minecraft:huge_brown_mushroom")),
    (&["crimsonfungus", "rednethermushroom"], Tree::Feature("minecraft:crimson_fungus")),
    (&["warpedfungus", "greennethermushroom"], Tree::Feature("minecraft:warped_fungus")),
    (&["randmushroom", "randommushroom"], Tree::Pick(&["redmushroom", "brownmushroom", "crimsonfungus", "warpedfungus"])),
    (&["swamp", "swamptree"], Tree::Feature("minecraft:swamp_oak")),
    (&["acacia"], Tree::Feature("minecraft:acacia")),
    (&["darkoak"], Tree::Feature("minecraft:dark_oak")),
    (&["pine"], Tree::Pine),
    (&["chorusplant"], Tree::Chorus),
    (&["mangrove"], Tree::Feature("minecraft:mangrove")),
    (&["tall_mangrove"], Tree::Feature("minecraft:tall_mangrove")),
    (&["cherry"], Tree::Feature("minecraft:cherry")),
    (&["pale_oak"], Tree::Feature("minecraft:pale_oak")),
    (&["pale_oak_creaking"], Tree::Feature("minecraft:pale_oak_creaking")),
    (&["rand", "random"], Tree::Any),
];

/// `TreeType.lookup`: a tree type by any of its names.
pub(super) fn tree_lookup(name: &str) -> Option<Tree> {
    let name = name.to_ascii_lowercase();
    TREES.iter().find(|(names, _)| names.contains(&name.as_str())).map(|&(_, t)| t)
}

/// `TreeType.generate`: plants `tree` with its base at `p`; whether it did.
pub(super) fn plant(world: &mut dyn World, tree: Tree, p: Pos, rng: &mut Rng) -> bool {
    match tree {
        Tree::Feature(id) => world.generate_tree(p, id),
        Tree::Chorus => world.generate_tree([p[0], p[1] - 1, p[2]], "minecraft:chorus_plant"),
        Tree::Pine => pine(world, p, rng),
        Tree::Pick(names) => tree_lookup(names[rng.below(names.len())]).is_some_and(|t| plant(world, t, p, rng)),
        Tree::Any => {
            let choices: Vec<Tree> = TREES.iter().map(|&(_, t)| t).filter(|t| !matches!(t, Tree::Any)).collect();
            let t = choices[rng.below(choices.len())];
            plant(world, t, p, rng)
        }
    }
}

/// `TreeGenerator.makePineTree`: a trunk, then layers of logs and leaves, set only where the block is air.
fn pine(world: &mut dyn World, base: Pos, rng: &mut Rng) -> bool {
    let log = world.resolve(&BlockState::new("minecraft:oak_log", &[])).unwrap_or(0);
    let leaves = world.resolve(&BlockState::new("minecraft:oak_leaves", &[])).unwrap_or(0);
    let trunk = (rng.unit() * 2.0).floor() as i32 + 3;
    let height = (rng.unit() * 5.0).floor() as i32 + 8;
    let mut writes: Vec<(Pos, StateId)> = Vec::new();
    let mut taken: HashSet<Pos> = HashSet::new();
    let free = |world: &dyn World, taken: &HashSet<Pos>, p: Pos| !taken.contains(&p) && world.get(p).is_some_and(|s| world.is_air(s));
    let mut set = |world: &dyn World, writes: &mut Vec<(Pos, StateId)>, p: Pos, s: StateId| -> bool {
        if free(world, &taken, p) {
            taken.insert(p);
            writes.push((p, s));
            true
        } else {
            false
        }
    };
    for i in 0..trunk {
        if !set(world, &mut writes, add(base, [0, i, 0]), log) {
            return false;
        }
    }
    let top = add(base, [0, trunk, 0]);
    for i in 0..height {
        set(world, &mut writes, add(top, [0, i, 0]), log);
        let chance = if i == 0 || i == height - 1 { 0.6 } else { 1.0 };
        for d in [[-1, 0], [1, 0], [0, -1], [0, 1], [1, 1], [-1, 1], [1, -1], [-1, -1]] {
            if rng.unit() <= chance {
                set(world, &mut writes, add(top, [d[0], i, d[1]]), leaves);
            }
        }
        if !(i == 0 || i == height - 1) {
            for j in -2..=2 {
                for d in [[-2, j], [2, j], [j, -2], [j, 2]] {
                    if rng.unit() <= 0.6 {
                        set(world, &mut writes, add(top, [d[0], i, d[1]]), leaves);
                    }
                }
            }
        }
    }
    set(world, &mut writes, add(top, [0, height, 0]), leaves);
    world.write(&writes);
    true
}

/// `//forest`'s `LayerVisitor` over `columns`: in a share (`density`) of them the first solid block from `y1` down
/// takes a tree, on top of it, or in its place when it is plants or snow. How many trees stand.
pub(super) fn plant_forest(world: &mut dyn World, columns: &[(i32, i32)], y0: i32, y1: i32, density: f64, tree: Tree, rng: &mut Rng) -> usize {
    let mut planted = 0;
    for &(x, z) in columns {
        if rng.unit() > density {
            continue;
        }
        let Some((p, state)) = ground(world, x, z, y0, y1) else { continue };
        let name = world.describe(state).name;
        if is_solid_name(&name) {
            if plant(world, tree, [p[0], p[1] + 1, p[2]], rng) {
                planted += 1;
            }
        } else if replaceable(&name) {
            // The generators only plant into air: clear the plant, plant, and put it back when nothing grew.
            let air = world.resolve(&BlockState::air()).unwrap_or(0);
            world.write(&[(p, air)]);
            if plant(world, tree, p, rng) {
                planted += 1;
            } else {
                world.write(&[(p, state)]);
            }
        }
    }
    planted
}

/// The first block that is not air from `y1` down to `y0` in the column, with its state; `None` when the column
/// starts below ground (its top block is not air) or holds none.
pub(super) fn ground(world: &dyn World, x: i32, z: i32, y0: i32, y1: i32) -> Option<(Pos, StateId)> {
    let top = (y1 + 1).min(world.max_y());
    if world.get([x, top, z]).is_some_and(|s| !world.is_air(s)) && top > y1 {
        return None;
    }
    (y0.max(world.min_y())..=y1.min(world.max_y())).rev().find_map(|y| {
        let s = world.get([x, y, z])?;
        (!world.is_air(s)).then_some(([x, y, z], s))
    })
}

/// What a placement replaces (`Material.isReplacedDuringPlacement`).
fn replaceable(name: &str) -> bool {
    matches!(name, "minecraft:short_grass" | "minecraft:tall_grass" | "minecraft:fern" | "minecraft:large_fern" | "minecraft:dead_bush" | "minecraft:snow" | "minecraft:vine" | "minecraft:seagrass" | "minecraft:tall_seagrass")
}

/// `//flora`'s writes: over a share of the columns, the block above the ground gets what the ground grows.
fn flora_ops(world: &dyn World, columns: &[(i32, i32)], y0: i32, y1: i32, density: f64, rng: &mut Rng) -> Result<(Vec<(Pos, StateId)>, usize), String> {
    let id = |n: &str| world.resolve(&BlockState::new(n, &[])).ok_or_else(|| format!("Block name '{n}' was not recognized."));
    let (grass_block, sand) = (id("minecraft:grass_block")?, id("minecraft:sand")?);
    let (air, short_grass, poppy, dandelion) = (id("minecraft:air")?, id("minecraft:short_grass")?, id("minecraft:poppy")?, id("minecraft:dandelion")?);
    let (dead_bush, cactus) = (id("minecraft:dead_bush")?, id("minecraft:cactus")?);
    let mut ops = Vec::new();
    let mut affected = 0;
    for &(x, z) in columns {
        if rng.unit() > density {
            continue;
        }
        let Some((p, state)) = ground(world, x, z, y0, y1) else { continue };
        let above = [p[0], p[1] + 1, p[2]];
        // A weighted pick: `RandomPattern`'s weights, grass 300, poppy 5, dandelion 5; dead bush 30, cactus 20, air 300.
        let pick = if state == grass_block {
            let r = rng.unit() * 310.0;
            if r < 300.0 { short_grass } else if r < 305.0 { poppy } else { dandelion }
        } else if state == sand {
            let r = rng.unit() * 350.0;
            if r < 30.0 { dead_bush } else if r < 50.0 { cactus } else { air }
        } else {
            continue;
        };
        ops.push((above, pick));
        affected += 1;
    }
    Ok((ops, affected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    #[test]
    fn biome_names_read_like_the_game_shows_them() {
        assert_eq!(rich_name("minecraft:old_growth_pine_taiga"), "Old Growth Pine Taiga");
        assert_eq!(rich_name("plains"), "Plains");
        assert_eq!(namespaced("Plains"), "minecraft:plains");
        assert_eq!(namespaced("mod:thing"), "mod:thing");
    }

    #[test]
    fn tree_types_answer_to_every_name_of_the_original() {
        for name in ["tree", "regular", "BIGTREE", "redwood", "tallsequoioideae", "whitebark", "junglebush", "greennethermushroom", "swamptree", "pale_oak_creaking", "rand"] {
            assert!(tree_lookup(name).is_some(), "{name}");
        }
        assert!(tree_lookup("palm").is_none());
        assert!(matches!(tree_lookup("pine"), Some(Tree::Pine)));
    }

    /// Flat ground of grass on stone, air above.
    fn meadow() -> FakeWorld {
        let mut w = FakeWorld::new();
        w.fill([0, 0, 0], [15, 5, 15], "minecraft:stone");
        w.fill([0, 6, 0], [15, 6, 15], "minecraft:grass_block");
        w
    }

    #[test]
    fn flora_puts_plants_on_the_grass_and_counts_the_columns() {
        let mut w = meadow();
        let columns: Vec<(i32, i32)> = (0..16).flat_map(|x| (0..16).map(move |z| (x, z))).collect();
        let mut rng = Rng(7);
        // The original's blocks are refused by name where the game lacks them.
        let err = flora_ops(&w, &columns, 0, 20, 1.0, &mut rng).unwrap_err();
        assert!(err.contains("was not recognized"), "{err}");
        for n in ["short_grass", "poppy", "dandelion", "dead_bush", "cactus"] {
            w.states.push(BlockState::new(&format!("minecraft:{n}"), &[]));
        }
        let (ops, affected) = flora_ops(&w, &columns, 0, 20, 1.0, &mut rng).unwrap();
        assert_eq!((ops.len(), affected), (256, 256));
        assert!(ops.iter().all(|&(p, _)| p[1] == 7));
        let grass = w.id("minecraft:short_grass");
        assert!(ops.iter().filter(|&&(_, s)| s == grass).count() > 230);
        // Density 0 makes none, and sand grows desert plants, not grass.
        assert!(flora_ops(&w, &columns, 0, 20, 0.0, &mut rng).unwrap().0.is_empty());
        w.fill([0, 6, 0], [15, 6, 15], "minecraft:sand");
        let (ops, _) = flora_ops(&w, &columns, 0, 20, 1.0, &mut rng).unwrap();
        assert!(ops.iter().all(|&(_, s)| s != grass));
    }

    #[test]
    fn the_ground_is_the_first_block_from_the_top() {
        let w = meadow();
        assert_eq!(ground(&w, 3, 4, 0, 20).map(|(p, _)| p), Some([3, 6, 4]));
        // A column whose top is solid is underground: nothing to plant on.
        assert_eq!(ground(&w, 3, 4, 0, 6), Some(([3, 6, 4], w.id("minecraft:grass_block"))));
        assert_eq!(ground(&w, 3, 4, 7, 20), None);
    }

    #[test]
    fn a_forest_plants_on_solid_ground_and_counts_each_tree() {
        let mut w = meadow();
        let columns = [(2, 2), (10, 10), (2, 10)];
        let tree = tree_lookup("birch").unwrap();
        let mut rng = Rng(11);
        let n = plant_forest(&mut w, &columns, 0, 20, 1.0, tree, &mut rng);
        assert_eq!(n, 3);
        assert_eq!(w.planted, vec!["minecraft:birch"; 3]);
        assert!(w.name_at([2, 7, 2]).starts_with("minecraft:oak_log"));
        // Density 0 plants nothing.
        let mut w = meadow();
        assert_eq!(plant_forest(&mut w, &columns, 0, 20, 0.0, tree, &mut rng), 0);
        assert!(w.planted.is_empty());
    }

    #[test]
    fn a_pine_is_made_of_logs_and_leaves() {
        let mut w = meadow();
        let mut rng = Rng(3);
        assert!(pine(&mut w, [8, 7, 8], &mut rng));
        assert!(w.name_at([8, 7, 8]).starts_with("minecraft:oak_log"));
        // Every pine has a trunk of 3 or 4 logs, then logs up the middle.
        assert!(w.name_at([8, 7 + 3, 8]).starts_with("minecraft:oak_log"));
    }

    #[test]
    fn biomes_are_set_by_cell_and_remembered_for_undo() {
        let mut w = FakeWorld::new();
        let c = Cuboid::new([0, 0, 0], [7, 3, 3]);
        let (affected, extras) = apply_biome(&mut w, &c, "minecraft:desert", None).unwrap();
        assert_eq!(affected, 8 * 4 * 4);
        assert_eq!(extras.biomes.len(), 2);
        assert_eq!(w.biome_at([5, 2, 2]).as_deref(), Some("minecraft:desert"));
        assert_eq!(extras.biomes[0].1, "minecraft:plains");
        // The same biome again changes no cell.
        let (_, again) = apply_biome(&mut w, &c, "minecraft:desert", None).unwrap();
        assert!(again.biomes.is_empty());
    }

    #[test]
    fn replace_biome_follows_the_mask() {
        let mut w = FakeWorld::new();
        w.fill([0, 0, 0], [3, 3, 3], "minecraft:stone");
        let mask = Mask::parse(&w, "stone", None).unwrap();
        let c = Cuboid::new([0, 0, 0], [7, 3, 3]);
        let (affected, extras) = apply_biome(&mut w, &c, "minecraft:desert", Some(&mask)).unwrap();
        assert_eq!(affected, 4 * 4 * 4);
        assert_eq!(extras.biomes.len(), 1);
        assert_eq!(w.biome_at([0, 0, 0]).as_deref(), Some("minecraft:desert"));
        assert_eq!(w.biome_at([4, 0, 0]).as_deref(), Some("minecraft:plains"));
    }

    #[test]
    fn the_world_override_is_kept_per_player() {
        set_world_override(0xFEED_0001, Some("minecraft:the_nether".into()));
        assert_eq!(world_override(0xFEED_0001).as_deref(), Some("minecraft:the_nether"));
        assert_eq!(world_override(0xFEED_0002), None);
        set_world_override(0xFEED_0001, None);
        assert_eq!(world_override(0xFEED_0001), None);
    }

    #[test]
    fn chunks_of_a_box_and_the_blocks_outside_it() {
        let c = Cuboid::new([8, 0, 8], [24, 3, 8]);
        assert_eq!(chunks_of(&c), vec![[0, 0], [1, 0]]);
        let mut w = FakeWorld::new();
        w.min_y = 0;
        w.max_y = 3;
        w.fill([0, 0, 0], [31, 3, 15], "minecraft:stone");
        let out = outside_of(&w, &c, &chunks_of(&c));
        // Two chunks of 16 x 4 x 16, less the 17 x 4 x 1 blocks the box owns.
        assert_eq!(out.blocks.len(), 2 * 16 * 16 * 4 - 17 * 4);
        let whole = Cuboid::new([0, 0, 0], [15, 3, 15]);
        assert!(outside_of(&w, &whole, &chunks_of(&whole)).blocks.is_empty());
    }

    // ---- the commands, end to end ----

    use crate::horizon_worldedit_core::engine::{Actor, Engine, Env};
    use crate::horizon_worldedit_core::world::World as WorldTrait;
    use crate::horizon_worldedit_core::world::Generated;

    /// A fake world that can also regenerate and delete chunks: a regeneration turns the whole chunk to grass blocks.
    struct Terrain {
        inner: FakeWorld,
        regenerated: Vec<[i32; 2]>,
        deleted: Vec<[i32; 2]>,
    }

    impl WorldTrait for Terrain {
        fn get(&self, p: Pos) -> Option<StateId> {
            self.inner.get(p)
        }
        fn min_y(&self) -> i32 {
            self.inner.min_y()
        }
        fn max_y(&self) -> i32 {
            self.inner.max_y()
        }
        fn describe(&self, s: StateId) -> BlockState {
            self.inner.describe(s)
        }
        fn resolve(&self, s: &BlockState) -> Option<StateId> {
            self.inner.resolve(s)
        }
        fn write(&mut self, w: &[(Pos, StateId)]) -> usize {
            self.inner.write(w)
        }
        fn block_entity_at(&self, p: Pos) -> Option<Tag> {
            self.inner.block_entity_at(p)
        }
        fn biome_at(&self, p: Pos) -> Option<String> {
            self.inner.biome_at(p)
        }
        fn set_biome(&mut self, p: Pos, b: &str) -> bool {
            self.inner.set_biome(p, b)
        }
        fn generate_tree(&mut self, p: Pos, f: &str) -> bool {
            self.inner.generate_tree(p, f)
        }
        fn biome_names(&self) -> Vec<String> {
            (0..20).map(|i| format!("minecraft:biome_{i:02}")).chain(["minecraft:desert".to_owned(), "minecraft:plains".to_owned()]).collect()
        }
        fn level_names(&self) -> Vec<String> {
            vec!["minecraft:overworld".into(), "minecraft:the_nether".into()]
        }
        fn regenerate_chunks(&mut self, chunks: &[[i32; 2]]) -> Result<u32, String> {
            let grass = self.inner.id("minecraft:grass_block");
            for &[cx, cz] in chunks {
                for x in cx * 16..cx * 16 + 16 {
                    for z in cz * 16..cz * 16 + 16 {
                        for y in self.inner.min_y..=self.inner.max_y {
                            self.inner.blocks.insert([x, y, z], grass);
                        }
                    }
                }
            }
            self.regenerated.extend_from_slice(chunks);
            Ok(chunks.len() as u32)
        }
        fn delete_chunks_with_backup(&mut self, chunks: &[[i32; 2]]) -> Result<u32, String> {
            // The chunk (0, 0) is held by a player.
            let gone: Vec<[i32; 2]> = chunks.iter().copied().filter(|c| *c != [0, 0]).collect();
            self.deleted.extend_from_slice(&gone);
            Ok(gone.len() as u32)
        }
        fn saved_chunk_modified(&self, chunk: [i32; 2]) -> Option<i64> {
            // (0, 0) was saved in 2001, (1, 0) in 2033, (2, 0) never.
            match chunk {
                [0, 0] => Some(1_000_000_000),
                [1, 0] => Some(2_000_000_000),
                _ => None,
            }
        }
        fn generate_region(&mut self, min: Pos, max: Pos, seed: Option<i64>, biomes: bool) -> Result<Generated, String> {
            // Seed 7 makes sand, any other seed (or the level's own) glass; every cell is desert with biomes.
            let state = self.inner.id(if seed == Some(7) { "minecraft:sand" } else { "minecraft:glass" });
            let mut out = Generated::default();
            for x in min[0]..=max[0] {
                for y in min[1]..=max[1] {
                    for z in min[2]..=max[2] {
                        out.blocks.push(([x, y, z], state));
                    }
                }
            }
            if biomes {
                for x in (min[0] >> 2)..=(max[0] >> 2) {
                    for y in (min[1] >> 2)..=(max[1] >> 2) {
                        for z in (min[2] >> 2)..=(max[2] >> 2) {
                            out.biomes.insert([x << 2, y << 2, z << 2], "minecraft:desert".into());
                        }
                    }
                }
            }
            Ok(out)
        }
    }

    fn terrain() -> Terrain {
        let mut inner = FakeWorld::new();
        inner.min_y = 0;
        inner.max_y = 15;
        Terrain { inner, regenerated: Vec::new(), deleted: Vec::new() }
    }

    fn actor(uuid: u128) -> Actor {
        Actor { uuid, name: format!("builder{uuid}"), inventory: None, offhand: None, permission: 2, dimension: "minecraft:overworld".into(), position: [0.5, 8.0, 0.5], yaw: 0.0, pitch: 0.0, game_mode: 1, held: None, granted: Default::default() }
    }

    fn env() -> Env {
        let dir = std::env::temp_dir().join("we-terrain-tests");
        let _ = std::fs::create_dir_all(&dir);
        Env { config_file: dir.join("worldedit.toml"), schematics_dir: dir, data_version: 4440, now_ms: 0 }
    }

    fn run(e: &mut Engine, w: &mut Terrain, a: &Actor, line: &str) -> String {
        let r = e.command(w, a, line, &env());
        assert!(r.lines.iter().all(|l| !l.error), "`{line}` failed: {}", r.text());
        r.text()
    }

    fn fails(e: &mut Engine, w: &mut Terrain, a: &Actor, line: &str) -> String {
        let r = e.command(w, a, line, &env());
        assert!(r.lines.iter().any(|l| l.error), "`{line}` should fail: {}", r.text());
        r.text()
    }

    #[test]
    fn set_biome_and_undo() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7101));
        e.select(a.uuid, &a.dimension, [0, 0, 0], [7, 3, 3]);
        let said = run(&mut e, &mut w, &a, "/setbiome desert");
        assert_eq!(said, "Biomes were changed for approximately 128 blocks.");
        assert_eq!(w.biome_at([5, 2, 2]).as_deref(), Some("minecraft:desert"));
        fails(&mut e, &mut w, &a, "/setbiome nowhere");
        // -p: the block the player stands in.
        run(&mut e, &mut w, &a, "/setbiome -p minecraft:desert");
        assert_eq!(w.biome_at([0, 8, 0]).as_deref(), Some("minecraft:desert"));
        run(&mut e, &mut w, &a, "/undo");
        run(&mut e, &mut w, &a, "/undo");
        assert_eq!(w.biome_at([5, 2, 2]).as_deref(), Some("minecraft:plains"));
    }

    #[test]
    fn replace_biome_only_where_the_mask_hits() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7102));
        w.inner.fill([0, 0, 0], [3, 3, 3], "minecraft:stone");
        e.select(a.uuid, &a.dimension, [0, 0, 0], [7, 3, 3]);
        let said = run(&mut e, &mut w, &a, "/replacebiome stone desert");
        assert_eq!(said, "Biomes were changed for approximately 64 blocks.");
        assert_eq!(w.biome_at([1, 1, 1]).as_deref(), Some("minecraft:desert"));
        assert_eq!(w.biome_at([6, 1, 1]).as_deref(), Some("minecraft:plains"));
    }

    #[test]
    fn biome_info_and_list() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7103));
        e.select(a.uuid, &a.dimension, [0, 0, 0], [7, 3, 3]);
        w.set_biome([0, 0, 0], "minecraft:desert");
        w.set_biome([0, 8, 0], "minecraft:desert");
        assert_eq!(run(&mut e, &mut w, &a, "biomeinfo"), "Biomes in your selection: Desert, Plains");
        assert_eq!(run(&mut e, &mut w, &a, "biomeinfo -p"), "Biomes at your position: Desert");
        let page = run(&mut e, &mut w, &a, "biomelist");
        assert!(page.contains("minecraft:biome_00 (Biome 00)"), "{page}");
        assert!(page.contains("Page 1 of 3"), "{page}");
        let last = run(&mut e, &mut w, &a, "biomels -p 3");
        assert!(last.contains("minecraft:plains (Plains)"), "{last}");
        fails(&mut e, &mut w, &a, "biomelist -p 9");
    }

    #[test]
    fn regen_replaces_the_selection_and_keeps_the_rest_of_its_chunks() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7104));
        w.inner.fill([0, 0, 0], [31, 15, 15], "minecraft:stone");
        // Half of chunk (0, 0) and all of chunk (1, 0).
        e.select(a.uuid, &a.dimension, [8, 0, 0], [31, 15, 15]);
        assert_eq!(run(&mut e, &mut w, &a, "/regen"), "Region regenerated.");
        assert_eq!(w.regenerated, vec![[0, 0], [1, 0]]);
        let grass = w.inner.id("minecraft:grass_block");
        let stone = w.inner.id("minecraft:stone");
        assert_eq!(w.get([20, 5, 5]), Some(grass));
        assert_eq!(w.get([8, 0, 0]), Some(grass));
        assert_eq!(w.get([7, 5, 5]), Some(stone), "outside the selection, in a chunk it partly covers");
        run(&mut e, &mut w, &a, "/undo");
        assert_eq!(w.get([20, 5, 5]), Some(stone));
        // A chunk that is not loaded.
        w.inner.unloaded.push((1, 0));
        fails(&mut e, &mut w, &a, "/regen");
    }

    #[test]
    fn regen_with_a_seed_takes_only_the_selection_from_the_generator() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7108));
        w.inner.fill([0, 0, 0], [15, 15, 15], "minecraft:stone");
        e.select(a.uuid, &a.dimension, [2, 0, 2], [5, 3, 5]);
        assert_eq!(run(&mut e, &mut w, &a, "/regen 7"), "Region regenerated.");
        assert!(w.regenerated.is_empty(), "the engine's own regeneration is not used");
        let (sand, stone) = (w.inner.id("minecraft:sand"), w.inner.id("minecraft:stone"));
        assert_eq!(w.get([3, 1, 3]), Some(sand));
        assert_eq!(w.get([6, 1, 3]), Some(stone));
        assert_eq!(w.get([3, 4, 3]), Some(stone));
        // The biomes stay without -b and come with it.
        assert_eq!(w.biome_at([3, 1, 3]).as_deref(), Some("minecraft:plains"));
        run(&mut e, &mut w, &a, "/regen -b 7");
        assert_eq!(w.biome_at([3, 1, 3]).as_deref(), Some("minecraft:desert"));
        // Undo puts back the biomes and then the blocks.
        run(&mut e, &mut w, &a, "/undo");
        assert_eq!(w.biome_at([3, 1, 3]).as_deref(), Some("minecraft:plains"));
        run(&mut e, &mut w, &a, "/undo");
        assert_eq!(w.get([3, 1, 3]), Some(stone));
        fails(&mut e, &mut w, &a, "/regen seven");
    }

    #[test]
    fn regen_into_the_clipboard_leaves_the_world_alone() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7109));
        w.inner.fill([0, 0, 0], [15, 15, 15], "minecraft:stone");
        e.select(a.uuid, &a.dimension, [2, 0, 2], [3, 1, 3]);
        assert_eq!(run(&mut e, &mut w, &a, "/regen -c 7"), "Region regenerated.");
        assert_eq!(w.get([2, 0, 2]), Some(w.inner.id("minecraft:stone")));
        // The origin is the player's block, so a paste there puts the generated sand where it would have stood.
        run(&mut e, &mut w, &a, "/paste");
        assert_eq!(w.get([2, 0, 2]), Some(w.inner.id("minecraft:sand")));
    }

    #[test]
    fn regen_takes_a_limited_number_of_chunks() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7105));
        e.select(a.uuid, &a.dimension, [0, 0, 0], [16 * 9 - 1, 15, 16 * 8 - 1]);
        let said = fails(&mut e, &mut w, &a, "/regen");
        assert!(said.contains("at most 64"), "{said}");
        assert!(w.regenerated.is_empty());
    }

    #[test]
    fn delchunks_says_what_was_left_alone() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7106));
        e.select(a.uuid, &a.dimension, [0, 0, 0], [31, 15, 15]);
        let said = run(&mut e, &mut w, &a, "delchunks");
        assert!(said.contains("1 chunk(s) have been erased"), "{said}");
        assert!(said.contains("1 chunk(s) are loaded or held"), "{said}");
        assert_eq!(w.deleted, vec![[1, 0]]);
    }

    #[test]
    fn delchunks_o_keeps_chunks_saved_after_the_time() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x710a));
        // Chunks (0, 0) saved 2001-09-09, (1, 0) 2033-05-18, (2, 0) never.
        e.select(a.uuid, &a.dimension, [0, 0, 0], [47, 15, 15]);
        let said = run(&mut e, &mut w, &a, "delchunks -o 2020-01-01");
        assert!(said.contains("2 chunk(s) of the selection are newer than that or not saved"), "{said}");
        // (0, 0) is held by a player, so nothing is erased, and (1, 0) and (2, 0) were not asked for.
        assert!(w.deleted.is_empty());
        let said = run(&mut e, &mut w, &a, "delchunks -o 2040-01-01T00:00");
        assert!(said.contains("1 chunk(s) have been erased") || said.contains("0 chunk(s) have been erased"), "{said}");
        assert_eq!(w.deleted, vec![[1, 0]]);
        fails(&mut e, &mut w, &a, "delchunks -o yesterday");
    }

    #[test]
    fn cutoffs_are_dates_or_ages() {
        assert_eq!(parse_cutoff("1970-01-02", 0), Ok(86_400));
        assert_eq!(parse_cutoff("2001-09-09T01:46:40", 0), Ok(1_000_000_000));
        assert_eq!(parse_cutoff("1d2h", 1_000_000), Ok(1_000_000 - 86_400 - 7200));
        assert_eq!(parse_cutoff("2w", 2_000_000), Ok(2_000_000 - 14 * 86_400));
        assert!(parse_cutoff("", 0).is_err());
        assert!(parse_cutoff("5", 0).is_err());
        assert!(parse_cutoff("2026-13-01", 0).is_err());
    }

    #[test]
    fn world_sets_and_removes_the_override() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7107));
        assert_eq!(run(&mut e, &mut w, &a, "/world the_nether"), "Set the world override to minecraft:the_nether. (Use //world to go back to default)");
        assert_eq!(world_override(a.uuid).as_deref(), Some("minecraft:the_nether"));
        fails(&mut e, &mut w, &a, "/world atlantis");
        assert_eq!(run(&mut e, &mut w, &a, "/world"), "Removed world override.");
        assert_eq!(world_override(a.uuid), None);
    }

    #[test]
    fn forest_and_flora_work_on_the_selection_and_undo() {
        let (mut e, mut w, a) = (Engine::default(), terrain(), actor(0x7108));
        w.inner.fill([0, 0, 0], [15, 5, 15], "minecraft:stone");
        w.inner.fill([0, 6, 0], [15, 6, 15], "minecraft:grass_block");
        e.select(a.uuid, &a.dimension, [0, 0, 0], [15, 12, 15]);
        let said = run(&mut e, &mut w, &a, "/forest birch 100");
        assert!(said.ends_with(" trees created."), "{said}");
        assert!(!w.inner.planted.is_empty());
        assert!(w.inner.planted.iter().all(|f| f == "minecraft:birch"));
        assert!(w.inner.count("minecraft:oak_log[axis=y]") > 0);
        run(&mut e, &mut w, &a, "/undo");
        assert_eq!(w.inner.count("minecraft:oak_log[axis=y]"), 0);
        fails(&mut e, &mut w, &a, "/forest palm");
        fails(&mut e, &mut w, &a, "/forest birch 101");
        fails(&mut e, &mut w, &a, "/flora 101");
    }

    #[test]
    fn the_terrain_brushes_equip_and_work() {
        let (mut e, mut w, mut a) = (Engine::default(), terrain(), actor(0x7109));
        w.inner.fill([0, 0, 0], [15, 5, 15], "minecraft:stone");
        w.inner.fill([0, 6, 0], [15, 6, 15], "minecraft:grass_block");
        a.held = Some("minecraft:stick".into());
        let said = run(&mut e, &mut w, &a, "/brush biome sphere 5 desert");
        assert!(said.starts_with("Set brush to set biome of type minecraft:desert."), "{said}");
        e.brush_use(&mut w, &a, Some([8, 6, 8]), 1000).expect("a brush");
        assert_eq!(w.biome_at([8, 6, 8]).as_deref(), Some("minecraft:desert"));
        let said = run(&mut e, &mut w, &a, "/brush forest sphere 5 100 birch");
        assert!(said.starts_with("Set brush to scatter tree of type BIRCH."), "{said}");
        e.brush_use(&mut w, &a, Some([8, 6, 8]), 2000).expect("a brush");
        assert!(!w.inner.planted.is_empty());
        let said = run(&mut e, &mut w, &a, "/brush feature cuboid 3 100 oak");
        assert!(said.starts_with("Set brush to set feature of type minecraft:oak."), "{said}");
        let before = w.inner.planted.len();
        e.brush_use(&mut w, &a, Some([8, 7, 8]), 3000).expect("a brush");
        assert!(w.inner.planted.len() > before);
        fails(&mut e, &mut w, &a, "/brush forest sphere 5 100 palm");
    }
}
