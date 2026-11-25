//! WorldEdit's tools: `/tool <type>` and what each does on a click, the brush settings (`/mask`, `/material`,
//! `/range`, `/size`, `/tracemask`) and the super pickaxe (`//`, `/,`, `/sp single|area|recursive`). The navigation
//! wand's clicks are in `nav.rs` and `Engine::nav_use`.
//!
//! As in WorldEdit a block tool acts on a right click on a block (`actPrimary`); the tools with two actions
//! (selection wand, replacer, cycler) use the left click for the other one. The trace tools (the far wand, the
//! long-range build tool, a brush) look along the player's view (`TargetBlock`, 0.2 steps from 1.65 above the
//! feet) and use `/range` and `/tracemask`. The super pickaxe is the left click with a pickaxe, whatever the
//! item is bound to.
//!
//! A left click in the air reaches the mod as an arm swing (`player_arm_swing`), which `swing_click` answers for
//! the far wand and the long-range build tool. The info tool prints no internal id and no legacy id (the game's
//! state ids are not ours, and there is no 1.12 table here).

use super::*;
use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::edit::{plan, Change};
use crate::horizon_worldedit_core::session::{BrushOpts, Pickaxe};
use std::collections::{HashSet, VecDeque};

/// `BrushTool.MAX_RANGE`.
pub(super) const MAX_RANGE: i32 = 500;
const NO_BRUSH: &str = "You have no brush bound to your current item. Try /brush sphere for a basic brush.";
const UNBIND_TOOL: &str = "Run /tool unbind while holding the item to unbind it.";

// ---- reading the view ----

/// `TargetBlock`: the walk along the view, a block at a time.
struct Trace {
    at: [f64; 3],
    cur: Pos,
    prev: Pos,
    dist: f64,
    max: f64,
    offset: [f64; 3],
}

const CHECK: f64 = 0.2;

fn floor3(v: [f64; 3]) -> Pos {
    v.map(|c| c.floor() as i32)
}

impl Trace {
    fn new(actor: &Actor, max_distance: i32) -> Self {
        let rot_x = (f64::from(actor.yaw) + 90.0) % 360.0;
        let rot_y = -f64::from(actor.pitch);
        let h = CHECK * rot_y.to_radians().cos();
        let offset = [h * rot_x.to_radians().cos(), CHECK * rot_y.to_radians().sin(), h * rot_x.to_radians().sin()];
        let at = [actor.position[0], actor.position[1] + 1.65, actor.position[2]];
        let cur = floor3(at);
        Self { at, cur, prev: cur, dist: 0.0, max: f64::from(max_distance), offset }
    }
    /// `getNextBlock`: the next block along the view; `None` past the distance.
    fn next(&mut self) -> Option<Pos> {
        self.prev = self.cur;
        loop {
            self.dist += CHECK;
            for a in 0..3 {
                self.at[a] += self.offset[a];
            }
            self.cur = floor3(self.at);
            if !(self.dist <= self.max && self.cur == self.prev) {
                break;
            }
        }
        if self.dist > self.max { None } else { Some(self.cur) }
    }
    fn current(&self) -> Option<Pos> {
        if self.dist > self.max { None } else { Some(self.cur) }
    }
}

/// `TargetBlock.getAnyTargetBlock` (`any`: the last block seen when nothing stops the trace) or `getTargetBlock`.
/// `stop` says which blocks stop it.
pub(super) fn target_block(world: &dyn World, actor: &Actor, max_distance: i32, stop: &dyn Fn(Pos) -> bool, any: bool) -> Option<Pos> {
    let mut t = Trace::new(actor, max_distance);
    if any {
        let mut last = None;
        while let Some(p) = t.next() {
            if stop(p) {
                break;
            }
            last = t.current();
            if let Some(l) = last {
                if l[1] < world.min_y() || l[1] > world.max_y() {
                    return None;
                }
            }
        }
        t.current().or(last)
    } else {
        while let Some(p) = t.next() {
            if stop(p) {
                break;
            }
        }
        t.current()
    }
}

// ---- trees ----

fn is_leaves(name: &str) -> bool {
    name.ends_with("_leaves")
}

/// `BlockCategories.LOGS`: every log and wood block, and the nether stems and hyphae.
fn is_log(name: &str) -> bool {
    let n = name.trim_start_matches("minecraft:");
    n.ends_with("_log")
        || n.ends_with("_wood")
        || matches!(
            n,
            "crimson_stem" | "warped_stem" | "stripped_crimson_stem" | "stripped_warped_stem" | "crimson_hyphae" | "warped_hyphae" | "stripped_crimson_hyphae" | "stripped_warped_hyphae"
        )
}

/// `FloatingTreeRemover.isTreeBlock`.
fn is_tree_block(name: &str) -> bool {
    is_leaves(name) || is_log(name) || matches!(name, "minecraft:red_mushroom_block" | "minecraft:brown_mushroom_block" | "minecraft:mushroom_stem" | "minecraft:vine")
}

/// The configured feature a tree type names, as `level_generate_tree` takes it. A registry id (`minecraft:oak`, `oak`,
/// `oak_checked`) goes as it is; the old WorldEdit words (`tree`, `bigtree`, `redwood`, `birch`, ...) map to the vanilla
/// feature Paper's `TreeType` used for them. `worldedit:pine` and `worldedit:random` are WorldEdit's own.
fn tree_feature(word: &str) -> Result<String, String> {
    let w = word.to_ascii_lowercase();
    let mapped = match w.as_str() {
        "tree" | "oak" | "regular" => "minecraft:oak",
        "big_tree" | "bigtree" | "bigoak" | "big" | "largeoak" => "minecraft:fancy_oak",
        "redwood" | "spruce" | "sequoia" | "sequoioideae" => "minecraft:spruce",
        "tallredwood" | "tall_redwood" | "bigspruce" | "tallspruce" | "tallsequoia" | "tallsequoioideae" => "minecraft:pine",
        "megaredwood" | "mega_redwood" | "largespruce" => "minecraft:mega_spruce",
        "white" | "whitebark" => "minecraft:birch",
        "tallbirch" | "tall_birch" => "minecraft:super_birch_bees_0002",
        "jungle" => "minecraft:mega_jungle_tree",
        "smalljungle" | "small_jungle" | "shortjungle" => "minecraft:jungle_tree_no_vine",
        "junglebush" | "jungle_bush" | "jungleshrub" => "minecraft:jungle_bush",
        "redmushroom" | "red_mushroom" | "redgiantmushroom" => "minecraft:huge_red_mushroom",
        "brownmushroom" | "brown_mushroom" | "browngiantmushroom" => "minecraft:huge_brown_mushroom",
        "crimsonfungus" | "rednethermushroom" => "minecraft:crimson_fungus",
        "warpedfungus" | "greennethermushroom" => "minecraft:warped_fungus",
        "swamptree" => "minecraft:swamp_oak",
        "darkoak" => "minecraft:dark_oak",
        "chorusplant" => "minecraft:chorus_plant",
        other => {
            let id = other.strip_suffix("_checked").unwrap_or(other);
            let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.' | '/' | ':'));
            if !ok || id.matches(':').count() > 1 {
                return Err(format!("Not a valid tree type: {word}"));
            }
            return Ok(if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") });
        }
    };
    Ok(mapped.to_owned())
}

/// The trees `worldedit:random` picks among.
const RANDOM_TREES: [&str; 12] = [
    "minecraft:oak",
    "minecraft:fancy_oak",
    "minecraft:spruce",
    "minecraft:pine",
    "minecraft:birch",
    "minecraft:jungle_tree_no_vine",
    "minecraft:swamp_oak",
    "minecraft:acacia",
    "minecraft:dark_oak",
    "minecraft:mega_spruce",
    "minecraft:cherry",
    "minecraft:mangrove",
];

/// A small generator for the pine tree and the random tree (WorldEdit uses `Math.random()`).
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// `AbstractPlayerActor.isHoldingPickAxe`.
fn is_pickaxe(item: &str) -> bool {
    matches!(
        item,
        "minecraft:iron_pickaxe" | "minecraft:wooden_pickaxe" | "minecraft:stone_pickaxe" | "minecraft:diamond_pickaxe" | "minecraft:golden_pickaxe" | "minecraft:netherite_pickaxe"
    )
}

/// `BooleanConverter`.
fn parse_bool(word: &str) -> Result<bool, String> {
    match word.to_ascii_lowercase().as_str() {
        "off" | "f" | "false" | "n" | "no" => Ok(false),
        "on" | "t" | "true" | "y" | "yes" => Ok(true),
        _ => Err(format!("Not a boolean value: {word}")),
    }
}

/// The name of a pattern's block for `/tool lrbuild`'s message: a single block, else `pattern`.
fn pattern_name(world: &dyn World, p: &Pattern) -> String {
    if p.entries.len() == 1 && !p.text.contains(',') && !p.text.contains('%') {
        block_title(&world.describe(p.entries[0].1).name)
    } else {
        "pattern".to_owned()
    }
}

/// `SelectionWand`-style left and right, for the tools that are looked at: `true` when the tool looks along the view.
fn is_trace_tool(tool: &Tool) -> bool {
    matches!(tool, Tool::FarWand | Tool::LrBuild { .. })
}

impl Engine {
    // ---- `/tool` ----

    /// `/tool <type> ...`: binds a tool to the item in the main hand, or unbinds it.
    pub(super) fn tool(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let kind = args.get(0).ok_or_else(|| usage("tool"))?.to_ascii_lowercase();
        if matches!(kind.as_str(), "none" | "unbind") {
            return self.unbind(actor, true);
        }
        let mut extra: Vec<String> = Vec::new();
        let (tool, key) = match kind.as_str() {
            "selwand" | "/selwand" => (Tool::SelWand, "Selection wand"),
            "info" => (Tool::Info, "Info tool"),
            "cycler" => (Tool::Cycler { property: None }, "Block data cycler tool"),
            "repl" => {
                let pattern = self.need_pattern(world, actor, args, 1, "tool repl")?;
                (Tool::Replacer { pattern }, "Block replacer tool")
            }
            "farwand" => (Tool::FarWand, "Far wand tool"),
            "navwand" | "/navwand" => (Tool::NavWand, "Navigation wand"),
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
            "tree" => {
                let feature = tree_feature(args.get(1).unwrap_or("tree"))?;
                (Tool::Tree { kind: feature }, "Tree tool")
            }
            "deltree" => (Tool::DelTree, "Floating tree remover tool"),
            "stacker" => {
                let range = args.int(1, 10)?;
                let mask = if args.get(2).is_some() { args.rest(2) } else { "!#existing".to_owned() };
                Mask::parse(world, &mask, actor.held.as_deref())?;
                (Tool::Stacker { range, mask }, "Stack tool")
            }
            "lrbuild" | "/lrbuild" => {
                let primary = self.need_pattern(world, actor, args, 1, "tool lrbuild")?;
                let secondary = self.need_pattern(world, actor, args, 2, "tool lrbuild")?;
                extra.push(format!("Left-click set to {}; right-click set to {}.", pattern_name(&*world, &primary), pattern_name(&*world, &secondary)));
                (Tool::LrBuild { primary, secondary }, "Long-range building tool")
            }
            other => return Err(format!("The sub-command '{other}' under 'tool' could not be found.")),
        };
        let item = self.held_tool(actor)?;
        if world.block_names().iter().any(|b| *b == item) {
            return Err(format!("Can't bind tool to {}: Blocks can't be used.", block_title(&item)));
        }
        let s = self.sessions.get(actor.uuid);
        s.brushes.remove(&item);
        s.brush_opts.remove(&item);
        s.tools.insert(item.clone(), tool);
        let mut lines = vec![format!("{key} bound to {}.", block_title(&item)), UNBIND_TOOL.to_owned()];
        lines.extend(extra);
        Ok(lines)
    }

    // ---- brush settings ----

    /// The item in the hand when it has a brush, a far wand or a long-range build tool (WorldEdit's `BrushTool`s).
    fn brush_like(&mut self, actor: &Actor) -> Result<String, String> {
        let item = actor.held.clone().unwrap_or_default();
        let s = self.sessions.get(actor.uuid);
        if s.brushes.contains_key(&item) || s.tools.get(&item).is_some_and(is_trace_tool) {
            Ok(item)
        } else {
            Err(NO_BRUSH.into())
        }
    }

    /// `/mask [mask]`: the brush mask; no argument turns it off.
    pub(super) fn mask(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let text = args.rest(0);
        if !text.is_empty() {
            Mask::parse(world, &text, actor.held.as_deref())?;
        }
        let item = self.brush_like(actor)?;
        let new = if text.is_empty() { None } else { Some(text) };
        let msg = if new.is_none() { "Brush mask disabled." } else { "Brush mask set." };
        let s = self.sessions.get(actor.uuid);
        if let Some(brush) = s.brushes.get_mut(&item) {
            brush.mask = new;
        } else {
            s.brush_opts.entry(item).or_default().mask = new;
        }
        Ok(vec![msg.into()])
    }

    /// `/material <pattern>`: the brush's pattern.
    pub(super) fn material(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let pattern = self.need_pattern(world, actor, args, 0, "material")?;
        let item = self.brush_like(actor)?;
        let s = self.sessions.get(actor.uuid);
        if let Some(brush) = s.brushes.get_mut(&item) {
            brush.pattern = Some(pattern);
        } else {
            s.brush_opts.entry(item).or_default().material = Some(pattern);
        }
        Ok(vec!["Brush material set.".into()])
    }

    /// `/range <range>`: how far the brush (or far wand, long-range build tool) reaches; negative means the most (500).
    pub(super) fn range(&mut self, actor: &Actor, args: &Args) -> R {
        if args.get(0).is_none() {
            return Err(usage("range"));
        }
        let range = args.int(0, 0)?;
        let item = self.brush_like(actor)?;
        self.sessions.get(actor.uuid).brush_opts.entry(item).or_default().range = Some(range);
        Ok(vec!["Brush range set.".into()])
    }

    /// `/size <size>`: the brush radius (`WorldEdit.checkMaxBrushRadius` first).
    pub(super) fn brush_size(&mut self, actor: &Actor, args: &Args) -> R {
        if args.get(0).is_none() {
            return Err(usage("size"));
        }
        let size = args.int(0, 0)?;
        let max = self.config.max_brush_radius;
        if max > 0.0 && f64::from(size) > max {
            return Err(format!("Maximum brush radius (in configuration): {}", max as i64));
        }
        let item = self.brush_like(actor)?;
        self.sessions.get(actor.uuid).brush_opts.entry(item).or_default().size = Some(f64::from(size));
        Ok(vec!["Brush size set.".into()])
    }

    /// `/tracemask [mask]`: the blocks that stop the brush's trace; no argument turns it off.
    pub(super) fn trace_mask(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let text = args.rest(0);
        if !text.is_empty() {
            Mask::parse(world, &text, actor.held.as_deref())?;
        }
        let item = self.brush_like(actor)?;
        let new = if text.is_empty() { None } else { Some(text) };
        let msg = if new.is_none() { "Trace mask disabled." } else { "Trace mask set." };
        self.sessions.get(actor.uuid).brush_opts.entry(item).or_default().trace_mask = new;
        Ok(vec![msg.into()])
    }

    /// The settings of the item's tool.
    fn opts_of(&mut self, actor: &Actor, item: &str) -> BrushOpts {
        self.sessions.get(actor.uuid).brush_opts.get(item).cloned().unwrap_or_default()
    }

    /// `/size` for a brush: the radius it was bound with is replaced (the brush's own height stays).
    pub(crate) fn apply_size(&mut self, actor: &Actor, item: &str, shape: &mut BrushShape) {
        let Some(size) = self.opts_of(actor, item).size else { return };
        match shape {
            BrushShape::Sphere { radius, .. }
            | BrushShape::Cylinder { radius, .. }
            | BrushShape::Smooth { radius, .. }
            | BrushShape::SnowSmooth { radius, .. }
            | BrushShape::Splatter { radius, .. }
            | BrushShape::Gravity { radius, .. }
            | BrushShape::Region { radius, .. }
            | BrushShape::Snow { radius, .. }
            | BrushShape::Morph { radius, .. }
            | BrushShape::Butcher { radius, .. }
            | BrushShape::Heightmap { radius, .. }
            | BrushShape::Terrain { radius, .. }
            | BrushShape::Apply { radius, .. }
            | BrushShape::Deform { radius, .. } => *radius = size,
            _ => {}
        }
    }

    /// The block a trace tool looks at (`getBlockTrace`): `/range` and `/tracemask` of the item. With a range set the trace may
    /// end on the last block of the range (`useLastBlock`); without, `last` says whether it may (a brush) or only a hit counts.
    /// `Ok(None)` when nothing is there.
    fn trace_hit(&mut self, world: &dyn World, actor: &Actor, item: &str, last: bool) -> Result<Option<Pos>, String> {
        let opts = self.opts_of(actor, item);
        let mask = match opts.trace_mask.as_deref() {
            Some(text) => Some(Mask::parse(world, text, None)?),
            None => None,
        };
        let (distance, any) = match opts.range {
            Some(r) if r > -1 => (r.min(MAX_RANGE), true),
            _ => (MAX_RANGE, last),
        };
        let stop = |p: Pos| match world.get(p) {
            Some(s) => match &mask {
                Some(m) => m.hits(world, s),
                None => !world.is_air(s),
            },
            None => false,
        };
        Ok(target_block(world, actor, distance, &stop, any))
    }

    /// Where a brush works: the clicked block, unless the item has a range or a trace mask, which only a trace honours.
    pub(super) fn brush_target(&mut self, world: &dyn World, actor: &Actor, item: &str, clicked: Option<Pos>) -> Result<Pos, String> {
        let opts = self.opts_of(actor, item);
        if let (Some(p), None, None) = (clicked, opts.range, opts.trace_mask.as_ref()) {
            return Ok(p);
        }
        self.trace_hit(world, actor, item, true)?.ok_or_else(|| "No block in sight!".to_owned())
    }

    // ---- clicks ----

    /// A left (`left`) or right click on the block at `pos` with an item that has a tool. `None` when
    /// it has none (or the tool does nothing on that click), which leaves the click to the game.
    pub fn tool_click(&mut self, world: &mut dyn World, actor: &Actor, left: bool, pos: Pos, now_ms: i64) -> Option<Reply> {
        self.tool_click_face(world, actor, left, pos, None, now_ms)
    }

    /// [`Engine::tool_click`] with the face that was clicked (`[0, 1, 0]` the top), which the stack tool needs.
    pub fn tool_click_face(&mut self, world: &mut dyn World, actor: &Actor, left: bool, pos: Pos, face: Option<[i32; 3]>, now_ms: i64) -> Option<Reply> {
        self.now_ms = now_ms;
        let item = actor.held.clone()?;
        if left {
            if let Some(done) = self.pickaxe_click(world, actor, &item, pos, now_ms) {
                return done;
            }
        }
        let tool = self.sessions.get(actor.uuid).tools.get(&item).cloned();
        let tool = tool?;
        let node = match tool {
            Tool::NavWand => return None,
            Tool::SelWand | Tool::FarWand => "worldedit.selection.pos",
            Tool::Info => "worldedit.tool.info",
            Tool::Cycler { .. } => "worldedit.tool.data-cycler",
            Tool::Replacer { .. } => "worldedit.tool.replacer",
            Tool::FloodFill { .. } => "worldedit.tool.flood-fill",
            Tool::Tree { .. } => "worldedit.tool.tree",
            Tool::DelTree => "worldedit.tool.deltree",
            Tool::Stacker { .. } => "worldedit.tool.stack",
            Tool::LrBuild { .. } => "worldedit.tool.lrbuild",
        };
        // Block tools with one action: the right click only; the left click digs as ever.
        if left && matches!(tool, Tool::Info | Tool::FloodFill { .. } | Tool::Tree { .. } | Tool::DelTree | Tool::Stacker { .. }) {
            return None;
        }
        if self.permitted(actor, Some(node)).is_err() || (self.config.creative_only && actor.game_mode != 1) {
            return None;
        }
        // The trace tools look along the view, not at the block that was hit.
        if is_trace_tool(&tool) {
            return self.trace_tool(world, actor, &tool, left, now_ms);
        }
        // A dig packet sent twice, and a right click asked once for each hand.
        if self.repeated(actor, pos, left, now_ms) {
            return Some(Reply::default());
        }
        let result: R = match tool {
            Tool::SelWand => match self.corner_set(&*world, actor, left, pos) {
                Ok(lines) => Ok(lines),
                Err(_) => Ok(Vec::new()),
            },
            Tool::Info => {
                let state = world.get(pos).map(|s| world.describe(s)).unwrap_or_else(BlockState::air);
                let light = world.block_light(pos);
                let above = world.block_light([pos[0], pos[1] + 1, pos[2]]);
                Ok(vec![format!("@{}: {} ({})  ({light}/{above})", pt(pos), block_title(&state.name), state.to_string_full())])
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
            Tool::Tree { kind } => self.plant_tree(world, actor, &kind, pos),
            Tool::DelTree => self.remove_tree(world, actor, pos),
            Tool::Stacker { range, mask } => match face {
                Some(face) => self.stack_block(world, actor, range, &mask, pos, face),
                None => return None,
            },
            // Returned above.
            Tool::NavWand | Tool::FarWand | Tool::LrBuild { .. } => Ok(Vec::new()),
        };
        Some(match result {
            Ok(lines) => Reply::ok(lines),
            Err(e) if e.starts_with("Max blocks changed") => Reply::err("Max blocks change limit reached.".into()),
            Err(e) => Reply::err(e),
        })
    }

    /// A right click in the air with a far wand or a long-range build tool: it looks along the view, as a click on a block
    /// would. `None` when the item has neither, or the player may not use it (the click then goes to the game).
    pub fn far_wand_air(&mut self, world: &mut dyn World, actor: &Actor, now_ms: i64) -> Option<Reply> {
        self.trace_air(world, actor, false, now_ms)
    }

    /// The arm swing of a left click with a far wand or a long-range build tool (a click on a block that was handled
    /// already is the same swing: `repeated` drops it).
    pub fn swing_click(&mut self, world: &mut dyn World, actor: &Actor, now_ms: i64) -> Option<Reply> {
        self.trace_air(world, actor, true, now_ms)
    }

    /// Whether the item in the hand is a far wand or a long-range build tool; cheap, for the arm swing hook.
    pub fn holds_trace_tool(&mut self, player: u128, held: Option<&str>) -> bool {
        held.is_some_and(|item| self.sessions.get(player).tools.get(item).is_some_and(is_trace_tool))
    }

    fn trace_air(&mut self, world: &mut dyn World, actor: &Actor, left: bool, now_ms: i64) -> Option<Reply> {
        let item = actor.held.clone()?;
        let tool = self.sessions.get(actor.uuid).tools.get(&item).cloned()?;
        if !is_trace_tool(&tool) {
            return None;
        }
        self.now_ms = now_ms;
        let node = if matches!(tool, Tool::LrBuild { .. }) { "worldedit.tool.lrbuild" } else { "worldedit.selection.pos" };
        if self.permitted(actor, Some(node)).is_err() || (self.config.creative_only && actor.game_mode != 1) {
            return None;
        }
        self.trace_tool(world, actor, &tool, left, now_ms)
    }

    /// `DistanceWand` and `LongRangeBuildTool`: the block in sight (`/range`, `/tracemask`): with a range the last block of the
    /// range counts when nothing is hit, without one only a hit does.
    fn trace_tool(&mut self, world: &mut dyn World, actor: &Actor, tool: &Tool, left: bool, now_ms: i64) -> Option<Reply> {
        let item = actor.held.clone()?;
        let target = match self.trace_hit(&*world, actor, &item, false) {
            Ok(Some(p)) => p,
            Ok(None) => return Some(Reply::err("No block in sight!".into())),
            Err(e) => return Some(Reply::err(e)),
        };
        if self.repeated(actor, target, left, now_ms) {
            return Some(Reply::default());
        }
        match tool {
            Tool::FarWand => Some(match self.corner_set(&*world, actor, left, target) {
                Ok(lines) => Reply::ok(lines),
                Err(_) => Reply::default(),
            }),
            Tool::LrBuild { primary, secondary } => {
                // The right click is `actPrimary` (the primary pattern), the left click `actSecondary`.
                let pattern = if left { secondary } else { primary };
                let salt = self.salt(actor);
                // A pattern that places air breaks the block hit; any other goes on top of it
                // (`pos.toVector().subtract(pos.getDirection())`, where the trace's location points straight down).
                let at = if world.is_air(pattern.pick(salt, target)) { target } else { [target[0], target[1] + 1, target[2]] };
                let mut ops = vec![(at, pattern.pick(salt, at))];
                self.cap(actor, &mut ops);
                Some(match self.commit(world, actor, ops) {
                    Ok(_) => Reply::default(),
                    Err(e) => Reply::err(e),
                })
            }
            _ => None,
        }
    }

    /// The writes up to the player's block limit: tools that "finish up" at the limit keep what fits.
    fn cap(&mut self, actor: &Actor, ops: &mut Vec<(Pos, StateId)>) {
        let max = self.max_blocks(actor);
        if max >= 0 && ops.len() as i64 > max {
            ops.truncate(max as usize);
        }
    }

    // ---- the super pickaxe ----

    /// `//` and `/,`: the super pickaxe on or off (`on`, `off`, ...); the mode stays.
    pub(super) fn toggle_pickaxe(&mut self, actor: &Actor, args: &Args) -> R {
        let want = match args.get(0) {
            Some(w) => Some(parse_bool(w)?),
            None => None,
        };
        let s = self.sessions.get(actor.uuid);
        let has = s.pickaxe_on;
        if let Some(w) = want {
            if w == has {
                return Err(if w { "Super pickaxe already enabled." } else { "Super pickaxe already disabled." }.into());
            }
        }
        s.pickaxe_on = !has;
        Ok(vec![if has { "Super pickaxe disabled." } else { "Super pickaxe enabled." }.into()])
    }

    /// `/sp single|area <range>|recursive <range>`: the mode, and the pickaxe on.
    pub(super) fn superpickaxe(&mut self, actor: &Actor, args: &Args) -> R {
        let sub = args.get(0).ok_or_else(|| usage("superpickaxe"))?.to_ascii_lowercase();
        let max = self.config.max_super_pickaxe_size;
        let (mode, msg) = match sub.as_str() {
            "single" => (Pickaxe::Single, "Mode is now single. Left click with a pickaxe. // to disable."),
            "area" => {
                if args.get(1).is_none() {
                    return Err(usage("superpickaxe area"));
                }
                let range = args.int(1, 0)?;
                if range > max {
                    return Err(format!("Maximum range is {max}."));
                }
                (Pickaxe::Area(range), "Mode is now area. Left click with a pickaxe. // to disable.")
            }
            "recursive" | "recur" => {
                let Some(word) = args.get(1) else { return Err(usage("superpickaxe recursive")) };
                let range = word.parse::<f64>().map_err(|_| number_error(word))?;
                if range > f64::from(max) {
                    return Err(format!("Maximum range is {max}."));
                }
                (Pickaxe::Recursive(range), "Mode is now recursive. Left click with a pickaxe. // to disable.")
            }
            other => return Err(format!("The sub-command '{other}' under 'superpickaxe' could not be found.")),
        };
        let s = self.sessions.get(actor.uuid);
        s.pickaxe = mode;
        s.pickaxe_on = true;
        Ok(vec![msg.into()])
    }

    /// The left click of the super pickaxe: `None` when it does not apply (off, no pickaxe in hand, not allowed), which leaves the
    /// click to the tools. `Some(None)` when it took the click but leaves it to the game (bedrock, air); `Some(Some(reply))` when it
    /// broke blocks and the dig is cancelled.
    fn pickaxe_click(&mut self, world: &mut dyn World, actor: &Actor, item: &str, pos: Pos, now_ms: i64) -> Option<Option<Reply>> {
        let (on, mode) = {
            let s = self.sessions.get(actor.uuid);
            (s.pickaxe_on, s.pickaxe)
        };
        if !on || !is_pickaxe(item) {
            return None;
        }
        let node = match mode {
            Pickaxe::Single => "worldedit.superpickaxe",
            Pickaxe::Area(_) => "worldedit.superpickaxe.area",
            Pickaxe::Recursive(_) => "worldedit.superpickaxe.recursive",
        };
        if self.permitted(actor, Some(node)).is_err() || (self.config.creative_only && actor.game_mode != 1) {
            return None;
        }
        if self.repeated(actor, pos, true, now_ms) {
            return Some(Some(Reply::default()));
        }
        if self.queue.busy(actor.uuid) {
            return Some(Some(Reply::err("Your last edit is still being written; wait a moment".into())));
        }
        let Some(start) = world.get(pos) else { return Some(None) };
        let kind = world.describe(start).name;
        if !matches!(mode, Pickaxe::Single) && world.is_air(start) {
            return Some(None);
        }
        if kind == "minecraft:bedrock" && !self.holds(actor, "worldedit.override.bedrock") {
            return Some(None);
        }
        let same = |w: &dyn World, p: Pos| w.get(p).is_some_and(|s| w.describe(s).name == kind);
        let mut targets: Vec<Pos> = Vec::new();
        let drop = match mode {
            Pickaxe::Single => {
                targets.push(pos);
                self.config.super_pickaxe_drop
            }
            Pickaxe::Area(range) => {
                for x in pos[0] - range..=pos[0] + range {
                    for y in pos[1] - range..=pos[1] + range {
                        for z in pos[2] - range..=pos[2] + range {
                            if same(&*world, [x, y, z]) {
                                targets.push([x, y, z]);
                            }
                        }
                    }
                }
                self.config.super_pickaxe_many_drop
            }
            Pickaxe::Recursive(range) => {
                // `RecursivePickaxe.recurse`: depth first, +x -x +z -z +y -y, a block seen once.
                let mut visited: HashSet<Pos> = HashSet::new();
                let mut stack = vec![pos];
                while let Some(p) = stack.pop() {
                    let d: f64 = (0..3).map(|a| f64::from(p[a] - pos[a]).powi(2)).sum();
                    if d > range * range || !visited.insert(p) {
                        continue;
                    }
                    if !same(&*world, p) {
                        continue;
                    }
                    targets.push(p);
                    for step in [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]] {
                        stack.push([p[0] + step[0], p[1] + step[1], p[2] + step[2]]);
                    }
                }
                self.config.super_pickaxe_many_drop
            }
        };
        Some(Some(self.break_all(world, actor, targets, drop)))
    }

    /// Breaks the blocks at `targets` (air in their place), up to the limit, and remembers it for `//undo`. `drop`: the blocks
    /// drop what they would for a tool.
    fn break_all(&mut self, world: &mut dyn World, actor: &Actor, mut targets: Vec<Pos>, drop: bool) -> Reply {
        if let Some(text) = self.sessions.get(actor.uuid).global_mask.clone() {
            match Mask::parse(&*world, &text, None) {
                Ok(mask) => {
                    let w: &dyn World = &*world;
                    targets.retain(|&p| w.get(p).is_some_and(|s| mask.hits(w, s)));
                }
                Err(e) => return Reply::err(e),
            }
        }
        let Some(air) = world.resolve(&BlockState::air()) else { return Reply::default() };
        let mut change = plan(&*world, &actor.dimension, targets.iter().map(|&p| (p, air)));
        change.at_ms = self.now_ms;
        let max = self.max_blocks(actor);
        let over = max >= 0 && change.blocks.len() as i64 > max;
        if over {
            change.blocks.truncate(max as usize);
        }
        if drop {
            for &(p, _, _) in &change.blocks {
                world.break_block(p, true);
            }
        } else {
            self.write(world, actor, change.forward(), HashMap::new());
        }
        let size = self.config.history_size;
        self.sessions.get(actor.uuid).remember(change, size);
        if over { Reply::err("Max blocks change limit reached.".into()) } else { Reply::default() }
    }

    // ---- the block tools ----

    /// `BlockDataCyler`: the right click sets the next value of the selected property, the left click
    /// selects the next property. (Properties go by name here; WorldEdit's go by the game's order.)
    fn cycle(&mut self, world: &mut dyn World, actor: &Actor, item: &str, selected: Option<String>, forward: bool, pos: Pos) -> R {
        let Some(state) = world.get(pos).map(|s| world.describe(s)) else { return Ok(Vec::new()) };
        // `limits.allowed-data-cycle-blocks`: when the list is not empty only those blocks may be cycled, unless the player overrides.
        if !self.config.allowed_data_cycle_blocks.is_empty() && !self.holds(actor, "worldedit.override.data-cycler") && !self.config.allowed_data_cycle_blocks.contains(&state.name) {
            return Err("You are not permitted to cycle the data value of that block.".into());
        }
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
        if kind == "minecraft:bedrock" && !self.holds(actor, "worldedit.override.bedrock") {
            return Ok(Vec::new());
        }
        let mut seen: HashSet<Pos> = HashSet::from([origin]);
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

    /// `TreePlanter`: a tree on the clicked block, tried up to ten times. The tree is the game's own feature, so what it
    /// wrote is found by comparing the blocks around the spot before and after, and kept for `//undo`.
    fn plant_tree(&mut self, world: &mut dyn World, actor: &Actor, kind: &str, clicked: Pos) -> R {
        let at = [clicked[0], clicked[1] + 1, clicked[2]];
        if kind == "worldedit:pine" {
            return self.pine_tree(world, actor, at);
        }
        let feature = if kind == "worldedit:random" {
            let mut dice = Dice(self.salt(actor));
            RANDOM_TREES[(dice.next() * RANDOM_TREES.len() as f64) as usize % RANDOM_TREES.len()].to_owned()
        } else {
            kind.to_owned()
        };
        // Room for the largest tree: giant trees and mushrooms.
        let (lo, hi) = ([at[0] - 14, at[1] - 2, at[2] - 14], [at[0] + 14, at[1] + 40, at[2] + 14]);
        let mut before: Vec<(Pos, StateId)> = Vec::new();
        for x in lo[0]..=hi[0] {
            for y in lo[1].max(world.min_y())..=hi[1].min(world.max_y()) {
                for z in lo[2]..=hi[2] {
                    if let Some(s) = world.get([x, y, z]) {
                        before.push(([x, y, z], s));
                    }
                }
            }
        }
        let mut planted = false;
        for _ in 0..10 {
            if world.generate_tree(at, &feature) {
                planted = true;
                break;
            }
        }
        let blocks: Vec<(Pos, StateId, StateId)> = before.into_iter().filter_map(|(p, old)| world.get(p).filter(|&new| new != old).map(|new| (p, old, new))).collect();
        if !blocks.is_empty() {
            let change = Change { dimension: actor.dimension.clone(), blocks, at_ms: self.now_ms, entities_before: HashMap::new(), entities_after: HashMap::new(), extras: crate::horizon_worldedit_core::edit::Extras::default() };
            let size = self.config.history_size;
            self.sessions.get(actor.uuid).remember(change, size);
        }
        if planted { Ok(Vec::new()) } else { Err("A tree can't go there.".into()) }
    }

    /// `TreeGenerator.makePineTree`: WorldEdit's own pine, an oak trunk with oak leaves round it.
    fn pine_tree(&mut self, world: &mut dyn World, actor: &Actor, base: Pos) -> R {
        let mut dice = Dice(self.salt(actor));
        let trunk_height = (dice.next() * 2.0).floor() as i32 + 3;
        let height = (dice.next() * 5.0).floor() as i32 + 8;
        let log = world.resolve(&BlockState::new("minecraft:oak_log", &[]));
        let leaves = world.resolve(&BlockState::new("minecraft:oak_leaves", &[]));
        let (Some(log), Some(leaves)) = (log, leaves) else { return Err("A tree can't go there.".into()) };
        let mut ops: HashMap<Pos, StateId> = HashMap::new();
        // `setBlockIfAir`: only where nothing is (a block put earlier counts).
        let free = |w: &dyn World, ops: &HashMap<Pos, StateId>, p: Pos| !ops.contains_key(&p) && w.get(p).is_some_and(|s| w.is_air(s));
        let mut blocked = false;
        for i in 0..trunk_height {
            let p = [base[0], base[1] + i, base[2]];
            if !free(&*world, &ops, p) {
                blocked = true;
                break;
            }
            ops.insert(p, log);
        }
        if !blocked {
            let top = [base[0], base[1] + trunk_height, base[2]];
            for i in 0..height {
                let chance = if i == 0 || i == height - 1 { 0.6 } else { 1.0 };
                let spot = |d: Pos| [top[0] + d[0], top[1] + i + d[1], top[2] + d[2]];
                let p = spot([0, 0, 0]);
                if free(&*world, &ops, p) {
                    ops.insert(p, log);
                }
                let mut ring: Vec<(Pos, f64)> = [[-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1], [1, 0, 1], [-1, 0, 1], [1, 0, -1], [-1, 0, -1]].iter().map(|&d| (d, chance)).collect();
                if !(i == 0 || i == height - 1) {
                    for j in -2..=2 {
                        for d in [[-2, 0, j], [2, 0, j], [j, 0, -2], [j, 0, 2]] {
                            ring.push((d, 0.6));
                        }
                    }
                }
                for (d, c) in ring {
                    let p = spot(d);
                    if dice.next() <= c && free(&*world, &ops, p) {
                        ops.insert(p, leaves);
                    }
                }
            }
            let p = [top[0], top[1] + height, top[2]];
            if free(&*world, &ops, p) {
                ops.insert(p, leaves);
            }
        }
        let mut list: Vec<(Pos, StateId)> = ops.into_iter().collect();
        list.sort();
        self.commit(world, actor, list)?;
        Ok(Vec::new())
    }

    /// `FloatingTreeRemover`: the tree the clicked block is part of, when it floats (nothing solid but leaves and vines touch it).
    fn remove_tree(&mut self, world: &mut dyn World, actor: &Actor, origin: Pos) -> R {
        let name_at = |w: &dyn World, p: Pos| w.get(p).map(|s| w.describe(s).name);
        if !name_at(&*world, origin).is_some_and(|n| is_tree_block(&n)) {
            return Err("That's not a tree.".into());
        }
        let range_sq: i64 = 100 * 100;
        let mut visited: HashSet<Pos> = HashSet::from([origin]);
        let mut queue: VecDeque<Pos> = VecDeque::from([origin]);
        // north, east, south, west, up, down
        let dirs: [Pos; 6] = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 1, 0], [0, -1, 0]];
        while let Some(current) = queue.pop_front() {
            for d in dirs {
                let next = [current[0] + d[0], current[1] + d[1], current[2] + d[2]];
                let dist_sq: i64 = (0..3).map(|a| i64::from(next[a] - origin[a]).pow(2)).sum();
                if dist_sq > range_sq || !visited.insert(next) {
                    continue;
                }
                let Some(state) = world.get(next) else { continue };
                if world.is_air(state) {
                    continue;
                }
                let name = world.describe(state).name;
                if name == "minecraft:snow" {
                    continue;
                }
                if is_tree_block(&name) {
                    queue.push_back(next);
                } else {
                    // Something solid: a log or a mushroom block against it is not a floating tree.
                    let from = name_at(&*world, current).unwrap_or_default();
                    if !is_leaves(&from) && from != "minecraft:vine" {
                        return Err("That's not a floating tree.".into());
                    }
                }
            }
        }
        let Some(air) = world.resolve(&BlockState::air()) else { return Ok(Vec::new()) };
        let mut ops: Vec<(Pos, StateId)> = visited.into_iter().filter(|&p| name_at(&*world, p).is_some_and(|n| is_tree_block(&n))).map(|p| (p, air)).collect();
        ops.sort();
        self.cap(actor, &mut ops);
        self.commit(world, actor, ops)?;
        Ok(Vec::new())
    }

    /// `StackTool`: the clicked block copied along the clicked face, `range` blocks, while the mask holds.
    fn stack_block(&mut self, world: &mut dyn World, actor: &Actor, range: i32, mask: &str, clicked: Pos, face: [i32; 3]) -> R {
        let mask = Mask::parse(&*world, mask, actor.held.as_deref())?;
        let Some(state) = world.get(clicked) else { return Ok(Vec::new()) };
        let entity = world.block_entity_at(clicked);
        let mut ops: Vec<(Pos, StateId)> = Vec::new();
        let mut p = clicked;
        for _ in 0..range {
            p = [p[0] + face[0], p[1] + face[1], p[2] + face[2]];
            let w: &dyn World = &*world;
            if !w.get(p).is_some_and(|s| mask.hits(w, s)) {
                break;
            }
            ops.push((p, state));
        }
        self.cap(actor, &mut ops);
        let entities: HashMap<Pos, Tag> = match entity {
            Some(tag) => ops.iter().map(|&(p, _)| (p, tag.clone())).collect(),
            None => HashMap::new(),
        };
        self.commit_with(world, actor, ops, entities)?;
        Ok(Vec::new())
    }
}
