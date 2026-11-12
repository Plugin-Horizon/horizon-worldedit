//! `/tool ...`: WorldEdit's block tools that need nothing but the clicked block, and their clicks.
//!
//! As in WorldEdit a tool is bound to an item type, and it is the right click that acts (`actPrimary`)
//! and the left click that acts a second way (`actSecondary`). Not written, because each needs a hook
//! this server lacks: `tree` and `deltree` (tree generation, tree detection), `stacker` (the clicked
//! face), `navwand` (teleporting), `farwand` and `lrbuild` (a left click in the air).

use super::*;
use crate::block::BlockState;

impl Engine {
    /// `/tool <type> ...`: binds a tool to the item in the main hand, or unbinds it.
    pub(super) fn tool(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let kind = args.get(0).ok_or_else(|| usage("tool"))?.to_ascii_lowercase();
        if matches!(kind.as_str(), "none" | "unbind") {
            return self.unbind(actor, true);
        }
        let (tool, key) = match kind.as_str() {
            "selwand" | "/selwand" => (Tool::SelWand, "Selection wand"),
            "info" => (Tool::Info, "Info tool"),
            "cycler" => (Tool::Cycler { property: None }, "Block data cycler tool"),
            "repl" => {
                let pattern = self.need_pattern(world, actor, args, 1, "tool repl")?;
                (Tool::Replacer { pattern }, "Block replacer tool")
            }
            "floodfill" | "flood" => {
                let pattern = self.need_pattern(world, actor, args, 1, "tool floodfill")?;
                if args.get(2).is_none() {
                    return Err(usage("tool floodfill"));
                }
                let range = args.int(2, 0)?;
                if range > self.config.max_super_pickaxe_size {
                    return Err(format!("Maximum range is {}.", self.config.max_super_pickaxe_size));
                }
                (Tool::FloodFill { pattern, range }, "Block flood fill tool")
            }
            "tree" | "deltree" | "stacker" | "navwand" | "farwand" | "lrbuild" => {
                return Err(format!("The {kind} tool is not available on this server: it needs {}.", match kind.as_str() {
                    "tree" | "deltree" => "tree generation, which Horizon's server hooks do not offer yet",
                    "stacker" => "the face of the clicked block, which the click hook does not give",
                    "navwand" => "a teleport hook",
                    _ => "a hook for a left click in the air",
                }));
            }
            other => return Err(format!("The sub-command '{other}' under 'tool' could not be found.")),
        };
        let item = self.held_tool(actor)?;
        let s = self.sessions.get(actor.uuid);
        s.brushes.remove(&item);
        s.tools.insert(item.clone(), tool);
        Ok(vec![format!("{key} bound to {}.", block_title(&item)), "Run /tool none while holding the item to unbind it.".into()])
    }

    /// A left (`left`) or right click on the block at `pos` with an item that has a tool. `None` when
    /// it has none (or the tool does nothing on that click), which leaves the click to the game.
    pub fn tool_click(&mut self, world: &mut dyn World, actor: &Actor, left: bool, pos: Pos, now_ms: i64) -> Option<Reply> {
        self.now_ms = now_ms;
        let item = actor.held.clone()?;
        let tool = self.sessions.get(actor.uuid).tools.get(&item)?.clone();
        let node = match tool {
            Tool::SelWand => "worldedit.selection.pos",
            Tool::Info => "worldedit.tool.info",
            Tool::Cycler { .. } => "worldedit.tool.data-cycler",
            Tool::Replacer { .. } => "worldedit.tool.replacer",
            Tool::FloodFill { .. } => "worldedit.tool.flood-fill",
        };
        // Info, flood fill: the right click only; the left click digs as ever.
        if left && matches!(tool, Tool::Info | Tool::FloodFill { .. }) {
            return None;
        }
        if self.permitted(actor, Some(node)).is_err() || (self.config.creative_only && actor.game_mode != 1) {
            return None;
        }
        // A dig packet sent twice, and a right click asked once for each hand.
        if self.repeated(actor, pos, left, now_ms) {
            return Some(Reply::default());
        }
        let result: R = match tool {
            Tool::SelWand => match self.corner_set(world, actor, left, pos) {
                Ok(lines) => Ok(lines),
                Err(_) => Ok(Vec::new()),
            },
            Tool::Info => {
                let state = world.get(pos).map(|s| world.describe(s)).unwrap_or_else(BlockState::air);
                Ok(vec![format!("@{}: {} ({})", pt(pos), block_title(&state.name), state.to_string_full())])
            }
            Tool::Replacer { pattern } => {
                if left {
                    match world.get(pos) {
                        Some(state) => {
                            let described = world.describe(state);
                            let picked = Pattern::single(state, &described.to_string_full());
                            self.sessions.get(actor.uuid).tools.insert(item, Tool::Replacer { pattern: picked });
                            Ok(vec![format!("Replacer tool switched to: {}", block_title(&described.name))])
                        }
                        None => Ok(Vec::new()),
                    }
                } else {
                    let salt = self.salt(actor);
                    self.commit(world, actor, vec![(pos, pattern.pick(salt, pos))]).map(|_| Vec::new())
                }
            }
            Tool::Cycler { property } => self.cycle(world, actor, &item, property, !left, pos),
            Tool::FloodFill { pattern, range } => self.flood_fill(world, actor, &pattern, range, pos),
        };
        Some(match result {
            Ok(lines) => Reply::ok(lines),
            Err(e) if e.starts_with("Max blocks changed") => Reply::err("Max blocks change limit reached.".into()),
            Err(e) => Reply::err(e),
        })
    }

    /// `BlockDataCyler`: the right click sets the next value of the selected property, the left click
    /// selects the next property. (Properties go by name here; WorldEdit's go by the game's order.)
    fn cycle(&mut self, world: &mut dyn World, actor: &Actor, item: &str, selected: Option<String>, forward: bool, pos: Pos) -> R {
        let Some(state) = world.get(pos).map(|s| world.describe(s)) else { return Ok(Vec::new()) };
        let keys: Vec<String> = state.properties.keys().cloned().collect();
        if keys.is_empty() {
            return Err("That block's data cannot be cycled!".into());
        }
        let mut current = selected;
        if current.is_none() || (forward && current.as_ref().is_none_or(|c| !keys.contains(c))) {
            current = keys.first().cloned();
        }
        let current = current.unwrap_or_default();
        let remember = |this: &mut Self, property: &str| {
            this.sessions.get(actor.uuid).tools.insert(item.to_owned(), Tool::Cycler { property: Some(property.to_owned()) });
        };
        if forward {
            remember(self, &current);
            let values = world.property_values(&state.name, &current);
            if values.is_empty() {
                return Err("That block's data cannot be cycled!".into());
            }
            let at = state.properties.get(&current).and_then(|v| values.iter().position(|x| x == v)).unwrap_or(0);
            let next = values[(at + 1) % values.len()].clone();
            let mut changed = state.clone();
            changed.properties.insert(current.clone(), next.clone());
            let Some(id) = world.resolve(&changed) else { return Err("That block's data cannot be cycled!".into()) };
            self.commit(world, actor, vec![(pos, id)])?;
            Ok(vec![format!("Value of {current} is now {next}.")])
        } else {
            let at = keys.iter().position(|k| *k == current).map_or(0, |i| (i + 1) % keys.len());
            let property = keys[at].clone();
            remember(self, &property);
            Ok(vec![format!("Now cycling {property}.")])
        }
    }

    /// `FloodFillTool`: the blocks of the clicked block's type connected to it (six ways) within
    /// `range` of it become the pattern.
    fn flood_fill(&mut self, world: &mut dyn World, actor: &Actor, pattern: &Pattern, range: i32, origin: Pos) -> R {
        let Some(start) = world.get(origin) else { return Ok(Vec::new()) };
        if world.is_air(start) {
            return Ok(Vec::new());
        }
        let kind = world.describe(start).name;
        let mut seen: std::collections::HashSet<Pos> = std::collections::HashSet::from([origin]);
        let mut stack = vec![origin];
        let mut found = Vec::new();
        while let Some(p) = stack.pop() {
            found.push(p);
            if found.len() as u64 > SCAN_LIMIT {
                return Err(format!("The area is more than {SCAN_LIMIT} blocks; one edit handles at most that many"));
            }
            for d in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, 1, 0], [0, -1, 0]] {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if distance(origin, n) > f64::from(range) || !seen.insert(n) {
                    continue;
                }
                if world.get(n).is_some_and(|s| world.describe(s).name == kind) {
                    stack.push(n);
                }
            }
        }
        let salt = self.salt(actor);
        let ops = found.into_iter().map(|p| (p, pattern.pick(salt, p))).collect();
        self.commit(world, actor, ops)?;
        Ok(Vec::new())
    }
}
