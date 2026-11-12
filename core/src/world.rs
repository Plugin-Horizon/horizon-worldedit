//! What the editor needs from the level it edits. The game glue implements [`World`] over the
//! level in place (a dedicated server's, a LAN host's); the tests use [`fake::FakeWorld`].

use crate::block::BlockState;

pub type Pos = [i32; 3];

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
    /// Every block name of the game (`minecraft:stone`), for `//searchitem`. Empty where unknown.
    fn block_names(&self) -> Vec<String> {
        Vec::new()
    }
    /// The values a block's property takes, in the game's order, for the data cycler tool.
    fn property_values(&self, _block: &str, _property: &str) -> Vec<String> {
        Vec::new()
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
            Self { states, blocks: HashMap::new(), min_y: -64, max_y: 319, unloaded: Vec::new(), write_calls: 0, written: 0 }
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
        fn write(&mut self, writes: &[(Pos, StateId)]) -> usize {
            self.write_calls += 1;
            self.written += writes.len();
            for &(p, s) in writes {
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
