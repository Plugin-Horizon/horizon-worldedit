//! Patterns (what an edit writes) and masks (which blocks it touches).
//!
//! The list syntax, the weights and the position hash come from the upstream bundle's
//! `runtime.rs` (XnotCykoX/WorldEdit-Horizon, MIT). Added: `!` and `#existing` masks, masks that
//! match named properties only, `0` for air and `hand` for the held block, as WorldEdit has them.

use crate::block::BlockState;
use crate::world::{Pos, StateId, World};
use std::cell::RefCell;
use std::collections::HashMap;

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
}

impl Pattern {
    pub fn single(state: StateId, text: &str) -> Self {
        Self { text: text.to_owned(), entries: vec![(1, state)], total: 1 }
    }
    /// The pattern `spec` names, or the entry that isn't a block.
    pub fn parse(world: &dyn World, spec: &str, hand: Option<&str>) -> Result<Self, String> {
        if spec.trim().is_empty() {
            return Err("A pattern is needed: a block such as stone, or a list such as 70%stone,30%gravel".into());
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
        Ok(Self { text: spec.trim().to_owned(), entries, total })
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
        if self.entries.len() == 1 {
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

#[derive(Clone, Debug, PartialEq)]
enum MaskEntry {
    /// A block, and the properties it must have.
    Block(BlockState),
    /// `#existing`: anything but air.
    Existing,
    /// `#air`.
    Air,
    /// `#solid`.
    Solid,
    /// `^[prop=value]` (`^=[...]` strict): blocks having the named properties at those values.
    State { props: Vec<(String, String)>, strict: bool },
}

/// One space-separated part of a mask: any entry of its comma list, turned round by `!`.
#[derive(Clone, Debug)]
struct Component {
    entries: Vec<MaskEntry>,
    inverted: bool,
}

/// Which blocks an edit touches: every space-separated part must accept the block, and each part is
/// a comma list of blocks or `#existing`, `#air`, `#solid`, `^[state]`, with `!` in front turning
/// the part round.
#[derive(Debug)]
pub struct Mask {
    pub text: String,
    components: Vec<Component>,
    seen: RefCell<HashMap<StateId, bool>>,
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

impl Mask {
    pub fn parse(world: &dyn World, spec: &str, hand: Option<&str>) -> Result<Self, String> {
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
            let entries = Self::component(world, body, hand).map_err(|e| e.unwrap_or_else(|| format!("No match for '{body}'.")))?;
            components.push(Component { entries, inverted });
        }
        if components.is_empty() {
            return Err(format!("No match for '{text}'."));
        }
        Ok(Self { text: text.to_owned(), components, seen: RefCell::new(HashMap::new()) })
    }

    /// The entries of one part; `Err(None)` for a part nothing recognizes ("No match").
    fn component(world: &dyn World, body: &str, hand: Option<&str>) -> Result<Vec<MaskEntry>, Option<String>> {
        match body {
            "#existing" => return Ok(vec![MaskEntry::Existing]),
            "#air" => return Ok(vec![MaskEntry::Air]),
            "#solid" => return Ok(vec![MaskEntry::Solid]),
            _ => {}
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
        Self { text: "#existing".into(), components: vec![Component { entries: vec![MaskEntry::Existing], inverted: false }], seen: RefCell::new(HashMap::new()) }
    }
    pub fn hits(&self, world: &dyn World, state: StateId) -> bool {
        if let Some(&hit) = self.seen.borrow().get(&state) {
            return hit;
        }
        let described = world.describe(state);
        let hit = self.components.iter().all(|c| {
            c.entries.iter().any(|e| match e {
                MaskEntry::Existing => !described.is_air(),
                MaskEntry::Air => described.is_air(),
                MaskEntry::Solid => is_solid_name(&described.name),
                MaskEntry::Block(b) => described.matches(b) || (b.is_air() && b.properties.is_empty() && described.is_air()),
                MaskEntry::State { props, strict } => {
                    let known: Vec<_> = props.iter().filter(|(k, _)| described.properties.contains_key(k)).collect();
                    !(*strict && known.is_empty()) && known.iter().all(|(k, v)| described.properties.get(k) == Some(v))
                }
            }) != c.inverted
        });
        self.seen.borrow_mut().insert(state, hit);
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::fake::FakeWorld;

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
