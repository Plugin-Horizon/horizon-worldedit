//! `/snapshot list|use|sel|before|after` (`/snap`) and `//restore` (`/restore`): WorldEdit's `SnapshotCommands`,
//! `LegacySnapshotCommands` and `SnapshotUtilCommands`. The backups are read from `snapshots.directory`
//! (`crate::snapshot`).

use super::*;
use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::snapshot::{self, Snapshot};

const NOT_CONFIGURED: &str = "Snapshot/backup restore is not configured.";
const NOT_AVAILABLE: &str = "That snapshot does not exist or is not available.";

impl Engine {
    /// The folder of the backups, or the answer WorldEdit gives when there is none.
    fn snapshot_root(&self) -> Result<PathBuf, String> {
        let dir = self.config.snapshots_dir.trim();
        if dir.is_empty() {
            return Err(NOT_CONFIGURED.into());
        }
        Ok(PathBuf::from(dir))
    }

    /// The player's time zone: seconds east of UTC and the name they gave it (`UTC` until they set one).
    fn zone_of(&mut self, actor: &Actor) -> (i64, String) {
        match self.sessions.get(actor.uuid).timezone.clone() {
            Some((minutes, shown)) => (i64::from(minutes) * 60, shown),
            None => (0, "UTC".to_owned()),
        }
    }

    /// The snapshot a name picks, with WorldEdit's answer when it picks none.
    fn snapshot_named(&self, root: &Path, world: &dyn World, name: &str) -> Result<Snapshot, String> {
        snapshot::find(root, &world.name(), name).ok_or_else(|| NOT_AVAILABLE.to_owned())
    }

    /// What a world with no snapshots is told (the new database names the world, the old one does not).
    fn no_snapshots(&self, world: &dyn World, console: bool) -> String {
        match (self.config.snapshots_experimental, console) {
            (true, true) => format!("No snapshots were found for world '{}'.", world.name()),
            (true, false) => "No snapshots were found for this world.".to_owned(),
            (false, true) => "No snapshots were found. See console for details.".to_owned(),
            (false, false) => "No snapshots were found.".to_owned(),
        }
    }

    /// `/snapshot <list|use|sel|before|after> ...`.
    pub(super) fn snapshot(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        let sub = args.get(0).map(str::to_ascii_lowercase).unwrap_or_default();
        let root = self.snapshot_root()?;
        match sub.as_str() {
            "list" => {
                let snaps = snapshot::list(&root, &world.name());
                if snaps.is_empty() {
                    return Err(self.no_snapshots(world, true));
                }
                let page: usize = match args.value('p') {
                    None => 1,
                    Some(p) => p.parse().map_err(|_| number_error(p))?,
                };
                let pages = snaps.len().div_ceil(8);
                if page < 1 || page > pages {
                    return Err("Invalid page number".into());
                }
                let mut out = vec![format!("Snapshots for: {}", world.name())];
                out.extend(snaps.iter().enumerate().skip((page - 1) * 8).take(8).map(|(i, s)| format!("{}. {}", i + 1, s.id)));
                if pages > 1 {
                    out.push(format!("Page {page} of {pages}"));
                }
                Ok(out)
            }
            "use" => {
                let name = args.rest(1).trim_matches('"').to_owned();
                if name.is_empty() {
                    return Err("Usage: //snapshot use <snapshot>".into());
                }
                if name.eq_ignore_ascii_case("latest") {
                    if snapshot::list(&root, &world.name()).is_empty() {
                        return Err(self.no_snapshots(world, false));
                    }
                    self.sessions.get(actor.uuid).snapshot = None;
                    return Ok(vec!["Now using newest snapshot.".into()]);
                }
                let snap = self.snapshot_named(&root, world, &name)?;
                self.sessions.get(actor.uuid).snapshot = Some(snap.id.clone());
                Ok(vec![format!("Snapshot set to: {name}")])
            }
            "sel" => {
                let word = args.get(1).ok_or("Usage: //snapshot sel <index>")?;
                let index: i64 = word.parse().map_err(|_| number_error(word))?;
                if index < 1 {
                    return Err("Invalid index, must be greater than or equal to 1.".into());
                }
                let snaps = snapshot::list(&root, &world.name());
                if (snaps.len() as i64) < index {
                    return Err(format!("Invalid index, must be between 1 and {}.", snaps.len()));
                }
                let snap = &snaps[index as usize - 1];
                self.sessions.get(actor.uuid).snapshot = Some(snap.id.clone());
                Ok(vec![format!("Snapshot set to: {}", snap.id)])
            }
            "before" | "after" => {
                let before = sub == "before";
                let text = args.rest(1);
                if text.is_empty() {
                    return Err(format!("Usage: //snapshot {sub} <date>"));
                }
                let (offset, zone) = self.zone_of(actor);
                let date = snapshot::parse_user_date(&text, self.now_ms / 1000, offset).ok_or_else(|| format!("Not a date: {text}"))?;
                // Newest first: the first one before the date is the newest of those; the last one after it, the oldest.
                let snaps = snapshot::list(&root, &world.name());
                let found = if before { snaps.iter().find(|s| s.date < date) } else { snaps.iter().rev().find(|s| s.date > date) };
                match found {
                    Some(snap) => {
                        self.sessions.get(actor.uuid).snapshot = Some(snap.id.clone());
                        Ok(vec![format!("Snapshot set to: {}", snap.id)])
                    }
                    None => Err(format!("Couldn't find a snapshot {} {}.", if before { "before" } else { "after" }, snapshot::format_date(date, offset, &zone))),
                }
            }
            _ => Err("Usage: //snapshot <list|use|sel|before|after> ...".into()),
        }
    }

    /// `//restore [snapshot]`: the selection takes its blocks (and the block entity data) from a snapshot.
    pub(super) fn restore(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let root = self.snapshot_root()?;
        let c = self.region(world, actor)?;
        let snap = if args.words.is_empty() {
            match self.sessions.get(actor.uuid).snapshot.clone() {
                Some(id) => self.snapshot_named(&root, world, &id)?,
                None => snapshot::list(&root, &world.name()).into_iter().next().ok_or_else(|| self.no_snapshots(world, true))?,
            }
        } else {
            self.snapshot_named(&root, world, args.rest(0).trim_matches('"'))?
        };
        let loaded = format!("Snapshot '{}' loaded; now restoring...", snap.id);
        let shape = self.shape_of(actor);
        let positions = Self::shape_positions(&shape, &c);
        let mut ops: Vec<(Pos, StateId)> = Vec::new();
        let mut entities: HashMap<Pos, Tag> = HashMap::new();
        let mut known: HashMap<BlockState, Option<StateId>> = HashMap::new();
        let outcome = {
            let w: &dyn World = &*world;
            snapshot::restore(&snap, &actor.dimension, &positions, |p, state, entity| {
                let id = match known.get(state) {
                    Some(id) => *id,
                    None => {
                        let id = w.resolve(state);
                        known.insert(state.clone(), id);
                        id
                    }
                };
                // A block this game does not know is skipped, as WorldEdit skips a block it cannot place.
                if let Some(id) = id {
                    ops.push((p, id));
                    if let Some(tag) = entity {
                        entities.insert(p, tag.clone());
                    }
                }
            })
        };
        if outcome.total_failure() {
            let why = if outcome.missing > 0 {
                "Chunks were not present in snapshot.".to_owned()
            } else if let Some(e) = &outcome.last_error {
                format!("Errors prevented any blocks from being restored.\nLast error: {e}")
            } else {
                "No chunks could be loaded. (Bad archive?)".to_owned()
            };
            return Err(format!("{loaded}\n{why}"));
        }
        let (_, note) = self.commit_with(world, actor, ops, entities)?;
        Ok(vec![loaded, format!("Restored; {} missing chunks and {} other errors.{note}", outcome.missing, outcome.errors)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("we-snapcmd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A world folder whose chunk (0, 0) is stone from y 64 to 79, with a chest at (1, 64, 1).
    fn write_world(dir: &Path) {
        let compound = |pairs: Vec<(&str, Tag)>| Tag::Compound(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        let palette = Tag::List(vec![compound(vec![("Name", Tag::String("minecraft:stone".into()))])]);
        let section = compound(vec![("Y", Tag::Byte(4)), ("block_states", compound(vec![("palette", palette)]))]);
        let chest = compound(vec![("id", Tag::String("minecraft:chest".into())), ("x", Tag::Int(1)), ("y", Tag::Int(64)), ("z", Tag::Int(1))]);
        let chunk = compound(vec![("sections", Tag::List(vec![section])), ("block_entities", Tag::List(vec![chest]))]);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&crate::horizon_worldedit_core::nbt::write("", &chunk)).unwrap();
        let packed = z.finish().unwrap();
        let mut file = vec![0u8; 8192];
        file[0..4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
        file.extend_from_slice(&(packed.len() as u32 + 1).to_be_bytes());
        file.push(2);
        file.extend_from_slice(&packed);
        file.resize(3 * 4096, 0);
        std::fs::create_dir_all(dir.join("dimensions/minecraft/overworld/region")).unwrap();
        std::fs::write(dir.join("level.dat"), b"x").unwrap();
        std::fs::write(dir.join("dimensions/minecraft/overworld/region/r.0.0.mca"), file).unwrap();
    }

    fn actor() -> Actor {
        Actor { uuid: 1, name: "Builder".into(), permission: 4, dimension: "minecraft:overworld".into(), ..Actor::default() }
    }

    fn run(e: &mut Engine, w: &mut FakeWorld, line: &str) -> String {
        e.command(w, &actor(), line, &Env::default()).text()
    }

    #[test]
    fn nothing_works_until_a_directory_is_set() {
        let mut e = Engine::default();
        let mut w = FakeWorld::new();
        assert_eq!(run(&mut e, &mut w, "/snapshot list"), "Snapshot/backup restore is not configured.");
        assert_eq!(run(&mut e, &mut w, "//restore"), "Snapshot/backup restore is not configured.");
    }

    #[test]
    fn snapshots_are_listed_chosen_and_restored() {
        let root = scratch("flow");
        write_world(&root.join("2024-01-02_03-04-05/world"));
        write_world(&root.join("2024-06-07_08-09-10/world"));
        let mut e = Engine::default();
        e.config.snapshots_dir = root.to_string_lossy().into_owned();
        let mut w = FakeWorld::new();
        let list = run(&mut e, &mut w, "/snap list");
        assert_eq!(list, "Snapshots for: world\n1. 2024-06-07_08-09-10/world\n2. 2024-01-02_03-04-05/world");
        assert_eq!(run(&mut e, &mut w, "/snapshot sel 0"), "Invalid index, must be greater than or equal to 1.");
        assert_eq!(run(&mut e, &mut w, "/snapshot sel 3"), "Invalid index, must be between 1 and 2.");
        assert_eq!(run(&mut e, &mut w, "/snapshot sel 2"), "Snapshot set to: 2024-01-02_03-04-05/world");
        assert_eq!(run(&mut e, &mut w, "/snapshot use nothing"), "That snapshot does not exist or is not available.");
        assert_eq!(run(&mut e, &mut w, "/snapshot use latest"), "Now using newest snapshot.");
        assert_eq!(run(&mut e, &mut w, "/snapshot before 2024-03-01"), "Snapshot set to: 2024-01-02_03-04-05/world");
        assert_eq!(run(&mut e, &mut w, "/snapshot after 2024-03-01"), "Snapshot set to: 2024-06-07_08-09-10/world");
        assert_eq!(run(&mut e, &mut w, "/snapshot before 2023-01-01"), "Couldn't find a snapshot before 2023-01-01 00:00:00 UTC.");
        // Restore a 2 x 1 x 2 box (the chest's block and its neighbours) from the first snapshot.
        run(&mut e, &mut w, "/pos1 1,64,1");
        run(&mut e, &mut w, "/pos2 2,64,2");
        let stone = w.id("stone");
        let out = run(&mut e, &mut w, "//restore 2024-01-02_03-04-05/world");
        assert_eq!(out, "Snapshot '2024-01-02_03-04-05/world' loaded; now restoring...\nRestored; 0 missing chunks and 0 other errors.");
        assert_eq!(w.blocks.get(&[1, 64, 1]), Some(&stone));
        assert_eq!(w.blocks.len(), 4);
        assert!(w.entities.contains_key(&[1, 64, 1]));
        // Another chunk is not in the snapshot: nothing can be restored.
        run(&mut e, &mut w, "/pos1 100,64,100");
        run(&mut e, &mut w, "/pos2 101,64,101");
        assert!(run(&mut e, &mut w, "//restore").ends_with("Chunks were not present in snapshot."));
        // The restore is one edit.
        run(&mut e, &mut w, "//undo");
        assert_eq!(w.blocks.len(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
