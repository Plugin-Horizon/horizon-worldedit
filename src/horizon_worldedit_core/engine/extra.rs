//! `//generate`, `//deform`, `/butcher` and `/remove` (WorldEdit's `GenerationCommands.generate`, `RegionCommands.deform`,
//! `UtilityCommands.butcher` and `remove`).

use super::expr::Expr;
use super::*;
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

fn vars_at(c: &Coords, p: Pos) -> HashMap<String, f64> {
    let v = c.input(p.map(f64::from));
    let mut m = HashMap::new();
    m.insert("x".to_owned(), v[0]);
    m.insert("y".to_owned(), v[1]);
    m.insert("z".to_owned(), v[2]);
    m.insert("type".to_owned(), -1.0);
    m.insert("data".to_owned(), -1.0);
    m
}

fn compile(text: &str, known: &[&str]) -> Result<Expr, String> {
    let e = Expr::parse(text)?;
    if let Some(v) = e.unknown_variables(known).first() {
        return Err(format!("Variable '{v}' not found"));
    }
    Ok(e)
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

    /// The positions of `c` (inside the selection's shape) where `expr` is above zero; with `-h` only those with a
    /// neighbour where it is not (`ArbitraryShape.generate`).
    fn expression_hits(&mut self, world: &mut dyn World, actor: &Actor, args: &Args, c: &Cuboid, expr: &Expr) -> Result<Vec<Pos>, String> {
        let coords = self.expression_coords(actor, args, c)?;
        let shape = self.shape_of(actor);
        let inside = |p: Pos| -> Result<bool, String> {
            let mut vars = vars_at(&coords, p);
            Ok(expr.run(&mut vars)? > 0.0)
        };
        let _ = world;
        let mut chosen: Vec<Pos> = Vec::new();
        for p in Self::shape_positions(&shape, c) {
            if inside(p)? {
                chosen.push(p);
            }
        }
        if args.flag('h') {
            let set: std::collections::HashSet<Pos> = chosen.iter().copied().collect();
            let mut hollow = Vec::new();
            for &p in &chosen {
                for d in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, 1, 0], [0, -1, 0]] {
                    let q = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    // A neighbour outside the region is asked too, as the original's cache does.
                    let inside_q = if c.contains(q) { set.contains(&q) } else { inside(q)? };
                    if !inside_q {
                        hollow.push(p);
                        break;
                    }
                }
            }
            chosen = hollow;
        }
        Ok(chosen)
    }

    /// The expression timeout of this player in milliseconds (`//timeout`, else `calculation.timeout`); negative for none.
    pub(super) fn timeout_of(&mut self, actor: &Actor) -> i64 {
        self.sessions.get(actor.uuid).timeout_ms.unwrap_or(self.config.calculation_timeout)
    }

    /// `//generate [-hroc] <pattern> <expression>`
    pub(super) fn generate(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        if args.words.len() < 2 {
            return Err(usage("generate"));
        }
        let pattern = self.pattern(world, actor, args.get(0))?;
        let expr = compile(&args.rest(1), &["x", "y", "z", "type", "data"])?;
        let salt = self.salt(actor);
        let chosen = self.expression_hits(world, actor, args, &c, &expr)?;
        let ops: Vec<(Pos, StateId)> = chosen.into_iter().map(|p| (p, pattern.pick(salt, p))).collect();
        let (n, note) = self.commit(world, actor, ops)?;
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
        let expr = compile(&args.rest(1), &["x", "y", "z"])?;
        let chosen = self.expression_hits(world, actor, args, &c, &expr)?;
        let mut extras = crate::horizon_worldedit_core::edit::Extras::default();
        let mut seen: std::collections::BTreeSet<Pos> = std::collections::BTreeSet::new();
        for p in &chosen {
            let cell = [(p[0] >> 2) << 2, (p[1] >> 2) << 2, (p[2] >> 2) << 2];
            if seen.insert(cell) {
                let before = world.biome_at(cell);
                if world.set_biome(cell, &biome) {
                    extras.biomes.push((cell, before.unwrap_or_else(|| "minecraft:plains".to_owned()), biome.clone()));
                }
            }
        }
        self.remember_biomes(actor, &actor.dimension, extras);
        Ok(vec![format!("{} biomes affected.", chosen.len())])
    }

    /// `//deform [-roc] [-l] <expression>`: with `-l` the blocks are fetched from the clipboard, in its own coordinate system.
    pub(super) fn deform(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        if args.words.is_empty() {
            return Err(usage("deform"));
        }
        let expr = compile(&args.rest(0), &["x", "y", "z"])?;
        let coords = self.expression_coords(actor, args, &c)?;
        let shape = self.shape_of(actor);
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
        let mut ops: Vec<(Pos, StateId)> = Vec::new();
        for p in Self::shape_positions(&shape, &c) {
            let mut vars = vars_at(&coords, p);
            expr.run(&mut vars)?;
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
        let mut ops: Vec<(Pos, StateId)> = Vec::new();
        for p in cells {
            if mask.is_some_and(|m| !world.get(p).is_some_and(|s| m.hits(&*world, s))) {
                continue;
            }
            let mut vars = vars_at(&coords, p);
            if let Err(e) = expr.run(&mut vars) {
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
