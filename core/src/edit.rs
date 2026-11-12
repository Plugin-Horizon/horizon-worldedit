//! Turning what a command wants into writes: what was there (for `//undo`), only blocks that change,
//! and the column runs Horizon's bulk edit (`native_edit_blocks`) takes.

use crate::world::{Pos, StateId, World};
use std::collections::HashMap;

/// One edit, as `//undo` and `//redo` replay it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub dimension: String,
    /// Position, the block before, the block after.
    pub blocks: Vec<(Pos, StateId, StateId)>,
    /// When the edit was made (ms, the server's clock); `history.expiration` reads it.
    pub at_ms: i64,
}

impl Change {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
    /// The writes that make it.
    pub fn forward(&self) -> Vec<(Pos, StateId)> {
        self.blocks.iter().map(|&(p, _, new)| (p, new)).collect()
    }
    /// The writes that take it back, newest first.
    pub fn backward(&self) -> Vec<(Pos, StateId)> {
        self.blocks.iter().rev().map(|&(p, old, _)| (p, old)).collect()
    }
}

/// The edit `ops` make in `world`: a later op on the same position wins (`//move` clears, then
/// places), unloaded positions and blocks already right are left out.
pub fn plan(world: &dyn World, dimension: &str, ops: impl IntoIterator<Item = (Pos, StateId)>) -> Change {
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
    blocks.retain(|&(_, old, new)| old != new);
    Change { dimension: dimension.to_owned(), blocks, at_ms: 0 }
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
    done: usize,
}

impl Queue {
    pub fn push(&mut self, player: u128, dimension: &str, writes: Vec<(Pos, StateId)>) {
        self.jobs.push(Job { player, dimension: dimension.to_owned(), writes, done: 0 });
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
        let mut out = Vec::new();
        let mut finished = Vec::new();
        for job in self.jobs.iter_mut().filter(|j| j.dimension == dimension) {
            if out.len() >= budget {
                break;
            }
            let n = (budget - out.len()).min(job.writes.len() - job.done);
            out.extend_from_slice(&job.writes[job.done..job.done + n]);
            job.done += n;
            if job.done == job.writes.len() {
                finished.push(job.player);
            }
        }
        self.jobs.retain(|j| j.done < j.writes.len());
        (out, finished)
    }
    /// Drops everything (the world closed).
    pub fn clear(&mut self) {
        self.jobs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::fake::FakeWorld;

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
