//! The trait-mod half of WorldEdit's server bundle: the functions other mods call
//! (`[dependencies] worldedit_server = ...`, docs/native-mods.md, "Dependencies between trait
//! mods"). It has no hooks of its own; the editor runs in the bundle's mixin code, and both halves
//! reach one engine through `horizon_client::worldedit_server::core`.

use horizon_client::mod_api::Hooks;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(WorldEditApi)
}

struct WorldEditApi;

impl Hooks for WorldEditApi {}

/// A player's selection, the interface WorldGuard's SELECTION.md asks for.
pub mod selection {
    use horizon_client::worldedit_server::core;

    /// A player's selection. `dimension` is the level id (`minecraft:overworld`). Corners and points
    /// are block coordinates, both ends inclusive.
    #[derive(Clone, Debug, PartialEq)]
    pub enum RegionSelection {
        /// The two clicked corners in click order (pos1, pos2), not sorted.
        Cuboid { dimension: String, pos1: [i32; 3], pos2: [i32; 3] },
        /// A polygon selection: x/z points in order, and its height range. WorldEdit's server bundle
        /// makes cuboids only, so `selection_of` never returns this and `set_selection` refuses it.
        Polygon2d { dimension: String, points: Vec<[i32; 2]>, min_y: i32, max_y: i32 },
    }

    /// The player's current selection; `None` when they have none or it is incomplete.
    pub fn selection_of(player: u128) -> Option<RegionSelection> {
        let (dimension, pos1, pos2) = core::with(|e| e.selection_of(player))?;
        Some(RegionSelection::Cuboid { dimension, pos1, pos2 })
    }

    /// Replaces the player's selection (their outline is drawn again at the next tick). `false` for
    /// a selection type the editor does not have.
    pub fn set_selection(player: u128, selection: RegionSelection) -> bool {
        match selection {
            RegionSelection::Cuboid { dimension, pos1, pos2 } => {
                core::with(|e| e.select(player, &dimension, pos1, pos2));
                true
            }
            RegionSelection::Polygon2d { .. } => false,
        }
    }
}
