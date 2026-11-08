//! `config/worldedit.toml`: the server's WorldEdit settings, named after WorldEdit's own
//! `config.yml` keys where there is one.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The file written when there is none.
pub const DEFAULT: &str = r#"# WorldEdit for Horizon (dedicated server). Changes are read again when the file is saved.

# The operator level a player needs for every WorldEdit command and the wand (0 = everyone), unless
# node_levels names a level for that command's worldedit.* node.
permission_level = 2

# The most blocks one edit may change (WorldEdit's limits.max-blocks-changed); -1 = no limit.
max_blocks = -1

# Blocks written per server tick. A bigger edit is written over the next ticks, so the server keeps
# ticking; the player is told when it is done.
blocks_per_tick = 200000

# How many edits //undo remembers per player (history.size).
history_size = 15

# The selection wand (wand-item): left click sets position 1, right click position 2.
wand_item = "minecraft:wooden_axe"

# Brushes: the largest radius (limits.max-brush-radius) and how far a brush reaches.
max_brush_radius = 5
brush_range = 300

# The largest radius of //sphere, //cyl and the like (limits.max-radius); -1 = no limit.
max_radius = -1

# The largest range of /tool floodfill (limits.max-super-pickaxe-size).
max_super_pickaxe_size = 5

# The height //snow, //thaw and //green work up and down from you when none is given
# (defaults.default-vertical-height).
default_vertical_height = 256

# Where //schem keeps its files. Relative paths are inside the server's config/ folder; point it at
# a Paper server's plugins/WorldEdit/schematics to share the files with WorldEdit there.
schematics_dir = "worldedit/schematics"

# Blocks a player below operator level 4 may not use in a pattern (limits.disallowed-blocks; this is
# WorldEdit's default list).
disallowed_blocks = [
    "minecraft:oak_sapling",
    "minecraft:jungle_sapling",
    "minecraft:dark_oak_sapling",
    "minecraft:spruce_sapling",
    "minecraft:birch_sapling",
    "minecraft:acacia_sapling",
    "minecraft:black_bed",
    "minecraft:blue_bed",
    "minecraft:brown_bed",
    "minecraft:cyan_bed",
    "minecraft:gray_bed",
    "minecraft:green_bed",
    "minecraft:light_blue_bed",
    "minecraft:light_gray_bed",
    "minecraft:lime_bed",
    "minecraft:magenta_bed",
    "minecraft:orange_bed",
    "minecraft:pink_bed",
    "minecraft:purple_bed",
    "minecraft:red_bed",
    "minecraft:white_bed",
    "minecraft:yellow_bed",
    "minecraft:powered_rail",
    "minecraft:detector_rail",
    "minecraft:short_grass",
    "minecraft:dead_bush",
    "minecraft:moving_piston",
    "minecraft:piston_head",
    "minecraft:sunflower",
    "minecraft:rose_bush",
    "minecraft:dandelion",
    "minecraft:poppy",
    "minecraft:brown_mushroom",
    "minecraft:red_mushroom",
    "minecraft:tnt",
    "minecraft:torch",
    "minecraft:fire",
    "minecraft:redstone_wire",
    "minecraft:wheat",
    "minecraft:potatoes",
    "minecraft:carrots",
    "minecraft:melon_stem",
    "minecraft:pumpkin_stem",
    "minecraft:beetroots",
    "minecraft:rail",
    "minecraft:lever",
    "minecraft:redstone_torch",
    "minecraft:redstone_wall_torch",
    "minecraft:repeater",
    "minecraft:comparator",
    "minecraft:stone_button",
    "minecraft:birch_button",
    "minecraft:acacia_button",
    "minecraft:dark_oak_button",
    "minecraft:jungle_button",
    "minecraft:oak_button",
    "minecraft:spruce_button",
    "minecraft:cactus",
    "minecraft:sugar_cane",
    "minecraft:bedrock",
]

# Whether WorldEdit works only in creative mode.
creative_only = false

# Outline the selection for its player with a structure block only they see (server-side-cui),
# for selections up to 48 blocks along each side, as a structure block can show.
server_side_cui = true

# Minimum operator level per worldedit.* node (a node not listed uses permission_level).
# Example: let every player make selections (0 = everyone):
# [node_levels]
# "worldedit.selection.pos" = 0
"#;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub permission_level: u8,
    pub max_blocks: i64,
    pub blocks_per_tick: usize,
    pub history_size: usize,
    pub wand_item: String,
    pub max_brush_radius: f64,
    pub brush_range: f64,
    pub max_radius: f64,
    pub max_super_pickaxe_size: i32,
    pub default_vertical_height: i32,
    pub schematics_dir: String,
    pub disallowed_blocks: Vec<String>,
    pub creative_only: bool,
    pub server_side_cui: bool,
    /// Minimum operator level per `worldedit.*` node.
    pub node_levels: BTreeMap<String, u8>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            permission_level: 2,
            max_blocks: -1,
            blocks_per_tick: 200_000,
            history_size: 15,
            wand_item: "minecraft:wooden_axe".into(),
            max_brush_radius: 5.0,
            brush_range: 300.0,
            max_radius: -1.0,
            max_super_pickaxe_size: 5,
            default_vertical_height: 256,
            schematics_dir: "worldedit/schematics".into(),
            disallowed_blocks: vec![
                "minecraft:oak_sapling".to_owned(),
                "minecraft:jungle_sapling".to_owned(),
                "minecraft:dark_oak_sapling".to_owned(),
                "minecraft:spruce_sapling".to_owned(),
                "minecraft:birch_sapling".to_owned(),
                "minecraft:acacia_sapling".to_owned(),
                "minecraft:black_bed".to_owned(),
                "minecraft:blue_bed".to_owned(),
                "minecraft:brown_bed".to_owned(),
                "minecraft:cyan_bed".to_owned(),
                "minecraft:gray_bed".to_owned(),
                "minecraft:green_bed".to_owned(),
                "minecraft:light_blue_bed".to_owned(),
                "minecraft:light_gray_bed".to_owned(),
                "minecraft:lime_bed".to_owned(),
                "minecraft:magenta_bed".to_owned(),
                "minecraft:orange_bed".to_owned(),
                "minecraft:pink_bed".to_owned(),
                "minecraft:purple_bed".to_owned(),
                "minecraft:red_bed".to_owned(),
                "minecraft:white_bed".to_owned(),
                "minecraft:yellow_bed".to_owned(),
                "minecraft:powered_rail".to_owned(),
                "minecraft:detector_rail".to_owned(),
                "minecraft:short_grass".to_owned(),
                "minecraft:dead_bush".to_owned(),
                "minecraft:moving_piston".to_owned(),
                "minecraft:piston_head".to_owned(),
                "minecraft:sunflower".to_owned(),
                "minecraft:rose_bush".to_owned(),
                "minecraft:dandelion".to_owned(),
                "minecraft:poppy".to_owned(),
                "minecraft:brown_mushroom".to_owned(),
                "minecraft:red_mushroom".to_owned(),
                "minecraft:tnt".to_owned(),
                "minecraft:torch".to_owned(),
                "minecraft:fire".to_owned(),
                "minecraft:redstone_wire".to_owned(),
                "minecraft:wheat".to_owned(),
                "minecraft:potatoes".to_owned(),
                "minecraft:carrots".to_owned(),
                "minecraft:melon_stem".to_owned(),
                "minecraft:pumpkin_stem".to_owned(),
                "minecraft:beetroots".to_owned(),
                "minecraft:rail".to_owned(),
                "minecraft:lever".to_owned(),
                "minecraft:redstone_torch".to_owned(),
                "minecraft:redstone_wall_torch".to_owned(),
                "minecraft:repeater".to_owned(),
                "minecraft:comparator".to_owned(),
                "minecraft:stone_button".to_owned(),
                "minecraft:birch_button".to_owned(),
                "minecraft:acacia_button".to_owned(),
                "minecraft:dark_oak_button".to_owned(),
                "minecraft:jungle_button".to_owned(),
                "minecraft:oak_button".to_owned(),
                "minecraft:spruce_button".to_owned(),
                "minecraft:cactus".to_owned(),
                "minecraft:sugar_cane".to_owned(),
                "minecraft:bedrock".to_owned(),
            ],
            creative_only: false,
            server_side_cui: true,
            node_levels: BTreeMap::new(),
        }
    }
}

impl Config {
    /// The settings `text` holds; a broken file keeps the defaults and says why.
    pub fn parse(text: &str) -> (Self, Option<String>) {
        match toml::from_str::<Config>(text) {
            Ok(mut c) => {
                c.blocks_per_tick = c.blocks_per_tick.max(1000);
                c.history_size = c.history_size.clamp(1, 1000);
                c.permission_level = c.permission_level.min(4);
                // `wand_item = "wooden_axe"` is the same item (the game names items with their namespace).
                c.wand_item = c.wand_item.trim().to_ascii_lowercase();
                if !c.wand_item.contains(':') {
                    c.wand_item = format!("minecraft:{}", c.wand_item);
                }
                for level in c.node_levels.values_mut() {
                    *level = (*level).min(4);
                }
                (c, None)
            }
            Err(e) => (Self::default(), Some(format!("config/worldedit.toml is not valid, using the defaults: {e}"))),
        }
    }
    /// Whether `blocks` is within `max_blocks`.
    pub fn allows(&self, blocks: u64) -> bool {
        self.max_blocks < 0 || blocks <= self.max_blocks as u64
    }
    pub fn radius_ok(&self, r: f64) -> bool {
        self.max_radius < 0.0 || r <= self.max_radius
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_is_the_default() {
        assert_eq!(Config::parse(DEFAULT), (Config::default(), None));
        let (c, e) = Config::parse("max_blocks = 500\npermission_level = 9\n");
        assert!(e.is_none());
        assert_eq!((c.max_blocks, c.permission_level, c.history_size), (500, 4, 15));
        assert!(c.allows(500) && !c.allows(501));
        let (c, e) = Config::parse("max_blocks = \"lots\"");
        assert!(e.is_some());
        assert_eq!(c, Config::default());
    }
}
