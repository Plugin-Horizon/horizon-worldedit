//! Patterns (what an edit writes) and masks (which blocks it touches).
//!
//! The list syntax, the weights and the position hash come from the upstream bundle's
//! `runtime.rs` (XnotCykoX/WorldEdit-Horizon, MIT). Added: `!` and `#existing` masks, masks that
//! match named properties only, `0` for air and `hand` for the held block, as WorldEdit has them.

use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::engine::expr::Expr;
use crate::horizon_worldedit_core::region::Cuboid;
use crate::horizon_worldedit_core::selector::Shape;
use crate::horizon_worldedit_core::world::{Pos, StateId, World};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Splits a pattern or mask on the commas outside `[...]`, so
/// `oak_stairs[facing=east,half=top],stone` is two entries.
pub fn split_entries(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in spec.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(spec[start..i].to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(spec[start..].to_owned());
    out
}

/// `70%stone` is 70 and `stone`; `stone` is 1 and `stone`. A weight is digits with an optional
/// fraction (`12.5%stone`), as WorldEdit's random pattern reads it; `spec` is the whole input,
/// named in the error for a weight without a block.
fn weighted<'a>(part: &'a str, spec: &str) -> Result<(f64, &'a str), String> {
    let t = part.trim();
    if let Some((w, rest)) = t.split_once('%') {
        let numeric = w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_digit() || c == '.') && w.matches('.').count() <= 1;
        if numeric {
            let rest = rest.trim();
            if rest.is_empty() {
                return Err(format!("Missing the type after the % symbol for '{}'", spec.trim()));
            }
            return Ok((w.parse::<f64>().unwrap_or(1.0), rest));
        }
    }
    Ok((1.0, t))
}

/// A typed block name as a state to look up: `0` is air, `hand` the held block.
fn typed_state(name: &str, hand: Option<&str>) -> Option<BlockState> {
    match name {
        "0" => Some(BlockState::air()),
        "hand" => BlockState::parse(hand?),
        _ => BlockState::parse(name),
    }
}

/// One block, or a weighted list picked per position.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub text: String,
    pub entries: Vec<(u32, StateId)>,
    total: u32,
    /// What depends on the block that stands there: `^[state]`, `*block`, `#clipboard`. `entries` then holds one placeholder.
    dynamic: Option<Rc<Dynamic>>,
}

/// The patterns that look at the world or the clipboard.
#[derive(Clone, Debug, PartialEq)]
enum Dynamic {
    /// `#clipboard`, `#copy`, with an optional `@[x,y,z]` offset: the clipboard repeated over the world.
    Tile { view: Rc<ClipView>, offset: Pos },
    /// `*block[state]`: any of the states that fit.
    RandomState(Vec<StateId>),
    /// `^[prop=value]`, `^block`, `^block[prop=value]`, `^pattern`: the block that stands there with the type of `base` (when
    /// given) and these property values.
    Apply { base: Option<Box<Pattern>>, props: Vec<(String, String)> },
}

impl Pattern {
    pub fn single(state: StateId, text: &str) -> Self {
        Self { text: text.to_owned(), entries: vec![(1, state)], total: 1, dynamic: None }
    }
    /// The pattern `spec` names, or the entry that isn't a block.
    pub fn parse(world: &dyn World, spec: &str, hand: Option<&str>) -> Result<Self, String> {
        if spec.trim().is_empty() {
            return Err("A pattern is needed: a block such as stone, or a list such as 70%stone,30%gravel".into());
        }
        let trimmed = spec.trim();
        // The parsers that take the whole input: `#clipboard`, `^...`, `*...` (the random list comes first, as in WorldEdit).
        if !trimmed.contains(',') || trimmed.starts_with('^') || trimmed.starts_with('*') || trimmed.starts_with("##") {
            if let Some(found) = Self::dynamic_pattern(world, trimmed, hand) {
                return found;
            }
        }
        let mut chances = Vec::new();
        for part in split_entries(spec) {
            let (chance, name) = weighted(&part, spec)?;
            let state = typed_state(name, hand)
                .and_then(|s| world.resolve(&s))
                .ok_or_else(|| unknown(world, name, hand))?;
            chances.push((chance, state));
        }
        // Whole chances stay as they are; a fractional one scales every chance by 1000.
        let scale = if chances.iter().any(|(c, _)| c.fract() != 0.0) { 1000.0 } else { 1.0 };
        let entries: Vec<(u32, StateId)> = chances.into_iter().map(|(c, s)| ((c * scale).round().clamp(0.0, f64::from(u32::MAX / 64)) as u32, s)).collect();
        let total = entries.iter().fold(0u32, |t, (w, _)| t.saturating_add(*w));
        Ok(Self { text: spec.trim().to_owned(), entries, total, dynamic: None })
    }
    /// `#clipboard`, `^...` and `*...`: `None` for any other input.
    fn dynamic_pattern(world: &dyn World, spec: &str, hand: Option<&str>) -> Option<Result<Self, String>> {
        let air = world.resolve(&BlockState::air()).unwrap_or(0);
        let made = |dynamic: Dynamic| Self { text: spec.to_owned(), entries: vec![(1, air)], total: 1, dynamic: Some(Rc::new(dynamic)) };
        let lower = spec.to_ascii_lowercase();
        // `#clipboard`, `#copy`, each with `@[x,y,z]`.
        let (head, offset_text) = match spec.split_once('@') {
            Some((h, o)) => (h, Some(o)),
            None => (spec, None),
        };
        if head.eq_ignore_ascii_case("#clipboard") || head.eq_ignore_ascii_case("#copy") {
            let mut offset = [0; 3];
            if let Some(coords) = offset_text {
                if coords.len() < 7 || !coords.starts_with('[') || !coords.ends_with(']') {
                    return Some(Err("Offset specified with @ but no offset given. Use '#copy@[x,y,z]'.".into()));
                }
                let parts: Vec<&str> = coords[1..coords.len() - 1].split(',').collect();
                if parts.len() != 3 {
                    return Some(Err("Clipboard offset needs x,y,z coordinates.".into()));
                }
                for (i, part) in parts.iter().enumerate() {
                    match part.trim().parse::<i32>() {
                        Ok(v) => offset[i] = v,
                        Err(_) => return Some(Err(format!("For input string: \"{part}\""))),
                    }
                }
            }
            let Some(view) = ctx().clipboard else { return Some(Err("Your clipboard is empty. Use //copy first.".into())) };
            return Some(Ok(made(Dynamic::Tile { view, offset })));
        }
        if let Some(tag) = spec.strip_prefix("##") {
            // `##tag`: a random block of the tag (its default state); `##*tag`: a random state of a random block.
            let (all_states, name) = match tag.strip_prefix('*') {
                Some(t) => (true, t),
                None => (false, tag),
            };
            let id = if name.contains(':') { name.to_ascii_lowercase() } else { format!("minecraft:{}", name.to_ascii_lowercase()) };
            if world.block_in_tag(air, &id).is_none() {
                return Some(Err(format!("Tag name '{name}' was not recognized.")));
            }
            let mut states = Vec::new();
            for block in world.block_names() {
                let Some(default) = world.resolve(&BlockState { name: block.clone(), properties: Default::default() }) else { continue };
                if world.block_in_tag(default, &id) != Some(true) {
                    continue;
                }
                if all_states {
                    let described = world.describe(default);
                    let mut axes: Vec<(String, Vec<String>)> = Vec::new();
                    for key in described.properties.keys() {
                        axes.push((key.clone(), world.property_values(&block, key)));
                    }
                    let total: usize = axes.iter().map(|(_, v)| v.len().max(1)).product();
                    for i in 0..total.min(4096) {
                        let mut n = i;
                        let mut props = std::collections::BTreeMap::new();
                        for (k, values) in &axes {
                            if values.is_empty() {
                                continue;
                            }
                            props.insert(k.clone(), values[n % values.len()].clone());
                            n /= values.len();
                        }
                        if let Some(state) = world.resolve(&BlockState { name: block.clone(), properties: props }) {
                            states.push(state);
                        }
                    }
                } else {
                    states.push(default);
                }
            }
            if states.is_empty() {
                return Some(Err(format!("Tag name '{name}' has no contents.")));
            }
            return Some(Ok(made(Dynamic::RandomState(states))));
        }
        if let Some(rest) = spec.strip_prefix('*') {
            // `*block[state]`: every state of the block that has the given properties.
            let Some(state) = BlockState::parse(rest) else { return Some(Err(format!("Block name '{rest}' was not recognized."))) };
            let Some(default) = world.resolve(&BlockState { name: state.name.clone(), properties: Default::default() }) else {
                return Some(Err(format!("Block name '{rest}' was not recognized.")));
            };
            let default = world.describe(default);
            let mut axes: Vec<(String, Vec<String>)> = Vec::new();
            for key in default.properties.keys() {
                match state.properties.get(key) {
                    Some(v) => axes.push((key.clone(), vec![v.clone()])),
                    None => axes.push((key.clone(), world.property_values(&default.name, key))),
                }
            }
            if axes.iter().any(|(_, v)| v.is_empty()) {
                return Some(Err(format!("Block name '{rest}' was not recognized.")));
            }
            let total: usize = axes.iter().map(|(_, v)| v.len()).product();
            if total > 100_000 {
                return Some(Err("Too many block states to pick from.".into()));
            }
            let mut states = Vec::new();
            for i in 0..total {
                let mut n = i;
                let mut props = std::collections::BTreeMap::new();
                for (k, values) in &axes {
                    props.insert(k.clone(), values[n % values.len()].clone());
                    n /= values.len();
                }
                if let Some(id) = world.resolve(&BlockState { name: default.name.clone(), properties: props }) {
                    states.push(id);
                }
            }
            if states.is_empty() {
                return Some(Err(format!("Block name '{rest}' was not recognized.")));
            }
            // All properties given: it is just the block.
            if states.len() == 1 {
                return Some(Ok(Self { text: spec.to_owned(), entries: vec![(1, states[0])], total: 1, dynamic: None }));
            }
            return Some(Ok(made(Dynamic::RandomState(states))));
        }
        if let Some(rest) = spec.strip_prefix('^') {
            let (type_part, states_part) = match rest.split_once('[') {
                Some((t, s)) if !rest.starts_with('#') => (t, Some(s)),
                _ => (rest, None),
            };
            let Some(states_text) = states_part else {
                // `^block` and `^pattern`: the type of the pattern's block.
                return Some(Self::parse(world, rest, hand).map(|base| made(Dynamic::Apply { base: Some(Box::new(base)), props: Vec::new() })));
            };
            let Some(inner) = states_text.strip_suffix(']') else { return Some(Err("State is missing trailing ']'".into())) };
            let mut props: Vec<(String, String)> = Vec::new();
            for state in inner.split(',') {
                if state.is_empty() {
                    return Some(Err("Empty part in state".into()));
                }
                let Some((k, v)) = state.split_once('=') else { return Some(Err("Missing '=' separator".into())) };
                if k.is_empty() {
                    return Some(Err("Empty property in state".into()));
                }
                if v.is_empty() {
                    return Some(Err("Empty value in state".into()));
                }
                if props.iter().any(|(p, _)| p == k) {
                    return Some(Err(format!("Duplicate property: {k}")));
                }
                props.push((k.to_owned(), v.to_owned()));
            }
            let base = if type_part.is_empty() {
                None
            } else {
                match Self::parse(world, type_part, hand) {
                    Ok(p) => Some(Box::new(p)),
                    Err(e) => return Some(Err(e)),
                }
            };
            return Some(Ok(made(Dynamic::Apply { base, props })));
        }
        let _ = lower;
        None
    }

    /// Whether the block depends on what stands at the position (then [`Pattern::pick_with`] must be used).
    pub fn is_dynamic(&self) -> bool {
        self.dynamic.is_some()
    }

    /// [`Pattern::pick`] for every kind of pattern: `world` is what `^`, `*` and `#clipboard` look at.
    pub fn pick_with(&self, world: &dyn World, salt: u64, pos: Pos) -> StateId {
        let Some(dynamic) = &self.dynamic else { return self.pick(salt, pos) };
        match &**dynamic {
            Dynamic::Tile { view, offset } => {
                let size = view.size;
                let rel = [0, 1, 2].map(|a| (pos[a] + offset[a]).rem_euclid(size[a].max(1)) + view.min[a]);
                view.blocks.get(&rel).copied().unwrap_or_else(|| world.resolve(&BlockState::air()).unwrap_or(0))
            }
            Dynamic::RandomState(states) => states[(mix(salt, pos) % states.len() as u64) as usize],
            Dynamic::Apply { base, props } => {
                let Some(old) = world.get(pos) else { return self.entries[0].1 };
                let old_state = world.describe(old);
                let mut block = match base {
                    Some(b) => {
                        let mut new = world.describe(b.pick_with(world, salt, pos));
                        // `TypeApplyingPattern`: the old block's properties go onto the new type where it has them.
                        for (k, v) in &old_state.properties {
                            if new.properties.contains_key(k) {
                                new.properties.insert(k.clone(), v.clone());
                            }
                        }
                        new
                    }
                    None => old_state,
                };
                // `StateApplyingPattern`: the named properties the block has, with values it accepts.
                for (k, v) in props {
                    if block.properties.contains_key(k) {
                        let mut probe = block.clone();
                        probe.properties.insert(k.clone(), v.clone());
                        if world.resolve(&probe).is_some() {
                            block = probe;
                        }
                    }
                }
                world.resolve(&block).unwrap_or(old)
            }
        }
    }

    /// The block for `pos` in an edit with `salt`: the only one, or a weighted pick that does not
    /// depend on the order a region is walked in.
    pub fn pick(&self, salt: u64, pos: Pos) -> StateId {
        if self.entries.len() == 1 {
            return self.entries[0].1;
        }
        let mut roll = (mix(salt, pos) % u64::from(self.total.max(1))) as u32;
        for &(weight, block) in &self.entries {
            if roll < weight {
                return block;
            }
            roll -= weight;
        }
        self.entries[self.entries.len() - 1].1
    }
    /// What a report calls it: the block, or the list as typed.
    pub fn label(&self, world: &dyn World) -> String {
        if self.entries.len() == 1 && self.dynamic.is_none() {
            world.describe(self.entries[0].1).name
        } else {
            self.text.clone()
        }
    }
}

/// WorldEdit's complaint about a block name that does not resolve: an unknown block, an unknown
/// property of a known one, or `hand` with no block in it.
fn unknown(world: &dyn World, name: &str, hand: Option<&str>) -> String {
    if name == "hand" {
        return "This item is not a block.".into();
    }
    let Some(state) = BlockState::parse(name) else {
        return format!("Block name '{name}' was not recognized.");
    };
    let Some(default) = world.resolve(&BlockState { name: state.name.clone(), properties: Default::default() }) else {
        return format!("Block name '{name}' was not recognized.");
    };
    let default = world.describe(default);
    let _ = hand;
    for (k, v) in &state.properties {
        if !default.properties.contains_key(k) {
            return format!("Unknown property '{k}' for block '{}'", state.name.trim_start_matches("minecraft:"));
        }
        let mut probe = default.clone();
        probe.properties.insert(k.clone(), v.clone());
        if world.resolve(&probe).is_none() {
            return format!("Unknown value '{v}' for property '{k}'");
        }
    }
    format!("Block name '{name}' was not recognized.")
}

/// A stable number for `pos` under `salt`.
pub fn mix(salt: u64, pos: Pos) -> u64 {
    let mut h = salt ^ 0xcbf2_9ce4_8422_2325;
    for axis in pos {
        h ^= u64::from(axis as u32);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 29)
}

/// The salt of the next edit: one more than the last, scrambled.
pub fn next_salt(salt: &mut u64) -> u64 {
    *salt = salt.wrapping_mul(0x5851_f42d_4c95_7f2d).wrapping_add(1);
    let mut h = *salt;
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 29)
}

/// A part of the world a mask is limited to (`#region`): the selection as it was when the mask was made.
#[derive(Clone, Debug, PartialEq)]
pub enum Area {
    Box(Cuboid),
    Shape(Shape),
}

impl Area {
    pub fn contains(&self, p: Pos) -> bool {
        match self {
            Area::Box(c) => c.contains(p),
            Area::Shape(s) => s.contains(p),
        }
    }
}

/// The player's clipboard as masks and patterns read it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipView {
    /// Every block, relative to the origin.
    pub blocks: HashMap<Pos, StateId>,
    /// Where the clipboard's origin was in the world (zero for a clipboard from a file).
    pub origin: Pos,
    /// The box of the clipboard, relative to the origin: its minimum and size.
    pub min: Pos,
    pub size: Pos,
}

thread_local! {
    /// What `Mask::parse` and `Pattern::parse` know of the player whose command or click is being handled.
    static CTX: RefCell<MaskCtx> = RefCell::new(MaskCtx::default());
}

/// Sets the session context for the masks and patterns parsed from now on (the engine does this for every command and click).
pub fn set_ctx(ctx: MaskCtx) {
    CTX.with(|c| *c.borrow_mut() = ctx);
}

fn ctx() -> MaskCtx {
    CTX.with(|c| c.borrow().clone())
}

/// What a mask or pattern may need from the player's session when it is made.
#[derive(Clone, Debug, Default)]
pub struct MaskCtx {
    /// The selection (`#region`, `#selection`, `#sel`); `None` when there is none.
    pub region: Option<Area>,
    /// The clipboard (`#clipboard`, `#copy`); `None` for an empty clipboard.
    pub clipboard: Option<Rc<ClipView>>,
    /// The player's expression timeout in milliseconds (negative: none).
    pub timeout_ms: i64,
    /// A number that makes `%n` masks differ from one command to the next.
    pub salt: u64,
}

#[derive(Clone, Debug)]
enum MaskEntry {
    /// A block, and the properties it must have.
    Block(BlockState),
    /// `#existing`: anything but air.
    Existing,
    /// `#air`.
    Air,
    /// `#solid`.
    Solid,
    /// `#fullcube`.
    FullCube,
    /// `#exposed`, `#surface`: not air, with air beside it.
    Exposed,
    /// `^[prop=value]` (`^=[...]` strict): blocks having the named properties at those values.
    State { props: Vec<(String, String)>, strict: bool },
    /// `>mask`, `<mask` (the block below, above, matches) and `~mask` (any neighbour matches): `single` is the offset's y.
    Offset { sub: Box<Mask>, single: Option<i32> },
    /// `%n`: a share of the positions, n percent.
    Noise { density: f64, salt: u64 },
    /// `=expression`.
    Expression { expr: Rc<Expr>, timeout_ms: i64 },
    /// `$biome,biome`.
    Biomes(Vec<String>),
    /// `#region`, `#dregion`: positions inside the selection (`None`: no selection, nothing matches).
    Area(Option<Area>),
    /// `#clipboard`: the block equals the clipboard's block at the same position.
    Clipboard(Rc<ClipView>),
    /// `##tag`: a block of the block tag.
    Tag(String),
}

/// One space-separated part of a mask: any entry of its comma list, turned round by `!`.
#[derive(Clone, Debug)]
struct Component {
    entries: Vec<MaskEntry>,
    inverted: bool,
}

/// Which blocks an edit touches: every space-separated part must accept the block, and each part is a comma list of blocks
/// or one of `#existing`, `#air`, `#solid`, `#fullcube`, `#exposed`, `#region`, `#clipboard`, `^[state]`, `=expression`, `%n`,
/// `$biome`, `>mask`, `<mask`, `~mask`, with `!` in front turning the part round.
#[derive(Clone, Debug)]
pub struct Mask {
    pub text: String,
    components: Vec<Component>,
    seen: RefCell<HashMap<StateId, bool>>,
    /// Whether the answer depends on the position (then it is not remembered by state).
    positional: bool,
}

/// Whether a block stops movement, as WorldEdit's `#solid` and `//hollow` see it (the game does not
/// say here, so plants, fluids, rails, buttons and other non-colliding blocks are listed by name).
pub fn is_solid_name(name: &str) -> bool {
    let n = name.trim_start_matches("minecraft:");
    if matches!(n, "air" | "cave_air" | "void_air" | "water" | "lava" | "light" | "structure_void" | "fire" | "soul_fire" | "vine" | "glow_lichen" | "sculk_vein" | "tripwire" | "tripwire_hook" | "lever" | "redstone_wire" | "bubble_column" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "dead_bush" | "seagrass" | "tall_seagrass" | "kelp" | "kelp_plant" | "sugar_cane" | "nether_sprouts" | "hanging_roots" | "spore_blossom" | "cobweb") {
        return false;
    }
    !(n.ends_with("_sapling")
        || n.ends_with("_button")
        || n.ends_with("_pressure_plate")
        || n.ends_with("torch")
        || n.ends_with("rail")
        || n.ends_with("_roots")
        || n.ends_with("_fungus")
        || n.ends_with("_mushroom")
        || n.ends_with("_flower")
        || n.ends_with("_tulip")
        || matches!(n, "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" | "wither_rose" | "torchflower" | "pink_petals" | "sunflower" | "lilac" | "rose_bush" | "peony" | "pitcher_plant" | "wheat" | "carrots" | "potatoes" | "beetroots" | "melon_stem" | "pumpkin_stem" | "attached_melon_stem" | "attached_pumpkin_stem" | "nether_wart" | "sweet_berry_bush" | "lily_pad" | "repeater" | "comparator" | "redstone_torch" | "redstone_wall_torch"))
}

/// Whether a block fills its whole cube (`Material.isFullCube`, as far as names say: stairs, slabs, fences, glass panes, plants
/// and other partial shapes are not).
pub fn is_full_cube_name(name: &str) -> bool {
    let n = name.trim_start_matches("minecraft:");
    if !is_solid_name(name) {
        return false;
    }
    !(n.ends_with("_stairs")
        || n.ends_with("_slab")
        || n.ends_with("_fence")
        || n.ends_with("_fence_gate")
        || n.ends_with("_wall")
        || n.ends_with("_pane")
        || n.ends_with("_door")
        || n.ends_with("_trapdoor")
        || n.ends_with("_bed")
        || n.ends_with("_sign")
        || n.ends_with("_banner")
        || n.ends_with("_head")
        || n.ends_with("_skull")
        || n.ends_with("_carpet")
        || n.ends_with("_candle")
        || n.ends_with("_leaves")
        || n.ends_with("_chain")
        || n.ends_with("_cauldron")
        || n.ends_with("_pot")
        || n.ends_with("_bars")
        || n.ends_with("_coral_fan")
        || n.ends_with("_coral")
        || n.ends_with("_shulker_box")
        || matches!(n, "chest" | "trapped_chest" | "ender_chest" | "enchanting_table" | "anvil" | "chipped_anvil" | "damaged_anvil" | "bell" | "brewing_stand" | "cauldron" | "hopper" | "end_rod" | "lightning_rod" | "ladder" | "lantern" | "soul_lantern" | "campfire" | "soul_campfire" | "grindstone" | "stonecutter" | "composter" | "daylight_detector" | "snow" | "farmland" | "dirt_path" | "end_portal_frame" | "dragon_egg" | "cake" | "flower_pot" | "scaffold" | "turtle_egg" | "sea_pickle" | "iron_bars" | "glass_pane" | "cactus" | "honey_block" | "slime_block" | "piston_head" | "moving_piston" | "conduit" | "lectern" | "decorated_pot" | "amethyst_cluster" | "small_amethyst_bud" | "medium_amethyst_bud" | "large_amethyst_bud" | "pointed_dripstone" | "chorus_plant" | "chorus_flower" | "cocoa" | "bamboo" | "big_dripleaf" | "small_dripleaf" | "sniffer_egg" | "vault" | "heavy_core" | "creaking_heart"))
}

impl Mask {
    pub fn parse(world: &dyn World, spec: &str, hand: Option<&str>) -> Result<Self, String> {
        Self::parse_in(world, spec, hand, &ctx())
    }

    /// [`Mask::parse`] for a player with a session: `#region`, `#clipboard`, `=expression` and `%n` know about it.
    pub fn parse_in(world: &dyn World, spec: &str, hand: Option<&str>, ctx: &MaskCtx) -> Result<Self, String> {
        let text = spec.trim();
        let mut components = Vec::new();
        for part in text.split(' ').filter(|c| !c.is_empty()) {
            let mut body = part;
            let mut inverted = false;
            while let Some(rest) = body.strip_prefix('!') {
                if rest.is_empty() {
                    return Err("Cannot negate nothing!".into());
                }
                inverted = !inverted;
                body = rest;
            }
            let entries = Self::component(world, body, hand, ctx).map_err(|e| e.unwrap_or_else(|| format!("No match for '{body}'.")))?;
            components.push(Component { entries, inverted });
        }
        if components.is_empty() {
            return Err(format!("No match for '{text}'."));
        }
        let positional = components.iter().any(|c| c.entries.iter().any(|e| !matches!(e, MaskEntry::Block(_) | MaskEntry::Existing | MaskEntry::Air | MaskEntry::Solid | MaskEntry::FullCube | MaskEntry::State { .. } | MaskEntry::Tag(_))));
        Ok(Self { text: text.to_owned(), components, seen: RefCell::new(HashMap::new()), positional })
    }

    /// The entries of one part; `Err(None)` for a part nothing recognizes ("No match").
    fn component(world: &dyn World, body: &str, hand: Option<&str>, ctx: &MaskCtx) -> Result<Vec<MaskEntry>, Option<String>> {
        match body.to_ascii_lowercase().as_str() {
            "#existing" => return Ok(vec![MaskEntry::Existing]),
            "#air" => return Ok(vec![MaskEntry::Air]),
            "#solid" => return Ok(vec![MaskEntry::Solid]),
            "#fullcube" => return Ok(vec![MaskEntry::FullCube]),
            "#exposed" | "#surface" => return Ok(vec![MaskEntry::Exposed]),
            "#region" | "#selection" | "#sel" => {
                let area = ctx.region.clone().ok_or_else(|| Some("Make a region selection first.".to_owned()))?;
                return Ok(vec![MaskEntry::Area(Some(area))]);
            }
            "#dregion" | "#dselection" | "#dsel" => return Ok(vec![MaskEntry::Area(ctx.region.clone())]),
            "#clipboard" | "#copy" => {
                let clip = ctx.clipboard.clone().ok_or_else(|| Some("Your clipboard is empty. Use //copy first.".to_owned()))?;
                return Ok(vec![MaskEntry::Clipboard(clip)]);
            }
            _ => {}
        }
        if let Some(first) = body.chars().next().filter(|c| matches!(c, '>' | '<' | '~')) {
            let sub = if body.len() > 1 { Mask::parse_in(world, &body[1..], hand, ctx).map_err(Some)? } else { Mask::existing() };
            let single = match first {
                '>' => Some(-1),
                '<' => Some(1),
                _ => None,
            };
            return Ok(vec![MaskEntry::Offset { sub: Box::new(sub), single }]);
        }
        if let Some(n) = body.strip_prefix('%') {
            let percent: i64 = n.parse().map_err(|_| Some(format!("For input string: \"{n}\"")))?;
            return Ok(vec![MaskEntry::Noise { density: percent as f64 / 100.0, salt: ctx.salt }]);
        }
        if let Some(text) = body.strip_prefix('=') {
            let expr = Expr::parse_with(text, &["x", "y", "z"]).map_err(|_| Some("Invalid expression".to_owned()))?;
            return Ok(vec![MaskEntry::Expression { expr: Rc::new(expr), timeout_ms: ctx.timeout_ms }]);
        }
        if let Some(tag) = body.strip_prefix("##") {
            let id = if tag.contains(':') { tag.to_ascii_lowercase() } else { format!("minecraft:{}", tag.to_ascii_lowercase()) };
            // The game answers `None` for a tag it cannot look at.
            let air = world.resolve(&BlockState::air()).unwrap_or(0);
            if world.block_in_tag(air, &id).is_none() {
                return Err(Some(format!("Tag name '{tag}' was not recognized.")));
            }
            return Ok(vec![MaskEntry::Tag(id)]);
        }
        if let Some(list) = body.strip_prefix('$') {
            let known = world.biome_names();
            let mut out = Vec::new();
            for name in list.split(',') {
                let id = if name.contains(':') { name.to_ascii_lowercase() } else { format!("minecraft:{}", name.to_ascii_lowercase()) };
                if !known.is_empty() && !known.iter().any(|b| *b == id) {
                    return Err(Some(format!("Unknown biome '{name}'")));
                }
                out.push(id);
            }
            return Ok(vec![MaskEntry::Biomes(out)]);
        }
        if (body.starts_with("^[") || body.starts_with("^=[")) && body.ends_with(']') {
            let strict = body.as_bytes()[1] == b'=';
            let inner = &body[2 + usize::from(strict)..body.len() - 1];
            let mut props = Vec::new();
            for pair in inner.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=').ok_or_else(|| Some(format!("Bad state format in {inner}")))?;
                props.push((k.trim().to_owned(), v.trim().to_owned()));
            }
            return Ok(vec![MaskEntry::State { props, strict }]);
        }
        let mut entries = Vec::new();
        for entry in split_entries(body) {
            let (_, name) = weighted(&entry, body).map_err(Some)?;
            let state = typed_state(name, hand).ok_or(None::<String>)?;
            // The name must be a block of this game, whatever properties it names.
            world.resolve(&BlockState { name: state.name.clone(), properties: Default::default() }).ok_or(None::<String>)?;
            entries.push(MaskEntry::Block(state));
        }
        Ok(entries)
    }

    /// `#existing`, the mask `//replace` uses when it is given only a pattern.
    pub fn existing() -> Self {
        Self { text: "#existing".into(), components: vec![Component { entries: vec![MaskEntry::Existing], inverted: false }], seen: RefCell::new(HashMap::new()), positional: false }
    }

    /// Whether the block `state` passes, without a position: a part that needs one (`#region`, `=expression`, `~mask`...) does
    /// not pass.
    pub fn hits(&self, world: &dyn World, state: StateId) -> bool {
        if !self.positional {
            if let Some(&hit) = self.seen.borrow().get(&state) {
                return hit;
            }
        }
        let hit = self.test(world, None, state);
        if !self.positional {
            self.seen.borrow_mut().insert(state, hit);
        }
        hit
    }

    /// Whether the block `state` at `pos` passes.
    pub fn hits_at(&self, world: &dyn World, pos: Pos, state: StateId) -> bool {
        if !self.positional {
            return self.hits(world, state);
        }
        self.test(world, Some(pos), state)
    }

    fn test(&self, world: &dyn World, pos: Option<Pos>, state: StateId) -> bool {
        let described = world.describe(state);
        self.components.iter().all(|c| {
            c.entries.iter().any(|e| match e {
                MaskEntry::Existing => !described.is_air(),
                MaskEntry::Air => described.is_air(),
                MaskEntry::Solid => is_solid_name(&described.name),
                MaskEntry::FullCube => match world.state_outline(state) {
                    Some(boxes) => boxes.len() == 1 && boxes[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    None => is_full_cube_name(&described.name),
                },
                MaskEntry::Block(b) => described.matches(b) || (b.is_air() && b.properties.is_empty() && described.is_air()),
                MaskEntry::State { props, strict } => {
                    let known: Vec<_> = props.iter().filter(|(k, _)| described.properties.contains_key(k)).collect();
                    !(*strict && known.is_empty()) && known.iter().all(|(k, v)| described.properties.get(k) == Some(v))
                }
                MaskEntry::Exposed => {
                    // `OffsetsMask`: the block is not air and some neighbour is.
                    !described.is_air() && pos.is_some_and(|p| SIX.iter().any(|d| world.get(crate::horizon_worldedit_core::region::add(p, *d)).is_some_and(|s| world.is_air(s))))
                }
                MaskEntry::Offset { sub, single } => pos.is_some_and(|p| match single {
                    Some(dy) => {
                        let q = [p[0], p[1] + dy, p[2]];
                        world.get(q).is_some_and(|s| sub.hits_at(world, q, s))
                    }
                    None => SIX.iter().any(|d| {
                        let q = crate::horizon_worldedit_core::region::add(p, *d);
                        world.get(q).is_some_and(|s| sub.hits_at(world, q, s))
                    }),
                }),
                MaskEntry::Noise { density, salt } => pos.is_some_and(|p| (mix(*salt, p) >> 11) as f64 / (1u64 << 53) as f64 <= *density),
                MaskEntry::Expression { expr, timeout_ms } => pos.is_some_and(|p| {
                    let mut vars: HashMap<String, f64> = HashMap::from([("x".to_owned(), f64::from(p[0])), ("y".to_owned(), f64::from(p[1])), ("z".to_owned(), f64::from(p[2]))]);
                    expr.run_in(&mut vars, &crate::horizon_worldedit_core::engine::expr::NoEnvPublic, Some(*timeout_ms)).is_ok_and(|v| v > 0.0)
                }),
                MaskEntry::Biomes(list) => pos.is_some_and(|p| world.biome_at(p).is_some_and(|b| list.iter().any(|x| *x == b))),
                MaskEntry::Tag(tag) => world.block_in_tag(state, tag).unwrap_or(false),
                MaskEntry::Area(area) => pos.is_some_and(|p| area.as_ref().is_some_and(|a| a.contains(p))),
                MaskEntry::Clipboard(view) => pos.is_some_and(|p| {
                    let air = world.resolve(&BlockState::air()).unwrap_or(0);
                    view.blocks.get(&[p[0] - view.origin[0], p[1] - view.origin[1], p[2] - view.origin[2]]).copied().unwrap_or(air) == state
                }),
            }) != c.inverted
        })
    }
}

/// The six neighbours, the order of `Direction.valuesOf(CARDINAL | UPRIGHT)` does not matter for a count.
const SIX: [Pos; 6] = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, 1, 0], [0, -1, 0]];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    #[test]
    fn lists_and_weights() {
        let w = FakeWorld::new();
        let p = Pattern::parse(&w, "70%stone,30%gravel", None).unwrap();
        assert_eq!(p.entries, vec![(70, w.id("stone")), (30, w.id("gravel"))]);
        let mut counts = [0usize; 2];
        for x in 0..100 {
            for z in 0..100 {
                counts[(p.pick(7, [x, 0, z]) == w.id("gravel")) as usize] += 1;
            }
        }
        assert!((6500..7500).contains(&counts[0]), "{counts:?}");
        assert_eq!(p.pick(7, [3, 4, 5]), p.pick(7, [3, 4, 5]));
        let stairs = Pattern::parse(&w, "oak_stairs[facing=east,half=top],stone", None).unwrap();
        assert_eq!(stairs.entries.len(), 2);
        assert_eq!(w.describe(stairs.entries[0].1).prop("half"), Some("top"));
        assert!(Pattern::parse(&w, "stone,nonsense", None).is_err());
        assert_eq!(Pattern::parse(&w, "0", None).unwrap().entries[0].1, 0);
        assert_eq!(Pattern::parse(&w, "hand", Some("minecraft:dirt")).unwrap().entries[0].1, w.id("dirt"));
        assert!(Pattern::parse(&w, "hand", None).is_err());
        assert_eq!(Pattern::parse(&w, "unobtainium", None).unwrap_err(), "Block name 'unobtainium' was not recognized.");
        assert_eq!(Pattern::parse(&w, "oak_stairs[nonsense=1]", None).unwrap_err(), "Unknown property 'nonsense' for block 'oak_stairs'");
        let frac = Pattern::parse(&w, "12.5%stone,87.5%gravel", None).unwrap();
        assert_eq!(frac.entries, vec![(12500, w.id("stone")), (87500, w.id("gravel"))]);
        assert!(Pattern::parse(&w, "50%", None).is_err());
    }

    #[test]
    fn masks() {
        let w = FakeWorld::new();
        let m = Mask::parse(&w, "stone,dirt", None).unwrap();
        assert!(m.hits(&w, w.id("stone")) && m.hits(&w, w.id("dirt")) && !m.hits(&w, 0));
        let not_air = Mask::parse(&w, "!air", None).unwrap();
        assert!(not_air.hits(&w, w.id("glass")) && !not_air.hits(&w, 0));
        let east = Mask::parse(&w, "oak_stairs[facing=east]", None).unwrap();
        assert!(east.hits(&w, w.id("oak_stairs[facing=east,half=top]")));
        assert!(!east.hits(&w, w.id("oak_stairs[facing=west]")));
        assert!(Mask::existing().hits(&w, w.id("sand")) && !Mask::existing().hits(&w, 0));
        assert!(Mask::parse(&w, "nonsense", None).is_err());
        assert_eq!(Mask::parse(&w, "nonsense", None).unwrap_err(), "No match for 'nonsense'.");
        let both = Mask::parse(&w, "stone,dirt !dirt", None).unwrap();
        assert!(both.hits(&w, w.id("stone")) && !both.hits(&w, w.id("dirt")));
        assert!(Mask::parse(&w, "#air", None).unwrap().hits(&w, 0) && Mask::parse(&w, "#solid", None).unwrap().hits(&w, w.id("stone")));
        let state = Mask::parse(&w, "^[facing=east]", None).unwrap();
        assert!(state.hits(&w, w.id("oak_stairs[facing=east]")) && !state.hits(&w, w.id("oak_stairs[facing=west]")) && state.hits(&w, w.id("stone")));
        assert!(!Mask::parse(&w, "^=[facing=east]", None).unwrap().hits(&w, w.id("stone")));
        assert_eq!(Mask::parse(&w, "!", None).unwrap_err(), "Cannot negate nothing!");
    }
}
