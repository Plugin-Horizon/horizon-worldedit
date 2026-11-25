//! Entities (`-e`) and biomes (`-b`) in the clipboard: what `//copy` and `//cut` take with the blocks, what `//paste`
//! makes of them, and how `//undo` and `//redo` take them back and make them again (WorldEdit's `ForwardExtentCopy`
//! with `copyingEntities` and `copyingBiomes`, `EntityCreate` and the biome changes of a change set).

use super::*;
use crate::horizon_worldedit_core::clipboard::ExtrasPlan;
use crate::horizon_worldedit_core::edit::{Change, Extras, Spawned};
use crate::horizon_worldedit_core::selector::Shape;
use std::collections::HashSet;

impl Engine {
    /// `-e` and `-b` of `//copy` and `//cut`: puts the entities and the biomes of the copied region (cut to a shape's
    /// own blocks) into `clip`. Returns the columns of biomes and the ids of the entities taken.
    pub(super) fn copy_extras(&mut self, world: &mut dyn World, args: &Args, clip: &mut Clipboard, c: &Cuboid, origin: Pos, shape: &Option<Shape>) -> (usize, Vec<u64>) {
        let inside = |p: Pos| shape.as_ref().is_none_or(|s| s.contains(p));
        let (mut cols, mut mobs) = (0, Vec::new());
        if args.flag('b') {
            clip.copy_biomes(world, c, origin, &inside);
            // WorldEdit counts the (x, z) columns of the region as "biomes affected".
            cols = match shape {
                None => (c.size()[0].max(0) as usize) * (c.size()[2].max(0) as usize),
                Some(_) => c.positions().filter(|&p| inside(p)).map(|p| (p[0], p[2])).collect::<HashSet<_>>().len(),
            };
        }
        if args.flag('e') {
            mobs = clip.copy_mobs(world, c, origin, &inside);
        }
        (cols, mobs)
    }

    /// Makes the entities and biome cells of a paste's plan. Returns what `//undo` needs and the number of entities
    /// that came into the world.
    pub(super) fn paste_extras(world: &mut dyn World, plan: ExtrasPlan) -> (Extras, usize) {
        let mut extras = Extras::default();
        for (p, biome) in plan.biomes {
            let before = world.biome_at(p);
            if world.set_biome(p, &biome) {
                extras.biomes.push((p, before.unwrap_or_else(|| "minecraft:plains".to_owned()), biome));
            }
        }
        let mut mobs = 0;
        for (kind, pos, nbt) in plan.mobs {
            if let Some(id) = world.spawn_entity(&kind, pos, &nbt) {
                extras.spawned.push(Spawned { id, kind, pos, nbt });
                mobs += 1;
            }
        }
        (extras, mobs)
    }

    /// Keeps what a paste did besides blocks with that paste's history entry: on the edit just remembered when the
    /// paste changed blocks, else as an edit of its own.
    pub(super) fn remember_extras(&mut self, actor: &Actor, blocks_changed: usize, extras: Extras) {
        if extras.is_empty() {
            return;
        }
        let size = self.config.history_size;
        let at_ms = self.now_ms;
        let s = self.sessions.get(actor.uuid);
        if blocks_changed > 0 {
            if let Some(last) = s.undo.back_mut() {
                last.extras = extras;
                return;
            }
        }
        s.remember(Change { dimension: actor.dimension.clone(), at_ms, extras, ..Change::default() }, size);
    }

    /// `//undo` (`undo`) removes the entities an edit made and puts the biomes back; `//redo` makes them again.
    pub(super) fn replay_extras(world: &mut dyn World, change: &mut Change, undo: bool) {
        if undo {
            let ids: Vec<u64> = change.extras.spawned.iter().map(|s| s.id).collect();
            if !ids.is_empty() {
                world.remove_entities(&ids);
            }
            for (p, before, _) in change.extras.biomes.iter().rev() {
                world.set_biome(*p, before);
            }
        } else {
            for s in &mut change.extras.spawned {
                if let Some(id) = world.spawn_entity(&s.kind, s.pos, &s.nbt) {
                    s.id = id;
                }
            }
            for (p, _, after) in &change.extras.biomes {
                world.set_biome(*p, after);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;
    use crate::horizon_worldedit_core::world::EntityData;

    fn actor_at(position: [f64; 3]) -> Actor {
        Actor { uuid: 3, name: "Builder".into(), permission: 4, dimension: "minecraft:overworld".into(), position, ..Actor::default() }
    }

    fn run(e: &mut Engine, w: &mut FakeWorld, a: &Actor, line: &str) -> String {
        e.command(w, a, line, &Env::default()).text()
    }

    fn cow(id: u64, pos: [f64; 3]) -> EntityData {
        let doubles = Tag::List(pos.iter().map(|v| Tag::Double(*v)).collect());
        let nbt = Tag::Compound(
            [("UUID", Tag::IntArray(vec![1, 2, 3, 4])), ("Pos", doubles), ("Rotation", Tag::List(vec![Tag::Float(90.0), Tag::Float(0.0)]))].into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        );
        EntityData { id, kind: "minecraft:cow".into(), pos, nbt }
    }

    #[test]
    fn copy_takes_entities_and_biomes_and_paste_makes_them_and_undo_takes_them_back() {
        let mut e = Engine::default();
        let mut w = FakeWorld::new();
        w.mob_data.push(cow(5, [10.5, 64.0, 10.5]));
        w.mob_data.push(cow(6, [50.5, 64.0, 50.5]));
        // The biome of the cell holding (10, 64, 10).
        w.biomes.insert([2, 16, 2], "minecraft:desert".into());
        let here = actor_at([10.0, 64.0, 10.0]);
        run(&mut e, &mut w, &here, "/pos1 10,64,10");
        run(&mut e, &mut w, &here, "/pos2 11,64,11");
        // Without the flags only blocks are copied.
        assert_eq!(run(&mut e, &mut w, &here, "//copy"), "4 blocks affected\n0 biomes affected\n0 entities affected");
        assert_eq!(run(&mut e, &mut w, &here, "//copy -e -b"), "4 blocks affected\n4 biomes affected\n1 entities affected");
        // Paste elsewhere, without the flags first: no entity, no biome.
        let there = actor_at([30.0, 70.0, 30.0]);
        assert_eq!(run(&mut e, &mut w, &there, "//paste"), "The clipboard has been pasted at (30, 70, 30)\n4 blocks affected\n0 biomes affected\n0 entities affected");
        assert!(w.spawned.is_empty());
        assert_eq!(run(&mut e, &mut w, &there, "//paste -e -b"), "The clipboard has been pasted at (30, 70, 30)\n4 blocks affected\n4 biomes affected\n1 entities affected");
        assert_eq!(w.spawned.len(), 1);
        let (kind, pos, nbt) = &w.spawned[0];
        assert_eq!(kind, "minecraft:cow");
        assert_eq!(*pos, [30.5, 70.0, 30.5]);
        assert!(nbt.get("UUID").is_none(), "the new entity gets a UUID of its own");
        assert_eq!(nbt.get("Pos"), Some(&Tag::List(vec![Tag::Double(30.5), Tag::Double(70.0), Tag::Double(30.5)])));
        assert_eq!(w.biomes.get(&[7, 17, 7]).map(String::as_str), Some("minecraft:desert"));
        // `//undo` of the paste with entities removes the cow and the biome goes back.
        run(&mut e, &mut w, &there, "//undo");
        assert_eq!(w.mob_data.len(), 2);
        assert!(w.mob_data.iter().all(|m| m.id == 5 || m.id == 6));
        assert_eq!(w.biomes.get(&[7, 17, 7]).map(String::as_str), Some("minecraft:plains"));
        // `//redo` makes them again.
        run(&mut e, &mut w, &there, "//redo");
        assert_eq!(w.spawned.len(), 2);
        assert_eq!(w.biomes.get(&[7, 17, 7]).map(String::as_str), Some("minecraft:desert"));
    }

    #[test]
    fn a_turned_clipboard_turns_its_entities() {
        let mut e = Engine::default();
        let mut w = FakeWorld::new();
        // A cow one block east of the origin, looking along 90 degrees.
        w.mob_data.push(cow(5, [13.5, 64.0, 10.5]));
        let here = actor_at([10.0, 64.0, 10.0]);
        run(&mut e, &mut w, &here, "/pos1 10,64,10");
        run(&mut e, &mut w, &here, "/pos2 13,64,10");
        run(&mut e, &mut w, &here, "//copy -e");
        run(&mut e, &mut w, &here, "//rotate 90");
        run(&mut e, &mut w, &here, "//paste -e");
        // East of the origin (3.5, 0.5) a quarter turn clockwise puts it south of it: rotating the block (3, 0, 0) gives
        // (0, 0, 3), whose centre is (0.5, 0.5, 3.5) from the block's corner; the yaw turns with it.
        let (_, pos, nbt) = &w.spawned[0];
        assert_eq!(*pos, [10.5, 64.0, 13.5]);
        assert_eq!(nbt.get("Rotation"), Some(&Tag::List(vec![Tag::Float(180.0), Tag::Float(0.0)])));
    }
}
