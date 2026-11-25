//! A player's clipboard: blocks relative to the point they stood on when they copied, as
//! WorldEdit keeps it, so `//paste` puts the copy where they stand now the way it lay around them.

use crate::horizon_worldedit_core::block::{flip_y_state, mirror_state, rotate_state, BlockState, Mirror, Rotation};
use crate::horizon_worldedit_core::nbt::Tag;
use crate::horizon_worldedit_core::region::{add, Cuboid, Dir};
use crate::horizon_worldedit_core::world::{Pos, StateId, World};
use std::collections::HashMap;

/// An entity copied with `//copy -e`: its type, where it stood relative to the origin and its saved data.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipEntity {
    pub kind: String,
    pub pos: [f64; 3],
    pub nbt: Tag,
}

/// What a paste makes besides blocks, planned from the clipboard before the blocks go in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtrasPlan {
    pub mobs: Vec<(String, [f64; 3], Tag)>,
    pub biomes: Vec<(Pos, String)>,
    /// The (x, z) columns of the pasted biomes: WorldEdit's "biomes affected".
    pub columns: usize,
}

/// The biome cell of a clipboard position: biomes are kept per 4x4x4 cells, counted from the origin.
pub fn biome_cell(rel: Pos) -> Pos {
    [rel[0].div_euclid(4), rel[1].div_euclid(4), rel[2].div_euclid(4)]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clipboard {
    /// Every block of the copied box (air included), relative to the origin.
    pub blocks: Vec<(Pos, StateId)>,
    /// The box, relative to the origin.
    pub bounds: Cuboid,
    /// Where the origin was in the world (`//paste -o`), and in which level.
    pub origin: Option<(String, Pos)>,
    /// Block entity data (chest contents, sign text...) by position relative to the origin, with its `id`.
    pub entities: HashMap<Pos, Tag>,
    /// What is in it, for reports: "the copied region", "castle.schem (Sponge v3)".
    pub name: String,
    /// Entities copied with `-e` (never players), relative to the origin.
    pub mobs: Vec<ClipEntity>,
    /// Biomes copied with `-b`, by [`biome_cell`] of the position.
    pub biomes: HashMap<Pos, String>,
}

impl Clipboard {
    /// The blocks of `region` around `origin`; `None` when a chunk of it isn't loaded.
    pub fn copy(world: &dyn World, region: &Cuboid, origin: Pos, dimension: &str) -> Result<Self, String> {
        let mut blocks = Vec::with_capacity(region.volume() as usize);
        let mut entities = HashMap::new();
        let mut air: HashMap<StateId, bool> = HashMap::new();
        for p in region.positions() {
            let state = world.get(p).ok_or_else(|| format!("Part of the selection ({}, {}, {}) is not loaded", p[0], p[1], p[2]))?;
            let rel = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
            if !*air.entry(state).or_insert_with(|| world.is_air(state)) {
                if let Some(tag) = world.block_entity_at(p) {
                    entities.insert(rel, tag);
                }
            }
            blocks.push((rel, state));
        }
        Ok(Self {
            blocks,
            entities,
            bounds: region.shifted([-origin[0], -origin[1], -origin[2]]),
            origin: Some((dimension.to_owned(), origin)),
            name: "the copied region".into(),
            mobs: Vec::new(),
            biomes: HashMap::new(),
        })
    }
    /// Blocks laid out by a schematic: relative positions as the file places them.
    pub fn from_blocks(blocks: Vec<(Pos, StateId)>, name: String) -> Self {
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for (p, _) in &blocks {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        if blocks.is_empty() {
            lo = [0; 3];
            hi = [0; 3];
        }
        Self { blocks, entities: HashMap::new(), bounds: Cuboid::new(lo, hi), origin: None, name, mobs: Vec::new(), biomes: HashMap::new() }
    }
    /// The writes a paste at `at` makes: the blocks moved there, air left out with `skip_air`.
    pub fn writes(&self, world: &dyn World, at: Pos, skip_air: bool) -> Vec<(Pos, StateId)> {
        self.blocks
            .iter()
            .filter(|(_, s)| !(skip_air && world.is_air(*s)))
            .map(|&(p, s)| (add(at, p), s))
            .collect()
    }
    /// The non-air blocks turned `degrees` round the origin's vertical axis, for `//revolve`. Each block's
    /// relative position goes through WorldEdit's `rotateY` (x' = x cos + z sin, z' = -x sin + z cos) and is
    /// rounded to the nearest block. Block states turn only for whole quarter turns; other angles keep them.
    /// The block entity data goes with its block. Positions are relative to the origin.
    pub fn turned(&self, world: &dyn World, degrees: f64) -> (Vec<(Pos, StateId)>, HashMap<Pos, Tag>) {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let quarters = degrees / 90.0;
        let whole = quarters.round();
        let exact = (quarters - whole).abs() < 1e-9;
        let rotation = Rotation::from_quarter_turns(whole.rem_euclid(4.0) as u8);
        let mut turned_state: HashMap<StateId, StateId> = HashMap::new();
        let mut blocks = Vec::with_capacity(self.blocks.len());
        let mut entities = HashMap::new();
        for &(p, s) in &self.blocks {
            if world.is_air(s) {
                continue;
            }
            let (fx, fz) = (f64::from(p[0]), f64::from(p[2]));
            let q = [(fx * cos + fz * sin + 0.5).floor() as i32, p[1], (-fx * sin + fz * cos + 0.5).floor() as i32];
            let s = if exact {
                *turned_state.entry(s).or_insert_with(|| world.resolve(&rotate_state(&world.describe(s), rotation)).unwrap_or(s))
            } else {
                s
            };
            if let Some(tag) = self.entities.get(&p) {
                entities.insert(q, tag.clone());
            }
            blocks.push((q, s));
        }
        (blocks, entities)
    }
    /// The block entity data of a paste at `at` for the blocks `writes` place there.
    pub fn entities_for(&self, at: Pos, writes: &[(Pos, StateId)]) -> HashMap<Pos, Tag> {
        let mut out = HashMap::new();
        for &(p, _) in writes {
            if let Some(tag) = self.entities.get(&[p[0] - at[0], p[1] - at[1], p[2] - at[2]]) {
                out.insert(p, tag.clone());
            }
        }
        out
    }
    /// `//copy -e`: the entities of `world` that stand in `region` (and in `inside`, a shape's test), kept relative to
    /// `origin`. Players are never copied (the world leaves them out). The ids of the entities taken, for `//cut -e`.
    pub fn copy_mobs(&mut self, world: &mut dyn World, region: &Cuboid, origin: Pos, inside: &dyn Fn(Pos) -> bool) -> Vec<u64> {
        let min = region.min.map(f64::from);
        let max = region.max.map(|v| f64::from(v) + 1.0);
        let mut taken = Vec::new();
        for e in world.entities_data(min, max) {
            let block = e.pos.map(|v| v.floor() as i32);
            if !region.contains(block) || !inside(block) {
                continue;
            }
            let pos = [e.pos[0] - f64::from(origin[0]), e.pos[1] - f64::from(origin[1]), e.pos[2] - f64::from(origin[2])];
            let mut nbt = e.nbt;
            move_hanging(&mut nbt, |p| [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]]);
            self.mobs.push(ClipEntity { kind: e.kind, pos, nbt });
            taken.push(e.id);
        }
        taken
    }

    /// `//copy -b`: the biome of every [`biome_cell`] the region (and `inside`) reaches, read at the first block there.
    /// How many cells were kept.
    pub fn copy_biomes(&mut self, world: &dyn World, region: &Cuboid, origin: Pos, inside: &dyn Fn(Pos) -> bool) -> usize {
        for p in region.positions() {
            if !inside(p) {
                continue;
            }
            let cell = biome_cell([p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]]);
            if self.biomes.contains_key(&cell) {
                continue;
            }
            if let Some(biome) = world.biome_at(p) {
                self.biomes.insert(cell, biome);
            }
        }
        self.biomes.len()
    }

    /// The entities (`mobs`) and the biome cells (`biomes`) a paste at `at` makes, as asked by `-e` and `-b`.
    pub fn extras_plan(&self, at: Pos, mobs: bool, biomes: bool) -> ExtrasPlan {
        let mut plan = ExtrasPlan::default();
        if mobs {
            plan.mobs = self.mobs_at(at);
        }
        if biomes && !self.biomes.is_empty() {
            plan.biomes = self.biomes_at(at);
            plan.columns = self.blocks.iter().map(|(p, _)| (p[0], p[2])).collect::<std::collections::HashSet<_>>().len();
        }
        plan
    }

    /// The entities a paste at `at` creates: type, position in the world, and the saved data made fit for a new
    /// entity (no UUID, a hanging entity's block position moved with the paste).
    pub fn mobs_at(&self, at: Pos) -> Vec<(String, [f64; 3], Tag)> {
        self.mobs
            .iter()
            .map(|m| {
                let pos = [m.pos[0] + f64::from(at[0]), m.pos[1] + f64::from(at[1]), m.pos[2] + f64::from(at[2])];
                (m.kind.clone(), pos, entity_data_at(&m.nbt, at, pos))
            })
            .collect()
    }

    /// The biome cells a paste at `at` sets: for each world cell the pasted blocks reach, one position in it and the
    /// biome the clipboard has where the first block there came from.
    pub fn biomes_at(&self, at: Pos) -> Vec<(Pos, String)> {
        let mut seen: HashMap<Pos, ()> = HashMap::new();
        let mut out = Vec::new();
        for &(rel, _) in &self.blocks {
            let Some(biome) = self.biomes.get(&biome_cell(rel)) else { continue };
            let p = add(at, rel);
            if seen.insert([p[0] >> 2, p[1] >> 2, p[2] >> 2], ()).is_none() {
                out.push((p, biome.clone()));
            }
        }
        out
    }
    /// Turned `quarter_turns` times clockwise (seen from above) round the origin, block states
    /// turned with it.
    pub fn rotate(&mut self, world: &dyn World, quarter_turns: i32) {
        let r = Rotation::from_quarter_turns(quarter_turns.rem_euclid(4) as u8);
        if r == Rotation::None {
            return;
        }
        let turn = 90.0 * f32::from(r.quarter_turns());
        self.map(world, |p| r.apply(p), |s| rotate_state(s, r), |yaw| yaw + turn);
    }
    /// Mirrored across the origin along `dir`'s axis (`//flip`).
    pub fn flip(&mut self, world: &dyn World, dir: Dir) {
        match dir.axis() {
            0 => self.map(world, |p| Mirror::FrontBack.apply(p), |s| mirror_state(s, Mirror::FrontBack), |yaw| -yaw),
            2 => self.map(world, |p| Mirror::LeftRight.apply(p), |s| mirror_state(s, Mirror::LeftRight), |yaw| 180.0 - yaw),
            _ => self.map(world, |[x, y, z]| [x, -y, z], flip_y_state, |yaw| yaw),
        }
    }
    fn map(&mut self, world: &dyn World, at: impl Fn(Pos) -> Pos, state: impl Fn(&BlockState) -> BlockState, yaw: impl Fn(f32) -> f32) {
        let mut turned: HashMap<StateId, StateId> = HashMap::new();
        for (p, s) in &mut self.blocks {
            *p = at(*p);
            *s = *turned.entry(*s).or_insert_with(|| {
                // A state the game has no turned form for (a block whose property values don't
                // turn) stays as it was.
                world.resolve(&state(&world.describe(*s))).unwrap_or(*s)
            });
        }
        self.bounds = Cuboid::new(at(self.bounds.min), at(self.bounds.max));
        self.entities = std::mem::take(&mut self.entities).into_iter().map(|(p, t)| (at(p), t)).collect();
        // Entities move as points: the transform is linear about the origin block's centre.
        let origin = at([0, 0, 0]);
        let axis = |i: usize| {
            let mut unit = [0; 3];
            unit[i] = 1;
            let t = at(unit);
            [f64::from(t[0] - origin[0]), f64::from(t[1] - origin[1]), f64::from(t[2] - origin[2])]
        };
        let l = [axis(0), axis(1), axis(2)];
        for mob in &mut self.mobs {
            let c = [mob.pos[0] - 0.5, mob.pos[1] - 0.5, mob.pos[2] - 0.5];
            mob.pos = [0, 1, 2].map(|row| l[0][row] * c[0] + l[1][row] * c[1] + l[2][row] * c[2] + 0.5);
            set_yaw(&mut mob.nbt, &yaw);
            move_hanging(&mut mob.nbt, &at);
        }
        // A biome cell moves with its centre block.
        self.biomes = std::mem::take(&mut self.biomes).into_iter().map(|(cell, b)| (biome_cell(at([cell[0] * 4 + 2, cell[1] * 4 + 2, cell[2] * 4 + 2])), b)).collect();
    }
}

/// Moves the block position of a hanging entity (`block_pos`, or `TileX/Y/Z` in older data) through `f`.
fn move_hanging(nbt: &mut Tag, f: impl Fn(Pos) -> Pos) {
    let Tag::Compound(map) = nbt else { return };
    if let Some(Tag::IntArray(p)) = map.get_mut("block_pos") {
        if p.len() == 3 {
            let q = f([p[0], p[1], p[2]]);
            p.copy_from_slice(&q);
        }
    }
    if let (Some(Tag::Int(x)), Some(Tag::Int(y)), Some(Tag::Int(z))) = (map.get("TileX"), map.get("TileY"), map.get("TileZ")) {
        let q = f([*x, *y, *z]);
        for (key, v) in ["TileX", "TileY", "TileZ"].into_iter().zip(q) {
            map.insert(key.into(), Tag::Int(v));
        }
    }
}

/// Saved entity data for a new entity at `pos` after a paste at `at`: `UUID` dropped (the game gives a fresh one),
/// `Pos` set, a hanging entity's block position (kept relative in the clipboard) moved to the paste.
fn entity_data_at(nbt: &Tag, at: Pos, pos: [f64; 3]) -> Tag {
    let mut data = nbt.clone();
    if let Tag::Compound(map) = &mut data {
        map.remove("UUID");
        map.insert("Pos".into(), Tag::List(pos.iter().map(|v| Tag::Double(*v)).collect()));
    }
    move_hanging(&mut data, |p| add(at, p));
    data
}

/// Turns the `Rotation` tag's yaw (`[yaw, pitch]`) of saved entity data.
fn set_yaw(nbt: &mut Tag, yaw: &impl Fn(f32) -> f32) {
    let Tag::Compound(map) = nbt else { return };
    if let Some(Tag::List(rotation)) = map.get_mut("Rotation") {
        if let Some(Tag::Float(v)) = rotation.first_mut() {
            *v = yaw(*v).rem_euclid(360.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    #[test]
    fn copy_is_relative_to_the_player() {
        let mut w = FakeWorld::new();
        w.fill([10, 64, 10], [11, 64, 10], "stone");
        let c = Clipboard::copy(&w, &Cuboid::new([10, 64, 10], [11, 65, 10]), [10, 63, 12], "minecraft:overworld").unwrap();
        assert_eq!(c.blocks.len(), 4);
        assert_eq!(c.bounds, Cuboid::new([0, 1, -2], [1, 2, -2]));
        let writes = c.writes(&w, [0, 100, 0], true);
        assert_eq!(writes.len(), 2);
        assert!(writes.contains(&([1, 101, -2], w.id("stone"))));
        w.unloaded.push((0, 0));
        assert!(Clipboard::copy(&w, &Cuboid::new([1, 64, 1], [2, 64, 2]), [0, 0, 0], "x").is_err());
    }

    #[test]
    fn copy_and_paste_carry_block_entities() {
        let mut w = FakeWorld::new();
        w.fill([10, 64, 10], [11, 64, 10], "stone");
        let chest = Tag::Compound([("id".to_string(), Tag::String("minecraft:chest".into()))].into_iter().collect());
        w.entities.insert([11, 64, 10], chest.clone());
        w.entities.insert([10, 65, 10], chest.clone()); // air: no block entity can stand there
        let mut c = Clipboard::copy(&w, &Cuboid::new([10, 64, 10], [11, 65, 10]), [10, 64, 10], "d").unwrap();
        assert_eq!(c.entities, HashMap::from([([1, 0, 0], chest.clone())]));
        let writes = c.writes(&w, [100, 70, 0], false);
        assert_eq!(c.entities_for([100, 70, 0], &writes), HashMap::from([([101, 70, 0], chest.clone())]));
        c.rotate(&w, 1);
        assert_eq!(c.entities, HashMap::from([([0, 0, 1], chest.clone())]));
        let only_air: Vec<(Pos, StateId)> = vec![([100, 70, 0], 0)];
        assert!(c.entities_for([100, 70, 0], &only_air).is_empty());
    }

    #[test]
    fn rotate_and_flip_turn_states() {
        let w = FakeWorld::new();
        let stairs = w.id("oak_stairs[facing=north]");
        let log = w.id("oak_log[axis=x]");
        let mut c = Clipboard::from_blocks(vec![([2, 0, 0], stairs), ([0, 0, 1], log)], "t".into());
        c.rotate(&w, 1);
        assert!(c.blocks.contains(&([0, 0, 2], w.id("oak_stairs[facing=east]"))));
        assert!(c.blocks.contains(&([-1, 0, 0], w.id("oak_log[axis=z]"))));
        c.rotate(&w, -1);
        assert!(c.blocks.contains(&([2, 0, 0], stairs)));
        c.flip(&w, Dir::West);
        assert!(c.blocks.contains(&([-2, 0, 0], stairs)));
        let mut v = Clipboard::from_blocks(vec![([0, 3, 0], w.id("stone_slab[type=bottom]"))], "v".into());
        v.flip(&w, Dir::Up);
        assert_eq!(v.blocks, vec![([0, -3, 0], w.id("stone_slab[type=top]"))]);
    }
}
