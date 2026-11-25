//! What the editor needs from the level it edits. The game glue implements [`World`] over the
//! level in place (a dedicated server's, a LAN host's); the tests use [`fake::FakeWorld`].

use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::nbt::Tag;

pub type Pos = [i32; 3];

/// An entity as `//butcher` and `//remove` see it.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityInfo {
    pub id: u64,
    /// `minecraft:cow`.
    pub kind: String,
    pub pos: [f64; 3],
    /// A living entity (a mob, an armor stand, a player).
    pub living: bool,
    /// It has a custom name (a name tag).
    pub named: bool,
    /// It has an owner (a tamed pet).
    pub tamed: bool,
}

/// An entity with its saved data, for `//copy -e`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityData {
    pub id: u64,
    /// `minecraft:cow`.
    pub kind: String,
    pub pos: [f64; 3],
    /// What `/summon` takes as data: the entity's saved compound.
    pub nbt: Tag,
}

/// A block state id, as the game's registry numbers them.
pub type StateId = u32;

pub trait World {
    /// The block at `p`, `None` where the chunk isn't loaded or `p` is outside the build height.
    /// An edit never loads a chunk.
    fn get(&self, p: Pos) -> Option<StateId>;
    /// The lowest and the highest buildable y (both inclusive).
    fn min_y(&self) -> i32;
    fn max_y(&self) -> i32;
    /// A state's name and every property.
    fn describe(&self, state: StateId) -> BlockState;
    /// A state from a name and some properties (the others take the block's defaults); `None` for
    /// a block this game doesn't have or a property it doesn't take.
    fn resolve(&self, state: &BlockState) -> Option<StateId>;
    fn is_air(&self, state: StateId) -> bool {
        self.describe(state).is_air()
    }
    /// Writes `writes` (each one differs from what is there), and says how many changed.
    fn write(&mut self, writes: &[(Pos, StateId)]) -> usize;
    /// The name of the world folder the level is saved in (`world`), which `/snapshot` looks its backups up by.
    fn name(&self) -> String {
        "world".to_owned()
    }
    /// Every block name of the game (`minecraft:stone`), for `//searchitem`. Empty where unknown.
    fn block_names(&self) -> Vec<String> {
        Vec::new()
    }
    /// The values a block's property takes, in the game's order, for the data cycler tool.
    fn property_values(&self, _block: &str, _property: &str) -> Vec<String> {
        Vec::new()
    }
    /// Every item name of the game (`minecraft:stone`), for `//searchitem`. Empty where unknown.
    fn item_names(&self) -> Vec<String> {
        Vec::new()
    }
    /// The saved data of the block entity at `p` (chest contents, sign text, a spawner's settings), with its
    /// `id` set to the type; `None` where there is none.
    fn block_entity_at(&self, _p: Pos) -> Option<Tag> {
        None
    }
    /// The entities whose box meets the box `min`..`max`. Empty where the game cannot say.
    fn entities_in(&mut self, _min: [f64; 3], _max: [f64; 3]) -> Vec<EntityInfo> {
        Vec::new()
    }
    /// The entities (never players) whose box meets the box `min`..`max`, with their saved data. Empty where the game cannot say.
    fn entities_data(&mut self, _min: [f64; 3], _max: [f64; 3]) -> Vec<EntityData> {
        Vec::new()
    }
    /// Creates an entity of `kind` at `pos` from saved data (`/summon`'s); its id, `None` where the game refuses.
    fn spawn_entity(&mut self, _kind: &str, _pos: [f64; 3], _nbt: &Tag) -> Option<u64> {
        None
    }
    /// The biome (`minecraft:plains`) at `p`; `None` where the game cannot say.
    fn biome_at(&self, _p: Pos) -> Option<String> {
        None
    }
    /// Sets the biome of the 4x4x4 cell that holds `p`; whether it changed.
    fn set_biome(&mut self, _p: Pos, _biome: &str) -> bool {
        false
    }
    /// Removes these entities (by `EntityInfo::id`) without drops; how many went.
    fn remove_entities(&mut self, _ids: &[u64]) -> usize {
        0
    }
    /// Loads saved data into the block entities of blocks just written (`id`, `x`, `y`, `z` in it are ignored).
    fn set_block_entities(&mut self, _entities: &[(Pos, Tag)]) {}
    /// Block entity data saved at `data_version` (a schematic's) brought up to the game's version.
    fn upgrade_block_entity(&self, _data_version: i32, data: Tag) -> Tag {
        data
    }
    /// Plants the configured feature `feature` (`minecraft:oak`, a tree or a giant mushroom) with its base at `p`, as
    /// `/place feature` does; whether it placed anything. Where the game cannot say, nothing is planted.
    fn generate_tree(&mut self, _p: Pos, _feature: &str) -> bool {
        false
    }
    /// Whether the block of `state` is in the block tag `tag` (`minecraft:logs`; `None` when the game cannot say).
    fn block_in_tag(&self, _state: StateId, _tag: &str) -> Option<bool> {
        None
    }
    /// The collision boxes of `state` in its own space (`[min x, y, z, max x, y, z]`), if the game can say.
    fn state_outline(&self, _state: StateId) -> Option<Vec<[f64; 6]>> {
        None
    }
    /// Generates the structure `structure` (a `minecraft:` structure id: a village, a shipwreck...) with its start at `p`, as
    /// `/place structure` does; whether it placed anything. Where the game cannot say, nothing is placed.
    fn generate_structure(&mut self, _p: Pos, _structure: &str) -> bool {
        false
    }
    /// Uses the item `item` on the block at `p` from the side `face` (`ItemUseFunction`); bone meal grows what it can. Whether
    /// something happened. The default does nothing.
    fn use_item(&mut self, _p: Pos, _item: &str, _face: [i32; 3]) -> bool {
        false
    }
    /// Applies the side effects `//update` names to the blocks at `positions`: `lighting` (the light is checked), `neighbors`
    /// (the neighbours are told) and `update` (the block's shape follows its neighbours). How many positions were visited.
    fn apply_side_effects(&mut self, _positions: &[Pos], _lighting: bool, _neighbors: bool, _update: bool) -> usize {
        0
    }
    /// Breaks the block at `p` the way a tool does: with its drops when `drop`, none otherwise. Whether a block broke.
    /// The default writes air and drops nothing.
    fn break_block(&mut self, p: Pos, _drop: bool) -> bool {
        let Some(air) = self.resolve(&BlockState::air()) else { return false };
        match self.get(p) {
            Some(s) if s != air => self.write(&[(p, air)]) > 0,
            _ => false,
        }
    }
    /// The stored block light at `p`, 0 to 15 (0 where unknown).
    fn block_light(&self, _p: Pos) -> i32 {
        0
    }
    /// Every biome id (`minecraft:plains`) of the level, for `/biomelist`. Empty where the game cannot say.
    fn biome_names(&self) -> Vec<String> {
        Vec::new()
    }
    /// Sets the biome of every 4x4x4 cell that holds a block of the box `min..=max` (inclusive corners); how many
    /// cells changed. The default sets the cells one by one; a game with a box call (one resend a column) overrides it.
    fn fill_biome(&mut self, min: Pos, max: Pos, biome: &str) -> Result<u32, String> {
        let mut changed = 0;
        for x in (min[0] >> 2)..=(max[0] >> 2) {
            for y in (min[1] >> 2)..=(max[1] >> 2) {
                for z in (min[2] >> 2)..=(max[2] >> 2) {
                    if self.set_biome([x << 2, y << 2, z << 2], biome) {
                        changed += 1;
                    }
                }
            }
        }
        Ok(changed)
    }
    /// Generates these loaded chunks (chunk x, z) again from the level's generator and seed, in place; how many were
    /// replaced. `Err` with the reason when the game refuses (no such API, a chunk not loaded, too many chunks).
    fn regenerate_chunks(&mut self, _chunks: &[[i32; 2]]) -> Result<u32, String> {
        Err("This game cannot regenerate chunks".into())
    }
    /// What the level's generator makes over the box `min..=max` under `seed` (None: the level's own), as it would
    /// make it in a new world, without touching the level: every block of the box, and with `biomes` the biome of
    /// each 4x4x4 cell it reaches (keyed by the cell's minimum corner). Chunks need not be loaded.
    fn generate_region(&mut self, _min: Pos, _max: Pos, _seed: Option<i64>, _biomes: bool) -> Result<Generated, String> {
        Err("This game cannot generate chunks on the side".into())
    }
    /// When the saved chunk (chunk x, z) was last written, in whole seconds since the Unix epoch; None for a chunk the
    /// save does not hold.
    fn saved_chunk_modified(&self, _chunk: [i32; 2]) -> Option<i64> {
        None
    }
    /// `delete_chunks`, after copying the region files that hold the chunks to the level's `backups` folder; nothing is
    /// erased when the copy fails.
    fn delete_chunks_with_backup(&mut self, _chunks: &[[i32; 2]]) -> Result<u32, String> {
        Err("This game cannot back up chunks before deleting them".into())
    }
    /// Erases these chunks (chunk x, z) from the level's saved data; how many were erased. Chunks that are loaded now
    /// are left alone (`Err` is for a refusal of the whole call).
    fn delete_chunks(&mut self, _chunks: &[[i32; 2]]) -> Result<u32, String> {
        Err("This game cannot delete chunks while the server runs".into())
    }
    /// The ids of the levels this server has (`minecraft:overworld`), for `//world`. Empty where the game cannot say.
    fn level_names(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The blocks and biome cells `World::generate_region` returns.
#[derive(Clone, Debug, Default)]
pub struct Generated {
    pub blocks: Vec<(Pos, StateId)>,
    /// The biome of each 4x4x4 cell, by the cell's minimum corner.
    pub biomes: std::collections::HashMap<Pos, String>,
}

impl Generated {
    /// The biome of the cell that holds `p`.
    pub fn biome_at(&self, p: Pos) -> Option<&String> {
        self.biomes.get(&[p[0] >> 2 << 2, p[1] >> 2 << 2, p[2] >> 2 << 2])
    }
}

pub mod fake {
    //! A world in memory for tests: a fixed list of block states, every chunk loaded.
    use super::*;
    use std::collections::HashMap;

    pub struct FakeWorld {
        pub states: Vec<BlockState>,
        pub blocks: HashMap<Pos, StateId>,
        pub min_y: i32,
        pub max_y: i32,
        /// Columns (chunk x, chunk z) that count as not loaded.
        pub unloaded: Vec<(i32, i32)>,
        /// Block entity data by position (a block written over loses its entry, as in the game).
        pub entities: HashMap<Pos, Tag>,
        /// Entities for `//butcher` and `//remove`.
        pub mobs: Vec<EntityInfo>,
        /// Entities with their data (`//copy -e`), and what `spawn_entity` made.
        pub mob_data: Vec<EntityData>,
        pub spawned: Vec<(String, [f64; 3], Tag)>,
        /// The feature ids `generate_tree` was asked for.
        pub planted: Vec<String>,
        /// The structures `generate_structure` was asked for, the items `use_item` was asked to use (with the block), and the
        /// positions `apply_side_effects` visited.
        pub structures: Vec<String>,
        pub used: Vec<(Pos, String)>,
        pub side_effects: Vec<(Pos, bool, bool, bool)>,
        /// Biomes by 4x4x4 cell (`p >> 2`); a cell not here is plains.
        pub biomes: HashMap<Pos, String>,
        /// How many `write` calls there were, and how many blocks they carried.
        pub write_calls: usize,
        pub written: usize,
    }

    impl Default for FakeWorld {
        fn default() -> Self {
            Self::new()
        }
    }

    impl FakeWorld {
        pub fn new() -> Self {
            let names = [
                "minecraft:air",
                "minecraft:stone",
                "minecraft:dirt",
                "minecraft:grass_block",
                "minecraft:glass",
                "minecraft:gold_block",
                "minecraft:water",
                "minecraft:gravel",
                "minecraft:oak_planks",
                "minecraft:sand",
                "minecraft:snow_block",
                "minecraft:ice",
                "minecraft:fire",
                "minecraft:grass_block",
                "minecraft:coarse_dirt",
                "minecraft:lava",
            ];
            let mut states: Vec<BlockState> = names.iter().map(|n| if *n == "minecraft:water" { BlockState::new(n, &[("level", "0")]) } else { BlockState::new(n, &[]) }).collect();
            for layers in 1..=8 {
                states.push(BlockState::new("minecraft:snow", &[("layers", &layers.to_string())]));
            }
            for facing in ["north", "east", "south", "west"] {
                for half in ["bottom", "top"] {
                    states.push(BlockState::new(
                        "minecraft:oak_stairs",
                        &[("facing", facing), ("half", half), ("shape", "straight"), ("waterlogged", "false")],
                    ));
                }
            }
            for axis in ["x", "y", "z"] {
                states.push(BlockState::new("minecraft:oak_log", &[("axis", axis)]));
            }
            for t in ["bottom", "top", "double"] {
                states.push(BlockState::new("minecraft:stone_slab", &[("type", t), ("waterlogged", "false")]));
            }
            Self { states, blocks: HashMap::new(), min_y: -64, max_y: 319, unloaded: Vec::new(), entities: HashMap::new(), mobs: Vec::new(), mob_data: Vec::new(), spawned: Vec::new(), planted: Vec::new(), structures: Vec::new(), used: Vec::new(), side_effects: Vec::new(), biomes: HashMap::new(), write_calls: 0, written: 0 }
        }
        pub fn id(&self, spec: &str) -> StateId {
            self.resolve(&BlockState::parse(spec).unwrap()).unwrap_or_else(|| panic!("no state {spec}"))
        }
        pub fn name_at(&self, p: Pos) -> String {
            self.describe(self.get(p).unwrap_or(0)).to_string_full()
        }
        pub fn count(&self, spec: &str) -> usize {
            let id = self.id(spec);
            self.blocks.values().filter(|&&s| s == id).count()
        }
        pub fn fill(&mut self, lo: Pos, hi: Pos, spec: &str) {
            let id = self.id(spec);
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        self.blocks.insert([x, y, z], id);
                    }
                }
            }
        }
    }

    impl World for FakeWorld {
        fn get(&self, p: Pos) -> Option<StateId> {
            if p[1] < self.min_y || p[1] > self.max_y || self.unloaded.contains(&(p[0] >> 4, p[2] >> 4)) {
                return None;
            }
            Some(self.blocks.get(&p).copied().unwrap_or(0))
        }
        fn min_y(&self) -> i32 {
            self.min_y
        }
        fn max_y(&self) -> i32 {
            self.max_y
        }
        fn describe(&self, state: StateId) -> BlockState {
            self.states.get(state as usize).cloned().unwrap_or_else(BlockState::air)
        }
        fn resolve(&self, state: &BlockState) -> Option<StateId> {
            // The first state of the block whose properties include the given ones: the
            // defaults come first in `states`, as the registry's default does.
            self.states
                .iter()
                .position(|s| s.matches(state) && state.properties.keys().all(|k| s.properties.contains_key(k)))
                .map(|i| i as StateId)
        }
        fn item_names(&self) -> Vec<String> {
            let mut names = self.block_names();
            names.push("minecraft:stick".into());
            names
        }
        fn block_names(&self) -> Vec<String> {
            let mut names: Vec<String> = self.states.iter().map(|s| s.name.clone()).collect();
            names.sort();
            names.dedup();
            names
        }
        fn property_values(&self, block: &str, property: &str) -> Vec<String> {
            let mut values: Vec<String> = Vec::new();
            for s in self.states.iter().filter(|s| s.name == block) {
                if let Some(v) = s.properties.get(property) {
                    if !values.contains(v) {
                        values.push(v.clone());
                    }
                }
            }
            values
        }
        fn block_entity_at(&self, p: Pos) -> Option<Tag> {
            self.entities.get(&p).cloned()
        }
        fn entities_in(&mut self, min: [f64; 3], max: [f64; 3]) -> Vec<EntityInfo> {
            self.mobs.iter().filter(|m| (0..3).all(|a| m.pos[a] >= min[a] && m.pos[a] <= max[a])).cloned().collect()
        }
        fn entities_data(&mut self, min: [f64; 3], max: [f64; 3]) -> Vec<EntityData> {
            self.mob_data.iter().filter(|m| (0..3).all(|a| m.pos[a] >= min[a] && m.pos[a] <= max[a])).cloned().collect()
        }
        fn spawn_entity(&mut self, kind: &str, pos: [f64; 3], nbt: &Tag) -> Option<u64> {
            self.spawned.push((kind.to_owned(), pos, nbt.clone()));
            let id = 1000 + self.spawned.len() as u64;
            self.mob_data.push(EntityData { id, kind: kind.to_owned(), pos, nbt: nbt.clone() });
            Some(id)
        }
        fn biome_at(&self, p: Pos) -> Option<String> {
            Some(self.biomes.get(&[p[0] >> 2, p[1] >> 2, p[2] >> 2]).cloned().unwrap_or_else(|| "minecraft:plains".to_owned()))
        }
        fn set_biome(&mut self, p: Pos, biome: &str) -> bool {
            let cell = [p[0] >> 2, p[1] >> 2, p[2] >> 2];
            let before = self.biome_at(p);
            self.biomes.insert(cell, biome.to_owned());
            before.as_deref() != Some(biome)
        }
        fn remove_entities(&mut self, ids: &[u64]) -> usize {
            let before = self.mobs.len() + self.mob_data.len();
            self.mobs.retain(|m| !ids.contains(&m.id));
            self.mob_data.retain(|m| !ids.contains(&m.id));
            before - self.mobs.len() - self.mob_data.len()
        }
        fn set_block_entities(&mut self, entities: &[(Pos, Tag)]) {
            for (p, t) in entities {
                self.entities.insert(*p, t.clone());
            }
        }
        fn block_in_tag(&self, state: StateId, tag: &str) -> Option<bool> {
            let name = self.describe(state).name;
            Some(match tag {
                "minecraft:logs" => name.ends_with("_log"),
                "minecraft:stairs" => name.ends_with("_stairs"),
                _ => false,
            })
        }
        fn generate_structure(&mut self, _p: Pos, structure: &str) -> bool {
            self.structures.push(structure.to_owned());
            structure != "minecraft:nothing"
        }
        fn use_item(&mut self, p: Pos, item: &str, _face: [i32; 3]) -> bool {
            self.used.push((p, item.to_owned()));
            true
        }
        fn apply_side_effects(&mut self, positions: &[Pos], lighting: bool, neighbors: bool, update: bool) -> usize {
            for &p in positions {
                self.side_effects.push((p, lighting, neighbors, update));
            }
            positions.len()
        }
        fn generate_tree(&mut self, p: Pos, feature: &str) -> bool {
            // A trunk of four logs with a cap of leaves, where the way is free; the feature id is only recorded.
            self.planted.push(feature.to_owned());
            if self.get(p).is_none_or(|s| s != 0) {
                return false;
            }
            let log = self.id("oak_log[axis=y]");
            for y in 0..4 {
                self.blocks.insert([p[0], p[1] + y, p[2]], log);
            }
            if let Some(leaves) = self.resolve(&BlockState::new("minecraft:oak_leaves", &[])) {
                for d in [[1, 3, 0], [-1, 3, 0], [0, 3, 1], [0, 3, -1], [0, 4, 0]] {
                    self.blocks.entry([p[0] + d[0], p[1] + d[1], p[2] + d[2]]).or_insert(leaves);
                }
            }
            true
        }
        fn write(&mut self, writes: &[(Pos, StateId)]) -> usize {
            self.write_calls += 1;
            self.written += writes.len();
            for &(p, s) in writes {
                if self.blocks.get(&p).copied().unwrap_or(0) != s {
                    self.entities.remove(&p);
                }
                if s == 0 {
                    self.blocks.remove(&p);
                } else {
                    self.blocks.insert(p, s);
                }
            }
            writes.len()
        }
    }
}
