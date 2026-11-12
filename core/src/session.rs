//! What WorldEdit keeps for each player: the selection, the clipboard, the edit history and the
//! brushes bound to items. Kept by UUID, so builders on one server never share a selection, a
//! clipboard or an `//undo`.

use crate::clipboard::Clipboard;
use crate::edit::Change;
use crate::pattern::Pattern;
use crate::region::Cuboid;
use crate::world::Pos;
use std::collections::{HashMap, VecDeque};

/// A player's UUID from a file name, as WorldEdit writes it (`8-4-4-4-12` hex digits, dashes optional).
pub fn uuid_from_stem(stem: &str) -> Option<u128> {
    let digits: String = stem.chars().filter(|c| *c != '-').collect();
    if digits.len() != 32 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(&digits, 16).ok()
}

/// `useServerCUI` of a player's session file (`plugins/WorldEdit/sessions/<uuid>.json`). Missing means WorldEdit's
/// default, on. A file that is not JSON is an error; the other keys (wand items) are not read.
pub fn use_server_cui(text: &str) -> Result<bool, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| "not JSON".to_string())?;
    Ok(value.get("useServerCUI").and_then(serde_json::Value::as_bool).unwrap_or(true))
}

#[derive(Clone, Debug, PartialEq)]
pub enum BrushShape {
    Sphere { radius: f64, hollow: bool },
    Cylinder { radius: f64, height: i32, hollow: bool },
    Smooth { radius: f64, iterations: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub shape: BrushShape,
    /// What a sphere or cylinder brush writes (smooth writes the ground it moves).
    pub pattern: Option<Pattern>,
    /// `/mask`: only blocks this accepts are changed.
    pub mask: Option<String>,
}

/// What an item type does on a click (`/tool ...`): WorldEdit's block tools that need no more than
/// the clicked block.
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    /// `/tool selwand`: left click position 1, right click position 2, on any item.
    SelWand,
    /// `/tool info`: right click tells what the block is.
    Info,
    /// `/tool repl <pattern>`: right click places the pattern, left click takes the block as the pattern.
    Replacer { pattern: Pattern },
    /// `/tool cycler`: right click cycles the selected property of the block, left click selects the next.
    Cycler { property: Option<String> },
    /// `/tool floodfill <pattern> <range>`: right click replaces the connected blocks of that type.
    FloodFill { pattern: Pattern, range: i32 },
}

#[derive(Debug, Default)]
pub struct Session {
    pub pos1: Option<Pos>,
    pub pos2: Option<Pos>,
    /// The level the corners were set in.
    pub dimension: String,
    pub clipboard: Option<Clipboard>,
    pub undo: VecDeque<Change>,
    pub redo: Vec<Change>,
    /// Brushes by item id.
    pub brushes: HashMap<String, Brush>,
    pub salt: u64,
    /// `//sel extend`: the second corner grows the box instead of replacing the first.
    pub extend: bool,
    /// The last `//distr`: block name and count, most frequent first (`//distr -p <page>`).
    pub distribution: Option<Vec<(String, u64)>>,
    /// Tools by item id (`/tool ...`); an item has a brush or a tool, never both.
    pub tools: HashMap<String, Tool>,
    /// `//toggleplace`: shapes and the like are made at position 1 instead of at the player.
    pub place_at_pos1: bool,
    /// `//limit`: this player's most blocks changed in one operation (`None`: the server's).
    pub block_limit: Option<i64>,
    /// `//gmask`: only blocks this accepts are changed, by every edit.
    pub global_mask: Option<String>,
    /// `/we trace`.
    pub trace: bool,
    /// `/we tz`: the offset from UTC in minutes, and how the player wrote it.
    pub timezone: Option<(i32, String)>,
}

impl Session {
    /// The selection, in `dimension`; `Err` says what is missing.
    pub fn selection(&self, dimension: &str) -> Result<Cuboid, String> {
        match (self.pos1, self.pos2) {
            (Some(a), Some(b)) if self.dimension == dimension => Ok(Cuboid::new(a, b)),
            (Some(_), Some(_)) => Err(format!("Your selection is in {}; go there or make a new one", self.dimension)),
            _ => Err("Make a region selection first.".into()),
        }
    }
    pub fn set_corner(&mut self, first: bool, dimension: &str, p: Pos) {
        if self.dimension != dimension {
            // Corners in two levels make no region: the other one goes.
            self.pos1 = None;
            self.pos2 = None;
            self.dimension = dimension.to_owned();
        }
        if first {
            self.pos1 = Some(p);
        } else {
            self.pos2 = Some(p);
        }
    }
    pub fn set_selection(&mut self, c: Cuboid, dimension: &str) {
        self.pos1 = Some(c.min);
        self.pos2 = Some(c.max);
        self.dimension = dimension.to_owned();
    }
    pub fn clear_selection(&mut self) {
        self.pos1 = None;
        self.pos2 = None;
    }
    /// Remembers an edit for `//undo`, the oldest dropped past `size`; a new edit ends the redos.
    pub fn remember(&mut self, change: Change, size: usize) {
        if change.is_empty() {
            return;
        }
        self.redo.clear();
        self.undo.push_back(change);
        while self.undo.len() > size.max(1) {
            self.undo.pop_front();
        }
    }
}

/// Every player's session.
#[derive(Debug, Default)]
pub struct Sessions {
    map: HashMap<u128, Session>,
}

impl Sessions {
    pub fn get(&mut self, player: u128) -> &mut Session {
        self.map.entry(player).or_default()
    }
    pub fn clear(&mut self) {
        self.map.clear();
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_names_parse_with_or_without_dashes() {
        assert_eq!(uuid_from_stem("00000000-0000-0000-0000-000000000001"), Some(1));
        assert_eq!(uuid_from_stem("0000000000000000000000000000000a"), Some(10));
        assert_eq!(uuid_from_stem("not-a-uuid"), None);
        assert_eq!(uuid_from_stem("0000000000000000000000000000000g"), None);
    }

    #[test]
    fn cui_flag_is_read_and_defaults_on() {
        assert_eq!(use_server_cui(r#"{"useServerCUI": false, "wandItem": "minecraft:wooden_axe"}"#), Ok(false));
        assert_eq!(use_server_cui(r#"{"useServerCUI": true}"#), Ok(true));
        assert_eq!(use_server_cui("{}"), Ok(true));
        assert!(use_server_cui("{ broken").is_err());
    }
}
