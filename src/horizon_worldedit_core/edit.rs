//! Turning what a command wants into writes: what was there (for `//undo`), only blocks that change,
//! and the column runs Horizon's bulk edit (`native_edit_blocks`) takes.

use crate::horizon_worldedit_core::nbt::Tag;
use crate::horizon_worldedit_core::world::{Pos, StateId, World};
use std::collections::HashMap;

/// An entity an edit created (`//paste -e`), kept so `//undo` can remove it and `//redo` make it again.
#[derive(Clone, Debug, PartialEq)]
pub struct Spawned {
    pub id: u64,
    pub kind: String,
    pub pos: [f64; 3],
    pub nbt: Tag,
}

/// What an edit did besides blocks: the entities it created and the biome cells it set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extras {
    pub spawned: Vec<Spawned>,
    /// A position in the cell, the biome before and the biome after.
    pub biomes: Vec<(Pos, String, String)>,
}

impl Extras {
    pub fn is_empty(&self) -> bool {
        self.spawned.is_empty() && self.biomes.is_empty()
    }
}

/// One edit, as `//undo` and `//redo` replay it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub dimension: String,
    /// Position, the block before, the block after.
    pub blocks: Vec<(Pos, StateId, StateId)>,
    /// When the edit was made (ms, the server's clock); `history.expiration` reads it.
    pub at_ms: i64,
    /// Block entity data of the blocks before the edit, by position (`//undo` puts it back).
    pub entities_before: HashMap<Pos, Tag>,
    /// Block entity data the edit brought (`//redo` puts it back).
    pub entities_after: HashMap<Pos, Tag>,
    /// Entities and biomes the edit made.
    pub extras: Extras,
}

impl Change {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.extras.is_empty()
    }
    /// The writes that make it.
    pub fn forward(&self) -> Vec<(Pos, StateId)> {
        self.blocks.iter().map(|&(p, _, new)| (p, new)).collect()
    }
    /// The writes that take it back, newest first.
    pub fn backward(&self) -> Vec<(Pos, StateId)> {
        self.blocks.iter().rev().map(|&(p, old, _)| (p, old)).collect()
    }
    /// The block entity data that goes with [`Change::forward`].
    pub fn forward_entities(&self) -> HashMap<Pos, Tag> {
        self.entities_after.clone()
    }
    /// The block entity data that goes with [`Change::backward`].
    pub fn backward_entities(&self) -> HashMap<Pos, Tag> {
        self.entities_before.clone()
    }
}

/// The edit `ops` make in `world`: a later op on the same position wins (`//move` clears, then
/// places), unloaded positions and blocks already right are left out.
pub fn plan(world: &dyn World, dimension: &str, ops: impl IntoIterator<Item = (Pos, StateId)>) -> Change {
    plan_with(world, dimension, ops, HashMap::new())
}

/// [`plan`] for an edit that brings block entity data (`entities`, by position): a block already right is kept
/// when it brings data, and the data the blocks held before is remembered.
pub fn plan_with(world: &dyn World, dimension: &str, ops: impl IntoIterator<Item = (Pos, StateId)>, entities: HashMap<Pos, Tag>) -> Change {
    let mut index: HashMap<Pos, usize> = HashMap::new();
    let mut blocks: Vec<(Pos, StateId, StateId)> = Vec::new();
    for (p, new) in ops {
        if let Some(&i) = index.get(&p) {
            blocks[i].2 = new;
            continue;
        }
        let Some(old) = world.get(p) else { continue };
        index.insert(p, blocks.len());
        blocks.push((p, old, new));
    }
    blocks.retain(|&(p, old, new)| old != new || entities.contains_key(&p));
    let mut air: HashMap<StateId, bool> = HashMap::new();
    let mut entities_before = HashMap::new();
    for &(p, old, _) in &blocks {
        if *air.entry(old).or_insert_with(|| world.is_air(old)) {
            continue;
        }
        if let Some(tag) = world.block_entity_at(p) {
            entities_before.insert(p, tag);
        }
    }
    let entities_after = entities.into_iter().filter(|(p, _)| index.contains_key(p)).collect();
    Change { dimension: dimension.to_owned(), blocks, at_ms: 0, entities_before, entities_after, extras: Extras::default() }
}

/// A run of blocks up a column: `height` blocks from `y`, all palette entry `block`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub height: u32,
    pub block: u32,
}

/// `writes` as a palette and column runs: equal blocks stacked in a column become one run.
pub fn runs(writes: &[(Pos, StateId)]) -> (Vec<StateId>, Vec<Run>) {
    let mut sorted: Vec<(Pos, StateId)> = writes.to_vec();
    sorted.sort_by_key(|&(p, _)| (p[0], p[2], p[1]));
    sorted.dedup_by_key(|&mut (p, _)| p);
    let mut palette: Vec<StateId> = Vec::new();
    let mut slot: HashMap<StateId, u32> = HashMap::new();
    let mut out: Vec<Run> = Vec::new();
    for (p, s) in sorted {
        let block = *slot.entry(s).or_insert_with(|| {
            palette.push(s);
            palette.len() as u32 - 1
        });
        if let Some(last) = out.last_mut() {
            if last.x == p[0] && last.z == p[2] && last.block == block && last.y + last.height as i32 == p[1] {
                last.height += 1;
                continue;
            }
        }
        out.push(Run { x: p[0], z: p[2], y: p[1], height: 1, block });
    }
    (palette, out)
}

/// Writes that did not fit in one tick, made over the next ones.
#[derive(Debug, Default)]
pub struct Queue {
    jobs: Vec<Job>,
}

#[derive(Debug)]
struct Job {
    player: u128,
    dimension: String,
    writes: Vec<(Pos, StateId)>,
    /// Block entity data still to load, by position.
    entities: HashMap<Pos, Tag>,
    done: usize,
}

impl Queue {
    pub fn push(&mut self, player: u128, dimension: &str, writes: Vec<(Pos, StateId)>) {
        self.push_with(player, dimension, writes, HashMap::new());
    }
    pub fn push_with(&mut self, player: u128, dimension: &str, writes: Vec<(Pos, StateId)>, entities: HashMap<Pos, Tag>) {
        self.jobs.push(Job { player, dimension: dimension.to_owned(), writes, entities, done: 0 });
    }
    /// Whether an edit of `player` is still being written.
    pub fn busy(&self, player: u128) -> bool {
        self.jobs.iter().any(|j| j.player == player)
    }
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
    /// The next `budget` writes for `dimension`, oldest job first, and the players whose jobs that
    /// finished.
    pub fn take(&mut self, dimension: &str, budget: usize) -> (Vec<(Pos, StateId)>, Vec<u128>) {
        let (out, _, finished) = self.take_with(dimension, budget);
        (out, finished)
    }
    /// [`Queue::take`], and the block entity data of the blocks taken.
    pub fn take_with(&mut self, dimension: &str, budget: usize) -> (Vec<(Pos, StateId)>, Vec<(Pos, Tag)>, Vec<u128>) {
        let mut out = Vec::new();
        let mut entities = Vec::new();
        let mut finished = Vec::new();
        for job in self.jobs.iter_mut().filter(|j| j.dimension == dimension) {
            if out.len() >= budget {
                break;
            }
            let n = (budget - out.len()).min(job.writes.len() - job.done);
            out.extend_from_slice(&job.writes[job.done..job.done + n]);
            for &(p, _) in &job.writes[job.done..job.done + n] {
                if let Some(tag) = job.entities.remove(&p) {
                    entities.push((p, tag));
                }
            }
            job.done += n;
            if job.done == job.writes.len() {
                finished.push(job.player);
            }
        }
        self.jobs.retain(|j| j.done < j.writes.len());
        (out, entities, finished)
    }
    /// Drops everything (the world closed).
    pub fn clear(&mut self) {
        self.jobs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    #[test]
    fn plan_skips_unchanged_and_merges() {
        let mut w = FakeWorld::new();
        let stone = w.id("stone");
        let dirt = w.id("dirt");
        w.fill([0, 0, 0], [0, 0, 0], "stone");
        let c = plan(&w, "d", vec![([0, 0, 0], stone), ([1, 0, 0], 0), ([2, 0, 0], stone), ([2, 0, 0], dirt), ([3, 0, 0], stone), ([3, 0, 0], 0)]);
        assert_eq!(c.blocks, vec![([2, 0, 0], 0, dirt)]);
        assert_eq!(c.backward(), vec![([2, 0, 0], 0)]);
        w.unloaded.push((0, 0));
        assert!(plan(&w, "d", vec![([5, 0, 5], stone)]).is_empty());
    }

    #[test]
    fn plan_remembers_block_entities() {
        let mut w = FakeWorld::new();
        let stone = w.id("stone");
        let dirt = w.id("dirt");
        w.fill([0, 0, 0], [1, 0, 0], "stone");
        let chest = Tag::Compound([("id".to_string(), Tag::String("minecraft:chest".into()))].into_iter().collect());
        w.entities.insert([0, 0, 0], chest.clone());
        // Position 0 loses its chest; position 1 is stone already but is given data.
        let c = plan_with(&w, "d", vec![([0, 0, 0], dirt), ([1, 0, 0], stone)], HashMap::from([([1, 0, 0], chest.clone()), ([9, 9, 9], chest.clone())]));
        assert_eq!(c.blocks.len(), 2);
        assert_eq!(c.entities_before, HashMap::from([([0, 0, 0], chest.clone())]));
        assert_eq!(c.forward_entities(), HashMap::from([([1, 0, 0], chest.clone())]));
        assert_eq!(c.backward_entities(), HashMap::from([([0, 0, 0], chest)]));
    }

    #[test]
    fn queue_hands_out_block_entities_with_their_blocks() {
        let mut q = Queue::default();
        let tag = Tag::Byte(1);
        q.push_with(1, "a", (0..4).map(|i| ([i, 0, 0], 1)).collect(), HashMap::from([([0, 0, 0], tag.clone()), ([3, 0, 0], tag.clone())]));
        let (w, e, done) = q.take_with("a", 2);
        assert_eq!((w.len(), e, done), (2, vec![([0, 0, 0], tag.clone())], vec![]));
        let (_, e, done) = q.take_with("a", 2);
        assert_eq!((e, done), (vec![([3, 0, 0], tag)], vec![1]));
    }

    #[test]
    fn column_runs() {
        let writes = vec![([0, 3, 0], 7), ([0, 1, 0], 7), ([0, 2, 0], 7), ([0, 4, 0], 9), ([1, 1, 0], 7)];
        let (palette, runs) = runs(&writes);
        assert_eq!(palette, vec![7, 9]);
        assert_eq!(
            runs,
            vec![
                Run { x: 0, z: 0, y: 1, height: 3, block: 0 },
                Run { x: 0, z: 0, y: 4, height: 1, block: 1 },
                Run { x: 1, z: 0, y: 1, height: 1, block: 0 },
            ]
        );
    }

    #[test]
    fn queue_spreads_over_ticks() {
        let mut q = Queue::default();
        q.push(1, "a", (0..5).map(|i| ([i, 0, 0], 1)).collect());
        q.push(2, "b", vec![([0, 0, 0], 1)]);
        assert!(q.busy(1) && q.busy(2));
        let (w, done) = q.take("a", 3);
        assert_eq!((w.len(), done), (3, vec![]));
        let (w, done) = q.take("a", 3);
        assert_eq!((w.len(), done), (2, vec![1]));
        assert!(!q.busy(1) && q.busy(2));
        assert_eq!(q.take("b", 10).1, vec![2]);
        assert!(q.is_empty());
    }
}
