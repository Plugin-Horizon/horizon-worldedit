//! WorldEdit's legacy ids (`LegacyMapper`, `legacy.json`): the numeric `id:data` of the blocks of Minecraft 1.12, which the
//! expression variables `type` and `data` and the `query` functions speak, and `35:5`-style block names.

use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::world::{StateId, World};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// `(id, data, state text)` of every block of `legacy.json`, ordered by id and data.
fn entries() -> &'static Vec<(i32, i32, String)> {
    static ENTRIES: OnceLock<Vec<(i32, i32, String)>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct File {
            blocks: BTreeMap<String, String>,
        }
        let file: File = serde_json::from_str(include_str!("legacy.json")).unwrap_or(File { blocks: BTreeMap::new() });
        let mut out: Vec<(i32, i32, String)> = file
            .blocks
            .into_iter()
            .filter_map(|(key, value)| {
                let (id, data) = key.split_once(':')?;
                Some((id.parse().ok()?, data.parse().ok()?, value))
            })
            .collect();
        out.sort();
        out
    })
}

/// The legacy ids of the blocks one world knows.
pub struct Legacy {
    to_state: HashMap<(i32, i32), StateId>,
    from_state: HashMap<StateId, (i32, i32)>,
}

impl Legacy {
    /// Reads every legacy block through `world` (the states it has for them).
    pub fn build(world: &dyn World) -> Legacy {
        let mut to_state = HashMap::new();
        let mut from_state: HashMap<StateId, (i32, i32)> = HashMap::new();
        for (id, data, text) in entries() {
            let Some(state) = BlockState::parse(text).and_then(|s| world.resolve(&s)) else { continue };
            to_state.insert((*id, *data), state);
            from_state.entry(state).or_insert((*id, *data));
        }
        Legacy { to_state, from_state }
    }

    /// `LegacyMapper.getLegacyFromBlock`: the id and data of a state, if it has one.
    pub fn legacy_of(&self, state: StateId) -> Option<(i32, i32)> {
        self.from_state.get(&state).copied()
    }

    /// `LegacyMapper.getBlockFromLegacy`.
    pub fn state_of(&self, id: i32, data: i32) -> Option<StateId> {
        self.to_state.get(&(id, data)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    #[test]
    fn the_file_holds_the_blocks_of_1_12() {
        let all = entries();
        assert!(all.len() > 1600);
        assert!(all.iter().any(|(id, data, s)| (*id, *data) == (1, 0) && s == "minecraft:stone"));
        assert!(all.iter().any(|(id, data, s)| (*id, *data) == (1, 1) && s == "minecraft:granite"));
    }

    #[test]
    fn states_go_both_ways() {
        let w = FakeWorld::new();
        let l = Legacy::build(&w);
        let stone = w.id("stone");
        assert_eq!(l.legacy_of(stone), Some((1, 0)));
        assert_eq!(l.state_of(1, 0), Some(stone));
        assert_eq!(l.state_of(0, 0), Some(w.id("air")));
        assert_eq!(l.legacy_of(w.id("air")), Some((0, 0)));
        assert_eq!(l.state_of(9999, 0), None);
    }
}
