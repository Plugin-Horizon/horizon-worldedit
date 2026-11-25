//! `config/worldedit.toml`: the server's WorldEdit settings, named after WorldEdit's own
//! `config.yml` keys where there is one.

use crate::horizon_worldedit_core::yaml::{self, Node};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The file written when there is none.
pub const DEFAULT: &str = r#"# WorldEdit for Horizon (dedicated server). Changes are read again when the file is saved.

# The operator level a player needs for every WorldEdit command and the wand (0 = everyone), unless
# node_levels names a level for that command's worldedit.* node.
permission_level = 2

# The most blocks one edit may change (WorldEdit's limits.max-blocks-changed); -1 = no limit.
max_blocks = -1

# The most a player may raise their own limit to with //limit (limits.max-blocks-changed.maximum); -1 = no maximum.
# Players with the worldedit.limit.unrestricted node are not held to it.
max_blocks_maximum = -1

# /butcher: the radius used when none is given, and the largest allowed (limits.butcher-radius.default / .maximum); -1 = all loaded chunks / no maximum.
butcher_default_radius = -1
butcher_max_radius = -1

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
# Default 128, not WorldEdit's -1: `//sphere stone 99999` would otherwise try to plan ~10^15 blocks and kill the server.
max_radius = 128

# The largest range of /tool floodfill (limits.max-super-pickaxe-size).
max_super_pickaxe_size = 5

# Whether the super pickaxe drops items (super-pickaxe.drop-items), and its area and recursive modes do
# (super-pickaxe.many-drop-items).
super_pickaxe_drop = true
super_pickaxe_many_drop = false

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

# Edits older than this many minutes are forgotten by //undo and //redo (0 or less keeps them).
history_expiration_minutes = 10

# A right click with this item (in the air or on a block) teleports the player to the block in sight.
nav_wand_item = "minecraft:compass"
nav_wand_max_distance = 100.0

# Where the world backups for /snapshot and //restore are (snapshots.directory): a folder of world folders and .zip
# files, relative to the server's folder. Empty = not configured. snapshots_experimental is WorldEdit's
# snapshots.experimental.
snapshots_dir = ""
snapshots_experimental = true

# Selection limits (limits.max-polygonal-points / max-polyhedron-points): -1 = no limit.
max_polygonal_default = -1
max_polygonal_max = 20
max_polyhedron_default = -1
max_polyhedron_max = 20

# //timeout: the default and the largest, in milliseconds (calculation.timeout, calculation.max-timeout).
calculation_timeout = 100
calculation_max_timeout = 300

# Take the blocks an edit places from the player's inventory (use-inventory.*).
use_inventory = false
use_inventory_override = false
use_inventory_creative_override = false

# Log every WorldEdit command to a file (logging.*); the file is in the config folder.
log_commands = false
log_file = "worldedit.log"

# Whether schematic and snapshot paths may be symbolic links (files.allow-symbolic-links).
allow_symlinks = false

# Whether command blocks may run WorldEdit commands (command-block-support).
command_block_support = false

# Operators get no WorldEdit permission just for being operators (no-op-permissions).
no_op_permissions = false

# /thru and /jumpto may stand on glass (navigation.use-glass).
navigation_use_glass = true

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
    pub max_blocks_maximum: i64,
    pub butcher_default_radius: i32,
    pub butcher_max_radius: i32,
    pub blocks_per_tick: usize,
    pub history_size: usize,
    pub wand_item: String,
    pub max_brush_radius: f64,
    pub brush_range: f64,
    pub max_radius: f64,
    pub max_super_pickaxe_size: i32,
    pub super_pickaxe_drop: bool,
    pub super_pickaxe_many_drop: bool,
    pub default_vertical_height: i32,
    pub schematics_dir: String,
    pub disallowed_blocks: Vec<String>,
    pub creative_only: bool,
    pub server_side_cui: bool,
    /// Minimum operator level per `worldedit.*` node.
    pub node_levels: BTreeMap<String, u8>,
    /// `history.expiration`: minutes after which an edit is forgotten by `//undo` and `//redo`; 0 or less keeps them.
    pub history_expiration_minutes: i64,
    /// `navigation-wand.item`: a right click with this item (in the air or on a block) teleports the player.
    pub nav_wand_item: String,
    /// `navigation-wand.max-distance`: how far the navigation wand looks, in blocks.
    pub nav_wand_max_distance: f64,
    /// `snapshots.directory`: where the world backups are (a folder, relative to the server's); empty: snapshots are
    /// not configured and `/snapshot` and `//restore` say so.
    pub snapshots_dir: String,
    /// `snapshots.experimental`: WorldEdit's newer snapshot database (the default). Both kinds read the same folders
    /// here; the old one lists a world's backups by the world's folder name only.
    pub snapshots_experimental: bool,
    /// `limits.max-polygonal-points.default` / `.maximum`: how many points a polygon selection takes (-1 = no limit).
    pub max_polygonal_default: i32,
    pub max_polygonal_max: i32,
    /// `limits.max-polyhedron-points.default` / `.maximum`: the same for a convex polyhedron.
    pub max_polyhedron_default: i32,
    pub max_polyhedron_max: i32,
    /// `limits.allowed-data-cycle-blocks`: the blocks `/tool cycler` may cycle for a player without `worldedit.override.data-cycler`
    /// (empty = all).
    pub allowed_data_cycle_blocks: Vec<String>,
    /// `calculation.timeout` and `calculation.max-timeout`: the default and the largest `//timeout`, in milliseconds.
    pub calculation_timeout: i64,
    pub calculation_max_timeout: i64,
    /// `use-inventory.enable`, `.allow-override`, `.creative-mode-overrides`: edits take their blocks from the player's inventory.
    pub use_inventory: bool,
    pub use_inventory_override: bool,
    pub use_inventory_creative_override: bool,
    /// `logging.log-commands`, `.file`, `.format`.
    pub log_commands: bool,
    pub log_file: String,
    pub log_format: String,
    /// `files.allow-symbolic-links`.
    pub allow_symlinks: bool,
    /// `show-help-on-first-use`: the pointer to `/we help` a player gets at their first command.
    pub show_help_on_first_use: bool,
    /// `command-block-support`: whether a command block may run WorldEdit commands.
    pub command_block_support: bool,
    /// `no-op-permissions`: operators get no WorldEdit permission just for being operators.
    pub no_op_permissions: bool,
    /// `debug` (profile output), `debugging.trace-unflushed-sessions`.
    pub debug: bool,
    pub trace_unflushed_sessions: bool,
    /// `navigation.use-glass`: `/thru` and `/jumpto` may stand on glass.
    pub navigation_use_glass: bool,
    /// `register-help`, `compat.extended-y-limit`, `default-locale`: read, not acted on (Bukkit's help registry, the
    /// extended build height of a mod, the JVM's locale).
    pub register_help: bool,
    pub extended_y_limit: bool,
    pub default_locale: String,
    /// `scripting.timeout`, `scripting.dir`, `shell-save-type`: CraftScript settings, read and kept (there is no script engine).
    pub script_timeout: i32,
    pub scripts_dir: String,
    pub shell_save_type: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            permission_level: 2,
            max_blocks: -1,
            max_blocks_maximum: -1,
            butcher_default_radius: -1,
            butcher_max_radius: -1,
            blocks_per_tick: 200_000,
            history_size: 15,
            wand_item: "minecraft:wooden_axe".into(),
            max_brush_radius: 5.0,
            brush_range: 300.0,
            max_radius: 128.0,
            max_super_pickaxe_size: 5,
            super_pickaxe_drop: true,
            super_pickaxe_many_drop: false,
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
            history_expiration_minutes: 10,
            nav_wand_item: "minecraft:compass".into(),
            nav_wand_max_distance: 100.0,
            snapshots_dir: String::new(),
            snapshots_experimental: true,
            max_polygonal_default: -1,
            max_polygonal_max: 20,
            max_polyhedron_default: -1,
            max_polyhedron_max: 20,
            allowed_data_cycle_blocks: Vec::new(),
            calculation_timeout: 100,
            calculation_max_timeout: 300,
            use_inventory: false,
            use_inventory_override: false,
            use_inventory_creative_override: false,
            log_commands: false,
            log_file: "worldedit.log".into(),
            log_format: "[%1$tY-%1$tm-%1$td %1$tH:%1$tM:%1$tS %4$s]: %5$s%6$s%n".into(),
            allow_symlinks: false,
            show_help_on_first_use: true,
            command_block_support: false,
            no_op_permissions: false,
            debug: false,
            trace_unflushed_sessions: false,
            navigation_use_glass: true,
            register_help: true,
            extended_y_limit: false,
            default_locale: "default".into(),
            script_timeout: 3000,
            scripts_dir: "craftscripts".into(),
            shell_save_type: String::new(),
        }
    }
}

impl Config {
    /// The settings `text` holds; a broken file keeps the defaults and says why.
    pub fn parse(text: &str) -> (Self, Option<String>) {
        match toml::from_str::<Config>(text) {
            Ok(c) => (c.normalised(), None),
            Err(e) => (Self::default(), Some(format!("config/worldedit.toml is not valid, using the defaults: {e}"))),
        }
    }

    /// The same clamps and spellings whatever file the settings came from.
    fn normalised(mut self) -> Self {
        self.blocks_per_tick = self.blocks_per_tick.max(1000);
        self.history_size = self.history_size.clamp(1, 1000);
        self.permission_level = self.permission_level.min(4);
        // `wand_item = "wooden_axe"` is the same item (the game names items with their namespace).
        self.wand_item = item_id(&self.wand_item);
        self.nav_wand_item = item_id(&self.nav_wand_item);
        for level in self.node_levels.values_mut() {
            *level = (*level).min(4);
        }
        self
    }

    /// The original `config.yml` of WorldEdit (`plugins/WorldEdit/config.yml`), read with the vendored YAML reader.
    /// Returns the settings and every key path the loader asked for, whether the file has it or not. A key of the
    /// wrong type keeps its default. `schematics_dir` is `saving.dir` as written (relative to the plugin folder;
    /// `read_original` makes it a path).
    pub fn from_original_yaml(text: &str) -> Result<(Self, BTreeSet<String>), yaml::Error> {
        let root = yaml::parse(text)?;
        let mut keys = Keys { root: &root, read: BTreeSet::new() };
        let mut c = Config::default();
        keys.i64("limits.max-blocks-changed.default", &mut c.max_blocks);
        keys.i64("limits.max-blocks-changed.maximum", &mut c.max_blocks_maximum);
        keys.int("limits.butcher-radius.default", &mut c.butcher_default_radius);
        keys.int("limits.butcher-radius.maximum", &mut c.butcher_max_radius);
        keys.int("limits.vertical-height.default", &mut c.default_vertical_height);
        keys.float("limits.max-radius", &mut c.max_radius);
        keys.int("limits.max-super-pickaxe-size", &mut c.max_super_pickaxe_size);
        keys.bool("super-pickaxe.drop-items", &mut c.super_pickaxe_drop);
        keys.bool("super-pickaxe.many-drop-items", &mut c.super_pickaxe_many_drop);
        keys.float("limits.max-brush-radius", &mut c.max_brush_radius);
        keys.list("limits.disallowed-blocks", &mut c.disallowed_blocks);
        keys.usize("history.size", &mut c.history_size);
        keys.i64("history.expiration", &mut c.history_expiration_minutes);
        keys.string("navigation-wand.item", &mut c.nav_wand_item);
        keys.float("navigation-wand.max-distance", &mut c.nav_wand_max_distance);
        keys.string("saving.dir", &mut c.schematics_dir);
        keys.string("wand-item", &mut c.wand_item);
        keys.bool("server-side-cui", &mut c.server_side_cui);
        keys.string("snapshots.directory", &mut c.snapshots_dir);
        keys.bool("snapshots.experimental", &mut c.snapshots_experimental);
        keys.int("limits.max-polygonal-points.default", &mut c.max_polygonal_default);
        keys.int("limits.max-polygonal-points.maximum", &mut c.max_polygonal_max);
        keys.int("limits.max-polyhedron-points.default", &mut c.max_polyhedron_default);
        keys.int("limits.max-polyhedron-points.maximum", &mut c.max_polyhedron_max);
        keys.list("limits.allowed-data-cycle-blocks", &mut c.allowed_data_cycle_blocks);
        keys.i64("calculation.timeout", &mut c.calculation_timeout);
        keys.i64("calculation.max-timeout", &mut c.calculation_max_timeout);
        keys.bool("use-inventory.enable", &mut c.use_inventory);
        keys.bool("use-inventory.allow-override", &mut c.use_inventory_override);
        keys.bool("use-inventory.creative-mode-overrides", &mut c.use_inventory_creative_override);
        keys.bool("logging.log-commands", &mut c.log_commands);
        keys.string("logging.file", &mut c.log_file);
        keys.string("logging.format", &mut c.log_format);
        keys.bool("files.allow-symbolic-links", &mut c.allow_symlinks);
        keys.bool("show-help-on-first-use", &mut c.show_help_on_first_use);
        keys.bool("command-block-support", &mut c.command_block_support);
        keys.bool("no-op-permissions", &mut c.no_op_permissions);
        keys.bool("debug", &mut c.debug);
        keys.bool("debugging.trace-unflushed-sessions", &mut c.trace_unflushed_sessions);
        keys.bool("navigation.use-glass", &mut c.navigation_use_glass);
        keys.bool("register-help", &mut c.register_help);
        keys.bool("compat.extended-y-limit", &mut c.extended_y_limit);
        keys.string("default-locale", &mut c.default_locale);
        keys.int("scripting.timeout", &mut c.script_timeout);
        keys.string("scripting.dir", &mut c.scripts_dir);
        keys.string("shell-save-type", &mut c.shell_save_type);
        Ok((c.normalised(), keys.read))
    }

    /// Reads `plugin_dir/config.yml` (WorldEdit's plugin folder, `plugins/WorldEdit`) and makes `schematics_dir` the
    /// folder `saving.dir` names, inside that plugin folder. Errors name the file, not its text.
    pub fn read_original(plugin_dir: &Path) -> Result<(Self, BTreeSet<String>), String> {
        let file = plugin_dir.join("config.yml");
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let (mut c, read) = Self::from_original_yaml(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        c.schematics_dir = plugin_dir.join(&c.schematics_dir).to_string_lossy().into_owned();
        Ok((c, read))
    }
    /// Whether `blocks` is within `max_blocks`.
    pub fn allows(&self, blocks: u64) -> bool {
        self.max_blocks < 0 || blocks <= self.max_blocks as u64
    }
    pub fn radius_ok(&self, r: f64) -> bool {
        self.max_radius < 0.0 || r <= self.max_radius
    }
}

/// An item id with its namespace: `wooden_axe` is `minecraft:wooden_axe`.
fn item_id(text: &str) -> String {
    let id = text.trim().to_ascii_lowercase();
    if id.contains(':') { id } else { format!("minecraft:{id}") }
}

/// Keys of the original file, read from the parsed tree; every key path asked for is recorded.
struct Keys<'a> {
    root: &'a Node,
    read: BTreeSet<String>,
}

impl<'a> Keys<'a> {
    fn node(&mut self, path: &str) -> Option<&'a Node> {
        self.read.insert(path.to_string());
        self.root.path(path)
    }
    fn i64(&mut self, path: &str, into: &mut i64) {
        if let Some(v) = self.node(path).and_then(Node::as_i64) {
            *into = v;
        }
    }
    fn int(&mut self, path: &str, into: &mut i32) {
        if let Some(v) = self.node(path).and_then(Node::as_i64).and_then(|v| i32::try_from(v).ok()) {
            *into = v;
        }
    }
    fn usize(&mut self, path: &str, into: &mut usize) {
        if let Some(v) = self.node(path).and_then(Node::as_i64).and_then(|v| usize::try_from(v).ok()) {
            *into = v;
        }
    }
    fn float(&mut self, path: &str, into: &mut f64) {
        if let Some(v) = self.node(path).and_then(|n| n.as_f64().or_else(|| n.as_i64().map(|i| i as f64))) {
            *into = v;
        }
    }
    fn bool(&mut self, path: &str, into: &mut bool) {
        if let Some(v) = self.node(path).and_then(Node::as_bool) {
            *into = v;
        }
    }
    fn string(&mut self, path: &str, into: &mut String) {
        if let Some(v) = self.node(path).and_then(Node::as_str) {
            *into = v.to_string();
        }
    }
    /// Bukkit's `getStringList`: a missing or non-list value is an empty list.
    fn list(&mut self, path: &str, into: &mut Vec<String>) {
        *into = self.node(path).map(Node::str_list).unwrap_or_default();
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

    /// The structure of WorldEdit 7.4.6's config.yml (the keys this port reads, in the same nesting), with the
    /// default values. `disallowed-blocks` is the default list. Hand-written; no server file is copied.
    fn original_fixture() -> String {
        let blocks: String = Config::default().disallowed_blocks.iter().map(|b| format!("    - \"{b}\"\n")).collect();
        format!(
            "limits:\n    max-blocks-changed:\n        default: -1\n        maximum: -1\n    vertical-height:\n        default: 256\n    max-polygonal-points:\n        default: -1\n        maximum: 20\n    max-radius: -1\n    max-super-pickaxe-size: 5\n    max-brush-radius: 5\n    butcher-radius:\n        default: -1\n        maximum: -1\n    disallowed-blocks:\n{blocks}\nsuper-pickaxe:\n    drop-items: true\n    many-drop-items: false\nnavigation-wand:\n    item: minecraft:compass\n    max-distance: 100\nsaving:\n    dir: schematics\nhistory:\n    size: 15\n    expiration: 10\nwand-item: minecraft:wooden_axe\nno-op-permissions: false\nserver-side-cui: true\n"
        )
    }

    const ORIGINAL_KEYS: [&str; 47] = [
        "limits.butcher-radius.default",
        "limits.butcher-radius.maximum",
        "limits.max-blocks-changed.default",
        "limits.max-blocks-changed.maximum",
        "limits.vertical-height.default",
        "limits.max-radius",
        "limits.max-super-pickaxe-size",
        "super-pickaxe.drop-items",
        "super-pickaxe.many-drop-items",
        "limits.max-brush-radius",
        "limits.disallowed-blocks",
        "history.size",
        "history.expiration",
        "navigation-wand.item",
        "navigation-wand.max-distance",
        "saving.dir",
        "wand-item",
        "server-side-cui",
        "snapshots.directory",
        "snapshots.experimental",
        "limits.max-polygonal-points.default",
        "limits.max-polygonal-points.maximum",
        "limits.max-polyhedron-points.default",
        "limits.max-polyhedron-points.maximum",
        "limits.allowed-data-cycle-blocks",
        "calculation.timeout",
        "calculation.max-timeout",
        "use-inventory.enable",
        "use-inventory.allow-override",
        "use-inventory.creative-mode-overrides",
        "logging.log-commands",
        "logging.file",
        "logging.format",
        "files.allow-symbolic-links",
        "show-help-on-first-use",
        "command-block-support",
        "no-op-permissions",
        "debug",
        "debugging.trace-unflushed-sessions",
        "navigation.use-glass",
        "register-help",
        "compat.extended-y-limit",
        "default-locale",
        "scripting.timeout",
        "scripting.dir",
        "shell-save-type",
    ];

    #[test]
    fn original_fixture_equals_defaults() {
        let (c, read) = Config::from_original_yaml(&original_fixture()).unwrap();
        // The file's saving.dir is relative to the plugin folder; everything else is the default.
        let mut expected = Config::default();
        expected.schematics_dir = "schematics".into();
        // The port's own default is 128 (see `max_radius` above); the original's file says -1.
        expected.max_radius = -1.0;
        assert_eq!(c, expected);
        // Keys the port does not implement (polygon limit, butcher radius, ...) are not asked for.
        let want: BTreeSet<String> = ORIGINAL_KEYS.iter().map(|k| k.to_string()).collect();
        assert_eq!(read, want);
    }

    #[test]
    fn missing_keys_keep_their_defaults() {
        let (c, _) = Config::from_original_yaml("").unwrap();
        assert_eq!(c.history_expiration_minutes, 10);
        assert_eq!(c.nav_wand_item, "minecraft:compass");
        assert_eq!(c.nav_wand_max_distance, 100.0);
        assert_eq!(c.max_blocks, -1);
    }

    #[test]
    fn original_values_are_read_and_normalised() {
        let text = "limits:\n  max-blocks-changed:\n    default: 500\n  max-radius: 40.5\n  disallowed-blocks: []\nhistory:\n  expiration: -1\nnavigation-wand:\n  item: compass\n  max-distance: 25\nwand-item: wooden_axe\nserver-side-cui: false\n";
        let (c, _) = Config::from_original_yaml(text).unwrap();
        assert_eq!(c.max_blocks, 500);
        assert_eq!(c.max_radius, 40.5);
        assert!(c.disallowed_blocks.is_empty());
        assert_eq!(c.history_expiration_minutes, -1);
        assert_eq!(c.nav_wand_item, "minecraft:compass");
        assert_eq!(c.nav_wand_max_distance, 25.0);
        assert_eq!(c.wand_item, "minecraft:wooden_axe");
        assert!(!c.server_side_cui);
    }

    #[test]
    fn values_of_the_wrong_type_keep_their_defaults() {
        let (c, _) = Config::from_original_yaml("limits:\n  max-blocks-changed:\n    default: \"lots\"\nhistory:\n  size: 2.5\n").unwrap();
        assert_eq!(c.max_blocks, -1);
        assert_eq!(c.history_size, 15);
    }

    #[test]
    fn read_original_makes_the_schematics_folder_part_of_the_plugin_folder() {
        let dir = std::env::temp_dir().join(format!("we-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.yml"), original_fixture()).unwrap();
        let (c, _) = Config::read_original(&dir).unwrap();
        assert_eq!(c.schematics_dir, dir.join("schematics").to_string_lossy());
        let err = Config::read_original(&dir.join("missing-folder")).unwrap_err();
        assert!(err.contains("config.yml"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tab_indented_original_is_a_parse_error() {
        assert!(Config::from_original_yaml("limits:\n\tmax-radius: 1\n").is_err());
    }

    #[test]
    fn item_ids_get_their_namespace() {
        assert_eq!(item_id("Wooden_Axe"), "minecraft:wooden_axe");
        assert_eq!(item_id("mymod:wand"), "mymod:wand");
    }
}
