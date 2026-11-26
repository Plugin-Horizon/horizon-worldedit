//! `//generate`, `//deform`, `/butcher` and `/remove` (WorldEdit's `GenerationCommands.generate`, `RegionCommands.deform`,
//! `UtilityCommands.butcher` and `remove`).

use super::expr::{Expr, ExprEnv};
use super::*;
use crate::horizon_worldedit_core::legacy::Legacy;
use std::cell::Cell;
use crate::horizon_worldedit_core::world::EntityInfo;

/// How the expression's x, y and z relate to the world (`TransformUtil.createTransformForExpressionCommand`).
struct Coords {
    center: [f64; 3],
    scale: [f64; 3],
}

impl Coords {
    fn new(raw: bool, offset_placement: bool, offset_center: bool, c: &Cuboid, placement: Pos) -> Coords {
        if raw {
            return Coords { center: [0.0; 3], scale: [1.0; 3] };
        }
        if offset_placement {
            return Coords { center: placement.map(f64::from), scale: [1.0; 3] };
        }
        let center = [0, 1, 2].map(|a| (f64::from(c.min[a]) + f64::from(c.max[a])) * 0.5);
        if offset_center {
            return Coords { center, scale: [1.0; 3] };
        }
        let scale = [0, 1, 2].map(|a| {
            let s = f64::from(c.max[a]) - center[a];
            if s == 0.0 {
                1.0
            } else {
                s
            }
        });
        Coords { center, scale }
    }
    /// From the world to the expression's coordinates.
    fn input(&self, p: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|a| (p[a] - self.center[a]) / self.scale[a])
    }
    /// From the expression's coordinates to the world.
    fn output(&self, v: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|a| v[a] * self.scale[a] + self.center[a])
    }
}

/// Compiles an expression whose provided variables are `known`.
fn compile(text: &str, known: &[&str]) -> Result<Expr, String> {
    Expr::parse_with(text, known)
}

/// The variables an expression takes from the caller: set (and kept between blocks, as WorldEdit's slot table keeps them).
fn set_vars(vars: &mut HashMap<String, f64>, coords: &Coords, p: Pos) {
    let v = coords.input(p.map(f64::from));
    vars.insert("x".to_owned(), v[0]);
    vars.insert("y".to_owned(), v[1]);
    vars.insert("z".to_owned(), v[2]);
}

/// `WorldEditExpressionEnvironment`: what `query`, `queryAbs` and `queryRel` read.
struct WorldEnv<'a> {
    world: &'a dyn World,
    coords: &'a Coords,
    legacy: Option<&'a Legacy>,
    current: Cell<Pos>,
}

impl ExprEnv for WorldEnv<'_> {
    fn query(&self, mode: u8, x: f64, y: f64, z: f64) -> (f64, f64) {
        let at: Pos = match mode {
            0 => self.coords.output([x, y, z]).map(|v| (v + 0.5).floor() as i32),
            1 => [x.floor() as i32, y.floor() as i32, z.floor() as i32],
            _ => {
                let c = self.current.get();
                [(f64::from(c[0]) + x).floor() as i32, (f64::from(c[1]) + y).floor() as i32, (f64::from(c[2]) + z).floor() as i32]
            }
        };
        match (self.legacy, self.world.get(at)) {
            (Some(l), Some(s)) => l.legacy_of(s).map_or((-1.0, -1.0), |(id, data)| (f64::from(id), f64::from(data))),
            _ => (-1.0, -1.0),
        }
    }
}

/// Whether the expression text may read legacy ids (`type`, `data`, `query...`).
fn wants_legacy(text: &str) -> bool {
    text.contains("type") || text.contains("data") || text.contains("query")
}

/// `ArbitraryShape.generate`: the material `get` gives at each position (`None` is outside the shape); with `hollow` only the
/// blocks that have a neighbour outside the shape remain (the neighbours may lie outside the region and are asked too).
fn arbitrary<T: Clone>(positions: &[Pos], hollow: bool, get: &mut dyn FnMut(Pos) -> Result<Option<T>, String>) -> Result<Vec<(Pos, T)>, String> {
    let mut out = Vec::new();
    if !hollow {
        for &p in positions {
            if let Some(m) = get(p)? {
                out.push((p, m));
            }
        }
        return Ok(out);
    }
    fn cached<T: Clone>(cache: &mut HashMap<Pos, Option<T>>, get: &mut dyn FnMut(Pos) -> Result<Option<T>, String>, p: Pos) -> Result<Option<T>, String> {
        if let Some(v) = cache.get(&p) {
            return Ok(v.clone());
        }
        let v = get(p)?;
        cache.insert(p, v.clone());
        Ok(v)
    }
    let mut cache: HashMap<Pos, Option<T>> = HashMap::new();
    for &p in positions {
        let Some(m) = cached(&mut cache, get, p)? else { continue };
        for d in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, 1, 0], [0, -1, 0]] {
            if cached(&mut cache, get, [p[0] + d[0], p[1] + d[1], p[2] + d[2]])?.is_none() {
                out.push((p, m));
                break;
            }
        }
    }
    Ok(out)
}

/// The kinds `//remove` takes (`EntityRemover.Type`).
fn remove_matches(word: &str, kind: &str) -> Option<bool> {
    let k = kind.trim_start_matches("minecraft:");
    let projectile = matches!(k, "arrow" | "spectral_arrow" | "trident" | "snowball" | "egg" | "ender_pearl" | "eye_of_ender" | "fireball" | "small_fireball" | "dragon_fireball" | "wither_skull" | "llama_spit" | "shulker_bullet" | "fishing_bobber" | "potion" | "splash_potion" | "lingering_potion" | "experience_bottle" | "wind_charge" | "breeze_wind_charge" | "firework_rocket");
    let by_type = |t: &str| match t {
        "projectiles" => projectile,
        "items" => k == "item",
        "falling" => k == "falling_block",
        "paintings" => k == "painting",
        "itemframes" => k == "item_frame" || k == "glow_item_frame",
        "boats" => k.ends_with("_boat") || k.ends_with("_raft") || k == "boat" || k == "chest_boat",
        "minecarts" => k.ends_with("minecart"),
        "tnt" => k == "tnt",
        _ => k == "experience_orb",
    };
    let w = word.to_ascii_lowercase();
    let t = match w.as_str() {
        "all" => return Some(["projectiles", "items", "falling", "paintings", "itemframes", "boats", "minecarts", "tnt", "xp"].iter().any(|t| by_type(t))),
        "projectile" | "projectiles" | "arrow" | "arrows" => "projectiles",
        "item" | "items" | "drop" | "drops" => "items",
        "fallingblock" | "fallingblocks" | "fallingsand" | "fallinggravel" => "falling",
        "painting" | "paintings" | "art" => "paintings",
        "itemframe" | "itemframes" | "frame" | "frames" => "itemframes",
        "boat" | "boats" => "boats",
        "minecart" | "minecarts" | "cart" | "carts" => "minecarts",
        "tnt" => "tnt",
        "xp" => "xp",
        _ => return None,
    };
    Some(by_type(t))
}

const ANIMALS: &[&str] = &["cow", "pig", "sheep", "chicken", "horse", "donkey", "mule", "llama", "trader_llama", "rabbit", "cat", "wolf", "fox", "ocelot", "panda", "polar_bear", "turtle", "axolotl", "frog", "goat", "camel", "sniffer", "mooshroom", "parrot", "strider", "armadillo", "happy_ghast", "skeleton_horse", "zombie_horse"];
const WATER: &[&str] = &["squid", "glow_squid", "cod", "salmon", "pufferfish", "tropical_fish", "dolphin", "tadpole"];

/// Whether a butcher brush with `flags` (`brushes::butcher_flags`) kills this entity (`CreatureButcher.createFunction`).
pub(super) fn butcher_kills(m: &EntityInfo, flags: u8) -> bool {
    use crate::horizon_worldedit_core::brushes::butcher_flags as f;
    let k = m.kind.trim_start_matches("minecraft:");
    m.living
        && k != "player"
        && (flags & f::ANIMALS != 0 || !ANIMALS.contains(&k))
        && (flags & f::PETS != 0 || !m.tamed)
        && (flags & f::GOLEMS != 0 || !k.ends_with("_golem"))
        && (flags & f::NPCS != 0 || !matches!(k, "villager" | "wandering_trader" | "zombie_villager_trader"))
        && (flags & f::AMBIENT != 0 || k != "bat")
        && (flags & f::TAGGED != 0 || !m.named)
        && (flags & f::ARMOR_STAND != 0 || k != "armor_stand")
        && (flags & f::WATER != 0 || !WATER.contains(&k))
}

impl Engine {
    fn expression_coords(&mut self, actor: &Actor, args: &Args, c: &Cuboid) -> Result<Coords, String> {
        let placement = self.placement(actor)?;
        Ok(Coords::new(args.flag('r'), args.flag('o'), args.flag('c'), c, placement))
    }

    /// The expression timeout of this player in milliseconds (`//timeout`, else `calculation.timeout`); negative for none.
    pub(super) fn timeout_of(&mut self, actor: &Actor) -> i64 {
        self.sessions.get(actor.uuid).timeout_ms.unwrap_or(self.config.calculation_timeout)
    }

    /// `//generate [-hroc] <pattern> <expression>`: `type` and `data` are the legacy id and data of the block the pattern gives
    /// for the position, and the shape's block is the legacy block they name afterwards (`EditSession.makeShape`).
    pub(super) fn generate(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        if args.words.len() < 2 {
            return Err(usage("generate"));
        }
        let pattern = self.pattern(world, actor, args.get(0))?;
        let text = args.rest(1);
        let expr = compile(&text, &["x", "y", "z", "type", "data"])?;
        let coords = self.expression_coords(actor, args, &c)?;
        let positions = Self::shape_positions(&self.shape_of(actor), &c);
        let (salt, timeout) = (self.salt(actor), self.timeout_of(actor));
        let legacy = wants_legacy(&text).then(|| Legacy::build(&*world));
        let env = WorldEnv { world: &*world, coords: &coords, legacy: legacy.as_ref(), current: Cell::new([0; 3]) };
        let mut vars: HashMap<String, f64> = HashMap::new();
        let mut timed_out = 0usize;
        let chosen = arbitrary(&positions, args.flag('h'), &mut |p| {
            env.current.set(p);
            let default = pattern.pick_with(&*world, salt, p);
            let (type_var, data_var) = legacy.as_ref().and_then(|l| l.legacy_of(default)).map_or((-1.0, -1.0), |(id, data)| (f64::from(id), f64::from(data)));
            set_vars(&mut vars, &coords, p);
            vars.insert("type".to_owned(), type_var);
            vars.insert("data".to_owned(), data_var);
            match expr.run_in(&mut vars, &env, Some(timeout)) {
                Ok(v) if v > 0.0 => {
                    let (new_type, new_data) = (vars["type"] as i32, vars["data"] as i32);
                    if f64::from(new_type) != type_var || f64::from(new_data) != data_var {
                        Ok(Some(legacy.as_ref().and_then(|l| l.state_of(new_type, new_data)).unwrap_or(default)))
                    } else {
                        Ok(Some(default))
                    }
                }
                Ok(_) => Ok(None),
                Err(e) if e.contains("exceeded time limit") => {
                    timed_out += 1;
                    Ok(None)
                }
                Err(e) => Err(e),
            }
        })?;
        let (n, note) = self.commit(world, actor, chosen)?;
        if timed_out > 0 {
            return Err(format!("{n} blocks changed. {timed_out} blocks took too long to evaluate (increase with //timeout)."));
        }
        Ok(vec![format!("{n} blocks have been created.{note}")])
    }

    /// `//generatebiome [-hroc] <biome> <expression>` (`//genbiome`, `//gb`): the biome of every block of the selection where
    /// the expression is above zero. Biomes are kept per 4x4x4 cell, so a cell changes when any of its blocks is chosen.
    pub(super) fn generate_biome(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        if args.words.len() < 2 {
            return Err(usage("generatebiome"));
        }
        let biome = super::terrain::biome_arg(world, args.get(0).unwrap_or_default())?;
        let text = args.rest(1);
        let expr = compile(&text, &["x", "y", "z"])?;
        let coords = self.expression_coords(actor, args, &c)?;
        let positions = Self::shape_positions(&self.shape_of(actor), &c);
        let timeout = self.timeout_of(actor);
        let legacy = wants_legacy(&text).then(|| Legacy::build(&*world));
        let env = WorldEnv { world: &*world, coords: &coords, legacy: legacy.as_ref(), current: Cell::new([0; 3]) };
        let mut vars: HashMap<String, f64> = HashMap::new();
        let mut timed_out = 0usize;
        let chosen = arbitrary(&positions, args.flag('h'), &mut |p| {
            env.current.set(p);
            set_vars(&mut vars, &coords, p);
            // A failing expression leaves the block out, as WorldEdit logs and goes on.
            match expr.run_in(&mut vars, &env, Some(timeout)) {
                Ok(v) if v > 0.0 => Ok(Some(())),
                Err(e) if e.contains("exceeded time limit") => {
                    timed_out += 1;
                    Ok(None)
                }
                _ => Ok(None),
            }
        })?;
        let mut extras = crate::horizon_worldedit_core::edit::Extras::default();
        let mut seen: std::collections::BTreeSet<Pos> = std::collections::BTreeSet::new();
        for (p, ()) in &chosen {
            let cell = [(p[0] >> 2) << 2, (p[1] >> 2) << 2, (p[2] >> 2) << 2];
            if seen.insert(cell) {
                let before = world.biome_at(cell);
                if world.set_biome(cell, &biome) {
                    extras.biomes.push((cell, before.unwrap_or_else(|| "minecraft:plains".to_owned()), biome.clone()));
                }
            }
        }
        self.remember_biomes(actor, &actor.dimension, extras);
        if timed_out > 0 {
            return Err(format!("{} biomes changed. {timed_out} biomes took too long to evaluate (increase time with //timeout)", chosen.len()));
        }
        Ok(vec![format!("{} biomes affected.", chosen.len())])
    }

    /// `//deform [-roc] [-l] <expression>`: with `-l` the blocks are fetched from the clipboard, in its own coordinate system.
    pub(super) fn deform(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        if args.words.is_empty() {
            return Err(usage("deform"));
        }
        let text = args.rest(0);
        let expr = compile(&text, &["x", "y", "z"])?;
        let coords = self.expression_coords(actor, args, &c)?;
        let positions = Self::shape_positions(&self.shape_of(actor), &c);
        let timeout = self.timeout_of(actor);
        // `-l`: the source is the clipboard, with a coordinate system made from its own box (`TransformUtil`, clipboard minimum
        // as the placement).
        let source: Option<(HashMap<Pos, StateId>, Coords, Cuboid)> = if args.flag('l') {
            let clip = self.sessions.get(actor.uuid).clipboard.as_ref().ok_or("Your clipboard is empty. Use //copy first.")?;
            let b = clip.bounds;
            let blocks: HashMap<Pos, StateId> = clip.blocks.iter().copied().collect();
            Some((blocks, Coords::new(args.flag('r'), args.flag('o'), args.flag('c'), &b, b.min), b))
        } else {
            None
        };
        let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
        let legacy = wants_legacy(&text).then(|| Legacy::build(&*world));
        let env = WorldEnv { world: &*world, coords: &coords, legacy: legacy.as_ref(), current: Cell::new([0; 3]) };
        let mut vars: HashMap<String, f64> = HashMap::new();
        let mut ops: Vec<(Pos, StateId)> = Vec::new();
        for p in positions {
            env.current.set(p);
            set_vars(&mut vars, &coords, p);
            expr.run_in(&mut vars, &env, Some(timeout))?;
            let out = [vars["x"], vars["y"], vars["z"]];
            match &source {
                Some((blocks, src_coords, bounds)) => {
                    let at = src_coords.output(out).map(|v| (v + 0.5).floor() as i32);
                    let state = if bounds.contains(at) { blocks.get(&at).copied().unwrap_or(air) } else { air };
                    ops.push((p, state));
                }
                None => {
                    let src = coords.output(out).map(|v| (v + 0.5).floor() as i32);
                    // A source that is not loaded or outside the world leaves the block as it is.
                    if let Some(s) = world.get(src) {
                        ops.push((p, s));
                    }
                }
            }
        }
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been deformed.{note}")])
    }

    /// `/brush deform <shape> [radius] [expression] [-r] [-o]`: the brush the item will carry.
    pub(super) fn deform_brush_shape(&mut self, actor: &Actor, args: &Args) -> Result<BrushShape, String> {
        let shape = Self::region_shape(args.get(1).ok_or_else(|| usage("brush deform"))?)?;
        let mut at = 2;
        let mut radius = 5.0;
        if let Some(w) = args.get(at).filter(|w| w.parse::<f64>().is_ok()) {
            radius = w.parse::<f64>().map_err(|_| number_error(w))?;
            at += 1;
        }
        let expression = if args.words.len() > at { args.rest(at) } else { "y-=0.2".to_owned() };
        compile(&expression, &["x", "y", "z"])?;
        self.check_brush_radius(radius)?;
        let (mode, offset) = if args.flag('r') {
            (1, [0; 3])
        } else if args.flag('o') {
            (2, self.placement(actor)?)
        } else {
            (0, [0; 3])
        };
        Ok(BrushShape::Deform { shape, radius, expression, mode, offset })
    }

    /// A click with a deform brush at `target`.
    pub(super) fn deform_brush_use(&mut self, world: &mut dyn World, actor: &Actor, brush: &BrushShape, target: Pos, mask: Option<&Mask>) -> Reply {
        let BrushShape::Deform { shape, radius, expression, mode, offset } = brush else { return Reply::default() };
        let (lo, hi) = (world.min_y(), world.max_y());
        let cells: Vec<Pos> = super::more::region_cells(target, *shape, *radius).into_iter().filter(|p| (lo..=hi).contains(&p[1])).collect();
        let Some(first) = cells.first().copied() else { return Reply::default() };
        let mut c = Cuboid::new(first, first);
        for p in &cells {
            c = Cuboid { min: [0, 1, 2].map(|a| c.min[a].min(p[a])), max: [0, 1, 2].map(|a| c.max[a].max(p[a])) };
        }
        let expr = match compile(expression, &["x", "y", "z"]) {
            Ok(e) => e,
            Err(e) => return Reply::err(e),
        };
        let coords = Coords::new(*mode == 1, *mode == 2, false, &c, *offset);
        let timeout = self.timeout_of(actor);
        let legacy = wants_legacy(expression).then(|| Legacy::build(&*world));
        let env = WorldEnv { world: &*world, coords: &coords, legacy: legacy.as_ref(), current: Cell::new([0; 3]) };
        let mut vars: HashMap<String, f64> = HashMap::new();
        let mut ops: Vec<(Pos, StateId)> = Vec::new();
        for p in cells {
            if mask.is_some_and(|m| !world.get(p).is_some_and(|s| m.hits_at(&*world, p, s))) {
                continue;
            }
            env.current.set(p);
            set_vars(&mut vars, &coords, p);
            if let Err(e) = expr.run_in(&mut vars, &env, Some(timeout)) {
                return Reply::err(e);
            }
            let out = coords.output([vars["x"], vars["y"], vars["z"]]);
            let src = out.map(|v| (v + 0.5).floor() as i32);
            if let Some(s) = world.get(src) {
                ops.push((p, s));
            }
        }
        match self.commit(world, actor, ops) {
            Ok(_) => Reply::default(),
            Err(e) if e.starts_with("Max blocks changed") => Reply::err("Max blocks change limit reached.".into()),
            Err(e) => Reply::err(e),
        }
    }

    /// The entities within `radius` of the placement (a cylinder of full height), or all loaded ones for `-1`.
    fn entities_around(&mut self, world: &mut dyn World, actor: &Actor, radius: i32) -> Result<Vec<EntityInfo>, String> {
        let at = self.placement(actor)?;
        if radius < 0 {
            return Ok(world.entities_in([-3.0e7, -4096.0, -3.0e7], [3.0e7, 4096.0, 3.0e7]));
        }
        let r = f64::from(radius);
        let all = world.entities_in([f64::from(at[0]) - r - 1.0, -4096.0, f64::from(at[2]) - r - 1.0], [f64::from(at[0]) + r + 2.0, 4096.0, f64::from(at[2]) + r + 2.0]);
        let inside = |m: &EntityInfo| {
            let b = [m.pos[0].floor() as i32, m.pos[2].floor() as i32];
            let (dx, dz) = (f64::from(b[0] - at[0]) / (r + 0.5), f64::from(b[1] - at[2]) / (r + 0.5));
            dx * dx + dz * dz <= 1.0
        };
        Ok(all.into_iter().filter(|m| inside(m)).collect())
    }

    /// `/butcher [-pnagbtfrw] [radius]`
    pub(super) fn butcher(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let mut radius = match args.get(0) {
            Some(w) => w.parse::<i32>().map_err(|_| number_error(w))?,
            None => self.config.butcher_default_radius,
        };
        if radius < -1 {
            return Err("Use -1 to remove all mobs in loaded chunks".into());
        }
        let max = self.config.butcher_max_radius;
        if radius == -1 && max != -1 {
            radius = max;
        }
        if max != -1 {
            radius = radius.min(max);
        }
        let friendly = args.flag('f');
        let allowed = |this: &Self, flag: char, node: &str| (args.flag(flag) || (friendly && "pnagbtw".contains(flag))) && this.holds(actor, node);
        let (pets, npcs, golems, animals, ambient, tagged, stands, water) = (
            allowed(self, 'p', "worldedit.butcher.pets"),
            allowed(self, 'n', "worldedit.butcher.npcs"),
            allowed(self, 'g', "worldedit.butcher.golems"),
            allowed(self, 'a', "worldedit.butcher.animals"),
            allowed(self, 'b', "worldedit.butcher.ambient"),
            allowed(self, 't', "worldedit.butcher.tagged"),
            allowed(self, 'r', "worldedit.butcher.armorstands"),
            allowed(self, 'w', "worldedit.butcher.water"),
        );
        if radius < 0 {
            // Paper's run: -1 (the default) works on the selection, and says so when there is none.
            let sess = self.sessions.get(actor.uuid);
            if sess.shape.is_none() && !(sess.pos1.is_some() && sess.pos2.is_some()) {
                return Err("Make a region selection first.".into());
            }
        }
        let found = self.entities_around(world, actor, radius)?;
        let ids: Vec<u64> = found
            .iter()
            .filter(|m| {
                let k = m.kind.trim_start_matches("minecraft:");
                m.living
                    && k != "player"
                    && (animals || !ANIMALS.contains(&k))
                    && (pets || !m.tamed)
                    && (golems || !k.ends_with("_golem"))
                    && (npcs || !matches!(k, "villager" | "wandering_trader" | "zombie_villager_trader"))
                    && (ambient || k != "bat")
                    && (tagged || !m.named)
                    && (stands || k != "armor_stand")
                    && (water || !WATER.contains(&k))
            })
            .map(|m| m.id)
            .collect();
        let killed = world.remove_entities(&ids);
        Ok(vec![format!("{killed} mobs have been killed in a radius of {radius}.")])
    }

    /// `/remove <type> <radius>`
    pub(super) fn remove_entities(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let (Some(word), Some(r)) = (args.get(0), args.get(1)) else { return Err(usage("remove")) };
        let radius = r.parse::<i32>().map_err(|_| number_error(r))?;
        if remove_matches(word, "minecraft:item").is_none() {
            return Err("Acceptable types: projectiles, items, paintings, itemframes, boats, minecarts, tnt, xp, or all".into());
        }
        if radius < -1 {
            return Err("Use -1 to remove all entities in loaded chunks".into());
        }
        let found = self.entities_around(world, actor, radius)?;
        let ids: Vec<u64> = found.iter().filter(|m| remove_matches(word, &m.kind) == Some(true)).map(|m| m.id).collect();
        let removed = world.remove_entities(&ids);
        Ok(vec![format!("{removed} entities have been marked for removal.")])
    }
}
