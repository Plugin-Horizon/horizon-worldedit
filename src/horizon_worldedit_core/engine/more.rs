//! Round 6: `//update`, `//feature`, `//structure`, `/toggleeditwand`, `forestgen`, `pumpkins`, `//pos`, and the apply and
//! paint brushes (`ApplyBrushCommands`, `PaintBrushCommands`). They follow `RegionCommands.update`, `GenerationCommands`,
//! `SelectionCommands.pos` and `toggleWand`, `GardenPatchGenerator` and the operation factories of WorldEdit.

use super::terrain::{ground, namespaced, plant, plant_forest, tree_lookup, Rng};
use super::*;
use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::selector::SelKind;
use crate::horizon_worldedit_core::session::{ApplyOp, BrushShape};

/// A direction word of `/brush apply|paint ... item <item> [direction]` (`@Direction(includeDiagonals = true)`): the six
/// axes, their one-letter forms and the diagonals made of them.
pub(super) fn direction_word(word: &str) -> Option<[i32; 3]> {
    let w = word.to_ascii_lowercase();
    let axis = |c: &str| -> Option<[i32; 3]> {
        Some(match c {
            "north" | "n" => [0, 0, -1],
            "south" | "s" => [0, 0, 1],
            "east" | "e" => [1, 0, 0],
            "west" | "w" => [-1, 0, 0],
            "up" | "u" => [0, 1, 0],
            "down" | "d" => [0, -1, 0],
            _ => return None,
        })
    };
    if let Some(v) = axis(&w) {
        return Some(v);
    }
    let (a, b) = match w.as_str() {
        "northeast" | "ne" => ("north", "east"),
        "northwest" | "nw" => ("north", "west"),
        "southeast" | "se" => ("south", "east"),
        "southwest" | "sw" => ("south", "west"),
        _ => return None,
    };
    let (a, b) = (axis(a)?, axis(b)?);
    Some([a[0] + b[0], a[1] + b[1], a[2] + b[2]])
}

/// The blocks of the region a brush shape makes round `target` (`RegionFactory.createCenteredAt`): 0 sphere, 1 cylinder
/// (one block high), 2 cuboid.
pub(super) fn region_cells(target: Pos, shape: u8, radius: f64) -> Vec<Pos> {
    match shape {
        0 => shapes::sphere(target, [radius; 3], false),
        1 => shapes::cylinder(target, [radius; 2], 1, false),
        _ => {
            let r = radius as i32;
            let mut v = Vec::new();
            for x in -r..=r {
                for y in -r..=r {
                    for z in -r..=r {
                        v.push([target[0] + x, target[1] + y, target[2] + z]);
                    }
                }
            }
            v
        }
    }
}

/// `Operation.toString` of a brush bound by `/brush apply|paint`: what `Set brush to {0}.` names.
pub(super) fn describe_apply(paint: bool, op: &ApplyOp) -> String {
    let function = match op {
        ApplyOp::Forest(tree) => format!("tree of type {}", tree.to_ascii_uppercase()),
        ApplyOp::Item(item, _) => format!("application of the item {item}"),
        ApplyOp::Set(_) => "replace blocks".to_owned(),
    };
    if paint {
        format!("scatter {function}")
    } else {
        format!("set {function}")
    }
}

/// The warning `/brush apply|paint ... item` prints first.
pub(super) const ITEM_WARNING: &str = "WARNING: This brush simulates item usages. Its effects may not work on all platforms, may not be undo-able, and may cause strange interactions with other mods/plugins. Use at your own risk.";

impl Engine {
    /// `//update [side effects]`: the side effects (comma separated; none given: the defaults) applied to every block of the selection.
    pub(super) fn update_cmd(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        if c.volume() > SCAN_LIMIT {
            return Err(format!("The area is {} blocks; one edit handles at most {SCAN_LIMIT}", c.volume()));
        }
        let (mut lighting, mut neighbors, mut update) = (true, true, true);
        if let Some(list) = args.get(0) {
            (lighting, neighbors, update) = (false, false, false);
            for name in list.split(',') {
                match name.trim().to_ascii_lowercase().as_str() {
                    "lighting" => lighting = true,
                    "neighbors" => neighbors = true,
                    "update" => update = true,
                    // No hook of the game stands behind these three: choosing them changes nothing.
                    "validation" | "entity_ai" | "events" => {}
                    other => return Err(format!("Unknown side effect '{other}'.")),
                }
            }
        }
        let shape = self.shape_of(actor);
        let mut positions = Self::shape_positions(&shape, &c);
        // `RegionMaskingFilter` with the session's mask, `ChangeLimitingFilter` with its block limit.
        if let Some(text) = self.sessions.get(actor.uuid).global_mask.clone() {
            let mask = Mask::parse(world, &text, None)?;
            let w: &dyn World = &*world;
            positions.retain(|&p| w.get(p).is_some_and(|s| mask.hits_at(w, p, s)));
        }
        let limit = self.max_blocks(actor);
        if limit >= 0 {
            positions.truncate(limit as usize);
        }
        world.apply_side_effects(&positions, lighting, neighbors, update);
        Ok(vec!["Applied side effects to the selection.".into()])
    }

    /// `//feature <feature>`: the configured feature at the placement position.
    pub(super) fn feature_cmd(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let id = namespaced(args.get(0).ok_or_else(|| usage("feature"))?);
        let at = self.placement(actor)?;
        let area = Cuboid::new([at[0] - 8, at[1] - 8, at[2] - 8], [at[0] + 8, at[1] + 8, at[2] + 8]);
        let mut made = false;
        self.plant_with_undo(world, actor, &area, |w| {
            made = w.generate_tree(at, &id);
            usize::from(made)
        })?;
        if made {
            Ok(vec!["Feature created.".into()])
        } else {
            Err("Failed to generate feature. Is it a valid spot for it?".into())
        }
    }

    /// `//structure <structure>`: the structure with its start at the placement position.
    pub(super) fn structure_cmd(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let id = namespaced(args.get(0).ok_or_else(|| usage("structure"))?);
        let at = self.placement(actor)?;
        let area = Cuboid::new([at[0] - 40, at[1] - 16, at[2] - 40], [at[0] + 40, at[1] + 8, at[2] + 40]);
        let mut made = false;
        self.plant_with_undo(world, actor, &area, |w| {
            made = w.generate_structure(at, &id);
            usize::from(made)
        })?;
        if made {
            Ok(vec!["Structure created.".into()])
        } else {
            Err("Failed to generate structure. Is it a valid spot for it?".into())
        }
    }

    /// `/toggleeditwand`: the reminder that the wand is a tool now.
    pub(super) fn toggle_edit_wand(&mut self) -> R {
        Ok(vec!["The selection wand is now a normal tool. You can disable it with /tool none and rebind it to any item with /tool selwand or get a new wand with //wand.".into()])
    }

    /// `/forestgen [size] [type] [density]`: trees in the cube of `size` round the placement position (`CuboidRegion.fromCenter`).
    pub(super) fn forest_gen(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let size = args.int(0, 10)?;
        let name = args.get(1).unwrap_or("tree");
        let tree = tree_lookup(name).ok_or_else(|| format!("Tree type '{name}' is unknown."))?;
        let density = match args.get(2) {
            Some(w) => w.parse::<f64>().map_err(|_| number_error(w))?,
            None => 5.0,
        };
        if !(0.0..=100.0).contains(&density) {
            return Err("Density must be between 0 and 100".into());
        }
        self.check_radius(&[f64::from(size)])?;
        let at = self.placement(actor)?;
        let (lo, hi) = (world.min_y(), world.max_y());
        let c = Cuboid::new([at[0] - size, (at[1] - size).max(lo), at[2] - size], [at[0] + size, (at[1] + size).min(hi), at[2] + size]);
        let mut rng = Rng(self.salt(actor) ^ (self.now_ms as u64));
        let columns: Vec<(i32, i32)> = (c.min[0]..=c.max[0]).flat_map(|x| (c.min[2]..=c.max[2]).map(move |z| (x, z))).collect();
        let planted = self.plant_with_undo(world, actor, &c, |w| plant_forest(w, &columns, c.min[1], c.max[1], density / 100.0, tree, &mut rng))?;
        Ok(vec![format!("{planted} trees created.")])
    }

    /// `/pumpkins [size]`: pumpkin patches on the grass round the placement position (`makePumpkinPatches`).
    pub(super) fn pumpkins(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let size = args.int(0, 10)?;
        self.check_radius(&[f64::from(size)])?;
        let at = self.placement(actor)?;
        let (lo, hi) = (world.min_y(), world.max_y());
        let c = Cuboid::new([at[0] - size, (at[1] - 5).max(lo), at[2] - size], [at[0] + size, (at[1] + 10).min(hi), at[2] + size]);
        let mut rng = Rng(self.salt(actor) ^ (self.now_ms as u64));
        let columns: Vec<(i32, i32)> = (c.min[0]..=c.max[0]).flat_map(|x| (c.min[2]..=c.max[2]).map(move |z| (x, z))).collect();
        let made = self.plant_with_undo(world, actor, &c, |w| {
            let mut patches = 0;
            for &(x, z) in &columns {
                if rng.unit() > 0.02 {
                    continue;
                }
                if let Some((p, _)) = ground(w, x, z, c.min[1], c.max[1]) {
                    if garden_patch(w, p, &mut rng) {
                        patches += 1;
                    }
                }
            }
            patches
        })?;
        Ok(vec![format!("{made} pumpkin patches created.")])
    }

    /// `//pos [pos1] [pos2...] [-s selector]`: the first position and any number of further ones; without coordinates both
    /// are where the player stands.
    pub(super) fn pos_cmd(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        let here = actor.block();
        let parse = |text: &str| -> Result<Pos, String> {
            let parts: Vec<&str> = text.split(',').map(str::trim).collect();
            if parts.len() != 3 {
                return Err(usage("pos"));
            }
            let mut p = [0; 3];
            for i in 0..3 {
                p[i] = match parts[i].strip_prefix('~') {
                    Some("") => here[i],
                    Some(off) => here[i] + off.parse::<f64>().map_err(|_| number_error(parts[i]))?.floor() as i32,
                    None => parts[i].parse::<f64>().map_err(|_| number_error(parts[i]))?.floor() as i32,
                };
            }
            Ok(p)
        };
        let mut points: Vec<Pos> = Vec::new();
        if args.words.iter().any(|w| w.contains(',')) {
            for w in &args.words {
                points.push(parse(w)?);
            }
        } else {
            if args.words.len() % 3 != 0 {
                return Err(usage("pos"));
            }
            for chunk in args.words.chunks(3) {
                points.push(parse(&chunk.join(","))?);
            }
        }
        let first = points.first().copied().unwrap_or(here);
        let rest: Vec<Pos> = if points.len() > 1 { points[1..].to_vec() } else { vec![here] };
        let mut out = Vec::new();
        if let Some(word) = args.value('s') {
            let kind = SelKind::parse(word).ok_or_else(|| usage("pos"))?;
            out.extend(self.sel_type(actor, kind, false)?);
        }
        if let Ok(lines) = self.corner_set(world, actor, true, first) {
            out.extend(lines);
        }
        for p in rest {
            if let Ok(lines) = self.corner_set(world, actor, false, p) {
                out.extend(lines);
            }
        }
        Ok(out)
    }

    /// `/brush apply|paint <shape> [radius] [density] forest|item|set ...`: the brush the item will carry.
    pub(super) fn apply_brush_shape(&mut self, world: &dyn World, actor: &Actor, args: &Args, paint: bool) -> Result<BrushShape, String> {
        let name = if paint { "brush paint" } else { "brush apply" };
        let words = &args.words;
        let mut at = 1;
        let shape = match words.get(at).map(|w| Self::region_shape(w)) {
            Some(Ok(s)) => {
                at += 1;
                s
            }
            _ => 0,
        };
        let number = |w: &str| w.parse::<f64>().map_err(|_| number_error(w));
        let mut radius = 5.0;
        let mut density = 20.0;
        if let Some(w) = words.get(at).filter(|w| w.parse::<f64>().is_ok()) {
            radius = number(w)?;
            at += 1;
            if paint {
                if let Some(d) = words.get(at).filter(|w| w.parse::<f64>().is_ok()) {
                    density = number(d)?;
                    at += 1;
                }
            }
        }
        self.check_brush_radius(radius)?;
        if !(0.0..=100.0).contains(&density) {
            return Err("Density must be in [0, 100]".into());
        }
        let sub = words.get(at).ok_or_else(|| usage(name))?.to_ascii_lowercase();
        at += 1;
        let op = match sub.as_str() {
            "forest" => {
                let tree = words.get(at).ok_or_else(|| usage(&format!("{name} forest")))?;
                tree_lookup(tree).ok_or_else(|| format!("Tree type '{tree}' is unknown."))?;
                ApplyOp::Forest((*tree).to_ascii_lowercase())
            }
            "item" => {
                self.permitted(actor, Some("worldedit.brush.item"))?;
                let item = namespaced(words.get(at).ok_or_else(|| usage(&format!("{name} item")))?);
                let known = world.item_names();
                if !known.is_empty() && !known.iter().any(|i| *i == item) {
                    return Err(format!("Item '{item}' was not recognized."));
                }
                let dir = match words.get(at + 1) {
                    Some(w) => direction_word(w).ok_or_else(|| format!("'{w}' is not a valid direction."))?,
                    None => [0, 1, 0],
                };
                ApplyOp::Item(item, dir)
            }
            "set" => ApplyOp::Set(self.need_pattern(world, actor, args, at, &format!("{name} set"))?),
            other => return Err(format!("The sub-command '{other}' under '{name}' could not be found.")),
        };
        Ok(BrushShape::Apply { paint, op, shape, radius, density })
    }

    /// A click with an apply or paint brush at `target`.
    pub(super) fn apply_brush_use(&mut self, world: &mut dyn World, actor: &Actor, brush: &BrushShape, target: Pos, mask: Option<&Mask>) -> Reply {
        let BrushShape::Apply { paint, op, shape, radius, density } = brush else { return Reply::default() };
        if let ApplyOp::Item(..) = op {
            if let Err(e) = self.permitted(actor, Some("worldedit.brush.item")) {
                return Reply::err(e);
            }
        }
        let (lo, hi) = (world.min_y(), world.max_y());
        let cells: Vec<Pos> = region_cells(target, *shape, *radius).into_iter().filter(|p| (lo..=hi).contains(&p[1])).collect();
        let pass = |w: &dyn World, p: &Pos| mask.is_none_or(|m| w.get(*p).is_some_and(|s| m.hits_at(w, *p, s)));
        let salt = self.salt(actor);
        let mut rng = Rng(salt ^ (self.now_ms as u64));
        // The cells an operation acts on: all of them (apply), or the ground of a share of the columns (paint).
        let targets: Vec<Pos> = if *paint {
            let columns: std::collections::BTreeSet<(i32, i32)> = cells.iter().map(|p| (p[0], p[2])).collect();
            let (y0, y1) = (cells.iter().map(|p| p[1]).min().unwrap_or(target[1]), cells.iter().map(|p| p[1]).max().unwrap_or(target[1]));
            columns.into_iter().filter(|_| rng.unit() <= density / 100.0).filter_map(|(x, z)| ground(&*world, x, z, y0, y1).map(|(p, _)| p)).collect()
        } else {
            cells
        };
        let targets: Vec<Pos> = targets.into_iter().filter(|p| pass(&*world, p)).collect();
        let reply = match op {
            ApplyOp::Set(pattern) => {
                let ops: Vec<(Pos, StateId)> = targets.into_iter().map(|p| (p, pattern.pick_with(&*world, salt, p))).collect();
                return match self.commit(world, actor, ops) {
                    Ok(_) => Reply::default(),
                    Err(e) if e.starts_with("Max blocks changed") => Reply::err("Max blocks change limit reached.".into()),
                    Err(e) => Reply::err(e),
                };
            }
            ApplyOp::Forest(kind) => {
                let Some(tree) = tree_lookup(kind) else { return Reply::err(format!("Tree type '{kind}' is unknown.")) };
                let area = Cuboid::new(sub(target, [*radius as i32; 3]), add(target, [*radius as i32; 3]));
                self.plant_with_undo(world, actor, &area, |w| {
                    let mut planted = 0;
                    for p in &targets {
                        // `TreeGenerator.apply`: one block up, up to ten tries.
                        let at = [p[0], p[1] + 1, p[2]];
                        if (0..10).any(|_| plant(w, tree, at, &mut rng)) {
                            planted += 1;
                        }
                    }
                    planted
                })
            }
            ApplyOp::Item(item, dir) => {
                let area = Cuboid::new(sub(target, [*radius as i32 + 1; 3]), add(target, [*radius as i32 + 1; 3]));
                self.plant_with_undo(world, actor, &area, |w| targets.iter().filter(|p| w.use_item(**p, item, *dir)).count())
            }
        };
        match reply {
            Ok(_) => Reply::default(),
            Err(e) => Reply::err(e),
        }
    }
}

use crate::horizon_worldedit_core::region::{add, sub};

/// `GardenPatchGenerator.apply` for the ground block `ground_at`: a leaf block on the grass above it and up to four vines of
/// leaves, logs and pumpkins round it; whether it was the grass the generator wants.
fn garden_patch(world: &mut dyn World, ground_at: Pos, rng: &mut Rng) -> bool {
    let pos = [ground_at[0], ground_at[1] + 1, ground_at[2]];
    let grass = world.resolve(&BlockState::new("minecraft:grass_block", &[]));
    let below = world.get([pos[0], pos[1] - 1, pos[2]]);
    if grass.is_none() || below != grass {
        return false;
    }
    let (Some(leaf), Some(log), Some(pumpkin)) = (
        world.resolve(&BlockState::new("minecraft:oak_leaves", &[("persistent", "true")])),
        world.resolve(&BlockState::new("minecraft:oak_log", &[])),
        world.resolve(&BlockState::new("minecraft:pumpkin", &[])),
    ) else {
        return false;
    };
    set_if_air(world, pos, leaf);
    for d in [[0, 0, 1], [0, 0, -1], [1, 0, 0], [-1, 0, 0]] {
        garden_vine(world, pos, add(pos, d), rng, [leaf, log, pumpkin]);
    }
    true
}

fn set_if_air(world: &mut dyn World, p: Pos, state: StateId) -> bool {
    if world.get(p).is_some_and(|s| world.is_air(s)) {
        world.write(&[(p, state)]);
        true
    } else {
        false
    }
}

/// `GardenPatchGenerator.placeVine`.
fn garden_vine(world: &mut dyn World, base: Pos, mut pos: Pos, rng: &mut Rng, [leaf, log, pumpkin]: [StateId; 3]) {
    let dist = (0..3).map(|a| f64::from(pos[a] - base[a]).powi(2)).sum::<f64>().sqrt();
    if dist > 4.0 {
        return;
    }
    if !world.get(pos).is_some_and(|s| world.is_air(s)) {
        return;
    }
    for i in [-1, -2] {
        let test = [pos[0], pos[1] + i, pos[2]];
        if world.get(test).is_some_and(|s| world.is_air(s)) {
            pos = test;
        } else {
            break;
        }
    }
    set_if_air(world, pos, leaf);
    let t = rng.below(4);
    let h = rng.below(3) as i32 - 1;
    let coin = |rng: &mut Rng| rng.below(2) == 0;
    // Per case: the vine continues one way, a log may stand diagonally, a pumpkin goes beside it.
    let (next, log_at, pumpkin_at) = match t {
        0 => ([1, 0, 0], [1, h, -1], [0, 0, -1]),
        1 => ([0, 0, 1], [1, h, 0], [1, 0, 1]),
        2 => ([0, 0, -1], [-1, h, 0], [-1, 0, 1]),
        _ => ([-1, 0, 0], [-1, h, -1], [-1, 0, -1]),
    };
    if coin(rng) {
        garden_vine(world, base, add(pos, next), rng, [leaf, log, pumpkin]);
    }
    if coin(rng) {
        set_if_air(world, add(pos, log_at), log);
    }
    set_if_air(world, add(pos, pumpkin_at), pumpkin);
}
