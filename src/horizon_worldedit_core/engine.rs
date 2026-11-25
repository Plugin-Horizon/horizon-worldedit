//! The commands, the wand and the brushes, for every player of a server: what a typed `//command`,
//! a click with the wand or a brush does to the player's session and to the world.

use crate::horizon_worldedit_core::brushes;
use crate::horizon_worldedit_core::clipboard::Clipboard;
use crate::horizon_worldedit_core::config::Config;
use crate::horizon_worldedit_core::edit::{plan_with, Change, Extras, Queue};
use crate::horizon_worldedit_core::nbt::Tag;
use crate::horizon_worldedit_core::ops;
use crate::horizon_worldedit_core::pattern::{next_salt, Mask, Pattern};
use crate::horizon_worldedit_core::region::{sub, Cuboid, Dir};
use crate::horizon_worldedit_core::schem;
use crate::horizon_worldedit_core::session::{Brush, BrushShape, PlaceKind, Sessions, Tool};
use crate::horizon_worldedit_core::shapes;
use crate::horizon_worldedit_core::world::{Pos, StateId, World};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

mod bag;
mod calc;
mod clipx;
mod cui;
pub mod expr;
mod extra;
mod help;
mod logging;
mod more;
mod nav;
mod noise;
mod select;
mod snap;
mod terrain;
mod tools;
mod utility;

pub use terrain::world_override;

/// The most positions one command looks at, whatever `max_blocks` says: a bigger region is a
/// mistake that would hold the server thread for minutes.
pub const SCAN_LIMIT: u64 = 10_000_000;

/// Every `worldedit.*` node a command, a tool or a brush can need (`permission_node`, the wand, the navigation
/// wand). The glue asks the permission system which of them a player holds (`Actor::granted`).
pub const NODES: &[&str] = &[
    "worldedit.analysis.count",
    "worldedit.analysis.distr",
    "worldedit.brush.clipboard",
    "worldedit.brush.cylinder",
    "worldedit.brush.ex",
    "worldedit.brush.options.mask",
    "worldedit.brush.options.material",
    "worldedit.brush.gravity",
    "worldedit.brush.lower",
    "worldedit.brush.raise",
    "worldedit.brush.set",
    "worldedit.brush.smooth",
    "worldedit.brush.snow",
    "worldedit.brush.morph",
    "worldedit.brush.butcher",
    "worldedit.brush.heightmap",
    "worldedit.brush.snowsmooth",
    "worldedit.brush.splatter",
    "worldedit.brush.sphere",
    "worldedit.calc",
    "worldedit.butcher",
    "worldedit.butcher.pets",
    "worldedit.butcher.npcs",
    "worldedit.butcher.golems",
    "worldedit.butcher.animals",
    "worldedit.butcher.ambient",
    "worldedit.butcher.tagged",
    "worldedit.butcher.armorstands",
    "worldedit.butcher.water",
    "worldedit.remove",
    "worldedit.generation.shape",
    "worldedit.region.deform",
    "worldedit.clipboard.clear",
    "worldedit.clipboard.copy",
    "worldedit.clipboard.cut",
    "worldedit.clipboard.flip",
    "worldedit.clipboard.load",
    "worldedit.clipboard.paste",
    "worldedit.clipboard.rotate",
    "worldedit.clipboard.save",
    "worldedit.drain",
    "worldedit.extinguish",
    "worldedit.fill",
    "worldedit.fill.recursive",
    "worldedit.fixlava",
    "worldedit.fixwater",
    "worldedit.generation.cone",
    "worldedit.generation.cylinder",
    "worldedit.generation.pyramid",
    "worldedit.generation.sphere",
    "worldedit.global-mask",
    "worldedit.green",
    "worldedit.help",
    "worldedit.history.clear",
    "worldedit.history.redo",
    "worldedit.history.redo.other",
    "worldedit.history.redo.self",
    "worldedit.history.undo",
    "worldedit.history.undo.other",
    "worldedit.history.undo.self",
    "worldedit.limit",
    "worldedit.placement",
    "worldedit.limit.unrestricted",
    "worldedit.chunkinfo",
    "worldedit.listchunks",
    "worldedit.biome.list",
    "worldedit.biome.info",
    "worldedit.biome.set",
    "worldedit.regen",
    "worldedit.region.forest",
    "worldedit.region.flora",
    "worldedit.delchunks",
    "worldedit.world",
    "worldedit.brush.forest",
    "worldedit.brush.feature",
    "worldedit.brush.biome",
    "worldedit.selection.trim",
    "worldedit.anyblock",
    "worldedit.navigation.ascend",
    "worldedit.navigation.ceiling",
    "worldedit.navigation.descend",
    "worldedit.navigation.jumpto.command",
    "worldedit.navigation.jumpto.tool",
    "worldedit.navigation.thru.command",
    "worldedit.navigation.unstuck",
    "worldedit.navigation.up",
    "worldedit.selection.sel",
    "worldedit.region.center",
    "worldedit.region.faces",
    "worldedit.region.hollow",
    "worldedit.region.line",
    "worldedit.region.curve",
    "worldedit.region.snowsmooth",
    "worldedit.revolve",
    "worldedit.region.move",
    "worldedit.region.naturalize",
    "worldedit.region.overlay",
    "worldedit.region.replace",
    "worldedit.region.set",
    "worldedit.region.smooth",
    "worldedit.region.stack",
    "worldedit.region.walls",
    "worldedit.reload",
    "worldedit.report",
    "worldedit.schematic.delete",
    "worldedit.schematic.formats",
    "worldedit.schematic.list",
    "worldedit.schematic.load",
    "worldedit.schematic.save",
    "worldedit.searchitem",
    "worldedit.selection.chunk",
    "worldedit.selection.contract",
    "worldedit.selection.expand",
    "worldedit.selection.hpos",
    "worldedit.selection.inset",
    "worldedit.selection.outset",
    "worldedit.selection.pos",
    "worldedit.selection.shift",
    "worldedit.selection.size",
    "worldedit.setwand",
    "worldedit.snow",
    "worldedit.thaw",
    "worldedit.tool.data-cycler",
    "worldedit.tool.flood-fill",
    "worldedit.tool.info",
    "worldedit.tool.replacer",
    "worldedit.wand",
    "worldedit.timeout",
    "worldedit.fast",
    "worldedit.perf",
    "worldedit.reorder",
    "worldedit.drawsel",
    "worldedit.watchdog",
    "worldedit.registry",
    "worldedit.tool.farwand",
    "worldedit.snapshots.list",
    "worldedit.snapshots.restore",
    "worldedit.navigation.thru.tool",
    "worldedit.tool.tree",
    "worldedit.tool.deltree",
    "worldedit.tool.stack",
    "worldedit.tool.lrbuild",
    "worldedit.superpickaxe",
    "worldedit.superpickaxe.area",
    "worldedit.superpickaxe.recursive",
    "worldedit.override.bedrock",
    "worldedit.brush.options.range",
    "worldedit.brush.options.size",
    "worldedit.brush.options.tracemask",
    "worldedit.brush.deform",
    "worldedit.brush.apply",
    "worldedit.brush.paint",
    "worldedit.brush.item",
    "worldedit.generation.feature",
    "worldedit.generation.structure",
    "worldedit.generation.shape.biome",
    "worldedit.generation.forest",
    "worldedit.generation.pumpkins",
    "worldedit.update",
    "worldedit.wand.toggle",
    "worldedit.timeout.unrestricted",
    "worldedit.override.data-cycler",
    "worldedit.inventory.unrestricted",
];

/// Nodes that lift a limit and need operator level 4 where `node_levels` names no other level (the original's `default: op`
/// would give them to level 2; a server whose operators are trusted that far can set the level in `node_levels`).
pub const FULL_OPERATOR_NODES: &[&str] = &["worldedit.limit.unrestricted", "worldedit.anyblock"];

/// The session of the server console and RCON: no player has this UUID, and such an actor has no position.
pub const CONSOLE_UUID: u128 = 0xC0C0_C0C0_C0C0_C0C0_C0C0_C0C0_C0C0_C0C0;

/// The player a command, a click or a brush belongs to, as the server sees them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Actor {
    pub uuid: u128,
    pub name: String,
    /// Operator level, 0 to 4.
    pub permission: u8,
    /// `minecraft:overworld`...
    pub dimension: String,
    /// Feet, as the server has them.
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub game_mode: i32,
    /// The item id in the main hand, if any.
    pub held: Option<String>,
    /// The item id in the off hand, if any (`offhand` as a block name).
    pub offhand: Option<String>,
    /// The `worldedit.*` nodes the permission system grants this player (see `NODES`). Operators pass on their
    /// level instead; anyone else needs a node here.
    pub granted: BTreeSet<String>,
    /// The item counts of the player's inventory when their edits use it (`use-inventory`); `None` when they do not.
    pub inventory: Option<std::collections::BTreeMap<String, u32>>,
}

impl Actor {
    /// The block the player stands in.
    pub fn block(&self) -> Pos {
        self.position.map(|v| v.floor() as i32)
    }
    pub fn eye(&self) -> [f64; 3] {
        [self.position[0], self.position[1] + 1.62, self.position[2]]
    }
}

/// What the server needs to know besides the world: where schematics live and what to stamp on
/// saved ones.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub schematics_dir: PathBuf,
    /// `config/worldedit.toml`, for `/we reload` (and the folder `/we report` writes into).
    pub config_file: PathBuf,
    /// The game's `DataVersion` for saved files.
    pub data_version: i32,
    /// Milliseconds since 1970, for saved files' dates and the wand's double clicks.
    pub now_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub error: bool,
}

/// The answer to a command or a click.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub lines: Vec<Line>,
    /// Item ids to give the player (the wand).
    pub give: Vec<String>,
    /// Where to teleport the player (the navigation wand), feet position.
    pub teleport: Option<[f64; 3]>,
}

impl Reply {
    fn ok(lines: Vec<String>) -> Self {
        Self { lines: lines.into_iter().map(|text| Line { text, error: false }).collect(), give: Vec::new(), teleport: None }
    }
    fn err(text: String) -> Self {
        Self { lines: vec![Line { text, error: true }], give: Vec::new(), teleport: None }
    }
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    }
}

/// The commands `//help` lists, for the tests.
#[cfg(test)]
pub(crate) fn help_entries() -> impl Iterator<Item = &'static help::Entry> {
    help::ENTRIES.iter()
}

/// The commands, each with the arguments its chat completion shows (Horizon's usage syntax). The
/// slashes are not part of the name: `//set` reaches the server as `set`. `//rotate` is not here:
/// vanilla owns `/rotate`, so it is taken through the player-command hook (`is_rotate`); `//rot` is
/// its registered alias.
pub const COMMANDS: &[(&str, &str)] = &[
    ("we", "[topic:text]"),
    ("worldedit", "[topic:text]"),
    ("wand", ""),
    ("pos1", "[coordinates:text]"),
    ("pos2", "[coordinates:text]"),
    ("hpos1", ""),
    ("hpos2", ""),
    ("sel", "[shape:word]"),
    ("desel", ""),
    ("deselect", ""),
    ("size", "[flags:text]"),
    ("expand", "<args:text>"),
    ("contract", "<args:text>"),
    ("shift", "<args:text>"),
    ("outset", "<args:text>"),
    ("inset", "<args:text>"),
    ("chunk", "[args:text]"),
    ("count", "<mask:text>"),
    ("distr", "[flags:text]"),
    ("set", "<pattern:text>"),
    ("replace", "<args:text>"),
    ("re", "<args:text>"),
    ("rep", "<args:text>"),
    ("walls", "<pattern:text>"),
    ("faces", "<pattern:text>"),
    ("outline", "<pattern:text>"),
    ("hollow", "[args:text]"),
    ("overlay", "<pattern:text>"),
    ("center", "<pattern:text>"),
    ("middle", "<pattern:text>"),
    ("naturalize", ""),
    ("smooth", "[args:text]"),
    ("snowsmooth", "[args:text]"),
    ("line", "<args:text>"),
    ("curve", "<args:text>"),
    ("revolve", "<args:text>"),
    ("stack", "[args:text]"),
    ("move", "[args:text]"),
    ("sphere", "<args:text>"),
    ("hsphere", "<args:text>"),
    ("cyl", "<args:text>"),
    ("hcyl", "<args:text>"),
    ("cone", "<args:text>"),
    ("removeabove", "[args:text]"),
    ("removebelow", "[args:text]"),
    ("removenear", "<args:text>"),
    ("replacenear", "<args:text>"),
    ("pyramid", "<args:text>"),
    ("hpyramid", "<args:text>"),
    ("copy", "[flags:text]"),
    ("cut", "[args:text]"),
    ("paste", "[flags:text]"),
    ("rot", "<args:text>"),
    ("flip", "[direction:word]"),
    ("mirror", "[direction:word]"),
    ("clearclipboard", ""),
    ("undo", "[times:integer]"),
    ("redo", "[times:integer]"),
    ("clearhistory", ""),
    ("schem", "[args:text]"),
    ("schematic", "[args:text]"),
    ("brush", "<args:text>"),
    ("br", "<args:text>"),
    ("mask", "[mask:text]"),
    ("material", "<pattern:text>"),
    ("range", "<range:integer>"),
    ("tracemask", "[mask:text]"),
    ("superpickaxe", "<args:text>"),
    ("pickaxe", "<args:text>"),
    ("sp", "<args:text>"),
    ("none", ""),
    ("tool", "<args:text>"),
    ("load", "<args:text>"),
    ("fillr", "<args:text>"),
    ("drain", "<args:text>"),
    ("fixwater", "<radius:number>"),
    ("fixlava", "<radius:number>"),
    ("extinguish", "[radius:integer]"),
    ("ex", "[radius:integer]"),
    ("ext", "[radius:integer]"),
    ("snow", "[args:text]"),
    ("thaw", "[args:text]"),
    ("green", "[args:text]"),
    ("limit", "[limit:integer]"),
    ("gmask", "[mask:text]"),
    ("toggleplace", ""),
    ("searchitem", "<args:text>"),
    ("search", "<args:text>"),
    ("l", "<args:text>"),
    ("calc", "<expression:text>"),
    ("calculate", "<expression:text>"),
    ("eval", "<expression:text>"),
    ("evaluate", "<expression:text>"),
    ("solve", "<expression:text>"),
    ("up", "<args:text>"),
    ("ascend", "[levels:integer]"),
    ("asc", "[levels:integer]"),
    ("descend", "[levels:integer]"),
    ("desc", "[levels:integer]"),
    ("thru", ""),
    ("jumpto", ""),
    ("j", ""),
    ("unstuck", ""),
    ("ceil", "[args:text]"),
    ("selwand", ""),
    ("navwand", ""),
    ("farwand", ""),
    ("tree", "[type:word]"),
    ("deltree", ""),
    ("lrbuild", "<args:text>"),
    ("info", ""),
    ("cycler", ""),
    ("repl", "<patternarg:text>"),
    ("floodfill", "<args:text>"),
    ("flood", "<args:text>"),
    ("placement", "<args:text>"),
    ("trim", "[mask:text]"),
    ("chunkinfo", ""),
    ("listchunks", "[flags:text]"),
    ("biomelist", "[flags:text]"),
    ("biomels", "[flags:text]"),
    ("biomeinfo", "[flags:text]"),
    ("setbiome", "<args:text>"),
    ("replacebiome", "<args:text>"),
    ("regen", "[args:text]"),
    ("forest", "[args:text]"),
    ("flora", "[density:number]"),
    ("delchunks", "[args:text]"),
    ("world", "[world:word]"),
    ("generate", "<args:text>"),
    ("gen", "<args:text>"),
    ("g", "<args:text>"),
    ("deform", "<expression:text>"),
    ("butcher", "[args:text]"),
    ("remove", "<args:text>"),
    ("rem", "<args:text>"),
    ("rement", "<args:text>"),
    ("timeout", "[limit:integer]"),
    ("fast", "[mode:word]"),
    ("perf", "[sideeffect:word] [state:word]"),
    ("reorder", "[mode:word]"),
    ("drawsel", "[state:word]"),
    ("watchdog", "[mode:word]"),
    ("registry", "<registry:word> [query:text]"),
    ("snapshot", "<args:text>"),
    ("snap", "<args:text>"),
    ("restore", "[snapshot:text]"),
    ("pos", "[args:text]"),
    ("update", "[sideeffects:word]"),
    ("feature", "<feature:word>"),
    ("structure", "<structure:word>"),
    ("generatebiome", "<args:text>"),
    ("genbiome", "<args:text>"),
    ("gb", "<args:text>"),
    ("toggleeditwand", ""),
    ("forestgen", "[args:text]"),
    ("pumpkins", "[size:integer]"),
];

/// The typed usages, one per sub-command path: the names a command answers to (aliases too), the flags
/// its usages may carry (only those this server implements, in the order listed) and the argument
/// paths. In a path, `{dir}` stands for each direction word and `{flags}` for each set of the flags.
/// Typed arguments: `integer`, `number`, `boolean`, `player_name`, `word` (a block, a mask or a pattern
/// without a space), `text` (the rest of the line, always last). Literals are the fixed choices.
const TYPED: &[(&[&str], &str, &[&str])] = &[
    (&["we", "worldedit"], "", &["", "version", "help"]),
    (&["wand"], "n", &["{flags}"]),
    (&["pos1", "pos2"], "", &["", "<x:word> <y:word> <z:word>"]),
    (&["hpos1", "hpos2"], "", &[""]),
    (&["sel"], "d", &["{flags}", "{flags} cuboid", "{flags} extend", "{flags} poly", "{flags} polyextend", "{flags} ellipsoid", "{flags} sphere", "{flags} cyl", "{flags} convex", "{flags} hull", "{flags} polyhedron"]),
    (&["desel", "deselect", "naturalize", "clearclipboard", "clearhistory", "none"], "", &[""]),
    (&["size"], "c", &["{flags}"]),
    (&["chunk"], "sc", &["{flags}", "{flags} <x:word> <y:word> <z:word>"]),
    (&["distr"], "cdmp", &["{flags}"]),
    (
        &["expand"],
        "",
        &["vert", "<amount:integer>", "<amount:integer> {dir}", "<amount:integer> <reverse:integer>", "<amount:integer> <reverse:integer> {dir}"],
    ),
    (&["contract"], "", &["<amount:integer>", "<amount:integer> {dir}", "<amount:integer> <reverse:integer>", "<amount:integer> <reverse:integer> {dir}"]),
    (&["shift"], "", &["<amount:integer>", "<amount:integer> {dir}"]),
    (&["outset", "inset"], "hv", &["{flags} <amount:integer>"]),
    (&["count"], "", &["<filter:mask>"]),
    (&["set", "walls", "faces", "outline", "center", "middle", "overlay"], "", &["<block:pattern>"]),
    (&["replace", "re", "rep"], "", &["<to:pattern>", "<from:mask> <to:pattern>"]),
    (&["hollow"], "", &["", "<thickness:integer>", "<thickness:integer> <block:pattern>"]),
    (&["smooth"], "", &["", "<iterations:integer>", "<iterations:integer> <filter:mask>"]),
    (&["snowsmooth"], "lm", &["{flags}", "<iterations:integer> {flags}"]),
    (&["line"], "h", &["{flags} <block:pattern>", "{flags} <block:pattern> <thickness:number>"]),
    (&["curve"], "h", &["{flags} <block:pattern>", "{flags} <block:pattern> <thickness:number>"]),
    (&["revolve"], "reb", &["<count:integer>", "{flags} <count:integer>"]),
    (&["stack"], "asrbe", &["{flags}", "<count:integer> {flags}", "<count:integer> {dir} {flags}"]),
    (
        &["move"],
        "asbe",
        &["{flags}", "<distance:integer> {flags}", "<distance:integer> {dir} {flags}", "<distance:integer> {dir} <leave:pattern> {flags}"],
    ),
    (&["sphere", "hsphere"], "hr", &["{flags} <block:pattern> <radii:word> {flags}"]),
    (&["cyl", "hcyl"], "h", &["{flags} <block:pattern> <radii:word> {flags}", "{flags} <block:pattern> <radii:word> <height:integer> {flags}"]),
    (&["removeabove", "removebelow"], "", &["", "<size:integer>", "<size:integer> <height:integer>"]),
    (&["removenear"], "", &["<mask:mask>", "<mask:mask> <radius:integer>"]),
    (&["replacenear"], "", &["<radius:integer> <to:pattern>", "<radius:integer> <from:mask> <to:pattern>"]),
    (
        &["cone"],
        "h",
        &["{flags} <block:pattern> <radii:word> {flags}", "{flags} <block:pattern> <radii:word> <height:integer> {flags}", "{flags} <block:pattern> <radii:word> <height:integer> <thickness:number>"],
    ),
    (&["pyramid", "hpyramid"], "h", &["{flags} <block:pattern> <size:integer> {flags}"]),
    (&["copy"], "m", &["{flags}"]),
    (&["cut"], "m", &["{flags}", "<leave:pattern> {flags}"]),
    (&["paste"], "aonsm", &["{flags}"]),
    (&["rot"], "", &["90", "180", "270", "<degrees:number>"]),
    (&["flip", "mirror"], "", &["", "{dir}"]),
    (&["undo", "redo"], "", &["", "<times:integer>", "<times:integer> <player:player>"]),
    (&["schem", "schematic"], "f", &["list", "list -p <page:integer>", "list -d", "list -n", "formats", "load <name:schematic>", "delete <name:schematic>", "save {flags} <name:text>"]),
    (&["load"], "", &["<name:schematic>"]),
    (
        &["brush", "br"],
        "h",
        &[
            "none",
            "unbind",
            "sphere {flags} <block:pattern>",
            "sphere {flags} <block:pattern> <radius:number>",
            "cylinder {flags} <block:pattern>",
            "cylinder {flags} <block:pattern> <radius:number>",
            "cylinder {flags} <block:pattern> <radius:number> <height:integer>",
            "smooth",
            "smooth <radius:number>",
            "smooth <radius:number> <iterations:integer>",
            "smooth <radius:number> <iterations:integer> <filter:mask>",
            "snowsmooth",
            "snowsmooth <radius:number>",
            "snowsmooth <radius:number> <iterations:integer>",
            "clipboard",
            "copy",
            "extinguish",
            "extinguish <radius:number>",
            "ex",
            "ex <radius:number>",
            "splatter <block:pattern>",
            "splatter <block:pattern> <radius:number>",
            "splatter <block:pattern> <radius:number> <decay:integer>",
            "gravity",
            "gravity <radius:number>",
            "set <shape:word> <radius:number> <block:pattern>",
            "forest <shape:word> <type:word>",
            "forest <shape:word> <radius:number> <type:word>",
            "forest <shape:word> <radius:number> <density:number> <type:word>",
            "feature <shape:word> <type:word>",
            "feature <shape:word> <radius:number> <type:word>",
            "feature <shape:word> <radius:number> <density:number> <type:word>",
            "biome {flags} <shape:word> <biome:word>",
            "biome {flags} <shape:word> <radius:number> <biome:word>",
            "raise <shape:word>",
            "raise <shape:word> <radius:number>",
            "lower <shape:word>",
            "lower <shape:word> <radius:number>",
            "snow <shape:word>",
            "snow <shape:word> <radius:number>",
            "morph",
            "morph <size:number>",
            "morph <size:number> <min-erode:integer> <erode-iterations:integer> <min-dilate:integer> <dilate-iterations:integer>",
            "erode",
            "erode <size:number>",
            "dilate",
            "dilate <size:number>",
            "butcher",
            "butcher <radius:number>",
            "heightmap <image:word>",
            "heightmap <image:word> <radius:number>",
            "heightmap <image:word> <radius:number> <intensity:number>",
            "deform <shape:word>",
            "deform <shape:word> <radius:number>",
            "deform <shape:word> <radius:number> <expression:text>",
            "apply <shape:word> <radius:number> forest <type:word>",
            "apply <shape:word> <radius:number> item <item:word>",
            "apply <shape:word> <radius:number> item <item:word> <direction:word>",
            "apply <shape:word> <radius:number> set <block:pattern>",
            "paint <shape:word> <radius:number> <density:number> forest <type:word>",
            "paint <shape:word> <radius:number> <density:number> item <item:word>",
            "paint <shape:word> <radius:number> <density:number> item <item:word> <direction:word>",
            "paint <shape:word> <radius:number> <density:number> set <block:pattern>",
        ],
    ),
    (&["mask"], "", &["", "<filter:mask>"]),
    (&["gmask"], "", &["", "<filter:mask>"]),
    (&["material"], "", &["<block:pattern>"]),
    (&["tracemask"], "", &["", "<filter:mask>"]),
    (&["superpickaxe", "pickaxe", "sp"], "", &["single", "area <range:integer>", "recursive <range:integer>", "recur <range:integer>"]),
    (&["tool"], "", &["none", "unbind", "selwand", "info", "navwand", "cycler", "farwand", "tree", "tree <type:word>", "deltree", "stacker", "stacker <range:integer>", "stacker <range:integer> <mask:mask>", "lrbuild <primary:pattern> <secondary:pattern>", "repl <pattern:pattern>", "floodfill <pattern:pattern> <range:integer>", "flood <pattern:pattern> <range:integer>"]),
    (&["we", "worldedit"], "", &["reload", "report", "trace", "cui", "ver", "version", "tz <timezone:word>", "help", "help <command:command>"]),
    (&["fixwater", "fixlava"], "", &["<radius:number>"]),
    (&["drain"], "w", &["{flags} <radius:number>"]),
    (&["fillr"], "", &["<pattern:pattern> <radius:number>", "<pattern:pattern> <radius:number> <depth:integer>"]),
    (&["snow"], "s", &["{flags}", "{flags} <size:number>", "{flags} <size:number> <height:integer>"]),
    (&["thaw"], "", &["", "<size:number>", "<size:number> <height:integer>"]),
    (&["green"], "f", &["{flags}", "{flags} <size:number>", "{flags} <size:number> <height:integer>"]),
    (&["extinguish", "ex", "ext"], "", &["", "<radius:integer>"]),
    (&["limit"], "", &["", "<limit:integer>"]),
    (&["unstuck", "thru", "jumpto", "j"], "", &[""]),
    (&["ascend", "asc", "descend", "desc"], "", &["", "<levels:integer>"]),
    (&["ceil"], "fg", &["{flags}", "{flags} <clearance:integer>"]),
    (&["up"], "fg", &["{flags} <distance:integer>"]),
    (&["selwand", "info", "cycler"], "", &[""]),
    (&["repl"], "", &["<pattern:pattern>"]),
    (&["floodfill", "flood"], "", &["<pattern:pattern> <range:integer>"]),
    (&["trim"], "", &["", "<filter:mask>"]),
    (&["chunkinfo"], "", &[""]),
    (&["listchunks"], "p", &["{flags}"]),
    (&["biomelist", "biomels"], "p", &["{flags}"]),
    (&["biomeinfo"], "tp", &["{flags}"]),
    (&["setbiome"], "p", &["{flags} <biome:word>"]),
    (&["replacebiome"], "p", &["{flags} <from:mask> <biome:word>"]),
    (&["regen"], "bc", &["{flags}", "{flags} <seed:integer>"]),
    (&["forest"], "", &["", "<type:word>", "<type:word> <density:number>"]),
    (&["flora"], "", &["", "<density:number>"]),
    (&["delchunks"], "o", &["{flags}"]),
    (&["world"], "", &["", "<world:word>"]),
    (&["placement"], "", &["pos1", "player", "here", "world", "min", "max", "pos1 <multiplier:integer>", "player <multiplier:integer>", "here <multiplier:integer>", "world <multiplier:integer>", "min <multiplier:integer>", "max <multiplier:integer>", "pos1 <multiplier:integer> {dir}", "player <multiplier:integer> {dir}", "here <multiplier:integer> {dir}", "world <multiplier:integer> {dir}", "min <multiplier:integer> {dir}", "max <multiplier:integer> {dir}"]),
    (&["generate", "gen", "g"], "hroc", &["{flags} <block:pattern> <expression:text>"]),
    (&["deform"], "rocl", &["{flags} <expression:text>"]),
    (&["butcher"], "pnagbtfrw", &["{flags}", "{flags} <radius:integer>"]),
    (&["remove", "rem", "rement"], "", &["<type:word> <radius:integer>"]),
    (&["searchitem", "search", "l"], "bip", &["{flags} <query:text>"]),
    (&["timeout"], "", &["", "<limit:integer>"]),
    (&["fast"], "", &["", "on", "off"]),
    (&["perf"], "h", &["{flags}", "{flags} <sideeffect:word>", "{flags} <sideeffect:word> <state:word>"]),
    (&["reorder"], "", &["", "multi", "fast", "none"]),
    (&["drawsel"], "", &["", "on", "off"]),
    (&["watchdog"], "", &["", "active", "inactive"]),
    (&["registry"], "p", &["{flags} <registry:word> <query:text>"]),
    (&["snapshot", "snap"], "p", &["list", "list -p <page:integer>", "use <name:text>", "sel <index:integer>", "before <date:text>", "after <date:text>"]),
    (&["restore"], "", &["", "<snapshot:text>"]),
    (&["pos"], "", &["", "<x:word> <y:word> <z:word>", "<position:word>"]),
    (&["update"], "", &["", "<sideeffects:word>"]),
    (&["feature"], "", &["<feature:word>"]),
    (&["structure"], "", &["<structure:word>"]),
    (&["generatebiome", "genbiome", "gb"], "hroc", &["{flags} <biome:word> <expression:text>"]),
    (&["toggleeditwand"], "", &[""]),
    (&["forestgen"], "", &["", "<size:integer>", "<size:integer> <type:word>", "<size:integer> <type:word> <density:number>"]),
    (&["pumpkins"], "", &["", "<size:integer>"]),
];

/// The direction words the usages list. The short forms (`n`, `u`, `f`...) are accepted too; the
/// catch-all usage takes them.
const DIRECTION_WORDS: [&str; 10] = ["north", "south", "east", "west", "up", "down", "me", "back", "left", "right"];

/// The registration lines for `native_register_commands`: every sub-command path with its typed
/// arguments and flags, then each command's catch-all `text` usage last. The usages only shape the
/// players' completions; `command` parses the typed line the same way whichever usage it matches.
pub fn usages() -> Vec<String> {
    let mut out = Vec::new();
    for (names, flags, paths) in TYPED {
        let mid = mid_args(paths);
        for name in names.iter() {
            for path in paths.iter() {
                for rest in expand(path, flags) {
                    for root in roots(name) {
                        push_unique(&mut out, tidy(&format!("{root} {}", qualify(&rest, &mid))));
                    }
                }
            }
        }
    }
    for (name, args) in COMMANDS {
        for root in roots(name) {
            push_unique(&mut out, tidy(&format!("{root} {args}")));
        }
    }
    out
}

/// The names of the `pattern` and `mask` arguments that have a word after them in some path of a row (or a
/// `{flags}`/`{dir}` placeholder): one argument name is one kind in the whole row.
fn mid_args(paths: &[&str]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for path in paths {
        let tokens: Vec<&str> = path.split_whitespace().collect();
        for (i, t) in tokens.iter().enumerate() {
            if i + 1 < tokens.len() {
                if let Some(name) = custom_arg(t).map(|(n, _, _)| n) {
                    out.insert(name.to_owned());
                }
            }
        }
    }
    out
}

/// `<name:kind>` / `[name:kind]` of a kind this mod completes itself: its name, kind and brackets.
fn custom_arg(token: &str) -> Option<(&str, &str, (char, char))> {
    for (open, close) in [('<', '>'), ('[', ']')] {
        if let Some(inner) = token.strip_prefix(open).and_then(|t| t.strip_suffix(close)) {
            if let Some((name, kind)) = inner.split_once(':') {
                if matches!(kind, "pattern" | "mask" | "schematic" | "command") {
                    return Some((name, kind, (open, close)));
                }
            }
        }
    }
    None
}

/// A usage with this mod's own argument kinds named in full: `worldedit:pattern` (one word) or
/// `worldedit:pattern_text` (the rest of the line, when the argument is last in every usage of its row),
/// `worldedit:mask[_text]`, `worldedit:schematic` and `worldedit:command` (the rest of the line).
fn qualify(rest: &str, mid: &BTreeSet<String>) -> String {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let last = tokens.len().saturating_sub(1);
    tokens
        .iter()
        .enumerate()
        .map(|(i, t)| match custom_arg(t) {
            Some((name, kind, (open, close))) => {
                // `maskarg` (the `-m` flag's value) is always one word: the flag may come with more flags after it, and one
                // argument name must be one kind in the tree.
                let greedy = matches!(kind, "schematic" | "command") || (i == last && !mid.contains(name) && name != "maskarg");
                let suffix = if greedy && matches!(kind, "pattern" | "mask") { "_text" } else { "" };
                format!("{open}{name}:worldedit:{kind}{suffix}{close}")
            }
            None => (*t).to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The commands WorldEdit registers with a single slash only (`/we`, `/mask`, `/tool`...); the
/// names in `BOTH_FORMS` it registers both ways (`/brush` and `//brush`); every other command is
/// registered with a leading `/` in its name (`@Command(name = "/set")`), which the player types as
/// `//set`.
pub const SINGLE_SLASH: &[&str] = &["we", "worldedit", "mask", "none", "tool", "range", "tracemask", "superpickaxe", "pickaxe", "sp", "clearclipboard", "selwand", "navwand", "farwand", "tree", "deltree", "lrbuild", "info", "cycler", "repl", "floodfill", "flood", "placement", "chunkinfo", "listchunks", "snapshot", "snap", "toggleeditwand", "forestgen", "pumpkins"];
pub const BOTH_FORMS: &[&str] = &[
    "brush", "br", "schem", "schematic", "undo", "redo", "clearhistory", "material", "removeabove", "removebelow", "removenear", "replacenear", "fixwater", "fixlava", "snow", "thaw", "green", "extinguish", "ex", "ext", "gmask",
    "toggleplace", "searchitem", "ascend", "asc", "descend", "desc", "thru", "jumpto", "j", "unstuck", "ceil", "restore",
];

/// The root words a command is registered under: `set` is `/set` (typed `//set`), `brush` both
/// `brush` and `/brush`. Dispatch is unchanged: the game strips the slashes before the mod hears it.
fn roots(name: &str) -> Vec<String> {
    if SINGLE_SLASH.contains(&name) {
        vec![name.to_owned()]
    } else if BOTH_FORMS.contains(&name) {
        vec![name.to_owned(), format!("/{name}")]
    } else {
        vec![format!("/{name}")]
    }
}

/// Whether `line` may run as the form it was typed in: `double` is true when it began with `//`. A command registered
/// as `/set` (typed `//set`) exists only with two slashes, `brush`-like single-form ones only with one, the names in
/// `BOTH_FORMS` either way. `//rotate`, `//fill`, `//help` (the shadowed ones) are double-slash only: the single-slash
/// forms are vanilla's. `/!` is single only.
pub fn form_allows(line: &str, double: bool) -> bool {
    let name = line.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if name == "!" || name == "," {
        return !double;
    }
    // `//` alone toggles the super pickaxe; `/size <number>` is the brush size (`//size` is the selection's).
    if name.is_empty() {
        return double;
    }
    if name == "size" && !double {
        return line.split_whitespace().nth(1).is_some_and(|w| w.parse::<f64>().is_ok());
    }
    match COMMANDS.iter().find(|(n, _)| *n == name) {
        Some((n, _)) => roots(n).iter().any(|r| r.starts_with('/') == double),
        None => double,
    }
}

fn push_unique(out: &mut Vec<String>, usage: String) {
    if !out.contains(&usage) {
        out.push(usage);
    }
}

/// The words with single spaces between them.
fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A path with its placeholders filled in, every way: each direction word for `{dir}`, each set of
/// the flags for `{flags}`.
fn expand(path: &str, flags: &str) -> Vec<String> {
    if path.contains("{dir}") {
        DIRECTION_WORDS.iter().flat_map(|d| expand(&path.replacen("{dir}", d, 1), flags)).collect()
    } else if path.contains("{flags}") {
        flag_sets(flags).iter().flat_map(|set| expand(&path.replacen("{flags}", set, 1), flags)).collect()
    } else {
        vec![path.to_owned()]
    }
}

/// Every set of the given flag letters as words, the empty set first (`-a -s`). `m` takes its mask.
fn flag_sets(letters: &str) -> Vec<String> {
    let letters: Vec<char> = letters.chars().collect();
    (0..(1usize << letters.len()))
        .map(|set| {
            letters
                .iter()
                .enumerate()
                .filter(|&(i, _)| set & (1 << i) != 0)
                .map(|(_, &c)| match c {
                    'm' => "-m <maskarg:mask>".to_owned(),
                    'p' => "-p <page:integer>".to_owned(),
                    'l' => "-l <layers:integer>".to_owned(),
                    _ => format!("-{c}"),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// Whether `line` (slashes stripped or not) is one of these commands.
pub fn is_command(line: &str) -> bool {
    let name = line.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    // `/!` is `/unstuck`; it cannot be a registered word.
    // `/,` and `//` toggle the super pickaxe (neither is a word the game could register).
    let toggle = name == "," || (line.starts_with('/') && name.is_empty());
    name == "!" || toggle || COMMANDS.iter().any(|(n, _)| *n == name) || is_shadowed(line)
}

/// `rotate <number> ...`: WorldEdit's `//rotate`, not vanilla's `/rotate <target> <rotation>`
/// (whose first argument is an entity, never a bare number of degrees).
pub fn is_rotate(line: &str) -> bool {
    let mut w = line.trim_start_matches('/').split_whitespace();
    w.next().is_some_and(|n| n.eq_ignore_ascii_case("rotate")) && w.next().is_some_and(|a| a.parse::<f64>().is_ok())
}

/// The WorldEdit commands vanilla also has, heard before the game parses them (`player_command`):
/// `rotate <degrees>`, `fill <pattern> <radius> [depth]` (vanilla's `/fill` takes seven words or
/// more) and `help <worldedit command>` / `help -s|-p ...`. A bare `//help` stays the game's: the
/// slashes are gone by the time the line is heard, so `/help` and `//help` look alike. `/we help`
/// is always WorldEdit's.
pub fn is_shadowed(line: &str) -> bool {
    if is_rotate(line) {
        return true;
    }
    let words: Vec<&str> = line.trim_start_matches('/').split_whitespace().collect();
    match words.first().map(|w| w.to_ascii_lowercase()).as_deref() {
        Some("fill") => (3..=4).contains(&words.len()) && words[2].parse::<f64>().is_ok(),
        Some("help") => words.get(1).is_some_and(|a| a.starts_with('-') || help::knows(a)),
        _ => false,
    }
}

/// The words of a command, `-a`-style flags apart, and the mask of `-m <mask>`.
struct Args<'a> {
    words: Vec<&'a str>,
    flags: Vec<char>,
    /// The values of the flags that take one: `-m <mask>`, `-p <page>`.
    values: Vec<(char, String)>,
}

/// The flags each command takes: (accepted, implemented). An accepted flag this server does not
/// implement is refused by name; a flag not accepted at all gets the usage line.
fn flag_spec(name: &str) -> (&'static str, &'static str) {
    match name {
        "copy" | "cut" => ("ebm", "ebm"),
        "paste" => ("abenosvm", "aonsmeb"),
        "stack" => ("abersm", "abersm"),
        "move" => ("abesm", "abesm"),
        "sphere" | "hsphere" => ("hr", "hr"),
        "cyl" | "hcyl" | "pyramid" | "hpyramid" | "line" | "curve" | "cone" => ("h", "h"),
        "brush" | "br" => ("hlmaovbepngtfrws", "hlmaovepngbtfrws"),
        "revolve" => ("mreb", "mreb"),
        "outset" | "inset" => ("hv", "hv"),
        "schem" | "schematic" => ("fdnp", "fdnp"),
        "distr" => ("cdpm", "cdpm"),
        "snowsmooth" => ("lm", "lm"),
        "chunk" => ("sc", "sc"),
        "size" => ("c", "c"),
        "generate" | "gen" | "g" | "generatebiome" => ("hroc", "hroc"),
        "pos" => ("s", "s"),
        "deform" => ("rocl", "rocl"),
        "butcher" => ("pnagbtfrw", "pnagbtfrw"),
        "wand" => ("n", "n"),
        "sel" => ("d", "d"),
        "up" | "ceil" => ("fg", "fg"),
        "drain" => ("w", "w"),
        "snow" => ("s", "s"),
        "green" => ("f", "f"),
        "searchitem" => ("bip", "bip"),
        "listchunks" => ("p", "p"),
        "biomelist" | "biomels" => ("p", "p"),
        "biomeinfo" => ("tp", "tp"),
        "setbiome" | "replacebiome" => ("p", "p"),
        "regen" => ("bc", "bc"),
        "delchunks" => ("o", "o"),
        "perf" => ("h", "h"),
        "registry" => ("p", "p"),
        "snapshot" | "snap" => ("p", "p"),
        "help" => ("sp", "sp"),
        "report" => ("p", ""),
        _ => ("", ""),
    }
}

/// The usage line of a command, as the chat completion shows it.
fn usage(name: &str) -> String {
    let args = match name {
        "set" | "walls" | "faces" | "outline" | "center" | "middle" | "overlay" | "material" => "<pattern>".to_owned(),
        "replace" | "re" | "rep" => "[from] <to>".into(),
        "expand" => "<amount> [reverse-amount] [direction]".into(),
        "contract" => "<amount> [reverse-amount] [direction]".into(),
        "shift" => "<amount> [direction]".into(),
        "outset" | "inset" => "<amount>".into(),
        "count" => "<mask>".into(),
        "hollow" => "[thickness] [pattern]".into(),
        "smooth" => "[iterations] [mask]".into(),
        "snowsmooth" => "[iterations]".into(),
        "line" => "<pattern> [thickness]".into(),
        "curve" => "<pattern> [thickness]".into(),
        "revolve" => "<count> [-m mask] [-r]".into(),
        "stack" => "[count] [offset]".into(),
        "move" => "[multiplier] [offset] [replace]".into(),
        "sphere" | "hsphere" => "<pattern> <radii>".into(),
        "cyl" | "hcyl" => "<pattern> <radii> [height]".into(),
        "cone" => "<pattern> <radii> [height] [thickness]".into(),
        "removeabove" | "removebelow" => "[size] [height]".into(),
        "removenear" => "<mask> [radius]".into(),
        "replacenear" => "<radius> [from] <to>".into(),
        "pyramid" | "hpyramid" => "<pattern> <size>".into(),
        "cut" => "[leave-pattern]".into(),
        "rot" | "rotate" => "<rotate-y> [rotate-x] [rotate-z]".into(),
        "flip" | "mirror" => "[direction]".into(),
        "pos1" | "pos2" => "[coordinates]".into(),
        "pos" => "[pos1] [pos2...] [-s selector]".into(),
        "update" => "[side-effects]".into(),
        "feature" => "<feature>".into(),
        "structure" => "<structure>".into(),
        "generatebiome" => "<biome> <expression...>".into(),
        "forestgen" => "[size] [type] [density]".into(),
        "pumpkins" => "[size]".into(),
        "brush deform" => "<shape> [radius] [expression] [-r] [-o]".into(),
        "chunk" => "[coordinates]".into(),
        "generate" | "gen" | "g" => "<pattern> <expression...>".into(),
        "deform" => "<expression...>".into(),
        "butcher" => "[radius]".into(),
        "remove" | "rem" | "rement" => "<type> <radius>".into(),
        "sel" => "[cuboid|extend|poly|polyextend|ellipsoid|sphere|cyl|convex]".into(),
        "brush" | "br" => "<sphere|cylinder|splatter|smooth|snowsmooth|clipboard|extinguish|gravity|set|raise|lower|snow|morph|erode|dilate|butcher|heightmap|none> ...".into(),
        "brush snow" | "brush raise" | "brush lower" => "<cuboid|sphere|cylinder> [radius]".into(),
        "brush set" => "<cuboid|sphere|cylinder> [radius] <pattern>".into(),
        "brush morph" => "[size] [min-erode-faces] [erode-iterations] [min-dilate-faces] [dilate-iterations]".into(),
        "brush erode" | "brush dilate" | "brush butcher" => "[radius]".into(),
        "brush heightmap" => "<image> [radius] [intensity]".into(),
        "brush sphere" => "<pattern> [radius]".into(),
        "brush cylinder" => "<pattern> [radius] [height]".into(),
        "fill" | "fillr" => "<pattern> <radius> [depth]".into(),
        "drain" => "<radius>".into(),
        "fixwater" | "fixlava" => "<radius>".into(),
        "snow" | "thaw" | "green" => "[size] [height]".into(),
        "setbiome" => "<biome>".into(),
        "replacebiome" => "<from-mask> <biome>".into(),
        "forest" => "[type] [density]".into(),
        "flora" => "[density]".into(),
        "world" => "[world]".into(),
        "extinguish" => "[radius]".into(),
        "limit" => "[limit]".into(),
        "up" => "<distance>".into(),
        "ceil" => "[clearance]".into(),
        "ascend" | "descend" => "[levels]".into(),
        "placement" => "<pos1|player|here|world|min|max> [multiplier] [offset]".into(),
        "gmask" => "[mask]".into(),
        "searchitem" => "<query>".into(),
        "trim" => "[mask]".into(),
        "calc" => "<expression>".into(),
        "help" => "[command...]".into(),
        "tool" => "<none|selwand|navwand|info|tree|deltree|stacker|repl|cycler|floodfill|farwand|lrbuild> ...".into(),
        "tool repl" => "<pattern>".into(),
        "tool lrbuild" => "<primary> <secondary>".into(),
        "range" => "<range>".into(),
        "size" => "<size>".into(),
        "tracemask" => "[mask]".into(),
        "superpickaxe" => "<single|area|recursive> ...".into(),
        "superpickaxe area" | "superpickaxe recursive" => "<range>".into(),
        "tool floodfill" => "<pattern> <range>".into(),
        "schem load" | "schem save" | "schem delete" => "<filename>".into(),
        "schem" | "schematic" => "<load|save|delete|list|formats>".into(),
        _ => COMMANDS.iter().find(|(n, _)| *n == name).map(|(_, a)| a.replace(":text", "").replace(":word", "").replace(":integer", "").replace("patternarg", "pattern")).unwrap_or_default(),
    };
    let base = name.split(' ').next().unwrap_or(name);
    let sub = name.strip_prefix(base).unwrap_or("");
    // Only the flags this server implements: an accepted but unsupported one is named in its own error.
    let (_, implemented) = flag_spec(base);
    let flags = if implemented.is_empty() { String::new() } else { format!(" [-{implemented}]") };
    format!("Usage: //{base}{sub}{flags} {}", args.trim()).trim_end().to_string()
}

/// The flags of one `/brush` sub-command, as BrushCommands declares them: (accepted, implemented, the ones that take a
/// value). `-h` is a switch on the sphere and the cylinder but takes the height on the gravity brush; `-e`/`-b` of the
/// clipboard brush (entities, biomes) are accepted and not implemented.
fn brush_flags(sub: &str) -> (&'static str, &'static str, &'static str) {
    match sub {
        "sphere" | "s" | "cylinder" | "cyl" | "c" => ("h", "h", ""),
        "clipboard" | "copy" => ("avoebm", "avoebm", "m"),
        "snowsmooth" => ("lm", "lm", "lm"),
        "gravity" | "grav" => ("h", "h", "h"),
        "snow" => ("s", "s", ""),
        "butcher" | "kill" => ("pngabtfrw", "pngabtfrw", ""),
        "heightmap" => ("efr", "efr", ""),
        "biome" => ("c", "c", ""),
        "deform" => ("ro", "ro", ""),
        _ => ("", "", ""),
    }
}

/// Whether flag `c` of command `name` is followed by a value.
fn takes_value(name: &str, c: char) -> bool {
    c == 'm' || (c == 's' && name == "pos") || (c == 'o' && name == "delchunks") || (c == 'l' && matches!(name, "snowsmooth" | "brush" | "br")) || (c == 'p' && matches!(name, "distr" | "schem" | "schematic" | "searchitem" | "help" | "listchunks" | "biomelist" | "biomels" | "registry" | "snapshot" | "snap"))
}

/// Splits a command's words from its flags. Unknown flags and flags this server does not implement
/// are errors, never silently dropped.
fn parse_args<'a>(name: &str, rest: &[&'a str]) -> Result<Args<'a>, String> {
    if name == "calc" {
        // An expression: `-` is a minus there, never a flag.
        return Ok(Args { words: rest.to_vec(), flags: Vec::new(), values: Vec::new() });
    }
    let is_brush = matches!(name, "brush" | "br");
    let (accepted, implemented, valued) = if is_brush {
        brush_flags(&rest.first().filter(|w| !w.starts_with('-')).map(|w| w.to_ascii_lowercase()).unwrap_or_default())
    } else {
        let (a, i) = flag_spec(name);
        (a, i, "")
    };
    let mut words = Vec::new();
    let mut flags = Vec::new();
    let mut values: Vec<(char, String)> = Vec::new();
    let mut want: Option<char> = None;
    for w in rest {
        let is_flag = w.len() >= 2 && w.starts_with('-') && w[1..].chars().all(|c| c.is_ascii_alphabetic());
        if let Some(c) = want.filter(|_| !is_flag) {
            values.push((c, w.to_string()));
            want = None;
        } else if !is_flag {
            words.push(*w);
        } else {
            if want.is_some() {
                return Err(usage(name));
            }
            for c in w[1..].chars() {
                if !accepted.contains(c) {
                    return Err(usage(name));
                }
                if !implemented.contains(c) {
                    return Err(format!("-{c} is not supported on this server"));
                }
                if if is_brush { valued.contains(c) } else { takes_value(name, c) } {
                    want = Some(c);
                }
                flags.push(c);
            }
        }
    }
    if want.is_some() {
        return Err(usage(name));
    }
    Ok(Args { words, flags, values })
}

/// The worldedit.* node a command needs (None: no permission check beyond the base level). The
/// nodes are the `@CommandPermissions` of WorldEdit's command classes.
fn permission_node(name: &str, args: &Args, actor: &Actor) -> Option<String> {
    let own = |who: Option<&str>| who.is_none_or(|w| w.eq_ignore_ascii_case(&actor.name));
    let node: &str = match name {
        "we" | "worldedit" => match args.get(0).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("reload") => "worldedit.reload",
            Some("report") => "worldedit.report",
            Some("help") => "worldedit.help",
            _ => return None,
        },
        "tool" => match args.get(0).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("selwand" | "/selwand") => "worldedit.setwand",
            Some("info") => "worldedit.tool.info",
            Some("tree") => "worldedit.tool.tree",
            Some("deltree") => "worldedit.tool.deltree",
            Some("stacker") => "worldedit.tool.stack",
            Some("lrbuild" | "/lrbuild") => "worldedit.tool.lrbuild",
            Some("repl") => "worldedit.tool.replacer",
            Some("cycler") => "worldedit.tool.data-cycler",
            Some("floodfill" | "flood") => "worldedit.tool.flood-fill",
            Some("farwand") => "worldedit.tool.farwand",
            Some("navwand" | "/navwand") => "worldedit.setwand",
            _ => return None,
        },
        "none" => return None,
        "sel" | "desel" | "deselect" => "worldedit.selection.sel",
        "version" | "ver" => return None,
        "trim" => "worldedit.selection.trim",
        "help" => "worldedit.help",
        "wand" => "worldedit.wand",
        "pos1" | "pos2" | "pos" => "worldedit.selection.pos",
        "toggleeditwand" => "worldedit.wand.toggle",
        "generatebiome" => "worldedit.generation.shape.biome",
        "update" => "worldedit.update",
        "feature" => "worldedit.generation.feature",
        "structure" => "worldedit.generation.structure",
        "forestgen" => "worldedit.generation.forest",
        "pumpkins" => "worldedit.generation.pumpkins",
        "hpos1" | "hpos2" => "worldedit.selection.hpos",
        "chunk" => "worldedit.selection.chunk",
        "expand" => "worldedit.selection.expand",
        "contract" => "worldedit.selection.contract",
        "shift" => "worldedit.selection.shift",
        "outset" => "worldedit.selection.outset",
        "inset" => "worldedit.selection.inset",
        "size" => "worldedit.selection.size",
        "count" => "worldedit.analysis.count",
        "distr" => "worldedit.analysis.distr",
        "set" => "worldedit.region.set",
        "line" => "worldedit.region.line",
        "curve" => "worldedit.region.curve",
        "revolve" => "worldedit.revolve",
        "replace" | "re" | "rep" => "worldedit.region.replace",
        "overlay" => "worldedit.region.overlay",
        "center" | "middle" => "worldedit.region.center",
        "naturalize" => "worldedit.region.naturalize",
        "walls" => "worldedit.region.walls",
        "faces" | "outline" => "worldedit.region.faces",
        "smooth" => "worldedit.region.smooth",
        "snowsmooth" => "worldedit.region.snowsmooth",
        "move" => "worldedit.region.move",
        "stack" => "worldedit.region.stack",
        "hollow" => "worldedit.region.hollow",
        "biomelist" | "biomels" => "worldedit.biome.list",
        "biomeinfo" => "worldedit.biome.info",
        "setbiome" | "replacebiome" => "worldedit.biome.set",
        "regen" => "worldedit.regen",
        "forest" => "worldedit.region.forest",
        "flora" => "worldedit.region.flora",
        "delchunks" => "worldedit.delchunks",
        "world" => "worldedit.world",
        "sphere" | "hsphere" => "worldedit.generation.sphere",
        "cyl" | "hcyl" => "worldedit.generation.cylinder",
        "cone" => "worldedit.generation.cone",
        "pyramid" | "hpyramid" => "worldedit.generation.pyramid",
        "copy" => "worldedit.clipboard.copy",
        "cut" => "worldedit.clipboard.cut",
        "paste" => "worldedit.clipboard.paste",
        "rot" | "rotate" => "worldedit.clipboard.rotate",
        "flip" | "mirror" => "worldedit.clipboard.flip",
        "clearclipboard" => "worldedit.clipboard.clear",
        "clearhistory" => "worldedit.history.clear",
        // WorldEdit accepts either node for the player's own history; another player's needs `.other`.
        "undo" => if own(args.get(1)) { "worldedit.history.undo|worldedit.history.undo.self" } else { "worldedit.history.undo.other" },
        "redo" => if own(args.get(1)) { "worldedit.history.redo|worldedit.history.redo.self" } else { "worldedit.history.redo.other" },
        "schem" | "schematic" => match args.get(0).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("load") => "worldedit.clipboard.load|worldedit.schematic.load",
            Some("save") => "worldedit.clipboard.save|worldedit.schematic.save",
            Some("delete" | "d") => "worldedit.schematic.delete",
            Some("formats" | "listformats" | "f") => "worldedit.schematic.formats",
            _ => "worldedit.schematic.list",
        },
        "brush" | "br" => match args.get(0).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("sphere" | "s") => "worldedit.brush.sphere",
            Some("cylinder" | "cyl" | "c") => "worldedit.brush.cylinder",
            Some("smooth") => "worldedit.brush.smooth",
            Some("splatter" | "splat") => "worldedit.brush.splatter",
            Some("forest") => "worldedit.brush.forest",
            Some("feature") => "worldedit.brush.feature",
            Some("biome") => "worldedit.brush.biome",
            Some("gravity" | "grav") => "worldedit.brush.gravity",
            Some("set") => "worldedit.brush.set",
            Some("raise") => "worldedit.brush.raise",
            Some("lower") => "worldedit.brush.lower",
            Some("snowsmooth") => "worldedit.brush.snowsmooth",
            Some("snow") => "worldedit.brush.snow",
            Some("morph" | "erode" | "dilate") => "worldedit.brush.morph",
            Some("butcher" | "kill") => "worldedit.brush.butcher",
            Some("heightmap") => "worldedit.brush.heightmap",
            Some("deform") => "worldedit.brush.deform",
            Some("apply") => "worldedit.brush.apply",
            Some("paint") => "worldedit.brush.paint",
            Some("clipboard" | "copy") => "worldedit.brush.clipboard",
            Some("extinguish" | "ex") => "worldedit.brush.ex",
            _ => return None,
        },
        "fill" => "worldedit.fill",
        "fillr" => "worldedit.fill.recursive",
        "drain" => "worldedit.drain",
        "fixwater" => "worldedit.fixwater",
        "fixlava" => "worldedit.fixlava",
        "snow" => "worldedit.snow",
        "thaw" => "worldedit.thaw",
        "green" => "worldedit.green",
        "extinguish" => "worldedit.extinguish",
        "limit" => "worldedit.limit",
        "gmask" => "worldedit.global-mask",
        "toggleplace" => return None,
        "placement" => "worldedit.placement",
        "up" => "worldedit.navigation.up",
        "ascend" => "worldedit.navigation.ascend",
        "descend" => "worldedit.navigation.descend",
        "thru" => "worldedit.navigation.thru.command",
        "jumpto" => "worldedit.navigation.jumpto.command",
        "unstuck" => "worldedit.navigation.unstuck",
        "ceil" => "worldedit.navigation.ceiling",
        "searchitem" => "worldedit.searchitem",
        "calc" => "worldedit.calc",
        "generate" => "worldedit.generation.shape",
        "deform" => "worldedit.region.deform",
        "mask" => "worldedit.brush.options.mask",
        "material" => "worldedit.brush.options.material",
        "range" => "worldedit.brush.options.range",
        "brushsize" => "worldedit.brush.options.size",
        "tracemask" => "worldedit.brush.options.tracemask",
        "superpickaxe" | "pickaxe" | "sp" => match args.get(0).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("area") => "worldedit.superpickaxe.area",
            Some("recursive" | "recur") => "worldedit.superpickaxe.recursive",
            _ => "worldedit.superpickaxe",
        },
        "togglepickaxe" => "worldedit.superpickaxe",
        "snapshot" | "snap" => match args.get(0).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("list") => "worldedit.snapshots.list",
            _ => "worldedit.snapshots.restore",
        },
        "restore" => "worldedit.snapshots.restore",
        other => return Some(format!("worldedit.{other}")),
    };
    Some(node.to_string())
}

impl<'a> Args<'a> {
    fn flag(&self, c: char) -> bool {
        self.flags.contains(&c)
    }
    fn value(&self, c: char) -> Option<&str> {
        self.values.iter().find(|(f, _)| *f == c).map(|(_, v)| v.as_str())
    }
    fn get(&self, i: usize) -> Option<&'a str> {
        self.words.get(i).copied()
    }
    fn int(&self, i: usize, default: i32) -> Result<i32, String> {
        match self.get(i) {
            None => Ok(default),
            Some(w) => w.parse::<i32>().map_err(|_| number_error(w)),
        }
    }
    fn rest(&self, from: usize) -> String {
        self.words.get(from..).map(|w| w.join(" ")).unwrap_or_default()
    }
}

/// The blocks between two corners, cut to the build height as WorldEdit's region is.
fn volume_of(world: &dyn World, a: Pos, b: Pos) -> u64 {
    Cuboid::new(a, b).clamp_y(world.min_y(), world.max_y()).map_or(0, |c| c.volume())
}

/// `(x, y, z)`, as WorldEdit writes a block position.
fn pt(p: Pos) -> String {
    format!("({}, {}, {})", p[0], p[1], p[2])
}

/// A double as Java prints it: `5.0`, `2.5`.
fn java_double(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e7 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// The distance between two block positions.
fn distance(a: Pos, b: Pos) -> f64 {
    let d = [f64::from(a[0] - b[0]), f64::from(a[1] - b[1]), f64::from(a[2] - b[2])];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// WorldEdit's `worldedit.error.invalid-number.matches`.
fn number_error(text: &str) -> String {
    format!("Number expected; string \"{text}\" given.")
}

/// `minecraft:oak_planks` as `Oak Planks` (a block's name as the game shows it).
fn block_title(id: &str) -> String {
    id.trim_start_matches("minecraft:")
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `3`, `3,5` or `3,5,2`: the radii of a shape (a command checks how many it takes).
fn radii(text: &str) -> Result<Vec<f64>, String> {
    text.split(',').map(|p| p.trim().parse::<f64>().map_err(|_| number_error(p.trim()))).collect()
}

pub struct Engine {
    pub config: Config,
    sessions: Sessions,
    queue: Queue,
    /// The last wand click of each player: position, which button, when (ms).
    last_click: HashMap<u128, (Pos, bool, i64)>,
    /// Players who used WorldEdit since the server started: lower-case name to UUID (`//undo <player>`).
    names: HashMap<String, u128>,
    /// Players whose selection another mod changed (`select`): their outline is drawn again.
    outline_due: Vec<u128>,
    /// The clock of the last command or click (ms): the time an edit is remembered with.
    now_ms: i64,
    /// Players whose `useServerCUI` is off (their session file): no selection outline for them.
    cui_off: BTreeSet<u128>,
    /// How many blocks the edits of the command being run changed (for `debug`'s timing line).
    changed_now: usize,
    /// The inventory change and the blocks missing from a bag edit, by player, until `take_bag` hands them over.
    bag: HashMap<u128, (std::collections::BTreeMap<String, i64>, std::collections::BTreeMap<String, u32>)>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

type R = Result<Vec<String>, String>;

impl Engine {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            sessions: Sessions::default(),
            queue: Queue::default(),
            last_click: HashMap::new(),
            names: HashMap::new(),
            outline_due: Vec::new(),
            now_ms: 0,
            cui_off: BTreeSet::new(),
            changed_now: 0,
            bag: HashMap::new(),
        }
    }

    /// The players whose `useServerCUI` is off, replacing the set (`configure_original` reads the session files).
    pub fn set_cui_off(&mut self, off: BTreeSet<u128>) {
        self.cui_off = off;
    }
    /// The world closed: sessions and unfinished edits go with it.
    pub fn closed(&mut self) {
        self.sessions.clear();
        self.queue.clear();
        self.last_click.clear();
    }

    /// Whether the actor may use what needs `node` (`a|b`: either one; no node: operators only). An operator at
    /// the level the node needs (`node_levels`, else `permission_level`) may; anyone else needs the node granted
    /// by the permission system (`Actor::granted`). No other player gets WorldEdit.
    fn permitted(&self, actor: &Actor, node: Option<&str>) -> Result<(), String> {
        let nodes: Vec<&str> = node.map(|n| n.split('|').collect()).unwrap_or_default();
        let level = nodes.iter().map(|n| self.node_level(n)).min().unwrap_or(self.config.permission_level);
        // `no-op-permissions`: operators are held to the permission system like everyone else (a level of 0 is everyone).
        let by_level = actor.permission >= level && (!self.config.no_op_permissions || level == 0);
        if by_level || nodes.iter().any(|n| actor.granted.contains(*n)) {
            return Ok(());
        }
        Err("You are not permitted to do that. Are you in the right mode?".into())
    }

    /// The operator level that holds `node` without the permission system: `node_levels`, else 4 for the nodes that lift
    /// a limit (`FULL_OPERATOR_NODES`), else `permission_level`.
    fn node_level(&self, node: &str) -> u8 {
        let fallback = if FULL_OPERATOR_NODES.contains(&node) { 4 } else { self.config.permission_level };
        self.config.node_levels.get(node).copied().unwrap_or(fallback)
    }

    /// Whether the actor holds `node` (a grant of the permission system, or the operator level of `node_level`).
    fn holds(&self, actor: &Actor, node: &str) -> bool {
        self.permitted(actor, Some(node)).is_ok()
    }

    /// The checks every command and click has besides its node: WorldEdit's creative-only mode.
    fn allowed(&self, actor: &Actor) -> Result<(), String> {
        if self.config.creative_only && actor.game_mode != 1 {
            return Err("WorldEdit works only in creative mode".into());
        }
        Ok(())
    }

    /// `history.expiration`: edits older than that many minutes are forgotten before `//undo` or `//redo` looks
    /// (WorldEdit's LocalSession). 0 or less keeps them.
    fn expire_history(&mut self, owner: u128) {
        let minutes = self.config.history_expiration_minutes;
        if minutes <= 0 {
            return;
        }
        let cutoff = self.now_ms - minutes * 60_000;
        let s = self.sessions.get(owner);
        s.undo.retain(|c| c.at_ms >= cutoff);
        s.redo.retain(|c| c.at_ms >= cutoff);
    }

    /// Tells the mask and pattern parsers about this player's selection, clipboard and timeout (`hint` is the text about to be
    /// parsed: the clipboard is only copied into the context when it names one).
    pub(crate) fn enter_session(&mut self, actor: &Actor, hint: &str) {
        use crate::horizon_worldedit_core::pattern::{set_ctx, Area, ClipView, MaskCtx};
        let (timeout_default, now) = (self.config.calculation_timeout, self.now_ms);
        let s = self.sessions.get(actor.uuid);
        let region = if s.dimension == actor.dimension {
            match (&s.shape, s.pos1, s.pos2) {
                (Some(shape), _, _) => Some(Area::Shape(shape.clone())),
                (None, Some(a), Some(b)) => Some(Area::Box(Cuboid::new(a, b))),
                _ => None,
            }
        } else {
            None
        };
        let lower = hint.to_ascii_lowercase();
        let clipboard = if lower.contains("#clipboard") || lower.contains("#copy") {
            s.clipboard.as_ref().map(|c| {
                std::rc::Rc::new(ClipView { blocks: c.blocks.iter().copied().collect(), origin: c.origin.as_ref().map_or([0; 3], |(_, o)| *o), min: c.bounds.min, size: c.bounds.size() })
            })
        } else {
            None
        };
        let timeout_ms = s.timeout_ms.unwrap_or(timeout_default);
        let primary = if s.dimension == actor.dimension { s.pos1 } else { None };
        let _ = crate::horizon_worldedit_core::pattern::take_pending_nbt();
        set_ctx(MaskCtx { region, clipboard, timeout_ms, primary, offhand: actor.offhand.clone(), salt: (now as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (actor.uuid as u64) });
    }

    /// A typed command (`set stone`, `//set stone`, `rotate 90`).
    pub fn command(&mut self, world: &mut dyn World, actor: &Actor, line: &str, env: &Env) -> Reply {
        self.now_ms = env.now_ms;
        self.changed_now = 0;
        let started = std::time::Instant::now();
        self.enter_session(actor, line);
        let raw = line.trim();
        let line = raw.trim_start_matches('/');
        let mut words: Vec<&str> = line.split_whitespace().collect();
        // `//` and `/,` toggle the super pickaxe.
        if words.is_empty() && raw.starts_with('/') {
            words.push("togglepickaxe");
        } else if words.first().is_some_and(|w| *w == ",") {
            words[0] = "togglepickaxe";
        }
        // `//load <file>` is `//schem load <file>`.
        if words.first().is_some_and(|w| w.eq_ignore_ascii_case("load")) {
            words.insert(0, "schem");
        }
        // The deprecated tool commands are `/tool <type>`.
        if words.first().is_some_and(|w| matches!(w.to_ascii_lowercase().as_str(), "selwand" | "navwand" | "info" | "cycler" | "repl" | "floodfill" | "flood" | "farwand" | "tree" | "deltree" | "lrbuild")) {
            words.insert(0, "tool");
        }
        let Some(first) = words.first() else { return Reply::err("No command".into()) };
        let mut name = first.to_ascii_lowercase();
        // WorldEdit's aliases that are not registered names of their own.
        name = match name.as_str() {
            "rotate" => "rot".to_owned(),
            "middle" => "center".to_owned(),
            "mirror" => "flip".to_owned(),
            "deselect" => "desel".to_owned(),
            "ex" | "ext" => "extinguish".to_owned(),
            "calculate" | "eval" | "evaluate" | "solve" => "calc".to_owned(),
            "search" | "l" => "searchitem".to_owned(),
            "asc" => "ascend".to_owned(),
            "desc" => "descend".to_owned(),
            "j" => "jumpto".to_owned(),
            "gen" | "g" => "generate".to_owned(),
            "genbiome" | "gb" => "generatebiome".to_owned(),
            "biomels" => "biomelist".to_owned(),
            "snap" => "snapshot".to_owned(),
            "pickaxe" | "sp" => "superpickaxe".to_owned(),
            "rem" | "rement" => "remove".to_owned(),
            "!" => "unstuck".to_owned(),
            other => other.to_owned(),
        };
        // `/size <number>` is the brush size; `//size` (the selection's) takes flags only.
        if name == "size" && words.get(1).is_some_and(|w| w.parse::<f64>().is_ok()) {
            name = "brushsize".to_owned();
        }
        // `/we help ...` is `//help ...`.
        if matches!(name.as_str(), "we" | "worldedit") && words.get(1).is_some_and(|w| w.eq_ignore_ascii_case("help")) {
            words.remove(0);
            name = "help".to_owned();
        }
        let args = match parse_args(&name, &words[1..]) {
            Ok(a) => a,
            Err(e) => return Reply::err(e),
        };
        if let Err(e) = self.permitted(actor, permission_node(&name, &args, actor).as_deref()).and(self.allowed(actor)) {
            return Reply::err(e);
        }
        // Commands that act through a player's hands and eyes have no meaning for the console.
        if actor.uuid == CONSOLE_UUID
            && matches!(
                name.as_str(),
                "wand" | "up" | "ascend" | "descend" | "ceil" | "thru" | "jumpto" | "unstuck" | "brush" | "br" | "tool" | "none" | "superpickaxe" | "togglepickaxe" | "mask" | "material" | "range" | "brushsize" | "tracemask"
            )
        {
            return Reply::err("This command must be used with a player.".into());
        }
        self.names.insert(actor.name.to_ascii_lowercase(), actor.uuid);
        self.log_command(&*world, actor, raw, env);
        let writes = !matches!(
            name.as_str(),
            "we" | "worldedit" | "wand" | "pos" | "toggleeditwand" | "pos1" | "pos2" | "hpos1" | "hpos2" | "sel" | "desel" | "size" | "expand" | "contract" | "shift" | "outset" | "inset" | "chunk" | "count" | "distr" | "copy" | "rot" | "flip" | "clearclipboard" | "clearhistory" | "schem" | "schematic" | "brush" | "br" | "mask" | "material" | "range" | "brushsize" | "tracemask" | "superpickaxe" | "togglepickaxe" | "none" | "tool" | "help" | "limit" | "gmask" | "toggleplace" | "placement" | "searchitem" | "calc" | "trim" | "chunkinfo" | "listchunks" | "biomelist" | "biomeinfo" | "world" | "butcher" | "remove" | "timeout" | "fast" | "perf" | "reorder" | "drawsel" | "watchdog" | "registry" | "snapshot"
        );
        if writes && self.queue.busy(actor.uuid) {
            return Reply::err("Your last edit is still being written; wait a moment".into());
        }
        if name == "wand" {
            return self.wand(actor, args.flag('n'));
        }
        if matches!(name.as_str(), "up" | "ascend" | "descend" | "thru" | "jumpto" | "unstuck" | "ceil") {
            return match self.navigate(world, actor, &name, &args) {
                Ok(reply) => reply,
                Err(e) => Reply::err(e),
            };
        }
        let result = match name.as_str() {
            "we" | "worldedit" => self.worldedit(world, actor, &args, env),
            "help" => self.help(&args),
            "fill" | "fillr" => self.fill_hole(world, actor, name == "fillr", &args),
            "drain" => self.drain(world, actor, &args),
            "fixwater" | "fixlava" => self.fix_liquid(world, actor, name == "fixlava", &args),
            "snow" => self.snow(world, actor, &args),
            "thaw" => self.thaw(world, actor, &args),
            "green" => self.green(world, actor, &args),
            "extinguish" => self.extinguish(world, actor, &args),
            "limit" => self.limit(actor, &args),
            "gmask" => self.gmask(world, actor, &args),
            "toggleplace" => self.toggle_place(actor),
            "placement" => self.placement_type(actor, &args),
            "trim" => self.trim(world, actor, &args),
            "chunkinfo" => self.chunk_info(actor),
            "biomelist" => self.biome_list(world, &args),
            "biomeinfo" => self.biome_info(world, actor, &args),
            "setbiome" | "replacebiome" => self.set_biome_cmd(world, actor, &args, name == "replacebiome"),
            "regen" => self.regen(world, actor, &args),
            "forest" => self.forest(world, actor, &args),
            "flora" => self.flora(world, actor, &args),
            "delchunks" => self.del_chunks(world, actor, &args),
            "world" => self.world_cmd(world, actor, &args),
            "listchunks" => self.list_chunks(actor, &args),
            "searchitem" => self.search_item(world, &args),
            "calc" => self.calc(actor, &args),
            "generate" => self.generate(world, actor, &args),
            "deform" => self.deform(world, actor, &args),
            "butcher" => self.butcher(world, actor, &args),
            "remove" => self.remove_entities(world, actor, &args),
            "pos1" | "pos2" => self.pos(world, actor, name == "pos1", &args),
            "pos" => self.pos_cmd(world, actor, &args),
            "update" => self.update_cmd(world, actor, &args),
            "feature" => self.feature_cmd(world, actor, &args),
            "structure" => self.structure_cmd(world, actor, &args),
            "generatebiome" => self.generate_biome(world, actor, &args),
            "toggleeditwand" => self.toggle_edit_wand(),
            "forestgen" => self.forest_gen(world, actor, &args),
            "pumpkins" => self.pumpkins(world, actor, &args),
            "hpos1" | "hpos2" => self.hpos(world, actor, name == "hpos1"),
            "sel" | "desel" => self.sel(actor, &name, &args),
            "size" => self.size(world, actor, &args),
            "expand" | "contract" | "shift" => self.resize(world, actor, &name, &args),
            "outset" | "inset" => self.outset(world, actor, name == "outset", &args),
            "chunk" => self.chunk(world, actor, &args),
            "count" => self.count(world, actor, &args),
            "distr" => self.distr(world, actor, &args),
            "set" | "walls" | "faces" | "outline" | "center" => self.fill(world, actor, &name, &args),
            "replace" | "re" | "rep" => self.replace(world, actor, &args),
            "hollow" => self.hollow(world, actor, &args),
            "overlay" => self.overlay(world, actor, &args),
            "naturalize" => self.naturalize(world, actor),
            "smooth" => self.smooth(world, actor, &args),
            "snowsmooth" => self.snow_smooth(world, actor, &args),
            "line" => self.line(world, actor, &args),
            "curve" => self.curve(world, actor, &args),
            "revolve" => self.revolve(world, actor, &args),
            "stack" => self.stack(world, actor, &args),
            "move" => self.move_(world, actor, &args),
            "sphere" | "hsphere" => self.sphere(world, actor, name == "hsphere", &args),
            "cyl" | "hcyl" => self.cyl(world, actor, name == "hcyl", &args),
            "cone" => self.cone(world, actor, &args),
            "removeabove" | "removebelow" | "removenear" | "replacenear" => self.near(world, actor, &name, &args),
            "pyramid" | "hpyramid" => self.pyramid(world, actor, name == "hpyramid", &args),
            "copy" | "cut" => self.copy(world, actor, name == "cut", &args),
            "paste" => self.paste(world, actor, &args),
            "rot" => self.rotate(world, actor, &args),
            "flip" => self.flip(world, actor, &args),
            "clearclipboard" => {
                self.sessions.get(actor.uuid).clipboard = None;
                Ok(vec!["Clipboard cleared.".into()])
            }
            "undo" | "redo" => self.history(world, actor, name == "undo", &args),
            "clearhistory" => {
                let s = self.sessions.get(actor.uuid);
                s.undo.clear();
                s.redo.clear();
                Ok(vec!["History cleared.".into()])
            }
            "schem" | "schematic" => self.schem(world, actor, &args, env),
            "brush" | "br" => self.brush(world, actor, &args, env),
            "mask" => self.mask(world, actor, &args),
            "material" => self.material(world, actor, &args),
            "range" => self.range(actor, &args),
            "brushsize" => self.brush_size(actor, &args),
            "tracemask" => self.trace_mask(world, actor, &args),
            "superpickaxe" => self.superpickaxe(actor, &args),
            "togglepickaxe" => self.toggle_pickaxe(actor, &args),
            "none" => self.unbind(actor, false),
            "tool" => self.tool(world, actor, &args),
            "timeout" => self.timeout(actor, &args),
            "fast" => self.fast(actor, &args),
            "perf" => self.perf(actor, &args),
            "reorder" => self.reorder(actor, &args),
            "drawsel" => self.drawsel(actor, &args),
            "watchdog" => self.watchdog(&args),
            "registry" => self.registry(world, &args),
            "snapshot" => self.snapshot(&*world, actor, &args),
            "restore" => self.restore(world, actor, &args),
            _ => Err(format!("The command '{name}' could not be found.")),
        };
        match result {
            Ok(mut lines) => {
                // `debug: true`: the time the command took and how fast it changed blocks (`PlatformCommandManager`).
                if self.config.debug && self.changed_now > 0 {
                    let seconds = started.elapsed().as_millis() as f64 / 1000.0;
                    let rate = if seconds == 0.0 { self.changed_now as f64 } else { self.changed_now as f64 / seconds };
                    lines.push(format!("{}s elapsed (history: {} changed; {} blocks/sec).", java_double(seconds), self.changed_now, rate.round() as i64));
                }
                Reply::ok(lines)
            }
            Err(e) => Reply::err(e),
        }
    }

    // ---- writing ----

    /// Writes `writes` now, or the first tick's worth now and the rest over the next ticks; the
    /// note for the report.
    fn write(&mut self, world: &mut dyn World, actor: &Actor, writes: Vec<(Pos, StateId)>, mut entities: HashMap<Pos, Tag>) -> String {
        let budget = self.config.blocks_per_tick.max(1);
        if writes.len() <= budget {
            world.write(&writes);
            load_entities(world, entities);
            return String::new();
        }
        world.write(&writes[..budget]);
        let now: HashMap<Pos, Tag> = writes[..budget].iter().filter_map(|&(p, _)| entities.remove(&p).map(|t| (p, t))).collect();
        load_entities(world, now);
        let rest = writes[budget..].to_vec();
        let ticks = rest.len().div_ceil(budget);
        self.queue.push_with(actor.uuid, &actor.dimension, rest, entities);
        format!(" (finishing over the next {ticks} tick{})", if ticks == 1 { "" } else { "s" })
    }

    /// Plans `ops`, checks the limit, writes it and remembers it for `//undo`: how many blocks
    /// changed, and the note on the ticks still to come.
    fn commit(&mut self, world: &mut dyn World, actor: &Actor, ops: Vec<(Pos, StateId)>) -> Result<(usize, String), String> {
        self.commit_with(world, actor, ops, HashMap::new())
    }

    /// [`Engine::commit`] for blocks that bring block entity data (`entities`, by world position).
    fn commit_with(&mut self, world: &mut dyn World, actor: &Actor, mut ops: Vec<(Pos, StateId)>, mut entities: HashMap<Pos, Tag>) -> Result<(usize, String), String> {
        // The block entity data the patterns attached (`chest{...}`, a sign's text) goes with the blocks that are written.
        let attached = crate::horizon_worldedit_core::pattern::take_pending_nbt();
        if !attached.is_empty() {
            let written: std::collections::HashSet<Pos> = ops.iter().map(|(p, _)| *p).collect();
            for (p, tag) in attached {
                if written.contains(&p) {
                    entities.insert(p, tag);
                }
            }
        }
        // `//gmask`: only blocks the player's global mask accepts are changed.
        if let Some(text) = self.sessions.get(actor.uuid).global_mask.clone() {
            let mask = Mask::parse(world, &text, None)?;
            let w: &dyn World = &*world;
            ops.retain(|&(p, _)| w.get(p).is_some_and(|s| mask.hits_at(w, p, s)));
        }
        // `use-inventory`: the blocks come out of the player's inventory.
        self.bag_filter(&*world, actor, &mut ops);
        let mut change = plan_with(world, &actor.dimension, ops, entities);
        change.at_ms = self.now_ms;
        if !self.allows(actor, change.len() as u64) {
            return Err(format!("Max blocks changed in an operation reached ({}).", self.max_blocks(actor)));
        }
        let n = change.len();
        self.changed_now += n;
        let note = self.write(world, actor, change.forward(), change.forward_entities());
        let size = self.config.history_size;
        self.sessions.get(actor.uuid).remember(change, size);
        Ok((n, note))
    }

    /// The most blocks one operation of this player may change: their `//limit`, else the server's.
    fn max_blocks(&mut self, actor: &Actor) -> i64 {
        let limit = self.sessions.get(actor.uuid).block_limit.unwrap_or(self.config.max_blocks);
        let maximum = self.config.max_blocks_maximum;
        // `//limit -1` is let through by WorldEdit's own check, but the maximum still holds for anyone not unrestricted.
        if maximum > -1 && (limit < 0 || limit > maximum) && !self.holds(actor, "worldedit.limit.unrestricted") {
            return maximum;
        }
        limit
    }

    fn allows(&mut self, actor: &Actor, blocks: u64) -> bool {
        let limit = self.max_blocks(actor);
        limit < 0 || blocks <= limit as u64
    }

    /// Where shapes, fills and the clipboard's origin are made: the block the player stands in, or
    /// position 1 after `//toggleplace`.
    fn placement(&mut self, actor: &Actor) -> Result<Pos, String> {
        let s = self.sessions.get(actor.uuid);
        let base = match s.place_kind {
            PlaceKind::Player => actor.block(),
            PlaceKind::World => [0; 3],
            PlaceKind::Pos1 => s.pos1.ok_or_else(|| "Make a region selection first.".to_owned())?,
            PlaceKind::Min => s.selection(&actor.dimension)?.min,
            PlaceKind::Max => s.selection(&actor.dimension)?.max,
        };
        Ok(crate::horizon_worldedit_core::region::add(base, s.place_offset))
    }

    /// The writes still queued for `dimension`, this tick's share; the players whose edits finished.
    pub fn tick(&mut self, world: &mut dyn World, dimension: &str) -> Vec<u128> {
        if self.queue.is_empty() {
            return Vec::new();
        }
        let (writes, entities, done) = self.queue.take_with(dimension, self.config.blocks_per_tick.max(1));
        if !writes.is_empty() {
            world.write(&writes);
            world.set_block_entities(&entities);
        }
        done
    }

    fn salt(&mut self, actor: &Actor) -> u64 {
        next_salt(&mut self.sessions.get(actor.uuid).salt)
    }

    /// The selection, cut to the build height and within the scan limit.
    fn region(&mut self, world: &dyn World, actor: &Actor) -> Result<Cuboid, String> {
        let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
        let c = c.clamp_y(world.min_y(), world.max_y()).ok_or("The selection is outside the build height")?;
        let max = self.max_blocks(actor);
        let limit = if max >= 0 { (max as u64).min(SCAN_LIMIT) } else { SCAN_LIMIT };
        if c.volume() > limit {
            return Err(if max >= 0 && limit == max as u64 {
                format!("Max blocks changed in an operation reached ({limit}).")
            } else {
                format!("The selection is {} blocks; one edit handles at most {limit}", c.volume())
            });
        }
        Ok(c)
    }

    fn pattern(&self, world: &dyn World, actor: &Actor, spec: Option<&str>) -> Result<Pattern, String> {
        let spec = spec.ok_or("A pattern is needed: a block such as stone, or a list such as 70%stone,30%gravel")?;
        let p = Pattern::parse(world, spec, actor.held.as_deref())?;
        if !self.config.disallowed_blocks.is_empty() && !self.holds(actor, "worldedit.anyblock") {
            for &(_, s) in &p.entries {
                let name = world.describe(s).name;
                if self.config.disallowed_blocks.iter().any(|d| *d == name) {
                    return Err(format!("Block '{name}' not allowed (see WorldEdit configuration)."));
                }
            }
        }
        Ok(p)
    }

    // ---- selection ----

    fn wand(&mut self, actor: &Actor, navigation: bool) -> Reply {
        if let Err(e) = self.allowed(actor) {
            return Reply::err(e);
        }
        if navigation {
            // WorldEdit's `NavigationWand.canUse`: the item is only worth giving to someone who may jump.
            if let Err(e) = self.permitted(actor, Some("worldedit.navigation.jumpto.tool")) {
                return Reply::err(e);
            }
            let mut r = Reply::ok(vec!["Left click: jump to location; Right click: pass through walls".into()]);
            r.give.push(self.config.nav_wand_item.clone());
            return r;
        }
        let mut r = Reply::ok(vec!["Left click: select pos #1; Right click: select pos #2".into()]);
        r.give.push(self.config.wand_item.clone());
        r
    }

    /// Sets a corner as WorldEdit's cuboid selector does (`//sel extend` grows the box instead) and
    /// says what it did; `Err` when nothing changed.
    fn corner_set(&mut self, world: &dyn World, actor: &Actor, first: bool, p: Pos) -> Result<Vec<String>, String> {
        let s = self.sessions.get(actor.uuid);
        if s.dimension != actor.dimension {
            // Corners in two levels make no region: the other one goes.
            s.pos1 = None;
            s.pos2 = None;
            s.shape = None;
            s.dimension = actor.dimension.clone();
        }
        if s.kind.is_shape() {
            return self.shape_corner(actor, first, p);
        }
        s.shape = None;
        let at = pt(p);
        if s.extend {
            let primary = first || s.pos1.is_none() || s.pos2.is_none();
            if primary {
                if s.pos1 == Some(p) && s.pos2 == Some(p) {
                    return Err("Position already set.".into());
                }
                s.pos1 = Some(p);
                s.pos2 = Some(p);
            } else {
                let c = Cuboid::new(s.pos1.unwrap_or(p), s.pos2.unwrap_or(p));
                if c.contains(p) {
                    return Err("Position already set.".into());
                }
                let grown = Cuboid { min: [c.min[0].min(p[0]), c.min[1].min(p[1]), c.min[2].min(p[2])], max: [c.max[0].max(p[0]), c.max[1].max(p[1]), c.max[2].max(p[2])] };
                s.pos1 = Some(grown.min);
                s.pos2 = Some(grown.max);
            }
            let volume = volume_of(world, s.pos1.unwrap_or(p), s.pos2.unwrap_or(p));
            return Ok(vec![if first { format!("Started selection at {at} ({volume}).") } else { format!("Extended selection to encompass {at} ({volume}).") }]);
        }
        if (if first { s.pos1 } else { s.pos2 }) == Some(p) {
            return Err("Position already set.".into());
        }
        s.set_corner(first, &actor.dimension, p);
        let which = if first { "First" } else { "Second" };
        Ok(vec![match (s.pos1, s.pos2) {
            (Some(a), Some(b)) => format!("{which} position set to {at} ({}).", volume_of(world, a, b)),
            _ => format!("{which} position set to {at}."),
        }])
    }

    fn pos(&mut self, world: &dyn World, actor: &Actor, first: bool, args: &Args) -> R {
        let p = if args.words.is_empty() {
            if actor.uuid == CONSOLE_UUID {
                return Err("You must provide coordinates as console.".into());
            }
            actor.block()
        } else {
            let text = args.rest(0).replace(',', " ");
            let parts: Vec<&str> = text.split_whitespace().collect();
            let name = if first { "pos1" } else { "pos2" };
            if parts.len() != 3 {
                return Err(usage(name));
            }
            let here = actor.block();
            let mut p = [0; 3];
            for i in 0..3 {
                p[i] = match parts[i].strip_prefix('~') {
                    Some("") => here[i],
                    Some(off) => here[i] + off.parse::<f64>().map_err(|_| number_error(parts[i]))?.floor() as i32,
                    None => parts[i].parse::<f64>().map_err(|_| number_error(parts[i]))?.floor() as i32,
                };
            }
            p
        };
        self.corner_set(world, actor, first, p)
    }

    fn hpos(&mut self, world: &dyn World, actor: &Actor, first: bool) -> R {
        if actor.uuid == CONSOLE_UUID {
            return Err("Raytracing commands require a player!".into());
        }
        let dir = ops::look_vector(actor.yaw, actor.pitch);
        let hit = ops::raycast(world, actor.eye(), dir, self.config.brush_range).ok_or("No block in sight!")?;
        self.corner_set(world, actor, first, hit)
    }

    fn sel(&mut self, actor: &Actor, name: &str, args: &Args) -> R {
        let clear = |this: &mut Self| {
            let s = this.sessions.get(actor.uuid);
            s.clear_selection();
            vec!["Selection cleared.".to_string()]
        };
        if name != "sel" {
            return Ok(clear(self));
        }
        match args.get(0) {
            None => Ok(clear(self)),
            Some(word) => match crate::horizon_worldedit_core::selector::SelKind::parse(word) {
                Some(kind) => self.sel_type(actor, kind, args.flag('d')),
                None => Err(usage("sel")),
            },
        }
    }

    fn describe(c: &Cuboid) -> String {
        let s = c.size();
        format!("{}, {}, {} to {}, {}, {} ({} x {} x {}, {} blocks)", c.min[0], c.min[1], c.min[2], c.max[0], c.max[1], c.max[2], s[0], s[1], s[2], c.volume())
    }

    fn size(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        if args.flag('c') {
            let clip = self.sessions.get(actor.uuid).clipboard.as_ref().ok_or("Your clipboard is empty. Use //copy first.")?;
            let (min, max) = (clip.bounds.min, clip.bounds.max);
            let size = [max[0] - min[0] + 1, max[1] - min[1] + 1, max[2] - min[2] + 1];
            let offset = clip.origin.as_ref().map(|(_, o)| *o).unwrap_or([0, 0, 0]);
            return Ok(vec![
                format!("Offset: {}", pt(offset)),
                format!("Size: {}", pt(size)),
                format!("Cuboid distance: {}", java_double(distance(min, max))),
                format!("# of blocks: {}", size[0] as i64 * size[1] as i64 * size[2] as i64),
            ]);
        }
        let s = self.sessions.get(actor.uuid);
        let c = s.selection(&actor.dimension)?;
        let (a, b) = (s.pos1.unwrap_or(c.min), s.pos2.unwrap_or(c.max));
        let c = c.clamp_y(world.min_y(), world.max_y()).unwrap_or(c);
        let size = c.size();
        if let Some(shape) = s.shape.clone() {
            let mut out = Self::shape_info(&shape, s.kind);
            out.push(format!("Size: {}", pt(size)));
            out.push(format!("Cuboid distance: {}", java_double(distance(c.min, c.max))));
            out.push(format!("# of blocks: {}", shape.volume()));
            return Ok(out);
        }
        Ok(vec![
            "Type: Cuboid".into(),
            format!("Position 1: {}", pt(a)),
            format!("Position 2: {}", pt(b)),
            format!("Size: {}", pt(size)),
            format!("Cuboid distance: {}", java_double(distance(c.min, c.max))),
            format!("# of blocks: {}", c.volume()),
        ])
    }

    fn resize(&mut self, world: &dyn World, actor: &Actor, name: &str, args: &Args) -> R {
        let (lo, hi) = (world.min_y(), world.max_y());
        let fit = |c: Cuboid| c.clamp_y(lo, hi).unwrap_or(c);
        let c = fit(self.sessions.get(actor.uuid).selection(&actor.dimension)?);
        let shape = self.sessions.get(actor.uuid).shape.clone();
        if name == "expand" && args.get(0) == Some("vert") {
            let next = Cuboid { min: [c.min[0], lo, c.min[2]], max: [c.max[0], hi, c.max[2]] };
            self.sessions.get(actor.uuid).set_selection(next, &actor.dimension);
            self.keep_shape(actor, shape, next, None);
            return Ok(vec![format!("Region expanded {} blocks (top-to-bottom).", next.volume() as i64 - c.volume() as i64)]);
        }
        if args.get(0).is_none() {
            return Err(usage(name));
        }
        let amount = args.int(0, 0)?;
        let (reverse, dir_at) = match name {
            "shift" => (0, 1),
            _ => match args.get(1).map(|w| w.parse::<i32>()) {
                Some(Ok(r)) => (r, 2),
                _ => (0, 1),
            },
        };
        let dir = Self::dir_arg(actor, args.get(dir_at))?;
        Self::shape_resizable(shape.as_ref(), name, Some(dir))?;
        // An ellipsoid and a cylinder grow and shrink round their centre: the net change along an axis must be even.
        if name != "shift" && (amount - reverse) & 1 != 0 {
            match shape.as_ref() {
                Some(crate::horizon_worldedit_core::selector::Shape::Ellipsoid { .. }) => return Err("Ellipsoid changes must be even for each dimension.".into()),
                Some(crate::horizon_worldedit_core::selector::Shape::Cyl { .. }) if !matches!(dir, Dir::Up | Dir::Down) => {
                    return Err("Cylinders changes must be even for each horizontal dimension.".into());
                }
                _ => {}
            }
        }
        let next = match name {
            "expand" => c.expand(dir, amount).and_then(|c| c.expand(dir.opposite(), reverse)),
            "contract" => c.contract(dir, amount).and_then(|c| c.contract(dir.opposite(), reverse)),
            _ => Some(c.shifted(crate::horizon_worldedit_core::region::scale(dir.vector(), amount))),
        };
        let next = fit(next.ok_or("The selection would be empty")?);
        self.sessions.get(actor.uuid).set_selection(next, &actor.dimension);
        let moved = (name == "shift").then(|| crate::horizon_worldedit_core::region::scale(dir.vector(), amount));
        let before = shape.as_ref().map_or(c.volume(), |sh| sh.volume());
        self.keep_shape(actor, shape, next, moved);
        let after = self.sessions.get(actor.uuid).shape.as_ref().map_or(next.volume(), |sh| sh.volume());
        Ok(vec![match name {
            "expand" => format!("Region expanded {} blocks", after as i64 - before as i64),
            "contract" => format!("Region contracted {} blocks.", before as i64 - after as i64),
            _ => "Region shifted.".into(),
        }])
    }

    fn outset(&mut self, world: &dyn World, actor: &Actor, out: bool, args: &Args) -> R {
        let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
        let shape = self.sessions.get(actor.uuid).shape.clone();
        if args.get(0).is_none() {
            return Err(usage(if out { "outset" } else { "inset" }));
        }
        Self::shape_resizable(shape.as_ref(), if out { "expand" } else { "contract" }, None)?;
        let n = args.int(0, 0)?;
        // WorldEdit: `-h` leaves out up and down, `-v` leaves out the four sides; both leave nothing.
        let (h, v) = (!args.flag('v'), !args.flag('h'));
        let next = c.outset(if out { n } else { -n }, h, v).ok_or("The selection would be empty")?;
        let next = next.clamp_y(world.min_y(), world.max_y()).unwrap_or(next);
        self.sessions.get(actor.uuid).set_selection(next, &actor.dimension);
        self.keep_shape(actor, shape, next, None);
        Ok(vec![if out { "Region outset." } else { "Region inset." }.to_owned()])
    }

    /// `//chunk [-s] [-c] [x,y,z]`: WorldEdit's chunks are 16 x 256 x 16 (y counted in steps of 256).
    fn chunk(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        let (min, max, line);
        if args.flag('s') {
            let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
            let (lo_y, hi_y) = (world.min_y() >> 8, world.max_y() >> 8);
            let a = [c.min[0] >> 4, (c.min[1] >> 8).clamp(lo_y, hi_y), c.min[2] >> 4];
            let b = [c.max[0] >> 4, (c.max[1] >> 8).clamp(lo_y, hi_y), c.max[2] >> 4];
            min = [a[0] << 4, a[1] << 8, a[2] << 4];
            max = [(b[0] << 4) + 15, (b[1] << 8) + 255, (b[2] << 4) + 15];
            line = format!("Chunks selected: ({}, {}, {}) - ({}, {}, {})", a[0], a[1], a[2], b[0], b[1], b[2]);
        } else {
            let chunk = if args.words.is_empty() {
                let p = actor.block();
                [p[0] >> 4, p[1] >> 8, p[2] >> 4]
            } else {
                let text = args.rest(0).replace(',', " ");
                let parts: Vec<&str> = text.split_whitespace().collect();
                if parts.len() != 3 {
                    return Err(usage("chunk"));
                }
                let mut v = [0i32; 3];
                for i in 0..3 {
                    v[i] = parts[i].parse::<f64>().map_err(|_| number_error(parts[i]))?.floor() as i32;
                }
                if args.flag('c') {
                    v
                } else {
                    [v[0] >> 4, v[1] >> 8, v[2] >> 4]
                }
            };
            min = [chunk[0] << 4, chunk[1] << 8, chunk[2] << 4];
            max = [min[0] + 15, min[1] + 255, min[2] + 15];
            line = format!("Chunk selected: {}, {}, {}", chunk[0], chunk[1], chunk[2]);
        }
        let s = self.sessions.get(actor.uuid);
        s.set_selection(Cuboid::new(min, max), &actor.dimension);
        Ok(vec![line])
    }

    fn count(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        if args.words.is_empty() {
            return Err(usage("count"));
        }
        let mask = Mask::parse(world, &args.rest(0), actor.held.as_deref())?;
        let shape = self.shape_of(actor);
        let n = Self::shape_positions(&shape, &c).into_iter().filter(|&p| world.get(p).is_some_and(|s| mask.hits_at(world, p, s))).count();
        Ok(vec![format!("Counted: {n}")])
    }

    /// `//distr [-d] [-m <mask>] [-p <page>]`: seven rows a page, most frequent first.
    fn distr(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        let rows: Vec<(String, u64)> = match args.value('p') {
            Some(_) => self.sessions.get(actor.uuid).distribution.clone().ok_or("No previous distribution.")?,
            None if args.flag('c') => {
                // `-c`: what the clipboard holds, not the selection.
                let clip = self.sessions.get(actor.uuid).clipboard.as_ref().ok_or("Your clipboard is empty. Use //copy first.")?;
                let mut counts: HashMap<String, u64> = HashMap::new();
                for (_, s) in &clip.blocks {
                    let state = world.describe(*s);
                    let key = if args.flag('d') { state.to_string_full() } else { state.name.clone() };
                    *counts.entry(key).or_default() += 1;
                }
                let mut rows: Vec<(String, u64)> = counts.into_iter().collect();
                rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                self.sessions.get(actor.uuid).distribution = Some(rows.clone());
                rows
            }
            None => {
                let c = self.region(world, actor)?;
                let mask = match args.value('m') {
                    Some(m) => Some(Mask::parse(world, m, actor.held.as_deref())?),
                    None => None,
                };
                let mut counts: HashMap<String, u64> = HashMap::new();
                let shape = self.shape_of(actor);
                for p in Self::shape_positions(&shape, &c) {
                    if let Some(s) = world.get(p) {
                        if mask.as_ref().is_some_and(|m| !m.hits_at(world, p, s)) {
                            continue;
                        }
                        let state = world.describe(s);
                        let key = if args.flag('d') { state.to_string_full() } else { state.name.clone() };
                        *counts.entry(key).or_default() += 1;
                    }
                }
                let mut rows: Vec<(String, u64)> = counts.into_iter().collect();
                rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                self.sessions.get(actor.uuid).distribution = Some(rows.clone());
                rows
            }
        };
        if rows.is_empty() {
            return Err("No blocks counted.".into());
        }
        let page: usize = match args.value('p') {
            Some(p) => p.parse().map_err(|_| number_error(p))?,
            None => 1,
        };
        let pages = rows.len().div_ceil(7);
        if page < 1 || page > pages {
            return Err("Invalid page number".into());
        }
        let total: u64 = rows.iter().map(|r| r.1).sum();
        let max_digits = (total as f64).log10() as usize + 1;
        let mut out = vec!["Block Distribution".to_owned(), format!("Total Block Count: {total}")];
        for (name, n) in rows.iter().skip((page - 1) * 7).take(7) {
            let perc = *n as f64 / total as f64 * 100.0;
            let cur_digits = (*n as f64).log10() as usize + 1;
            let space = max_digits.saturating_sub(cur_digits);
            let pad = " ".repeat(if space == 0 { 2 } else { 2 * space + 1 });
            let shown = match name.split_once('[') {
                Some((base, _)) => format!("{}*", block_title(base)),
                None => block_title(name),
            };
            out.push(format!("{}{perc:.3}%  {n}{pad}{shown}", if perc < 10.0 { "  " } else { "" }));
        }
        if pages > 1 {
            out.push(format!("Page {page} of {pages}"));
        }
        Ok(out)
    }

    // ---- region edits ----

    /// The pattern argument at `at`, or the command's usage line.
    fn need_pattern(&self, world: &dyn World, actor: &Actor, args: &Args, at: usize, name: &str) -> Result<Pattern, String> {
        match args.get(at) {
            Some(spec) => self.pattern(world, actor, Some(spec)),
            None => Err(usage(name)),
        }
    }

    fn fill(&mut self, world: &mut dyn World, actor: &Actor, name: &str, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let pattern = self.need_pattern(world, actor, args, 0, name)?;
        let salt = self.salt(actor);
        let shape = self.shape_of(actor);
        let ops: Vec<(Pos, StateId)> = match (&shape, name) {
            (None, "set") => c.positions().map(|p| (p, pattern.pick_with(&*world, salt, p))).collect(),
            (None, "walls") => c.positions().filter(|&p| c.is_wall(p)).map(|p| (p, pattern.pick_with(&*world, salt, p))).collect(),
            (None, "faces" | "outline") => c.positions().filter(|&p| c.is_face(p)).map(|p| (p, pattern.pick_with(&*world, salt, p))).collect(),
            (Some(_), "set") => Self::shape_positions(&shape, &c).into_iter().map(|p| (p, pattern.pick_with(&*world, salt, p))).collect(),
            // A shape's walls and faces: the blocks with a neighbour outside it (walls look at the four sides only).
            (Some(sh), "walls" | "faces" | "outline") => {
                let sides: &[Pos] = if name == "walls" { &[[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]] } else { &[[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, 1, 0], [0, -1, 0]] };
                Self::shape_positions(&shape, &c).into_iter().filter(|&p| sides.iter().any(|d| !sh.contains([p[0] + d[0], p[1] + d[1], p[2] + d[2]]))).map(|p| (p, pattern.pick_with(&*world, salt, p))).collect()
            }
            _ => c.center().positions().map(|p| (p, pattern.pick_with(&*world, salt, p))).collect(),
        };
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![match name {
            "set" => format!("Operation completed ({n} blocks affected){note}"),
            "center" => format!("Center set. ({n} blocks changed){note}"),
            _ => format!("{n} blocks have been changed.{note}"),
        }])
    }

    fn replace(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        // `//replace <to>` replaces every block that isn't air; `//replace <from> <to>` only `from`.
        let (mask, to) = match (args.get(0), args.get(1)) {
            (Some(to), None) => (Mask::existing(), to),
            (Some(from), Some(to)) => (Mask::parse(world, from, actor.held.as_deref())?, to),
            _ => return Err(usage("replace")),
        };
        let pattern = self.pattern(world, actor, Some(to))?;
        let salt = self.salt(actor);
        let shape = self.shape_of(actor);
        let ops: Vec<(Pos, StateId)> = Self::shape_positions(&shape, &c).into_iter().filter(|&p| world.get(p).is_some_and(|s| mask.hits_at(world, p, s))).map(|p| (p, pattern.pick_with(&*world, salt, p))).collect();
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been replaced.{note}")])
    }

    fn hollow(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let thickness = args.int(0, 0)?;
        if thickness < 0 {
            return Err("Thickness must be >= 0".into());
        }
        let pattern = match args.get(1) {
            Some(p) => self.pattern(world, actor, Some(p))?,
            None => Pattern::single(world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0), "air"),
        };
        let salt = self.salt(actor);
        let ops = ops::hollow(world, &c, thickness, &pattern, salt);
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been changed.{note}")])
    }

    fn overlay(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let pattern = self.need_pattern(world, actor, args, 0, "overlay")?;
        let salt = self.salt(actor);
        let ops = ops::overlay(world, &c, &pattern, salt);
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been overlaid.{note}")])
    }

    fn naturalize(&mut self, world: &mut dyn World, actor: &Actor) -> R {
        let c = self.region(world, actor)?;
        let id = |n: &str| world.resolve(&crate::horizon_worldedit_core::block::BlockState::new(n, &[])).ok_or_else(|| format!("Block name '{n}' was not recognized."));
        let ids = [id("minecraft:grass_block")?, id("minecraft:dirt")?, id("minecraft:stone")?];
        let ops = ops::naturalize(world, &c, ids);
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} block(s) have been made to look more natural.{note}")])
    }

    fn smooth(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let iterations = args.int(0, 1)?.clamp(1, 50) as u32;
        let mask = match args.get(1) {
            Some(m) => Some(Mask::parse(world, &args.rest(1), actor.held.as_deref()).map_err(|e| format!("{e} ({m})"))?),
            None => None,
        };
        let ops = ops::smooth(world, &c, iterations, mask.as_ref());
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("Terrain's height map smoothed. {n} blocks changed.{note}")])
    }

    /// `//snowsmooth [iterations] [-l layers] [-m mask]` (`RegionCommands.snowSmooth`).
    fn snow_smooth(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let iterations = args.int(0, 1)?.clamp(0, 50) as u32;
        let layers = match args.value('l') {
            Some(v) => v.parse::<i32>().map_err(|_| number_error(v))?,
            None => 1,
        };
        let mask = match args.value('m') {
            Some(m) => Some(Mask::parse(world, m, actor.held.as_deref())?),
            None => None,
        };
        let ops = ops::snow_smooth(world, &c, iterations, layers, mask.as_ref(), 5);
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("Snow's height map smoothed. {n} blocks changed.{note}")])
    }

    fn line(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
        // Only a cuboid or a convex polyhedral selection has corners to draw between.
        let vertices = match self.sessions.get(actor.uuid).shape.clone() {
            None => None,
            Some(crate::horizon_worldedit_core::selector::Shape::Convex { points }) => Some(points),
            Some(_) => return Err("//line only works with cuboid selections or convex polyhedral selections".into()),
        };
        let s = self.sessions.get(actor.uuid);
        let (a, b) = (s.pos1.unwrap_or(c.min), s.pos2.unwrap_or(c.max));
        let pattern = self.need_pattern(world, actor, args, 0, "line")?;
        let thickness = match args.get(1) {
            Some(w) => w.parse::<f64>().map_err(|_| number_error(w))?,
            None => 0.0,
        };
        if !(thickness >= 0.0) {
            return Err("Thickness must be >= 0".into());
        }
        self.check_radius(&[thickness])?;
        let salt = self.salt(actor);
        let points: Vec<Pos> = match &vertices {
            // The segments between consecutive vertices make one set of blocks (`EditSession.drawLine` over a list).
            Some(v) if v.len() > 1 => {
                let mut seen = std::collections::HashSet::new();
                v.windows(2).flat_map(|w| shapes::line(w[0], w[1], thickness, !args.flag('h'))).filter(|p| seen.insert(*p)).collect()
            }
            Some(_) => Vec::new(),
            None => shapes::line(a, b, thickness, !args.flag('h')),
        };
        let (lo, hi) = (world.min_y(), world.max_y());
        let ops = points.into_iter().filter(|p| (lo..=hi).contains(&p[1])).map(|p| (p, pattern.pick_with(&*world, salt, p))).collect();
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been changed.{note}")])
    }

    /// `//curve`: a spline through the vertices of the convex polyhedral selection (`EditSession.drawSpline`).
    fn curve(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let points = match self.sessions.get(actor.uuid).shape.clone() {
            Some(crate::horizon_worldedit_core::selector::Shape::Convex { points }) => points,
            _ => return Err("//curve only works with convex polyhedral selections".into()),
        };
        let pattern = self.need_pattern(world, actor, args, 0, "curve")?;
        let thickness = match args.get(1) {
            Some(w) => w.parse::<f64>().map_err(|_| number_error(w))?,
            None => 0.0,
        };
        if !(thickness >= 0.0) {
            return Err("Thickness must be >= 0".into());
        }
        self.check_radius(&[thickness])?;
        let salt = self.salt(actor);
        let (lo, hi) = (world.min_y(), world.max_y());
        let ops = shapes::spline(&points, thickness, !args.flag('h')).into_iter().filter(|p| (lo..=hi).contains(&p[1])).map(|p| (p, pattern.pick_with(&*world, salt, p))).collect();
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been changed.{note}")])
    }

    /// `//revolve <count> [-m mask] [-r]` (`ClipboardCommands.revolve`): the selection is copied around the
    /// placement point, then pasted `count - 1` more times, each copy turned a further 360 / count degrees
    /// round that point (the other way with `-r`). Air is never pasted, so a later copy only overwrites what it
    /// fills. `-m` leaves out the blocks it does not match.
    fn revolve(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let count = match args.get(0) {
            Some(w) => w.parse::<i32>().map_err(|_| number_error(w))?,
            None => return Err(usage("revolve")),
        };
        if count < 2 {
            return Err("Paste count must be at least 2.".into());
        }
        let origin = self.placement(actor)?;
        let mut clip = Clipboard::copy(world, &c, origin, &actor.dimension)?;
        if let Some(text) = args.value('m') {
            let mask = Mask::parse(world, text, actor.held.as_deref())?;
            let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
            let w: &dyn World = &*world;
            for (p, s) in clip.blocks.iter_mut() {
                if !mask.hits(w, *s) {
                    *s = air;
                    clip.entities.remove(&*p);
                }
            }
        }
        let sign = if args.flag('r') { 1.0 } else { -1.0 };
        if args.flag('e') || args.flag('b') {
            let _ = self.copy_extras(world, args, &mut clip, &c, origin, &None);
        }
        let mut writes: Vec<(Pos, StateId)> = Vec::new();
        let mut entities: HashMap<Pos, Tag> = HashMap::new();
        for i in 1..count {
            let (turned, block_entities) = clip.turned(world, sign * 360.0 * f64::from(i) / f64::from(count));
            writes.extend(turned);
            entities.extend(block_entities);
        }
        // Later copies overwrite earlier ones where they overlap.
        let mut last: HashMap<Pos, StateId> = HashMap::new();
        for (p, s) in writes {
            last.insert(p, s);
        }
        let writes: Vec<(Pos, StateId)> = last.into_iter().map(|(p, s)| (crate::horizon_worldedit_core::region::add(origin, p), s)).collect();
        let entities: HashMap<Pos, Tag> = entities.into_iter().map(|(p, t)| (crate::horizon_worldedit_core::region::add(origin, p), t)).collect();
        let (n, note) = self.commit_with(world, actor, writes, entities)?;
        if args.flag('e') || args.flag('b') {
            let mut all = Extras::default();
            for i in 1..count {
                let plan = clip.revolved_plan(origin, sign * 360.0 * f64::from(i) / f64::from(count), args.flag('e'), args.flag('b'));
                let (extras, _) = Self::paste_extras(world, plan);
                all.biomes.extend(extras.biomes);
                all.spawned.extend(extras.spawned);
            }
            self.remember_extras(actor, n, all);
        }
        Ok(vec![format!("The selection has been pasted {count} times."), format!("{n} blocks changed.{note}")])
    }

    fn dir_arg(actor: &Actor, word: Option<&str>) -> Result<Dir, String> {
        match word {
            Some(w) => Dir::parse(w, actor.yaw, actor.pitch).ok_or_else(|| format!("Unknown direction: {w}")),
            None => Ok(Dir::facing(actor.yaw, actor.pitch)),
        }
    }

    /// WorldEdit's offset argument: a direction word or an `x,y,z` vector.
    fn offset_arg(actor: &Actor, word: Option<&str>) -> Result<Pos, String> {
        match word {
            Some(w) if w.contains(',') => {
                let parts: Vec<&str> = w.split(',').collect();
                if parts.len() != 3 {
                    return Err(format!("Unknown direction: {w}"));
                }
                let mut v = [0i32; 3];
                for i in 0..3 {
                    v[i] = parts[i].trim().parse::<i32>().map_err(|_| number_error(parts[i]))?;
                }
                Ok(v)
            }
            _ => Ok(Self::dir_arg(actor, word)?.vector()),
        }
    }

    fn stack(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let count = args.int(0, 1)?;
        if count < 1 {
            return Err("Count must be >= 1".into());
        }
        let offset = Self::offset_arg(actor, args.get(1))?;
        let size = c.size();
        let step = if args.flag('r') {
            let abs = offset.map(i32::abs);
            if abs[0] < size[0] && abs[1] < size[1] && abs[2] < size[2] {
                return Err("Stack offset must not collide with the region when using block units".into());
            }
            offset
        } else {
            [offset[0].saturating_mul(size[0]), offset[1].saturating_mul(size[1]), offset[2].saturating_mul(size[2])]
        };
        if !self.allows(actor, c.volume().saturating_mul(count as u64)) {
            return Err(format!("Max blocks changed in an operation reached ({}).", self.max_blocks(actor)));
        }
        let mask = match args.value('m') {
            Some(m) => Some(Mask::parse(world, m, actor.held.as_deref())?),
            None => None,
        };
        let (ops, copied) = ops::stack(world, &c, step, count, args.flag('a'), mask.as_ref())?;
        let shifts: Vec<Pos> = (1..=count).map(|n| crate::horizon_worldedit_core::region::scale(step, n)).collect();
        let entities = ops::carried_entities(world, &c, &shifts, mask.as_ref());
        let source = self.extras_source(world, actor, args, &c)?;
        let (changed, note) = self.commit_with(world, actor, ops, entities)?;
        if let Some((clip, _)) = &source {
            let ats: Vec<Pos> = shifts.iter().map(|&by| crate::horizon_worldedit_core::region::add(c.min, by)).collect();
            let extras = Self::paste_extras_at(world, clip, args, &ats);
            self.remember_extras(actor, changed, extras);
        }
        if args.flag('s') {
            self.sessions.get(actor.uuid).set_selection(c.shifted(crate::horizon_worldedit_core::region::scale(step, count)), &actor.dimension);
        }
        Ok(vec![format!("{copied} blocks changed. Undo with //undo{note}")])
    }

    fn move_(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let multiplier = args.int(0, 1)?;
        if multiplier < 1 {
            return Err("Multiplier must be >= 1".into());
        }
        let offset = Self::offset_arg(actor, args.get(1))?;
        let leave = match args.get(2) {
            Some(p) => self.pattern(world, actor, Some(p))?,
            None => Pattern::single(world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0), "air"),
        };
        let mask = match args.value('m') {
            Some(m) => Some(Mask::parse(world, m, actor.held.as_deref())?),
            None => None,
        };
        let by = crate::horizon_worldedit_core::region::scale(offset, multiplier);
        let salt = self.salt(actor);
        let (ops, moved) = ops::move_region(world, &c, by, &leave, salt, args.flag('a'), mask.as_ref())?;
        let entities = ops::carried_entities(world, &c, &[by], mask.as_ref());
        let source = self.extras_source(world, actor, args, &c)?;
        let (changed, note) = self.commit_with(world, actor, ops, entities)?;
        if let Some((clip, taken)) = &source {
            // The entities move: the ones in the region go, and are made again at the target.
            if args.flag('e') && !taken.is_empty() {
                world.remove_entities(taken);
            }
            let extras = Self::paste_extras_at(world, clip, args, &[crate::horizon_worldedit_core::region::add(c.min, by)]);
            self.remember_extras(actor, changed, extras);
        }
        if args.flag('s') {
            self.sessions.get(actor.uuid).set_selection(c.shifted(by), &actor.dimension);
        }
        Ok(vec![format!("{moved} blocks moved.{note}")])
    }

    // ---- shapes ----

    fn check_radius(&self, r: &[f64]) -> Result<(), String> {
        match r.iter().find(|&&v| !self.config.radius_ok(v)) {
            Some(_) => Err(format!("Maximum radius (in configuration): {}", self.config.max_radius as i64)),
            None => Ok(()),
        }
    }

    fn shape(&mut self, world: &mut dyn World, actor: &Actor, pattern: &Pattern, positions: Vec<Pos>) -> Result<(usize, String), String> {
        if !self.allows(actor, positions.len() as u64) || positions.len() as u64 > SCAN_LIMIT {
            return Err(format!("Max blocks changed in an operation reached ({}).", self.max_blocks(actor)));
        }
        let salt = self.salt(actor);
        let (lo, hi) = (world.min_y(), world.max_y());
        let ops = positions.into_iter().filter(|p| (lo..=hi).contains(&p[1])).map(|p| (p, pattern.pick_with(&*world, salt, p))).collect();
        self.commit(world, actor, ops)
    }

    fn sphere(&mut self, world: &mut dyn World, actor: &Actor, hollow: bool, args: &Args) -> R {
        let name = if hollow { "hsphere" } else { "sphere" };
        let pattern = self.need_pattern(world, actor, args, 0, name)?;
        let r = radii(args.get(1).ok_or_else(|| usage(name))?)?;
        let r = match r.len() {
            1 => [r[0].max(0.0); 3],
            3 => [r[0].max(0.0), r[1].max(0.0), r[2].max(0.0)],
            _ => return Err("You must either specify 1 or 3 radius values.".into()),
        };
        self.check_radius(&r)?;
        let mut center = self.placement(actor)?;
        if args.flag('r') {
            center[1] += r[1] as i32;
        }
        let positions = shapes::sphere(center, r, hollow || args.flag('h'));
        let (n, note) = self.shape(world, actor, &pattern, positions)?;
        Ok(vec![format!("{n} blocks have been created.{note}")])
    }

    fn cyl(&mut self, world: &mut dyn World, actor: &Actor, hollow: bool, args: &Args) -> R {
        let name = if hollow { "hcyl" } else { "cyl" };
        let pattern = self.need_pattern(world, actor, args, 0, name)?;
        let r = radii(args.get(1).ok_or_else(|| usage(name))?)?;
        let r = match r.len() {
            1 => [r[0].max(1.0); 2],
            2 => [r[0].max(1.0), r[1].max(1.0)],
            _ => return Err("You must either specify 1 or 2 radius values.".into()),
        };
        let height = args.int(2, 1)?;
        self.check_radius(&r)?;
        self.check_radius(&[f64::from(height)])?;
        let positions = shapes::cylinder(self.placement(actor)?, r, height, hollow || args.flag('h'));
        let (n, note) = self.shape(world, actor, &pattern, positions)?;
        Ok(vec![format!("{n} blocks have been created.{note}")])
    }

    fn cone(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let pattern = self.need_pattern(world, actor, args, 0, "cone")?;
        let r = radii(args.get(1).ok_or_else(|| usage("cone"))?)?;
        let r = match r.len() {
            1 => [r[0].max(1.0); 2],
            2 => [r[0].max(1.0), r[1].max(1.0)],
            _ => return Err("You must either specify 1 or 2 radius values.".into()),
        };
        let height = args.int(2, 1)?;
        let thickness = match args.get(3) {
            Some(w) => w.parse::<f64>().map_err(|_| number_error(w))?,
            None => 1.0,
        };
        self.check_radius(&r)?;
        self.check_radius(&[f64::from(height)])?;
        let positions = shapes::cone(self.placement(actor)?, r, height, args.flag('h'), thickness);
        let (n, note) = self.shape(world, actor, &pattern, positions)?;
        Ok(vec![format!("{n} blocks have been created.{note}")])
    }

    fn pyramid(&mut self, world: &mut dyn World, actor: &Actor, hollow: bool, args: &Args) -> R {
        let name = if hollow { "hpyramid" } else { "pyramid" };
        let pattern = self.need_pattern(world, actor, args, 0, name)?;
        if args.get(1).is_none() {
            return Err(usage(name));
        }
        let size = args.int(1, 0)?;
        self.check_radius(&[f64::from(size)])?;
        let positions = shapes::pyramid(self.placement(actor)?, size, hollow || args.flag('h'));
        let (n, note) = self.shape(world, actor, &pattern, positions)?;
        Ok(vec![format!("{n} blocks have been created.{note}")])
    }

    /// `//removeabove|removebelow [size] [height]`, `//removenear <mask> [radius]`,
    /// `//replacenear <radius> [from] <to>`: a cube round the block the player stands in.
    fn near(&mut self, world: &mut dyn World, actor: &Actor, name: &str, args: &Args) -> R {
        let p = self.placement(actor)?;
        let (cube, mask, pattern, done) = match name {
            "removeabove" | "removebelow" => {
                let size = args.int(0, 1)?.max(1);
                self.check_radius(&[f64::from(size)])?;
                let height = args.int(1, 256)?;
                if height < 1 {
                    return Err("height >= 1".into());
                }
                let far = if name == "removeabove" { p[1] + height - 1 } else { p[1] - height + 1 };
                (Cuboid::new([p[0] - size + 1, p[1], p[2] - size + 1], [p[0] + size - 1, far, p[2] + size - 1]), None, None, "have been removed")
            }
            "removenear" => {
                let mask = Mask::parse(world, args.get(0).ok_or_else(|| usage(name))?, actor.held.as_deref())?;
                let radius = args.int(1, 50)?.max(1);
                self.check_radius(&[f64::from(radius)])?;
                let r = radius - 1;
                (Cuboid::new([p[0] - r, p[1] - r, p[2] - r], [p[0] + r, p[1] + r, p[2] + r]), Some(mask), None, "have been removed")
            }
            _ => {
                if args.words.len() < 2 || args.words.len() > 3 {
                    return Err(usage(name));
                }
                let radius = args.int(0, 1)?.max(1);
                self.check_radius(&[f64::from(radius)])?;
                let mask = if args.words.len() == 3 { Mask::parse(world, args.get(1).unwrap_or(""), actor.held.as_deref())? } else { Mask::existing() };
                let pattern = self.pattern(world, actor, args.words.last().copied())?;
                (Cuboid::new([p[0] - radius, p[1] - radius, p[2] - radius], [p[0] + radius, p[1] + radius, p[2] + radius]), Some(mask), Some(pattern), "have been replaced")
            }
        };
        let cube = cube.clamp_y(world.min_y(), world.max_y());
        let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
        let salt = self.salt(actor);
        let mut ops = Vec::new();
        if let Some(cube) = cube {
            if cube.volume() > SCAN_LIMIT {
                return Err(format!("The area is {} blocks; one edit handles at most {SCAN_LIMIT}", cube.volume()));
            }
            for q in cube.positions() {
                if mask.as_ref().is_none_or(|m| world.get(q).is_some_and(|s| m.hits_at(world, q, s))) {
                    ops.push((q, pattern.as_ref().map_or(air, |pt| pt.pick_with(&*world, salt, q))));
                }
            }
        }
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks {done}.{note}")])
    }

    // ---- clipboard ----

    fn copy(&mut self, world: &mut dyn World, actor: &Actor, cut: bool, args: &Args) -> R {
        let c = self.region(world, actor)?;
        let origin = self.placement(actor)?;
        let mut clip = Clipboard::copy(world, &c, origin, &actor.dimension)?;
        let shape = self.shape_of(actor);
        if let Some(sh) = &shape {
            // Only the blocks inside a non-box selection are copied.
            let inside = |rel: &Pos| sh.contains([rel[0] + origin[0], rel[1] + origin[1], rel[2] + origin[2]]);
            clip.blocks.retain(|(rel, _)| inside(rel));
            clip.entities.retain(|rel, _| inside(rel));
        }
        let mut affected = clip.blocks.len();
        let (biome_columns, taken) = self.copy_extras(world, args, &mut clip, &c, origin, &shape);
        let mobs = taken.len();
        // `//cut -e` takes the entities out of the world as well.
        if cut && !taken.is_empty() {
            world.remove_entities(&taken);
        }
        if let Some(text) = args.value('m') {
            let mask = Mask::parse(world, text, actor.held.as_deref())?;
            let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
            let w: &dyn World = &*world;
            affected = 0;
            for (p, s) in clip.blocks.iter_mut() {
                if mask.hits(w, *s) {
                    affected += 1;
                } else {
                    *s = air;
                    clip.entities.remove(&*p);
                }
            }
        }
        self.sessions.get(actor.uuid).clipboard = Some(clip);
        let mut note = String::new();
        if cut {
            let leave = match args.get(0) {
                Some(p) => self.pattern(world, actor, Some(p))?,
                None => Pattern::single(world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0), "air"),
            };
            let salt = self.salt(actor);
            let ops = Self::shape_positions(&shape, &c).into_iter().map(|p| (p, leave.pick_with(&*world, salt, p))).collect();
            note = self.commit(world, actor, ops)?.1;
        }
        Ok(vec![format!("{affected} blocks affected{note}"), format!("{biome_columns} biomes affected"), format!("{mobs} entities affected")])
    }

    fn paste(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let place = self.placement(actor)?;
        let s = self.sessions.get(actor.uuid);
        let clip = s.clipboard.as_ref().ok_or("Your clipboard is empty. Use //copy first.")?;
        let at = match (&clip.origin, args.flag('o')) {
            (Some((dim, origin)), true) if *dim == actor.dimension => *origin,
            (_, true) => return Err("-o pastes where it was copied: that is in another level, or the clipboard came from a file".into()),
            _ => place,
        };
        let pasted = clip.bounds.shifted(at);
        let mut lines = Vec::new();
        if !args.flag('n') {
            let mut writes = clip.writes(world, at, args.flag('a'));
            if let Some(text) = args.value('m') {
                let mask = Mask::parse(world, text, actor.held.as_deref())?;
                let w: &dyn World = &*world;
                writes.retain(|(p, s)| mask.hits_at(w, *p, *s));
            }
            let affected = writes.len();
            let entities = clip.entities_for(at, &writes);
            let plan = clip.extras_plan(at, args.flag('e'), args.flag('b'));
            let biome_columns = plan.columns;
            let (changed, note) = self.commit_with(world, actor, writes, entities)?;
            let (extras, mobs) = Self::paste_extras(world, plan);
            self.remember_extras(actor, changed, extras);
            lines.push(format!("The clipboard has been pasted at {}{note}", pt(at)));
            lines.push(format!("{affected} blocks affected"));
            lines.push(format!("{biome_columns} biomes affected"));
            lines.push(format!("{mobs} entities affected"));
        } else {
            lines.push("Selected clipboard paste region.".into());
        }
        if args.flag('s') || args.flag('n') {
            self.sessions.get(actor.uuid).set_selection(pasted, &actor.dimension);
        }
        Ok(lines)
    }

    fn rotate(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let angle = |i: usize| -> Result<f64, String> {
            match args.get(i) {
                Some(w) => w.parse::<f64>().map_err(|_| number_error(w)),
                None => Ok(0.0),
            }
        };
        if args.get(0).is_none() {
            return Err(usage("rotate"));
        }
        let (y, x, z) = (angle(0)?, angle(1)?, angle(2)?);
        let clip = self.sessions.get(actor.uuid).clipboard.as_mut().ok_or("Your clipboard is empty. Use //copy first.")?;
        let mut lines = Vec::new();
        // `abs(angle % 90) > 0.001`: an angle that is not a multiple of 90 is rounded to the nearest block.
        if (y % 90.0).abs() > 0.001 || (x % 90.0).abs() > 0.001 || (z % 90.0).abs() > 0.001 {
            lines.push("Note: Interpolation is not yet supported, so angles that are multiples of 90 is recommended.".to_owned());
        }
        // The transform is rotateY(-y), then rotateX(-x), then rotateZ(-z).
        if (y % 90.0).abs() <= 0.001 {
            clip.rotate(world, (y / 90.0).round() as i32);
        } else {
            clip.rotate_any(world, -y);
        }
        // Only whole quarter turns of x and z turn the blocks: another angle is rounded to the nearest quarter.
        clip.rotate_xz(world, true, (-x / 90.0).round() as i32);
        clip.rotate_xz(world, false, (-z / 90.0).round() as i32);
        lines.push("The clipboard copy has been rotated.".into());
        Ok(lines)
    }

    fn flip(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let dir = Self::dir_arg(actor, args.get(0))?;
        let clip = self.sessions.get(actor.uuid).clipboard.as_mut().ok_or("Your clipboard is empty. Use //copy first.")?;
        clip.flip(world, dir);
        Ok(vec!["The clipboard copy has been flipped.".into()])
    }

    // ---- history ----

    fn history(&mut self, world: &mut dyn World, actor: &Actor, undo: bool, args: &Args) -> R {
        let times = args.int(0, 1)?.max(1);
        let owner = match args.get(1) {
            None => actor.uuid,
            Some(name) if name.eq_ignore_ascii_case(&actor.name) => actor.uuid,
            Some(name) => *self.names.get(&name.to_ascii_lowercase()).ok_or_else(|| format!("Unable to find session for {name}"))?,
        };
        self.expire_history(owner);
        let (mut edits, mut note) = (0, String::new());
        for _ in 0..times {
            let s = self.sessions.get(owner);
            let change: Option<Change> = if undo { s.undo.pop_back() } else { s.redo.pop() };
            let Some(mut change) = change else { break };
            if change.dimension != actor.dimension {
                let dim = change.dimension.clone();
                let s = self.sessions.get(owner);
                if undo {
                    s.undo.push_back(change);
                } else {
                    s.redo.push(change);
                }
                if edits == 0 {
                    return Err(format!("That edit was made in {dim}: go there to {}", if undo { "undo" } else { "redo" }));
                }
                break;
            }
            let (writes, entities) = if undo { (change.backward(), change.backward_entities()) } else { (change.forward(), change.forward_entities()) };
            edits += 1;
            note = self.write(world, actor, writes, entities);
            Self::replay_extras(world, &mut change, undo);
            let s = self.sessions.get(owner);
            if undo {
                s.redo.push(change);
            } else {
                s.undo.push_back(change);
            }
            if self.queue.busy(actor.uuid) {
                break;
            }
        }
        if edits == 0 {
            return Err(if undo { "Nothing left to undo.".into() } else { "Nothing left to redo.".into() });
        }
        Ok(vec![format!("{} {edits} available edits.{note}", if undo { "Undid" } else { "Redid" })])
    }

    // ---- schematics ----

    fn schem(&mut self, world: &mut dyn World, actor: &Actor, args: &Args, env: &Env) -> R {
        let dir = &env.schematics_dir;
        match args.get(0).map(str::to_ascii_lowercase).as_deref() {
            None => Err(usage("schem")),
            Some("list" | "ls" | "all") => {
                let page: usize = match args.value('p') {
                    Some(p) => p.parse().map_err(|_| number_error(p))?,
                    None => 1,
                };
                list_schematics(dir, page, args.flag('d'), args.flag('n'), self.config.allow_symlinks)
            }
            Some("formats" | "listformats" | "f") => Ok(vec![
                "Available clipboard formats (Name: Lookup names)".into(),
                "sponge_v3: sponge, sponge.3, schem3 (write and read)".into(),
                "sponge_v1, sponge_v2: sponge.1, sponge.2 (read)".into(),
                "litematica: litematic (read)".into(),
                "minecraft_structure: structure, nbt (read)".into(),
            ]),
            Some("load") => {
                let name = args.rest(1);
                if name.is_empty() {
                    return Err(usage("schem load"));
                }
                let path = find_schematic(dir, &name, self.config.allow_symlinks)?;
                let bytes = std::fs::read(&path).map_err(|e| format!("Failed to load schematic: {e}"))?;
                let max_blocks = self.max_blocks(actor);
                let max = if max_blocks >= 0 { (max_blocks as u64).min(SCAN_LIMIT) } else { SCAN_LIMIT };
                let loaded = schem::read(&bytes, max).map_err(|e| format!("Failed to load schematic: {e}"))?;
                let ids: Vec<Option<StateId>> = loaded.palette.iter().map(|s| world.resolve(s)).collect();
                let mut unknown = 0usize;
                let blocks: Vec<(Pos, StateId)> = loaded
                    .blocks
                    .iter()
                    .filter_map(|&(p, i)| match ids.get(i as usize).copied().flatten() {
                        Some(s) => Some((p, s)),
                        None => {
                            unknown += 1;
                            None
                        }
                    })
                    .collect();
                let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| name.clone());
                let mut clip = Clipboard::from_blocks(blocks, format!("{file} ({})", loaded.format));
                clip.entities = loaded
                    .block_entities
                    .iter()
                    .map(|(p, t)| (*p, if loaded.data_version > 0 { world.upgrade_block_entity(loaded.data_version, t.clone()) } else { t.clone() }))
                    .collect();
                let mut out = vec!["(Please wait... loading schematic.)".to_owned(), format!("{name} loaded. Paste it with //paste")];
                if unknown > 0 {
                    out.push(format!("{unknown} blocks of it have no match in this game and were left out"));
                }
                self.sessions.get(actor.uuid).clipboard = Some(clip);
                Ok(out)
            }
            Some("save") => {
                let name = args.rest(1);
                if name.is_empty() {
                    return Err(usage("schem save"));
                }
                let path = save_path(dir, &name)?;
                let overwrite = path.exists();
                if overwrite && !args.flag('f') {
                    return Err("That schematic already exists. Use the -f flag to overwrite it.".into());
                }
                let clip = self.sessions.get(actor.uuid).clipboard.as_ref().ok_or("Your clipboard is empty. Use //copy first.")?;
                let bytes = encode(world, clip, env)?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|_| "Could not create folder for schematics!".to_string())?;
                }
                std::fs::write(&path, bytes).map_err(|e| format!("Failed to save schematic: {e}"))?;
                Ok(vec!["(Please wait... saving schematic.)".to_owned(), format!("{name} saved{}", if overwrite { " (overwriting previous file)." } else { "." })])
            }
            Some("delete" | "d" | "remove") => {
                let name = args.rest(1);
                if name.is_empty() {
                    return Err(usage("schem delete"));
                }
                let path = find_schematic(dir, &name, self.config.allow_symlinks)?;
                std::fs::remove_file(&path).map_err(|_| format!("Deletion of {name} failed! Is it read-only?"))?;
                Ok(vec![format!("{name} has been deleted.")])
            }
            Some(other) => Err(format!("The sub-command '{other}' under 'schem' could not be found.")),
        }
    }

    // ---- brushes ----

    fn held_tool(&self, actor: &Actor) -> Result<String, String> {
        match actor.held.as_deref() {
            None | Some("minecraft:air") => Err("Can't bind tool to Air: Blocks can't be used.".into()),
            Some(i) if i == self.config.wand_item => Err("The wand selects; bind brushes to another item".into()),
            Some(i) => Ok(i.to_owned()),
        }
    }

    fn unbind_hint() -> String {
        "Run /brush unbind while holding the item to unbind it.".into()
    }

    fn brush(&mut self, world: &mut dyn World, actor: &Actor, args: &Args, env: &Env) -> R {
        let kind = args.get(0).ok_or_else(|| usage("brush"))?.to_ascii_lowercase();
        if matches!(kind.as_str(), "none" | "unbind") {
            return self.unbind(actor, false);
        }
        let item = self.held_tool(actor)?;
        let radius = |at: usize, default: f64| -> Result<f64, String> {
            match args.get(at) {
                None => Ok(default),
                Some(w) => w.parse::<f64>().map_err(|_| number_error(w)),
            }
        };
        let (shape, pattern) = match kind.as_str() {
            "sphere" | "s" => {
                let p = self.need_pattern(world, actor, args, 1, "brush sphere")?;
                (BrushShape::Sphere { radius: radius(2, 2.0)?, hollow: args.flag('h') }, Some(p))
            }
            "cylinder" | "cyl" | "c" => {
                let p = self.need_pattern(world, actor, args, 1, "brush cylinder")?;
                (BrushShape::Cylinder { radius: radius(2, 2.0)?, height: args.int(3, 1)?, hollow: args.flag('h') }, Some(p))
            }
            "smooth" => (BrushShape::Smooth { radius: radius(1, 2.0)?, iterations: args.int(2, 4)?.clamp(1, 50) as u32 }, None),
            // ClipboardBrush: the clipboard as it is now (WorldEdit holds it from the equip on).
            "clipboard" | "copy" => {
                let Some(clip) = self.sessions.get(actor.uuid).clipboard.clone() else {
                    return Err("Your clipboard is empty. Use //copy first.".into());
                };
                for d in clip.bounds.size() {
                    // checkMaxBrushRadius(size / 2 - 1) on each axis.
                    if f64::from(d) / 2.0 - 1.0 > self.config.max_brush_radius {
                        return Err(format!("Maximum brush radius (in configuration): {}", self.config.max_brush_radius as i64));
                    }
                }
                let mask = match args.value('m') {
                    Some(text) => {
                        Mask::parse(world, text, actor.held.as_deref())?;
                        Some(text.to_owned())
                    }
                    None => None,
                };
                let shape = BrushShape::Paste { clip, ignore_air: args.flag('a'), use_origin: args.flag('o'), structure_void: args.flag('v'), mask, entities: args.flag('e'), biomes: args.flag('b') };
                (shape, None)
            }
            // SnowSmoothBrush: `-l` snow blocks under the snow, `-m` the mask of the height map.
            "snowsmooth" => {
                let layers = match args.value('l') {
                    Some(v) => v.parse::<i32>().map_err(|_| number_error(v))?,
                    None => 1,
                };
                let mask = match args.value('m') {
                    Some(text) => {
                        Mask::parse(world, text, actor.held.as_deref())?;
                        Some(text.to_owned())
                    }
                    None => None,
                };
                let shape = BrushShape::SnowSmooth { radius: radius(1, 2.0)?, iterations: args.int(2, 4)?.clamp(0, 50) as u32, layers, mask };
                (shape, None)
            }
            "forest" | "feature" | "biome" => {
                let (shape, message) = self.terrain_brush_shape(world, kind.as_str(), args)?;
                let s = self.sessions.get(actor.uuid);
                s.tools.remove(&item);
                s.brushes.insert(item.clone(), Brush { shape, pattern: None, mask: None });
                return Ok(vec![message, Self::unbind_hint()]);
            }
            "splatter" | "splat" => {
                let p = self.need_pattern(world, actor, args, 1, "brush splatter")?;
                let decay = args.int(3, 1)?;
                (BrushShape::Splatter { radius: radius(2, 2.0)?, decay }, Some(p))
            }
            // `-h` takes the height (`@VertHeight`): without it the configured default vertical height, not the radius.
            "gravity" | "grav" => {
                let height = match args.value('h') {
                    Some("default-vertical-height") | None => self.config.default_vertical_height,
                    Some(v) => v.parse::<i32>().map_err(|_| number_error(v))?,
                };
                (BrushShape::Gravity { radius: radius(1, 5.0)?, height: Some(height) }, None)
            }
            "set" | "raise" | "lower" | "snow" => {
                let shape = Self::region_shape(args.get(1).ok_or_else(|| usage(&format!("brush {kind}")))?)?;
                let radius = radius(2, 5.0)?;
                match kind.as_str() {
                    "set" => (BrushShape::Region { kind: 0, shape, radius, fill: Some(self.need_pattern(world, actor, args, 3, "brush set")?) }, None),
                    "raise" => (BrushShape::Region { kind: 1, shape, radius, fill: None }, None),
                    "lower" => (BrushShape::Region { kind: 2, shape, radius, fill: None }, None),
                    _ => (BrushShape::Snow { shape, radius, stack: args.flag('s') }, None),
                }
            }
            "morph" => {
                let shape = BrushShape::Morph { radius: radius(1, 5.0)?, min_erode: args.int(2, 3)?, erode_iter: args.int(3, 1)?, min_dilate: args.int(4, 3)?, dilate_iter: args.int(5, 1)? };
                (shape, None)
            }
            "erode" => (BrushShape::Morph { radius: radius(1, 5.0)?, min_erode: 2, erode_iter: 1, min_dilate: 5, dilate_iter: 1 }, None),
            "dilate" => (BrushShape::Morph { radius: radius(1, 5.0)?, min_erode: 5, erode_iter: 1, min_dilate: 2, dilate_iter: 1 }, None),
            // The radius limit of the butcher brush differs from the others (BrushCommands.butcherBrush): the
            // larger of the brush limit and the butcher limit for those who may butcher.
            "butcher" | "kill" => {
                let r = radius(1, 5.0)?;
                let max = if self.holds(actor, "worldedit.butcher") { self.config.max_brush_radius.max(f64::from(self.config.butcher_max_radius)) } else { self.config.max_brush_radius };
                if r > max {
                    return Err(format!("Maximum allowed brush radius: {}", java_double(max)));
                }
                use crate::horizon_worldedit_core::brushes::butcher_flags as f;
                // `-f` first (no permission of its own), then each flag cut by its node.
                let wanted = [('p', f::PETS, "pets"), ('n', f::NPCS, "npcs"), ('g', f::GOLEMS, "golems"), ('a', f::ANIMALS, "animals"), ('b', f::AMBIENT, "ambient"), ('t', f::TAGGED, "tagged"), ('r', f::ARMOR_STAND, "armorstands"), ('w', f::WATER, "water")];
                let mut flags = if args.flag('f') { f::FRIENDLY } else { 0 };
                for (c, bit, node) in wanted {
                    if args.flag(c) {
                        flags |= bit;
                    }
                    if flags & bit != 0 && !self.holds(actor, &format!("worldedit.butcher.{node}")) {
                        flags &= !bit;
                    }
                }
                (BrushShape::Butcher { radius: r, flags }, None)
            }
            "deform" => (self.deform_brush_shape(actor, args)?, None),
            "apply" | "paint" => (self.apply_brush_shape(world, actor, args, kind == "paint")?, None),
            "heightmap" => {
                let name = args.get(1).ok_or_else(|| usage("brush heightmap"))?;
                let dir = env.config_file.parent().map(|p| p.join("assets")).unwrap_or_else(|| PathBuf::from("assets"));
                let Some(path) = crate::horizon_worldedit_core::heightmap::find(&dir, name) else {
                    return Err(format!("Unknown heightmap brush: {name}."));
                };
                let r = radius(2, 5.0)?;
                let intensity = match args.get(3) {
                    Some(w) => w.parse::<f64>().map_err(|_| number_error(w))?,
                    None => 5.0,
                };
                self.check_brush_radius(r)?;
                let image = crate::horizon_worldedit_core::heightmap::load(&path).ok_or("Failed to load asset")?;
                (BrushShape::Heightmap { image: std::sync::Arc::new(image), radius: r, intensity, erase: args.flag('e'), flatten: args.flag('f'), randomize: args.flag('r') }, None)
            }
            // The fire extinguisher: a sphere of air over the fire it finds.
            "extinguish" | "ex" => {
                let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
                (BrushShape::Sphere { radius: radius(1, 5.0)?, hollow: false }, Some(Pattern::single(air, "air")))
            }
            other => return Err(format!("The sub-command '{other}' under 'brush' could not be found.")),
        };
        let (r, h) = match &shape {
            BrushShape::Sphere { radius, .. } | BrushShape::Smooth { radius, .. } | BrushShape::SnowSmooth { radius, .. } => (*radius, 0),
            BrushShape::Paste { .. } => (0.0, 0),
            BrushShape::Splatter { radius, .. } | BrushShape::Gravity { radius, .. } | BrushShape::Region { radius, .. } | BrushShape::Snow { radius, .. } | BrushShape::Morph { radius, .. } | BrushShape::Heightmap { radius, .. } => (*radius, 0),
            BrushShape::Butcher { .. } | BrushShape::Terrain { .. } => (0.0, 0),
            BrushShape::Apply { radius, .. } | BrushShape::Deform { radius, .. } => (*radius, 0),
            BrushShape::Cylinder { radius, height, .. } => (*radius, *height),
        };
        self.check_brush_radius(r.max(f64::from(h)))?;
        let arg_mask = if kind == "smooth" && args.words.len() > 3 { Some(args.rest(3)) } else { None };
        if let Some(text) = &arg_mask {
            Mask::parse(world, text, actor.held.as_deref())?;
        }
        let extinguisher = matches!(kind.as_str(), "extinguish" | "ex");
        // `String.format("%.0f")` for the sphere, a truncation for the others.
        let msg = match &shape {
            BrushShape::Sphere { radius, .. } if extinguisher => format!("Extinguisher equipped ({}).", *radius as i64),
            BrushShape::Sphere { radius, .. } => format!("Sphere brush shape equipped ({}).", (radius + 0.0).round() as i64),
            BrushShape::Cylinder { radius, height, .. } => format!("Cylinder brush shape equipped ({} by {height}).", *radius as i64),
            BrushShape::Smooth { radius, iterations } => {
                format!("Smooth brush equipped ({} x {iterations}x using {}).", *radius as i64, if arg_mask.is_some() { "filter" } else { "any block" })
            }
            BrushShape::Paste { .. } => "Clipboard brush shape equipped.".to_owned(),
            BrushShape::Splatter { radius, decay } => {
                if !(0..=10).contains(decay) {
                    return Err(format!("Splatter brush decay value: {decay} is out of range 0 - 10."));
                }
                format!("Splatter brush shape equipped ({} with {decay} decay).", *radius as i64)
            }
            BrushShape::Gravity { radius, .. } => format!("Gravity brush equipped ({}).", *radius as i64),
            // `OperationFactoryBrush`: the operation's own description.
            BrushShape::Region { kind: 0, .. } => "Set brush to set replace blocks.".to_owned(),
            BrushShape::Region { kind: 1, .. } => "Set brush to deformation of y-=1.".to_owned(),
            BrushShape::Region { .. } => "Set brush to deformation of y+=1.".to_owned(),
            BrushShape::Snow { stack, .. } => format!("Set brush to set snow{}.", if *stack { " stacker" } else { "" }),
            BrushShape::Morph { radius, .. } => format!("Morph brush shape equipped: {}.", *radius as i64),
            BrushShape::Butcher { radius, .. } => format!("Butcher brush equipped ({}).", *radius as i64),
            BrushShape::Heightmap { radius, .. } => format!("Heightmap brush equipped ({}).", *radius as i64),
            BrushShape::Terrain { .. } => String::new(),
            BrushShape::Apply { paint, op, .. } => format!("Set brush to {}.", more::describe_apply(*paint, op)),
            BrushShape::Deform { expression, .. } => format!("Set brush to deformation of {expression}."),
            BrushShape::SnowSmooth { radius, iterations, layers, mask } => {
                format!("SnowSmooth brush equipped ({} x {iterations}x using {}), {layers} snow blocks.", *radius as i64, if mask.is_some() { "filter" } else { "any block" })
            }
        };
        let s = self.sessions.get(actor.uuid);
        let mask = if extinguisher { Some("minecraft:fire".to_owned()) } else { arg_mask.or_else(|| s.brushes.get(&item).and_then(|b| b.mask.clone())) };
        s.tools.remove(&item);
        // A new brush has the size it was bound with; `/range`, `/tracemask` stay (WorldEdit reuses the item's BrushTool).
        if let Some(o) = s.brush_opts.get_mut(&item) {
            o.size = None;
        }
        let warn = matches!(&shape, BrushShape::Apply { op: crate::horizon_worldedit_core::session::ApplyOp::Item(..), .. });
        s.brushes.insert(item.clone(), Brush { shape, pattern, mask });
        let mut out = Vec::new();
        if warn {
            out.push(more::ITEM_WARNING.to_owned());
        }
        out.push(msg);
        out.push(Self::unbind_hint());
        Ok(out)
    }

    /// `WorldEdit.checkMaxBrushRadius`: only a positive limit limits.
    fn check_brush_radius(&self, r: f64) -> Result<(), String> {
        if self.config.max_brush_radius > 0.0 && r > self.config.max_brush_radius {
            return Err(format!("Maximum brush radius (in configuration): {}", self.config.max_brush_radius as i64));
        }
        Ok(())
    }

    /// The `RegionFactory` argument of the operation brushes: `cuboid`, `sphere`, `cyl` or `cylinder`.
    fn region_shape(word: &str) -> Result<u8, String> {
        match word.to_ascii_lowercase().as_str() {
            "sphere" => Ok(0),
            "cyl" | "cylinder" => Ok(1),
            "cuboid" => Ok(2),
            _ => Err(format!("Not a known region type: {word}")),
        }
    }

    fn brush_bound(&mut self, actor: &Actor) -> Result<String, String> {
        let item = actor.held.clone().unwrap_or_default();
        if self.sessions.get(actor.uuid).brushes.contains_key(&item) {
            Ok(item)
        } else {
            Err("You have no brush bound to your current item. Try /brush sphere for a basic brush.".into())
        }
    }

    /// `/brush none` and `/tool none`: an unbound item is a message, not an error.
    fn unbind(&mut self, actor: &Actor, tool: bool) -> R {
        let item = actor.held.clone().unwrap_or_default();
        let s = self.sessions.get(actor.uuid);
        let was_brush = s.brushes.remove(&item).is_some();
        let was_tool = s.tools.remove(&item).is_some();
        s.brush_opts.remove(&item);
        // `ToolCommands.setToolNone`: the wand items count as bound for brushes and tools alike.
        let bound = was_brush || was_tool || item == self.config.wand_item || item == self.config.nav_wand_item;
        Ok(vec![match (bound, tool) {
            (true, true) => "Tool unbound from your current item.",
            (true, false) => "Brush unbound from your current item.",
            (false, _) => "Your current item is not bound.",
        }
        .into()])
    }

    /// A player's cuboid selection for another mod (WorldGuard's `/rg define`): the level and the
    /// two corners in click order. `None` while a corner is missing.
    pub fn selection_of(&mut self, player: u128) -> Option<(String, Pos, Pos)> {
        let s = self.sessions.get(player);
        Some((s.dimension.clone(), s.pos1?, s.pos2?))
    }

    /// A player's corners as set so far (either may be missing), for the selection outline.
    pub fn corners_of(&mut self, player: u128) -> (Option<Pos>, Option<Pos>) {
        let s = self.sessions.get(player);
        (s.pos1, s.pos2)
    }

    /// Replaces a player's selection with the cuboid between `a` and `b` (WorldGuard's `/rg select`);
    /// their outline is drawn again at the next tick.
    pub fn select(&mut self, player: u128, dimension: &str, a: Pos, b: Pos) {
        let s = self.sessions.get(player);
        s.pos1 = Some(a);
        s.pos2 = Some(b);
        s.shape = None;
        s.kind = crate::horizon_worldedit_core::selector::SelKind::Cuboid;
        s.dimension = dimension.to_owned();
        if !self.outline_due.contains(&player) {
            self.outline_due.push(player);
        }
    }

    /// The players whose outline `select` asked for, once.
    pub fn take_outline_due(&mut self) -> Vec<u128> {
        std::mem::take(&mut self.outline_due)
    }

    /// The box to outline for `player` (`server_side_cui`): their selection, or the one corner set,
    /// and its level.
    pub fn outline_of(&mut self, player: u128) -> Option<(String, Cuboid)> {
        if !self.config.server_side_cui || self.cui_off.contains(&player) {
            return None;
        }
        let s = self.sessions.get(player);
        let c = match (s.pos1, s.pos2) {
            (Some(a), Some(b)) => Cuboid::new(a, b),
            (Some(a), None) | (None, Some(a)) => Cuboid::new(a, a),
            (None, None) => return None,
        };
        Some((s.dimension.clone(), c))
    }

    // ---- clicks ----

    /// Whether this click repeats the last one within 200 ms: a dig packet sent twice (left
    /// clicks), or a brush's block click followed by the same click's use in the air. Right clicks
    /// with the wand are not filtered: the game hook names the hand, and only the main hand counts.
    fn repeated(&mut self, actor: &Actor, p: Pos, first: bool, now_ms: i64) -> bool {
        let last = self.last_click.insert(actor.uuid, (p, first, now_ms));
        last.is_some_and(|(lp, lf, t)| lp == p && lf == first && now_ms - t < 200)
    }

    /// A left (`first`) or right click on the block at `pos`: with the wand, the corner, and the
    /// click does nothing else (`Some`). `None` leaves the click to the game.
    ///
    /// As WorldEdit's `SelectionWand`: it needs `worldedit.selection.pos`, and when the selector
    /// reports no change (the corner is already there: a held button, a dig packet or a click sent
    /// twice) it says nothing, where `//pos1` and `//pos2` answer "Position already set.". The click
    /// is taken either way, so the block is neither dug nor used.
    pub fn wand_click(&mut self, world: &dyn World, actor: &Actor, first: bool, pos: Pos, now_ms: i64) -> Option<Reply> {
        self.now_ms = now_ms;
        let usable = self.permitted(actor, Some("worldedit.selection.pos")).is_ok() && !(self.config.creative_only && actor.game_mode != 1);
        if actor.held.as_deref() != Some(self.config.wand_item.as_str()) || !usable {
            return None;
        }
        if first && self.repeated(actor, pos, first, now_ms) {
            return Some(Reply::default());
        }
        Some(match self.corner_set(world, actor, first, pos) {
            Ok(lines) => Reply::ok(lines),
            Err(_) => Reply::default(),
        })
    }

    /// A right click with an item that carries a brush: the brush at the block in sight (`pos`
    /// when the click was on a block). `None` when the item has no brush.
    pub fn brush_use(&mut self, world: &mut dyn World, actor: &Actor, pos: Option<Pos>, now_ms: i64) -> Option<Reply> {
        let item = actor.held.clone()?;
        let mut brush = self.sessions.get(actor.uuid).brushes.get(&item)?.clone();
        self.enter_session(actor, brush.mask.as_deref().unwrap_or(""));
        // `/size`: the radius the player set replaces the one the brush was bound with.
        self.apply_size(actor, &item, &mut brush.shape);
        self.now_ms = now_ms;
        let node = match brush.shape {
            BrushShape::Sphere { .. } => "worldedit.brush.sphere",
            BrushShape::Cylinder { .. } => "worldedit.brush.cylinder",
            BrushShape::Smooth { .. } => "worldedit.brush.smooth",
            BrushShape::SnowSmooth { .. } => "worldedit.brush.snowsmooth",
            BrushShape::Paste { .. } => "worldedit.brush.clipboard",
            BrushShape::Splatter { .. } => "worldedit.brush.splatter",
            BrushShape::Gravity { .. } => "worldedit.brush.gravity",
            BrushShape::Region { kind: 0, .. } => "worldedit.brush.set",
            BrushShape::Region { kind: 1, .. } => "worldedit.brush.raise",
            BrushShape::Region { .. } => "worldedit.brush.lower",
            BrushShape::Snow { .. } => "worldedit.brush.snow",
            BrushShape::Morph { .. } => "worldedit.brush.morph",
            BrushShape::Butcher { .. } => "worldedit.brush.butcher",
            BrushShape::Heightmap { .. } => "worldedit.brush.heightmap",
            BrushShape::Apply { paint: false, .. } => "worldedit.brush.apply",
            BrushShape::Apply { .. } => "worldedit.brush.paint",
            BrushShape::Deform { .. } => "worldedit.brush.deform",
            BrushShape::Terrain { kind: 0, .. } => "worldedit.brush.forest",
            BrushShape::Terrain { kind: 1, .. } => "worldedit.brush.feature",
            BrushShape::Terrain { .. } => "worldedit.brush.biome",
        };
        if let Err(e) = self.permitted(actor, Some(node)).and(self.allowed(actor)) {
            return Some(Reply::err(e));
        }
        if self.queue.busy(actor.uuid) {
            return Some(Reply::err("Your last edit is still being written; wait a moment".into()));
        }
        // The block in sight (`/range`, `/tracemask`), or the one that was clicked when the item has neither.
        let target = match self.brush_target(&*world, actor, &item, pos) {
            Ok(p) => p,
            Err(e) => return Some(Reply::err(e)),
        };
        if self.repeated(actor, target, false, now_ms) {
            return Some(Reply::default());
        }
        let mask = match brush.mask.as_deref().map(|m| Mask::parse(world, m, None)) {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => return Some(Reply::err(e)),
            None => None,
        };
        // ClipboardBrush.build: the copy's centre goes to the target (its origin with -o); air left out with -a,
        // structure voids unless -v, and only the blocks the -m mask accepts are pasted.
        if let BrushShape::Paste { clip, ignore_air, use_origin, structure_void, mask: source, entities: paste_mobs, biomes: paste_biomes } = &brush.shape {
            let source = match source.as_deref().map(|m| Mask::parse(world, m, None)) {
                Some(Ok(m)) => Some(m),
                Some(Err(e)) => return Some(Reply::err(e)),
                None => None,
            };
            let b = clip.bounds;
            // Region.getCenter() floored (toBlockPoint), minus the origin, which is zero in clipboard coordinates.
            let centre = [0, 1, 2].map(|a| ((f64::from(b.min[a]) + f64::from(b.max[a])) / 2.0).floor() as i32);
            let at = if *use_origin { target } else { sub(target, centre) };
            let mut writes = clip.writes(world, at, *ignore_air);
            if !*structure_void {
                writes.retain(|(_, s)| world.describe(*s).name != "minecraft:structure_void");
            }
            if let Some(m) = &source {
                let w: &dyn World = &*world;
                writes.retain(|(p, s)| m.hits_at(w, *p, *s));
            }
            let entities = clip.entities_for(at, &writes);
            let plan = clip.extras_plan(at, *paste_mobs, *paste_biomes);
            return Some(match self.commit_with(world, actor, writes, entities) {
                Ok(changed) => {
                    if *paste_mobs || *paste_biomes {
                        let (extras, _) = Self::paste_extras(world, plan);
                        self.remember_extras(actor, changed.0, extras);
                    }
                    Reply::default()
                }
                Err(e) if e.starts_with("Max blocks changed") => Reply::err("Max blocks change limit reached.".into()),
                Err(e) => Reply::err(e),
            });
        }
        if matches!(&brush.shape, BrushShape::Apply { .. }) {
            return Some(self.apply_brush_use(world, actor, &brush.shape, target, mask.as_ref()));
        }
        if matches!(&brush.shape, BrushShape::Deform { .. }) {
            return Some(self.deform_brush_use(world, actor, &brush.shape, target, mask.as_ref()));
        }
        // The forest, feature and biome brushes act through the game, not through block writes.
        if let BrushShape::Terrain { .. } = &brush.shape {
            return Some(self.terrain_brush_use(world, actor, &brush.shape, target));
        }
        // ButcherBrush.build: the entities in a full-height cylinder round the target, by the flags chosen at equip.
        if let BrushShape::Butcher { radius, flags } = &brush.shape {
            let r = *radius;
            let all = world.entities_in([f64::from(target[0]) - r - 1.0, -4096.0, f64::from(target[2]) - r - 1.0], [f64::from(target[0]) + r + 2.0, 4096.0, f64::from(target[2]) + r + 2.0]);
            let ids: Vec<u64> = brushes::in_butcher_cylinder(all, target, r).iter().filter(|m| extra::butcher_kills(m, *flags)).map(|m| m.id).collect();
            world.remove_entities(&ids);
            return Some(Reply::default());
        }
        let salt = self.salt(actor);
        let (lo, hi) = (world.min_y(), world.max_y());
        let keep = |p: &Pos, world: &dyn World| (lo..=hi).contains(&p[1]) && mask.as_ref().is_none_or(|m| world.get(*p).is_some_and(|s| m.hits_at(world, *p, s)));
        let ops: Vec<(Pos, StateId)> = match (&brush.shape, &brush.pattern) {
            (BrushShape::Sphere { radius, hollow }, Some(p)) => shapes::sphere(target, [*radius; 3], *hollow).into_iter().filter(|q| keep(q, world)).map(|q| (q, p.pick_with(&*world, salt, q))).collect(),
            (BrushShape::Cylinder { radius, height, hollow }, Some(p)) => shapes::cylinder(target, [*radius; 2], *height, *hollow).into_iter().filter(|q| keep(q, world)).map(|q| (q, p.pick_with(&*world, salt, q))).collect(),
            (BrushShape::Smooth { radius, iterations }, _) => {
                let r = radius.ceil() as i32;
                let region = Cuboid::new(sub(target, [r, r, r]), [target[0] + r, target[1] + r, target[2] + r]);
                ops::smooth(world, &region, *iterations, mask.as_ref())
            }
            (BrushShape::SnowSmooth { radius, iterations, layers, mask: heights }, _) => {
                let region = ops::snow_brush_box(target, *radius);
                let heights = match heights.as_deref().map(|m| Mask::parse(world, m, None)) {
                    Some(Ok(m)) => Some(m),
                    Some(Err(e)) => return Some(Reply::err(e)),
                    None => None,
                };
                ops::snow_smooth(world, &region, *iterations, *layers, heights.as_ref(), 10)
            }
            (BrushShape::Splatter { radius, decay }, Some(p)) => {
                let (d, sq) = (f64::from(*decay) / 10.0, radius * radius);
                shapes::sphere(target, [*radius; 3], false)
                    .into_iter()
                    .filter(|q| keep(q, world))
                    .filter(|q| {
                        let dist = (0..3).map(|a| f64::from(q[a] - target[a]).powi(2)).sum::<f64>();
                        let h = (salt ^ (q[0] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (q[1] as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (q[2] as u64).wrapping_mul(0x1656_67B1_9E37_79F9)).wrapping_mul(0xFF51_AFD7_ED55_8CCD);
                        let r = ((h ^ (h >> 33)) >> 11) as f64 / (1u64 << 53) as f64;
                        r > dist / sq * d * 2.0
                    })
                    .map(|q| (q, p.pick_with(&*world, salt, q)))
                    .collect()
            }
            (BrushShape::Gravity { radius, height }, _) => {
                let off = f64::from(height.unwrap_or(self.config.default_vertical_height));
                brushes::gravity(world, target, *radius, off).into_iter().filter(|(q, _)| keep(q, world)).collect()
            }
            (BrushShape::Snow { shape, radius, stack }, _) => {
                let (cols, y0, y1) = brushes::snow_columns(target, *radius, *shape);
                brushes::snow(world, &cols, y0, y1, *stack).into_iter().filter(|(q, _)| keep(q, world)).collect()
            }
            (BrushShape::Morph { radius, min_erode, erode_iter, min_dilate, dilate_iter }, _) => {
                match brushes::morph(world, target, *radius, *min_erode, *erode_iter, *min_dilate, *dilate_iter, SCAN_LIMIT) {
                    Ok(v) => v.into_iter().filter(|(q, _)| keep(q, world)).collect(),
                    Err(e) => return Some(Reply::err(e)),
                }
            }
            (BrushShape::Heightmap { image, radius, intensity, erase, flatten, randomize }, _) => {
                let random = (salt >> 11) as f64 / (1u64 << 53) as f64;
                brushes::heightmap(world, mask.as_ref(), image, *intensity, *erase, *flatten, *randomize, target, *radius, random).into_iter().filter(|(q, _)| keep(q, world)).collect()
            }
            (BrushShape::Region { kind, shape, radius, fill }, _) => {
                let cells = match shape {
                    0 => shapes::sphere(target, [*radius; 3], false),
                    1 => shapes::cylinder(target, [*radius; 2], 1, false),
                    _ => {
                        let r = *radius as i32;
                        let mut v = Vec::new();
                        for x in -r..=r {
                            for y in -r..=r {
                                for z in -r..=r {
                                    v.push([target[0] + x, target[1] + y, target[2] + z]);
                                }
                            }
                        }
                        v
                    }
                };
                let cells: Vec<Pos> = cells.into_iter().filter(|q| keep(q, world)).collect();
                match (kind, fill) {
                    (0, Some(p)) => cells.into_iter().map(|q| (q, p.pick_with(&*world, salt, q))).collect(),
                    // Deform "y-=1" / "y+=1" in raw coordinates: each block takes the one below (above).
                    (k, _) => {
                        let dy = if *k == 1 { -1 } else { 1 };
                        cells.into_iter().filter_map(|q| world.get([q[0], q[1] + dy, q[2]]).map(|s| (q, s))).collect()
                    }
                }
            }
            _ => Vec::new(),
        };
        match self.commit(world, actor, ops) {
            Ok(_) => Some(Reply::default()),
            Err(e) if e.starts_with("Max blocks changed") => Some(Reply::err("Max blocks change limit reached.".into())),
            Err(e) => Some(Reply::err(e)),
        }
    }
}

const EXTENSIONS: [&str; 3] = [".schem", ".litematic", ".nbt"];

/// A name inside the schematics folder: no absolute path, no `..`, no backslash.
fn safe_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("No file selected.".into());
    }
    if name.starts_with('/') || name.contains('\\') || name.contains(':') || name.split('/').any(|p| p == ".." || p == "." || p.is_empty()) {
        return Err(format!("File '{name}' resolution error: Path is outside allowable root"));
    }
    Ok(name)
}

/// The file `name` means: as typed, or with one of the extensions.
fn find_schematic(dir: &Path, name: &str, links: bool) -> Result<PathBuf, String> {
    let name = safe_name(name)?;
    for ext in std::iter::once("").chain(EXTENSIONS) {
        let path = dir.join(format!("{name}{ext}"));
        // No links out of the folder unless `files.allow-symbolic-links` says so.
        if let Ok(meta) = if links { std::fs::metadata(&path) } else { std::fs::symlink_metadata(&path) } {
            if meta.is_file() {
                return Ok(path);
            }
        }
    }
    Err(format!("Schematic {name} does not exist!"))
}

fn save_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = safe_name(name)?;
    let lower = name.to_ascii_lowercase();
    if EXTENSIONS.iter().any(|e| lower.ends_with(e)) && !lower.ends_with(".schem") {
        return Err("Saving writes .schem (Sponge v3) only".into());
    }
    Ok(dir.join(if lower.ends_with(".schem") { name.to_owned() } else { format!("{name}.schem") }))
}

/// `//schem list [-p page] [-d|-n]`: eight a page, oldest (`-d`) or newest (`-n`) first, or by name.
fn list_schematics(dir: &Path, page: usize, oldest_first: bool, newest_first: bool, links: bool) -> R {
    if oldest_first && newest_first {
        return Err("Cannot sort by oldest and newest.".into());
    }
    let mut files: Vec<(String, std::time::SystemTime)> = Vec::new();
    let mut walk = vec![dir.to_path_buf()];
    while let Some(d) = walk.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(meta) = (if links { std::fs::metadata(entry.path()) } else { std::fs::symlink_metadata(entry.path()) }) else { continue };
            if meta.is_dir() && files.len() < 5000 {
                walk.push(entry.path());
            } else if meta.is_file() {
                let rel = entry.path().strip_prefix(dir).map(|p| p.display().to_string()).unwrap_or_default();
                if EXTENSIONS.iter().any(|e| rel.to_ascii_lowercase().ends_with(e)) {
                    files.push((rel, meta.modified().unwrap_or(std::time::UNIX_EPOCH)));
                }
            }
        }
    }
    if files.is_empty() {
        return Err("No schematics found.".into());
    }
    if oldest_first {
        files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    } else if newest_first {
        files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    } else {
        files.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let pages = files.len().div_ceil(8);
    if page < 1 || page > pages {
        return Err("Invalid page number".into());
    }
    let mut out = vec!["Available schematics".to_owned()];
    out.extend(files.iter().skip((page - 1) * 8).take(8).map(|(n, _)| format!("[L] {n}")));
    if pages > 1 {
        out.push(format!("Page {page} of {pages}"));
    }
    Ok(out)
}

/// Loads block entity data into the blocks just written.
fn load_entities(world: &mut dyn World, entities: HashMap<Pos, Tag>) {
    if !entities.is_empty() {
        let list: Vec<(Pos, Tag)> = entities.into_iter().collect();
        world.set_block_entities(&list);
    }
}

/// The clipboard as a Sponge v3 file.
fn encode(world: &dyn World, clip: &Clipboard, env: &Env) -> Result<Vec<u8>, String> {
    let size = clip.bounds.size();
    let volume = size.iter().map(|&v| v as usize).product::<usize>();
    let air = world.resolve(&crate::horizon_worldedit_core::block::BlockState::air()).unwrap_or(0);
    let mut palette: Vec<String> = vec![world.describe(air).to_string_full()];
    let mut slot: HashMap<StateId, u32> = HashMap::from([(air, 0)]);
    let mut cells = vec![0u32; volume];
    let (w, l) = (size[0] as usize, size[2] as usize);
    for &(p, s) in &clip.blocks {
        let d = sub(p, clip.bounds.min);
        let i = (d[1] as usize * l + d[2] as usize) * w + d[0] as usize;
        let index = *slot.entry(s).or_insert_with(|| {
            palette.push(world.describe(s).to_string_full());
            palette.len() as u32 - 1
        });
        if let Some(c) = cells.get_mut(i) {
            *c = index;
        }
    }
    let origin = clip.origin.as_ref().map_or([0; 3], |o| o.1);
    let entities: Vec<(Pos, Tag)> = clip.entities.iter().map(|(p, t)| (sub(*p, clip.bounds.min), t.clone())).collect();
    schem::write_sponge3(size, clip.bounds.min, origin, env.data_version, &palette, &cells, env.now_ms, &entities)
}

#[cfg(test)]
mod node_tests {
    use super::*;

    #[test]
    fn node_list_has_each_node_once_and_the_navigation_wand() {
        let mut seen = BTreeSet::new();
        assert!(NODES.iter().all(|n| seen.insert(*n)), "a node is listed twice");
        assert!(NODES.contains(&"worldedit.navigation.jumpto.tool"));
        assert!(NODES.contains(&"worldedit.selection.sel"));
        assert!(NODES.contains(&"worldedit.wand"));
    }
}


/// The block a straight trace along the actor's view hits (solid, non-passable), for tests.
#[cfg(test)]
pub(crate) fn tools_target(world: &dyn World, actor: &Actor, max: i32) -> Option<Pos> {
    tools::target_block(world, actor, max, &|p| world.get(p).is_some() && !nav::passable(world, p), false)
}
