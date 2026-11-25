//! `use-inventory.*`: edits take the blocks they place from the player's inventory and give back the blocks they break
//! (`BlockBagExtent`, `BlockBag.fetchPlacedBlock`, `storeDroppedBlock`, `WorldEdit.flushBlockBag`).

use super::*;
use std::collections::BTreeMap;

/// Blocks WorldEdit's `Material.isReplacedDuringPlacement` lets be placed over, which are never fetched from a bag.
fn replaced_during_placement(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:water"
            | "minecraft:lava"
            | "minecraft:short_grass"
            | "minecraft:tall_grass"
            | "minecraft:fern"
            | "minecraft:large_fern"
            | "minecraft:dead_bush"
            | "minecraft:seagrass"
            | "minecraft:tall_seagrass"
            | "minecraft:vine"
            | "minecraft:fire"
            | "minecraft:soul_fire"
            | "minecraft:light"
            | "minecraft:structure_void"
    )
}

/// What a bag edit did: the blocks that could not be placed, and the inventory's change.
#[derive(Clone, Debug, Default)]
pub(super) struct BagResult {
    pub missing: BTreeMap<String, u32>,
    pub delta: BTreeMap<String, i64>,
}

impl Engine {
    /// `SessionManager`: whether this player's edits use their inventory. `granted` are the `worldedit.*` nodes the permission
    /// system gives them, `permission` their operator level and `game_mode` 1 for creative.
    pub fn wants_inventory(&self, granted: &BTreeSet<String>, permission: u8, game_mode: i32) -> bool {
        let c = &self.config;
        if !c.use_inventory {
            return false;
        }
        let unrestricted = granted.contains("worldedit.inventory.unrestricted") || permission >= self.node_level("worldedit.inventory.unrestricted");
        !(c.use_inventory_override && (unrestricted || (c.use_inventory_creative_override && game_mode == 1)))
    }

    /// Takes the blocks `ops` place out of the actor's inventory (`actor.inventory`) and puts the broken ones in: an op whose
    /// block cannot be fetched is dropped and counted as missing. Without an inventory nothing changes.
    pub(super) fn bag_filter(&mut self, world: &dyn World, actor: &Actor, ops: &mut Vec<(Pos, StateId)>) {
        let Some(inventory) = &actor.inventory else { return };
        let have_items = world.item_names();
        let item_of = |name: &str| -> Option<String> {
            if have_items.is_empty() || have_items.iter().any(|i| i == name) {
                Some(name.to_owned())
            } else {
                None
            }
        };
        let mut have = inventory.clone();
        let mut result = BagResult::default();
        ops.retain(|&(p, new)| {
            let Some(old) = world.get(p) else { return true };
            let (new_name, old_name) = (world.describe(new).name, world.describe(old).name);
            if new_name == old_name {
                return true;
            }
            if !replaced_during_placement(&new_name) {
                if let Some(item) = item_of(&new_name) {
                    match have.get_mut(&item) {
                        Some(n) if *n > 0 => {
                            *n -= 1;
                            *result.delta.entry(item).or_insert(0) -= 1;
                        }
                        _ => {
                            *result.missing.entry(new_name).or_insert(0) += 1;
                            return false;
                        }
                    }
                }
            }
            if !world.is_air(old) {
                if let Some(item) = item_of(&old_name) {
                    *have.entry(item.clone()).or_insert(0) += 1;
                    *result.delta.entry(item).or_insert(0) += 1;
                }
            }
            true
        });
        let pending = self.bag.entry(actor.uuid).or_default();
        for (item, n) in result.delta {
            *pending.0.entry(item).or_insert(0) += n;
        }
        for (name, n) in result.missing {
            *pending.1.entry(name).or_insert(0) += n;
        }
    }

    /// The inventory change and the report of the blocks a bag edit could not place, since the last call.
    pub fn take_bag(&mut self, player: u128) -> (Vec<(String, i64)>, Vec<String>) {
        let Some((delta, missing)) = self.bag.remove(&player) else { return (Vec::new(), Vec::new()) };
        let delta: Vec<(String, i64)> = delta.into_iter().filter(|(_, n)| *n != 0).collect();
        let mut notes = Vec::new();
        if !missing.is_empty() {
            let list: Vec<String> = missing.iter().map(|(name, n)| format!("{} [Amt: {n}]", block_title(name))).collect();
            notes.push(format!("Missing these blocks: {}", list.join(", ")));
        }
        (delta, notes)
    }
}
