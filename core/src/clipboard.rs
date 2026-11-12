//! A player's clipboard: blocks relative to the point they stood on when they copied, as
//! WorldEdit keeps it, so `//paste` puts the copy where they stand now the way it lay around them.

use crate::block::{flip_y_state, mirror_state, rotate_state, BlockState, Mirror, Rotation};
use crate::region::{add, Cuboid, Dir};
use crate::world::{Pos, StateId, World};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Clipboard {
    /// Every block of the copied box (air included), relative to the origin.
    pub blocks: Vec<(Pos, StateId)>,
    /// The box, relative to the origin.
    pub bounds: Cuboid,
    /// Where the origin was in the world (`//paste -o`), and in which level.
    pub origin: Option<(String, Pos)>,
    /// What is in it, for reports: "the copied region", "castle.schem (Sponge v3)".
    pub name: String,
}

impl Clipboard {
    /// The blocks of `region` around `origin`; `None` when a chunk of it isn't loaded.
    pub fn copy(world: &dyn World, region: &Cuboid, origin: Pos, dimension: &str) -> Result<Self, String> {
        let mut blocks = Vec::with_capacity(region.volume() as usize);
        for p in region.positions() {
            let state = world.get(p).ok_or_else(|| format!("Part of the selection ({}, {}, {}) is not loaded", p[0], p[1], p[2]))?;
            blocks.push(([p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]], state));
        }
        Ok(Self {
            blocks,
            bounds: region.shifted([-origin[0], -origin[1], -origin[2]]),
            origin: Some((dimension.to_owned(), origin)),
            name: "the copied region".into(),
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
        Self { blocks, bounds: Cuboid::new(lo, hi), origin: None, name }
    }
    /// The writes a paste at `at` makes: the blocks moved there, air left out with `skip_air`.
    pub fn writes(&self, world: &dyn World, at: Pos, skip_air: bool) -> Vec<(Pos, StateId)> {
        self.blocks
            .iter()
            .filter(|(_, s)| !(skip_air && world.is_air(*s)))
            .map(|&(p, s)| (add(at, p), s))
            .collect()
    }
    /// Turned `quarter_turns` times clockwise (seen from above) round the origin, block states
    /// turned with it.
    pub fn rotate(&mut self, world: &dyn World, quarter_turns: i32) {
        let r = Rotation::from_quarter_turns(quarter_turns.rem_euclid(4) as u8);
        if r == Rotation::None {
            return;
        }
        self.map(world, |p| r.apply(p), |s| rotate_state(s, r));
    }
    /// Mirrored across the origin along `dir`'s axis (`//flip`).
    pub fn flip(&mut self, world: &dyn World, dir: Dir) {
        match dir.axis() {
            0 => self.map(world, |p| Mirror::FrontBack.apply(p), |s| mirror_state(s, Mirror::FrontBack)),
            2 => self.map(world, |p| Mirror::LeftRight.apply(p), |s| mirror_state(s, Mirror::LeftRight)),
            _ => self.map(world, |[x, y, z]| [x, -y, z], flip_y_state),
        }
    }
    fn map(&mut self, world: &dyn World, at: impl Fn(Pos) -> Pos, state: impl Fn(&BlockState) -> BlockState) {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::fake::FakeWorld;

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
