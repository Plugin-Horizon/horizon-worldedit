//! Block states by name and properties, and how rotating and mirroring change them.
//!
//! `BlockState` and the rotation/mirror code are taken from horizonmatica
//! (https://github.com/keksi2k/horizonmatica, `core/src/schematic.rs` and `core/src/transform.rs`),
//! MIT, Copyright (c) 2026 keksi2k. Added here: `flip_y_state` (WorldEdit's `//flip up`), and
//! `BlockState::matches` for masks.

use std::collections::BTreeMap;

/// A block state as the file names it: `minecraft:oak_stairs` and its properties.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

impl BlockState {
    pub fn air() -> Self {
        Self { name: "minecraft:air".into(), properties: BTreeMap::new() }
    }
    pub fn new(name: &str, props: &[(&str, &str)]) -> Self {
        Self {
            name: name.into(),
            properties: props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
    /// `structure_void` cells are skipped when placing, as Litematica does.
    pub fn is_void(&self) -> bool {
        self.name == "minecraft:structure_void"
    }
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }
    /// `minecraft:oak_stairs[facing=north,half=bottom]`.
    pub fn to_string_full(&self) -> String {
        if self.properties.is_empty() {
            return self.name.clone();
        }
        let props: Vec<String> = self.properties.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{}[{}]", self.name, props.join(","))
    }
    /// Parses `name[k=v,...]`, adding `minecraft:` when the namespace is missing.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (name, rest) = match text.find('[') {
            Some(i) => (&text[..i], Some(&text[i + 1..])),
            None => (text, None),
        };
        if name.is_empty() {
            return None;
        }
        let name = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        let mut properties = BTreeMap::new();
        if let Some(rest) = rest {
            let rest = rest.strip_suffix(']')?;
            for pair in rest.split(',').filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=')?;
                properties.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        Some(Self { name, properties })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotation {
    #[default]
    None,
    Clockwise90,
    Clockwise180,
    CounterClockwise90,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mirror {
    #[default]
    None,
    /// Flips z (vanilla `LEFT_RIGHT`).
    LeftRight,
    /// Flips x (vanilla `FRONT_BACK`).
    FrontBack,
}

impl Rotation {
    pub const ALL: [Rotation; 4] =
        [Rotation::None, Rotation::Clockwise90, Rotation::Clockwise180, Rotation::CounterClockwise90];
    pub fn quarter_turns(self) -> u8 {
        match self {
            Rotation::None => 0,
            Rotation::Clockwise90 => 1,
            Rotation::Clockwise180 => 2,
            Rotation::CounterClockwise90 => 3,
        }
    }
    pub fn from_quarter_turns(n: u8) -> Self {
        Self::ALL[(n % 4) as usize]
    }
    pub fn then(self, other: Rotation) -> Rotation {
        Self::from_quarter_turns(self.quarter_turns() + other.quarter_turns())
    }
    pub fn name(self) -> &'static str {
        match self {
            Rotation::None => "none",
            Rotation::Clockwise90 => "cw_90",
            Rotation::Clockwise180 => "180",
            Rotation::CounterClockwise90 => "ccw_90",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "none" | "0" => Rotation::None,
            "cw" | "cw_90" | "90" => Rotation::Clockwise90,
            "180" => Rotation::Clockwise180,
            "ccw" | "ccw_90" | "270" | "-90" => Rotation::CounterClockwise90,
            _ => return None,
        })
    }
    /// A horizontal offset rotated about the origin (`BlockPos.rotate`).
    pub fn apply(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        match self {
            Rotation::None => [x, y, z],
            Rotation::Clockwise90 => [-z, y, x],
            Rotation::Clockwise180 => [-x, y, -z],
            Rotation::CounterClockwise90 => [z, y, -x],
        }
    }
}

impl Mirror {
    pub fn name(self) -> &'static str {
        match self {
            Mirror::None => "none",
            Mirror::LeftRight => "left_right",
            Mirror::FrontBack => "front_back",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "none" => Mirror::None,
            "left_right" | "z" | "lr" => Mirror::LeftRight,
            "front_back" | "x" | "fb" => Mirror::FrontBack,
            _ => return None,
        })
    }
    pub fn apply(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        match self {
            Mirror::None => [x, y, z],
            Mirror::LeftRight => [x, y, -z],
            Mirror::FrontBack => [-x, y, z],
        }
    }
}

/// Horizontal and vertical directions by name.
fn dir_rotate(d: &str, r: Rotation) -> &str {
    const RING: [&str; 4] = ["north", "east", "south", "west"];
    match RING.iter().position(|&n| n == d) {
        Some(i) => RING[(i + r.quarter_turns() as usize) % 4],
        None => d,
    }
}

fn dir_mirror(d: &str, m: Mirror) -> &str {
    match (m, d) {
        (Mirror::LeftRight, "north") => "south",
        (Mirror::LeftRight, "south") => "north",
        (Mirror::FrontBack, "east") => "west",
        (Mirror::FrontBack, "west") => "east",
        _ => d,
    }
}

fn dir_axis(d: &str) -> char {
    match d {
        "north" | "south" => 'z',
        "east" | "west" => 'x',
        _ => 'y',
    }
}

/// Maps each `_`-separated direction token of a value (`ascending_east`, `north_up`, `south_west`).
fn map_tokens(v: &str, f: impl Fn(&str) -> String) -> String {
    v.split('_').map(f).collect::<Vec<_>>().join("_")
}

/// Rail shapes are written north-first (`north_east`, never `east_north`).
fn normalize_rail(v: String) -> String {
    let parts: Vec<&str> = v.split('_').collect();
    if parts.len() == 2 && parts[0] != "ascending" {
        let order = |d: &str| ["north", "south", "east", "west"].iter().position(|&n| n == d).unwrap_or(9);
        let (a, b) = (parts[0], parts[1]);
        if order(a) > order(b) {
            return format!("{b}_{a}");
        }
        // north_south / east_west keep their order whatever the rotation.
        if (a == "south" && b == "north") || (a == "west" && b == "east") {
            return format!("{b}_{a}");
        }
    }
    v
}

fn rail_fix(v: String) -> String {
    match v.as_str() {
        "south_north" => "north_south".into(),
        "west_east" => "east_west".into(),
        _ => normalize_rail(v),
    }
}

/// Rotates a state's properties.
pub fn rotate_state(state: &BlockState, r: Rotation) -> BlockState {
    if r == Rotation::None {
        return state.clone();
    }
    let mut out = state.clone();
    let turns = r.quarter_turns();
    let mut sides = Vec::new();
    for (k, v) in &state.properties {
        let new = match k.as_str() {
            "facing" | "horizontal_facing" | "vertical_direction" => dir_rotate(v, r).to_string(),
            "axis" if turns % 2 == 1 => match v.as_str() {
                "x" => "z".into(),
                "z" => "x".into(),
                _ => v.clone(),
            },
            "rotation" => match v.parse::<u32>() {
                Ok(n) => ((n + 4 * u32::from(turns)) % 16).to_string(),
                Err(_) => v.clone(),
            },
            "shape" if v.contains("north") || v.contains("south") || v.contains("east") || v.contains("west") => {
                rail_fix(map_tokens(v, |t| dir_rotate(t, r).to_string()))
            }
            "orientation" => map_tokens(v, |t| dir_rotate(t, r).to_string()),
            "north" | "east" | "south" | "west" => {
                sides.push((dir_rotate(k, r).to_string(), v.clone()));
                continue;
            }
            _ => continue,
        };
        out.properties.insert(k.clone(), new);
    }
    for (k, v) in sides {
        out.properties.insert(k, v);
    }
    out
}

/// Mirrors a state's properties.
pub fn mirror_state(state: &BlockState, m: Mirror) -> BlockState {
    if m == Mirror::None {
        return state.clone();
    }
    let mut out = state.clone();
    let mirror_axis = if m == Mirror::LeftRight { 'z' } else { 'x' };
    let mut sides = Vec::new();
    let facing = state.prop("facing").unwrap_or("");
    let is_stairs = state.name.ends_with("_stairs");
    for (k, v) in &state.properties {
        let new = match k.as_str() {
            "facing" | "horizontal_facing" => {
                if is_stairs && dir_axis(v) != mirror_axis {
                    // StairBlock.mirror leaves stairs facing across the mirror unchanged.
                    continue;
                }
                dir_mirror(v, m).to_string()
            }
            "rotation" => match v.parse::<i32>() {
                Ok(r) => {
                    // Mirror.mirror(rotation, 16).
                    let j = if r > 8 { r - 16 } else { r };
                    let n = match m {
                        Mirror::LeftRight => (8 - j + 16) % 16,
                        Mirror::FrontBack => (16 - j) % 16,
                        Mirror::None => r,
                    };
                    n.to_string()
                }
                Err(_) => v.clone(),
            },
            "shape" if is_stairs => {
                if dir_axis(facing) != mirror_axis {
                    continue;
                }
                match v.as_str() {
                    "inner_left" => "inner_right".into(),
                    "inner_right" => "inner_left".into(),
                    "outer_left" => "outer_right".into(),
                    "outer_right" => "outer_left".into(),
                    _ => continue,
                }
            }
            "shape" if v.contains("north") || v.contains("south") || v.contains("east") || v.contains("west") => {
                rail_fix(map_tokens(v, |t| dir_mirror(t, m).to_string()))
            }
            "orientation" => map_tokens(v, |t| dir_mirror(t, m).to_string()),
            "hinge" => match v.as_str() {
                "left" => "right".into(),
                "right" => "left".into(),
                _ => continue,
            },
            "type" if state.name.ends_with("chest") => match v.as_str() {
                "left" => "right".into(),
                "right" => "left".into(),
                _ => continue,
            },
            "north" | "east" | "south" | "west" => {
                sides.push((dir_mirror(k, m).to_string(), v.clone()));
                continue;
            }
            _ => continue,
        };
        out.properties.insert(k.clone(), new);
    }
    for (k, v) in sides {
        out.properties.insert(k, v);
    }
    out
}

/// Mirror, then rotation (Litematica's order).
pub fn transform_state(state: &BlockState, m: Mirror, r: Rotation) -> BlockState {
    rotate_state(&mirror_state(state, m), r)
}

pub fn transform_pos(p: [i32; 3], m: Mirror, r: Rotation) -> [i32; 3] {
    r.apply(m.apply(p))
}


impl BlockState {
    /// Whether this state is `pattern`'s block with every property `pattern` names set as it says
    /// (properties it leaves out may be anything): `oak_stairs[facing=east]` matches every east-facing
    /// oak stair, as a WorldEdit mask does.
    pub fn matches(&self, pattern: &BlockState) -> bool {
        self.name == pattern.name
            && pattern.properties.iter().all(|(k, v)| self.properties.get(k) == Some(v))
    }
}

/// Mirrors a state top to bottom: stairs and trapdoors (`half`), slabs (`type`), up/down facings,
/// `face` of buttons and levers, `vertical_direction` of dripstone, `up`/`down` sides.
pub fn flip_y_state(state: &BlockState) -> BlockState {
    let mut out = state.clone();
    let mut sides = Vec::new();
    for (k, v) in &state.properties {
        let new = match (k.as_str(), v.as_str()) {
            ("half", "top") => "bottom",
            ("half", "bottom") => "top",
            // Doors and tall plants keep their halves in order: the pair is two blocks.
            ("half", _) => continue,
            ("type", "top") if state.name.ends_with("_slab") => "bottom",
            ("type", "bottom") if state.name.ends_with("_slab") => "top",
            ("facing" | "vertical_direction", "up") => "down",
            ("facing" | "vertical_direction", "down") => "up",
            ("face", "floor") => "ceiling",
            ("face", "ceiling") => "floor",
            ("hanging", _) if state.name.ends_with("lantern") => continue,
            // Mushroom blocks, chorus plants, vines: only where both sides are properties.
            ("up" | "down", _) if state.properties.contains_key("up") && state.properties.contains_key("down") => {
                sides.push((if k == "up" { "down" } else { "up" }, v.clone()));
                continue;
            }
            _ => continue,
        };
        out.properties.insert(k.clone(), new.to_string());
    }
    for (k, v) in sides {
        out.properties.insert(k.to_string(), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(name: &str, props: &[(&str, &str)]) -> BlockState {
        BlockState::new(name, props)
    }

    #[test]
    fn parse_and_print() {
        let s = BlockState::parse("oak_stairs[facing=east, half=top]").unwrap();
        assert_eq!(s.name, "minecraft:oak_stairs");
        assert_eq!(s.to_string_full(), "minecraft:oak_stairs[facing=east,half=top]");
        assert!(BlockState::parse("").is_none());
        assert!(BlockState::parse("stone[facing").is_none());
    }

    #[test]
    fn masks_match_named_properties_only() {
        let s = st("minecraft:oak_stairs", &[("facing", "east"), ("half", "top")]);
        assert!(s.matches(&BlockState::parse("oak_stairs").unwrap()));
        assert!(s.matches(&BlockState::parse("oak_stairs[facing=east]").unwrap()));
        assert!(!s.matches(&BlockState::parse("oak_stairs[facing=west]").unwrap()));
        assert!(!s.matches(&BlockState::parse("stone").unwrap()));
    }

    #[test]
    fn rotation_turns_facing_clockwise() {
        let s = st("minecraft:furnace", &[("facing", "north")]);
        assert_eq!(rotate_state(&s, Rotation::Clockwise90).prop("facing"), Some("east"));
        assert_eq!(Rotation::Clockwise90.apply([1, 0, 0]), [0, 0, 1]);
    }

    #[test]
    fn vertical_flip() {
        let stairs = st("minecraft:oak_stairs", &[("facing", "east"), ("half", "bottom")]);
        assert_eq!(flip_y_state(&stairs).prop("half"), Some("top"));
        let slab = st("minecraft:stone_slab", &[("type", "top")]);
        assert_eq!(flip_y_state(&slab).prop("type"), Some("bottom"));
        let double = st("minecraft:stone_slab", &[("type", "double")]);
        assert_eq!(flip_y_state(&double), double);
        let door = st("minecraft:oak_door", &[("half", "upper")]);
        assert_eq!(flip_y_state(&door), door);
        let stem = st("minecraft:mushroom_stem", &[("up", "true"), ("down", "false")]);
        let w = flip_y_state(&stem);
        assert_eq!((w.prop("up"), w.prop("down")), (Some("false"), Some("true")));
        let wall = st("minecraft:cobblestone_wall", &[("up", "true")]);
        assert_eq!(flip_y_state(&wall), wall);
        let lever = st("minecraft:lever", &[("face", "floor")]);
        assert_eq!(flip_y_state(&lever).prop("face"), Some("ceiling"));
    }
}
