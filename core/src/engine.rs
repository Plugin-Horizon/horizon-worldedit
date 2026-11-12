//! The commands, the wand and the brushes, for every player of a server: what a typed `//command`,
//! a click with the wand or a brush does to the player's session and to the world.

use crate::clipboard::Clipboard;
use crate::config::Config;
use crate::edit::{plan, Change, Queue};
use crate::ops;
use crate::pattern::{next_salt, Mask, Pattern};
use crate::region::{sub, Cuboid, Dir};
use crate::schem;
use crate::session::{Brush, BrushShape, Sessions, Tool};
use crate::shapes;
use crate::world::{Pos, StateId, World};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

mod calc;
mod help;
mod tools;
mod utility;

/// The most positions one command looks at, whatever `max_blocks` says: a bigger region is a
/// mistake that would hold the server thread for minutes.
pub const SCAN_LIMIT: u64 = 10_000_000;

/// Every `worldedit.*` node a command, a tool or a brush can need (`permission_node`, the wand, the navigation
/// wand). The glue asks the permission system which of them a player holds (`Actor::granted`).
pub const NODES: &[&str] = &[
    "worldedit.analysis.count",
    "worldedit.analysis.distr",
    "worldedit.brush.cylinder",
    "worldedit.brush.options.mask",
    "worldedit.brush.options.material",
    "worldedit.brush.smooth",
    "worldedit.brush.sphere",
    "worldedit.calc",
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
    "worldedit.navigation.jumpto.tool",
    "worldedit.selection.sel",
    "worldedit.region.center",
    "worldedit.region.faces",
    "worldedit.region.hollow",
    "worldedit.region.line",
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
];

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
    /// The `worldedit.*` nodes the permission system grants this player (see `NODES`). Operators pass on their
    /// level instead; anyone else needs a node here.
    pub granted: BTreeSet<String>,
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
    ("line", "<args:text>"),
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
];

/// The typed usages, one per sub-command path: the names a command answers to (aliases too), the flags
/// its usages may carry (only those this server implements, in the order listed) and the argument
/// paths. In a path, `{dir}` stands for each direction word and `{flags}` for each set of the flags.
/// Typed arguments: `integer`, `number`, `boolean`, `player_name`, `word` (a block, a mask or a pattern
/// without a space), `text` (the rest of the line, always last). Literals are the fixed choices.
const TYPED: &[(&[&str], &str, &[&str])] = &[
    (&["we", "worldedit"], "", &["", "version", "help"]),
    (&["wand"], "", &[""]),
    (&["pos1", "pos2"], "", &["", "<x:word> <y:word> <z:word>"]),
    (&["hpos1", "hpos2"], "", &[""]),
    (&["sel"], "", &["", "cuboid", "extend"]),
    (&["desel", "deselect", "naturalize", "clearclipboard", "clearhistory", "none"], "", &[""]),
    (&["size"], "c", &["{flags}"]),
    (&["chunk"], "sc", &["{flags}", "{flags} <x:word> <y:word> <z:word>"]),
    (&["distr"], "dmp", &["{flags}"]),
    (
        &["expand"],
        "",
        &["vert", "<amount:integer>", "<amount:integer> {dir}", "<amount:integer> <reverse:integer>", "<amount:integer> <reverse:integer> {dir}"],
    ),
    (&["contract"], "", &["<amount:integer>", "<amount:integer> {dir}", "<amount:integer> <reverse:integer>", "<amount:integer> <reverse:integer> {dir}"]),
    (&["shift"], "", &["<amount:integer>", "<amount:integer> {dir}"]),
    (&["outset", "inset"], "hv", &["{flags} <amount:integer>"]),
    (&["count"], "", &["<filter:word>"]),
    (&["set", "walls", "faces", "outline", "center", "middle", "overlay"], "", &["<block:word>"]),
    (&["replace", "re", "rep"], "", &["<to:word>", "<from:word> <to:word>"]),
    (&["hollow"], "", &["", "<thickness:integer>", "<thickness:integer> <block:word>"]),
    (&["smooth"], "", &["", "<iterations:integer>", "<iterations:integer> <filter:word>"]),
    (&["line"], "h", &["{flags} <block:word>", "{flags} <block:word> <thickness:number>"]),
    (&["stack"], "asr", &["{flags}", "<count:integer> {flags}", "<count:integer> {dir} {flags}"]),
    (
        &["move"],
        "as",
        &["{flags}", "<distance:integer> {flags}", "<distance:integer> {dir} {flags}", "<distance:integer> {dir} <leave:word> {flags}"],
    ),
    (&["sphere", "hsphere"], "hr", &["{flags} <block:word> <radii:word> {flags}"]),
    (&["cyl", "hcyl"], "h", &["{flags} <block:word> <radii:word> {flags}", "{flags} <block:word> <radii:word> <height:integer> {flags}"]),
    (&["removeabove", "removebelow"], "", &["", "<size:integer>", "<size:integer> <height:integer>"]),
    (&["removenear"], "", &["<mask:word>", "<mask:word> <radius:integer>"]),
    (&["replacenear"], "", &["<radius:integer> <to:word>", "<radius:integer> <from:word> <to:word>"]),
    (
        &["cone"],
        "h",
        &["{flags} <block:word> <radii:word> {flags}", "{flags} <block:word> <radii:word> <height:integer> {flags}", "{flags} <block:word> <radii:word> <height:integer> <thickness:number>"],
    ),
    (&["pyramid", "hpyramid"], "h", &["{flags} <block:word> <size:integer> {flags}"]),
    (&["copy"], "m", &["{flags}"]),
    (&["cut"], "m", &["{flags}", "<leave:word> {flags}"]),
    (&["paste"], "aonsm", &["{flags}"]),
    (&["rot"], "", &["90", "180", "270", "<degrees:number>"]),
    (&["flip", "mirror"], "", &["", "{dir}"]),
    (&["undo", "redo"], "", &["", "<times:integer>", "<times:integer> <player:player_name>"]),
    (&["schem", "schematic"], "f", &["list", "list -p <page:integer>", "list -d", "list -n", "formats", "load <name:text>", "delete <name:text>", "save {flags} <name:text>"]),
    (&["load"], "", &["<name:text>"]),
    (
        &["brush", "br"],
        "h",
        &[
            "none",
            "unbind",
            "sphere {flags} <block:word>",
            "sphere {flags} <block:word> <radius:number>",
            "cylinder {flags} <block:word>",
            "cylinder {flags} <block:word> <radius:number>",
            "cylinder {flags} <block:word> <radius:number> <height:integer>",
            "smooth",
            "smooth <radius:number>",
            "smooth <radius:number> <iterations:integer>",
            "smooth <radius:number> <iterations:integer> <filter:word>",
        ],
    ),
    (&["mask"], "", &["", "<filter:text>"]),
    (&["material"], "", &["<block:word>"]),
    (&["tool"], "", &["none", "unbind", "selwand", "info", "cycler", "repl <pattern:word>", "floodfill <pattern:word> <range:integer>", "flood <pattern:word> <range:integer>"]),
    (&["we", "worldedit"], "", &["reload", "report", "trace", "cui", "tz <timezone:word>", "help", "help <command:text>"]),
    (&["fixwater", "fixlava"], "", &["<radius:number>"]),
    (&["drain"], "w", &["{flags} <radius:number>"]),
    (&["fillr"], "", &["<pattern:word> <radius:number>", "<pattern:word> <radius:number> <depth:integer>"]),
    (&["snow"], "s", &["{flags}", "{flags} <size:number>", "{flags} <size:number> <height:integer>"]),
    (&["thaw"], "", &["", "<size:number>", "<size:number> <height:integer>"]),
    (&["green"], "f", &["{flags}", "{flags} <size:number>", "{flags} <size:number> <height:integer>"]),
    (&["extinguish", "ex", "ext"], "", &["", "<radius:integer>"]),
    (&["limit"], "", &["", "<limit:integer>"]),
    (&["searchitem", "search", "l"], "bip", &["{flags} <query:text>"]),
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
        for name in names.iter() {
            for path in paths.iter() {
                for rest in expand(path, flags) {
                    for root in roots(name) {
                        push_unique(&mut out, tidy(&format!("{root} {rest}")));
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

/// The commands WorldEdit registers with a single slash only (`/we`, `/mask`, `/tool`...); the
/// names in `BOTH_FORMS` it registers both ways (`/brush` and `//brush`); every other command is
/// registered with a leading `/` in its name (`@Command(name = "/set")`), which the player types as
/// `//set`.
pub const SINGLE_SLASH: &[&str] = &["we", "worldedit", "mask", "none", "tool", "clearclipboard"];
pub const BOTH_FORMS: &[&str] = &[
    "brush", "br", "schem", "schematic", "undo", "redo", "clearhistory", "material", "removeabove", "removebelow", "removenear", "replacenear", "fixwater", "fixlava", "snow", "thaw", "green", "extinguish", "ex", "ext", "gmask",
    "toggleplace", "searchitem",
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
                    'm' => "-m <mask:word>".to_owned(),
                    'p' => "-p <page:integer>".to_owned(),
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
    COMMANDS.iter().any(|(n, _)| *n == name) || is_shadowed(line)
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
        "copy" | "cut" => ("ebm", "m"),
        "paste" => ("abenosvm", "aonsm"),
        "stack" => ("abersm", "asrm"),
        "move" => ("abesm", "asm"),
        "sphere" | "hsphere" => ("hr", "hr"),
        "cyl" | "hcyl" | "pyramid" | "hpyramid" | "line" | "brush" | "br" | "cone" => ("h", "h"),
        "outset" | "inset" => ("hv", "hv"),
        "schem" | "schematic" => ("fdnp", "fdnp"),
        "distr" => ("cdpm", "dpm"),
        "chunk" => ("sc", "sc"),
        "size" => ("c", "c"),
        "wand" => ("n", ""),
        "drain" => ("w", "w"),
        "snow" => ("s", "s"),
        "green" => ("f", "f"),
        "searchitem" => ("bip", "bip"),
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
        "line" => "<pattern> [thickness]".into(),
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
        "chunk" => "[coordinates]".into(),
        "brush" | "br" => "<sphere|cylinder|smooth|none> ...".into(),
        "brush sphere" => "<pattern> [radius]".into(),
        "brush cylinder" => "<pattern> [radius] [height]".into(),
        "fill" | "fillr" => "<pattern> <radius> [depth]".into(),
        "drain" => "<radius>".into(),
        "fixwater" | "fixlava" => "<radius>".into(),
        "snow" | "thaw" | "green" => "[size] [height]".into(),
        "extinguish" => "[radius]".into(),
        "limit" => "[limit]".into(),
        "gmask" => "[mask]".into(),
        "searchitem" => "<query>".into(),
        "calc" => "<expression>".into(),
        "help" => "[command...]".into(),
        "tool" => "<none|selwand|info|repl|cycler|floodfill> ...".into(),
        "tool repl" => "<pattern>".into(),
        "tool floodfill" => "<pattern> <range>".into(),
        "schem load" | "schem save" | "schem delete" => "<filename>".into(),
        "schem" | "schematic" => "<load|save|delete|list|formats>".into(),
        _ => COMMANDS.iter().find(|(n, _)| *n == name).map(|(_, a)| a.replace(":text", "").replace(":word", "").replace(":integer", "")).unwrap_or_default(),
    };
    let base = name.split(' ').next().unwrap_or(name);
    let sub = name.strip_prefix(base).unwrap_or("");
    // Only the flags this server implements: an accepted but unsupported one is named in its own error.
    let (_, implemented) = flag_spec(base);
    let flags = if implemented.is_empty() { String::new() } else { format!(" [-{implemented}]") };
    format!("Usage: //{base}{sub}{flags} {}", args.trim()).trim_end().to_string()
}

/// Whether flag `c` of command `name` is followed by a value.
fn takes_value(name: &str, c: char) -> bool {
    c == 'm' || (c == 'p' && matches!(name, "distr" | "schem" | "schematic" | "searchitem" | "help"))
}

/// Splits a command's words from its flags. Unknown flags and flags this server does not implement
/// are errors, never silently dropped.
fn parse_args<'a>(name: &str, rest: &[&'a str]) -> Result<Args<'a>, String> {
    if name == "calc" {
        // An expression: `-` is a minus there, never a flag.
        return Ok(Args { words: rest.to_vec(), flags: Vec::new(), values: Vec::new() });
    }
    let (accepted, implemented) = flag_spec(name);
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
                if takes_value(name, c) {
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
            Some("selwand") => "worldedit.setwand",
            Some("info") => "worldedit.tool.info",
            Some("repl") => "worldedit.tool.replacer",
            Some("cycler") => "worldedit.tool.data-cycler",
            Some("floodfill" | "flood") => "worldedit.tool.flood-fill",
            _ => return None,
        },
        "none" => return None,
        "sel" | "desel" | "deselect" => "worldedit.selection.sel",
        "version" | "ver" => return None,
        "help" => "worldedit.help",
        "wand" => "worldedit.wand",
        "pos1" | "pos2" => "worldedit.selection.pos",
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
        "replace" | "re" | "rep" => "worldedit.region.replace",
        "overlay" => "worldedit.region.overlay",
        "center" | "middle" => "worldedit.region.center",
        "naturalize" => "worldedit.region.naturalize",
        "walls" => "worldedit.region.walls",
        "faces" | "outline" => "worldedit.region.faces",
        "smooth" => "worldedit.region.smooth",
        "move" => "worldedit.region.move",
        "stack" => "worldedit.region.stack",
        "hollow" => "worldedit.region.hollow",
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
        "searchitem" => "worldedit.searchitem",
        "calc" => "worldedit.calc",
        "mask" => "worldedit.brush.options.mask",
        "material" => "worldedit.brush.options.material",
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
        let level = nodes.iter().map(|n| self.config.node_levels.get(*n).copied().unwrap_or(self.config.permission_level)).min().unwrap_or(self.config.permission_level);
        if actor.permission >= level || nodes.iter().any(|n| actor.granted.contains(*n)) {
            return Ok(());
        }
        Err("You are not permitted to do that. Are you in the right mode?".into())
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

    /// A typed command (`set stone`, `//set stone`, `rotate 90`).
    pub fn command(&mut self, world: &mut dyn World, actor: &Actor, line: &str, env: &Env) -> Reply {
        self.now_ms = env.now_ms;
        let line = line.trim().trim_start_matches('/');
        let mut words: Vec<&str> = line.split_whitespace().collect();
        // `//load <file>` is `//schem load <file>`.
        if words.first().is_some_and(|w| w.eq_ignore_ascii_case("load")) {
            words.insert(0, "schem");
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
            other => other.to_owned(),
        };
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
        self.names.insert(actor.name.to_ascii_lowercase(), actor.uuid);
        let writes = !matches!(
            name.as_str(),
            "we" | "worldedit" | "wand" | "pos1" | "pos2" | "hpos1" | "hpos2" | "sel" | "desel" | "size" | "expand" | "contract" | "shift" | "outset" | "inset" | "chunk" | "count" | "distr" | "copy" | "rot" | "flip" | "clearclipboard" | "clearhistory" | "schem" | "schematic" | "brush" | "br" | "mask" | "material" | "none" | "tool" | "help" | "limit" | "gmask" | "toggleplace" | "searchitem" | "calc"
        );
        if writes && self.queue.busy(actor.uuid) {
            return Reply::err("Your last edit is still being written; wait a moment".into());
        }
        if name == "wand" {
            return self.wand(actor);
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
            "searchitem" => self.search_item(world, &args),
            "calc" => self.calc(&args),
            "pos1" | "pos2" => self.pos(world, actor, name == "pos1", &args),
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
            "line" => self.line(world, actor, &args),
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
            "brush" | "br" => self.brush(world, actor, &args),
            "mask" => self.mask(world, actor, &args),
            "material" => self.material(world, actor, &args),
            "none" => self.unbind(actor, false),
            "tool" => self.tool(world, actor, &args),
            _ => Err(format!("The command '{name}' could not be found.")),
        };
        match result {
            Ok(lines) => Reply::ok(lines),
            Err(e) => Reply::err(e),
        }
    }

    // ---- writing ----

    /// Writes `writes` now, or the first tick's worth now and the rest over the next ticks; the
    /// note for the report.
    fn write(&mut self, world: &mut dyn World, actor: &Actor, writes: Vec<(Pos, StateId)>) -> String {
        let budget = self.config.blocks_per_tick.max(1);
        if writes.len() <= budget {
            world.write(&writes);
            return String::new();
        }
        world.write(&writes[..budget]);
        let rest = writes[budget..].to_vec();
        let ticks = rest.len().div_ceil(budget);
        self.queue.push(actor.uuid, &actor.dimension, rest);
        format!(" (finishing over the next {ticks} tick{})", if ticks == 1 { "" } else { "s" })
    }

    /// Plans `ops`, checks the limit, writes it and remembers it for `//undo`: how many blocks
    /// changed, and the note on the ticks still to come.
    fn commit(&mut self, world: &mut dyn World, actor: &Actor, mut ops: Vec<(Pos, StateId)>) -> Result<(usize, String), String> {
        // `//gmask`: only blocks the player's global mask accepts are changed.
        if let Some(text) = self.sessions.get(actor.uuid).global_mask.clone() {
            let mask = Mask::parse(world, &text, None)?;
            let w: &dyn World = &*world;
            ops.retain(|&(p, _)| w.get(p).is_some_and(|s| mask.hits(w, s)));
        }
        let mut change = plan(world, &actor.dimension, ops);
        change.at_ms = self.now_ms;
        if !self.allows(actor, change.len() as u64) {
            return Err(format!("Max blocks changed in an operation reached ({}).", self.max_blocks(actor)));
        }
        let n = change.len();
        let note = self.write(world, actor, change.forward());
        let size = self.config.history_size;
        self.sessions.get(actor.uuid).remember(change, size);
        Ok((n, note))
    }

    /// The most blocks one operation of this player may change: their `//limit`, else the server's.
    fn max_blocks(&mut self, actor: &Actor) -> i64 {
        self.sessions.get(actor.uuid).block_limit.unwrap_or(self.config.max_blocks)
    }

    fn allows(&mut self, actor: &Actor, blocks: u64) -> bool {
        let limit = self.max_blocks(actor);
        limit < 0 || blocks <= limit as u64
    }

    /// Where shapes, fills and the clipboard's origin are made: the block the player stands in, or
    /// position 1 after `//toggleplace`.
    fn placement(&mut self, actor: &Actor) -> Result<Pos, String> {
        let s = self.sessions.get(actor.uuid);
        if s.place_at_pos1 {
            return s.pos1.ok_or_else(|| "Make a region selection first.".to_owned());
        }
        Ok(actor.block())
    }

    /// The writes still queued for `dimension`, this tick's share; the players whose edits finished.
    pub fn tick(&mut self, world: &mut dyn World, dimension: &str) -> Vec<u128> {
        if self.queue.is_empty() {
            return Vec::new();
        }
        let (writes, done) = self.queue.take(dimension, self.config.blocks_per_tick.max(1));
        if !writes.is_empty() {
            world.write(&writes);
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
        if actor.permission < 4 && !self.config.disallowed_blocks.is_empty() {
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

    fn wand(&mut self, actor: &Actor) -> Reply {
        if let Err(e) = self.allowed(actor) {
            return Reply::err(e);
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
            s.dimension = actor.dimension.clone();
        }
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
        match args.get(0).map(str::to_ascii_lowercase).as_deref() {
            None => Ok(clear(self)),
            Some("cuboid") => {
                self.sessions.get(actor.uuid).extend = false;
                Ok(vec!["Cuboid: left click for point 1, right click for point 2".into()])
            }
            Some("extend") => {
                let s = self.sessions.get(actor.uuid);
                if let (Some(a), Some(b)) = (s.pos1, s.pos2) {
                    let c = Cuboid::new(a, b);
                    s.pos1 = Some(c.min);
                    s.pos2 = Some(c.max);
                }
                s.extend = true;
                Ok(vec!["Cuboid: left click for a starting point, right click to extend".into()])
            }
            Some(other) => Err(format!("Only the cuboid and extend selectors are supported here, not `{other}`")),
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
        if name == "expand" && args.get(0) == Some("vert") {
            let next = Cuboid { min: [c.min[0], lo, c.min[2]], max: [c.max[0], hi, c.max[2]] };
            self.sessions.get(actor.uuid).set_selection(next, &actor.dimension);
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
        let next = match name {
            "expand" => c.expand(dir, amount).and_then(|c| c.expand(dir.opposite(), reverse)),
            "contract" => c.contract(dir, amount).and_then(|c| c.contract(dir.opposite(), reverse)),
            _ => Some(c.shifted(crate::region::scale(dir.vector(), amount))),
        };
        let next = fit(next.ok_or("The selection would be empty")?);
        self.sessions.get(actor.uuid).set_selection(next, &actor.dimension);
        Ok(vec![match name {
            "expand" => format!("Region expanded {} blocks", next.volume() as i64 - c.volume() as i64),
            "contract" => format!("Region contracted {} blocks.", c.volume() as i64 - next.volume() as i64),
            _ => "Region shifted.".into(),
        }])
    }

    fn outset(&mut self, world: &dyn World, actor: &Actor, out: bool, args: &Args) -> R {
        let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
        if args.get(0).is_none() {
            return Err(usage(if out { "outset" } else { "inset" }));
        }
        let n = args.int(0, 0)?;
        // WorldEdit: `-h` leaves out up and down, `-v` leaves out the four sides; both leave nothing.
        let (h, v) = (!args.flag('v'), !args.flag('h'));
        let next = c.outset(if out { n } else { -n }, h, v).ok_or("The selection would be empty")?;
        let next = next.clamp_y(world.min_y(), world.max_y()).unwrap_or(next);
        self.sessions.get(actor.uuid).set_selection(next, &actor.dimension);
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
        let n = c.positions().filter(|&p| world.get(p).is_some_and(|s| mask.hits(world, s))).count();
        Ok(vec![format!("Counted: {n}")])
    }

    /// `//distr [-d] [-m <mask>] [-p <page>]`: seven rows a page, most frequent first.
    fn distr(&mut self, world: &dyn World, actor: &Actor, args: &Args) -> R {
        let rows: Vec<(String, u64)> = match args.value('p') {
            Some(_) => self.sessions.get(actor.uuid).distribution.clone().ok_or("No previous distribution.")?,
            None => {
                let c = self.region(world, actor)?;
                let mask = match args.value('m') {
                    Some(m) => Some(Mask::parse(world, m, actor.held.as_deref())?),
                    None => None,
                };
                let mut counts: HashMap<String, u64> = HashMap::new();
                for p in c.positions() {
                    if let Some(s) = world.get(p) {
                        if mask.as_ref().is_some_and(|m| !m.hits(world, s)) {
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
        let ops: Vec<(Pos, StateId)> = match name {
            "set" => c.positions().map(|p| (p, pattern.pick(salt, p))).collect(),
            "walls" => c.positions().filter(|&p| c.is_wall(p)).map(|p| (p, pattern.pick(salt, p))).collect(),
            "faces" | "outline" => c.positions().filter(|&p| c.is_face(p)).map(|p| (p, pattern.pick(salt, p))).collect(),
            _ => c.center().positions().map(|p| (p, pattern.pick(salt, p))).collect(),
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
        let ops: Vec<(Pos, StateId)> = c.positions().filter(|&p| world.get(p).is_some_and(|s| mask.hits(world, s))).map(|p| (p, pattern.pick(salt, p))).collect();
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
            None => Pattern::single(world.resolve(&crate::block::BlockState::air()).unwrap_or(0), "air"),
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
        let id = |n: &str| world.resolve(&crate::block::BlockState::new(n, &[])).ok_or_else(|| format!("Block name '{n}' was not recognized."));
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

    fn line(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let c = self.sessions.get(actor.uuid).selection(&actor.dimension)?;
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
        let points = shapes::line(a, b, thickness, !args.flag('h'));
        let (lo, hi) = (world.min_y(), world.max_y());
        let ops = points.into_iter().filter(|p| (lo..=hi).contains(&p[1])).map(|p| (p, pattern.pick(salt, p))).collect();
        let (n, note) = self.commit(world, actor, ops)?;
        Ok(vec![format!("{n} blocks have been changed.{note}")])
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
        let (_, note) = self.commit(world, actor, ops)?;
        if args.flag('s') {
            self.sessions.get(actor.uuid).set_selection(c.shifted(crate::region::scale(step, count)), &actor.dimension);
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
            None => Pattern::single(world.resolve(&crate::block::BlockState::air()).unwrap_or(0), "air"),
        };
        let mask = match args.value('m') {
            Some(m) => Some(Mask::parse(world, m, actor.held.as_deref())?),
            None => None,
        };
        let by = crate::region::scale(offset, multiplier);
        let salt = self.salt(actor);
        let (ops, moved) = ops::move_region(world, &c, by, &leave, salt, args.flag('a'), mask.as_ref())?;
        let (_, note) = self.commit(world, actor, ops)?;
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
        let ops = positions.into_iter().filter(|p| (lo..=hi).contains(&p[1])).map(|p| (p, pattern.pick(salt, p))).collect();
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
        let air = world.resolve(&crate::block::BlockState::air()).unwrap_or(0);
        let salt = self.salt(actor);
        let mut ops = Vec::new();
        if let Some(cube) = cube {
            if cube.volume() > SCAN_LIMIT {
                return Err(format!("The area is {} blocks; one edit handles at most {SCAN_LIMIT}", cube.volume()));
            }
            for q in cube.positions() {
                if mask.as_ref().is_none_or(|m| world.get(q).is_some_and(|s| m.hits(world, s))) {
                    ops.push((q, pattern.as_ref().map_or(air, |pt| pt.pick(salt, q))));
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
        let mut affected = clip.blocks.len();
        if let Some(text) = args.value('m') {
            let mask = Mask::parse(world, text, actor.held.as_deref())?;
            let air = world.resolve(&crate::block::BlockState::air()).unwrap_or(0);
            let w: &dyn World = &*world;
            affected = 0;
            for (_, s) in clip.blocks.iter_mut() {
                if mask.hits(w, *s) {
                    affected += 1;
                } else {
                    *s = air;
                }
            }
        }
        self.sessions.get(actor.uuid).clipboard = Some(clip);
        let mut note = String::new();
        if cut {
            let leave = match args.get(0) {
                Some(p) => self.pattern(world, actor, Some(p))?,
                None => Pattern::single(world.resolve(&crate::block::BlockState::air()).unwrap_or(0), "air"),
            };
            let salt = self.salt(actor);
            let ops = c.positions().map(|p| (p, leave.pick(salt, p))).collect();
            note = self.commit(world, actor, ops)?.1;
        }
        Ok(vec![format!("{affected} blocks affected{note}"), "0 biomes affected".into(), "0 entities affected".into()])
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
                writes.retain(|(_, s)| mask.hits(w, *s));
            }
            let affected = writes.len();
            let note = self.commit(world, actor, writes)?.1;
            lines.push(format!("The clipboard has been pasted at {}{note}", pt(at)));
            lines.push(format!("{affected} blocks affected"));
            lines.push("0 biomes affected".into());
            lines.push("0 entities affected".into());
        } else {
            lines.push("Selected clipboard paste region.".into());
        }
        if args.flag('s') || args.flag('n') {
            self.sessions.get(actor.uuid).set_selection(pasted, &actor.dimension);
        }
        Ok(lines)
    }

    fn rotate(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let y: f64 = args.get(0).ok_or_else(|| usage("rotate"))?.parse().map_err(|_| number_error(args.get(0).unwrap_or("")))?;
        for i in [1, 2] {
            if args.get(i).and_then(|w| w.parse::<f64>().ok()).is_some_and(|v| v != 0.0) {
                return Err("Only turns around y (up) are supported".into());
            }
        }
        if (y % 90.0).abs() > 0.001 && (90.0 - (y % 90.0).abs()).abs() > 0.001 {
            return Err("Interpolation is not supported on this server: use a multiple of 90 degrees".into());
        }
        let clip = self.sessions.get(actor.uuid).clipboard.as_mut().ok_or("Your clipboard is empty. Use //copy first.")?;
        clip.rotate(world, (y / 90.0).round() as i32);
        Ok(vec!["The clipboard copy has been rotated.".into()])
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
            let Some(change) = change else { break };
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
            let writes = if undo { change.backward() } else { change.forward() };
            edits += 1;
            note = self.write(world, actor, writes);
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
                list_schematics(dir, page, args.flag('d'), args.flag('n'))
            }
            Some("formats" | "listformats" | "f") => Ok(vec![
                "Available clipboard formats (Name: Lookup names)".into(),
                "sponge_v3: sponge, sponge.3, schem3 (write and read)".into(),
                "sponge_v1, sponge_v2: sponge.1, sponge.2 (read)".into(),
                "litematica: litematic (read)".into(),
                "mce: mcedit, schematic (read)".into(),
                "minecraft_structure: structure, nbt (read)".into(),
            ]),
            Some("load") => {
                let name = args.rest(1);
                if name.is_empty() {
                    return Err(usage("schem load"));
                }
                let path = find_schematic(dir, &name)?;
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
                let clip = Clipboard::from_blocks(blocks, format!("{file} ({})", loaded.format));
                let mut out = vec![format!("{name} loaded. Paste it with //paste")];
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
                Ok(vec![format!("{name} saved{}", if overwrite { " (overwriting previous file)." } else { "." })])
            }
            Some("delete" | "d" | "remove") => {
                let name = args.rest(1);
                if name.is_empty() {
                    return Err(usage("schem delete"));
                }
                let path = find_schematic(dir, &name)?;
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

    fn brush(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
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
            other => return Err(format!("The sub-command '{other}' under 'brush' could not be found.")),
        };
        let (r, h) = match &shape {
            BrushShape::Sphere { radius, .. } | BrushShape::Smooth { radius, .. } => (*radius, 0),
            BrushShape::Cylinder { radius, height, .. } => (*radius, *height),
        };
        if r > self.config.max_brush_radius || f64::from(h) > self.config.max_brush_radius {
            return Err(format!("Maximum brush radius (in configuration): {}", self.config.max_brush_radius as i64));
        }
        let arg_mask = if kind == "smooth" && args.words.len() > 3 { Some(args.rest(3)) } else { None };
        if let Some(text) = &arg_mask {
            Mask::parse(world, text, actor.held.as_deref())?;
        }
        // `String.format("%.0f")` for the sphere, a truncation for the others.
        let msg = match &shape {
            BrushShape::Sphere { radius, .. } => format!("Sphere brush shape equipped ({}).", (radius + 0.0).round() as i64),
            BrushShape::Cylinder { radius, height, .. } => format!("Cylinder brush shape equipped ({} by {height}).", *radius as i64),
            BrushShape::Smooth { radius, iterations } => {
                format!("Smooth brush equipped ({} x {iterations}x using {}).", *radius as i64, if arg_mask.is_some() { "filter" } else { "any block" })
            }
        };
        let s = self.sessions.get(actor.uuid);
        let mask = arg_mask.or_else(|| s.brushes.get(&item).and_then(|b| b.mask.clone()));
        s.tools.remove(&item);
        s.brushes.insert(item.clone(), Brush { shape, pattern, mask });
        Ok(vec![msg, Self::unbind_hint()])
    }

    fn brush_bound(&mut self, actor: &Actor) -> Result<String, String> {
        let item = actor.held.clone().unwrap_or_default();
        if self.sessions.get(actor.uuid).brushes.contains_key(&item) {
            Ok(item)
        } else {
            Err("You have no brush bound to your current item. Try /brush sphere for a basic brush.".into())
        }
    }

    fn mask(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let text = args.rest(0);
        let item = self.brush_bound(actor)?;
        if !text.is_empty() {
            Mask::parse(world, &text, actor.held.as_deref())?;
        }
        let brush = self.sessions.get(actor.uuid).brushes.get_mut(&item).ok_or("You have no brush bound to your current item. Try /brush sphere for a basic brush.")?;
        if text.is_empty() {
            brush.mask = None;
            Ok(vec!["Brush mask disabled.".into()])
        } else {
            brush.mask = Some(text.clone());
            Ok(vec!["Brush mask set.".into()])
        }
    }

    /// `/material <pattern>`: the pattern of the brush in the main hand.
    fn material(&mut self, world: &mut dyn World, actor: &Actor, args: &Args) -> R {
        let item = self.brush_bound(actor)?;
        let pattern = self.need_pattern(world, actor, args, 0, "material")?;
        let brush = self.sessions.get(actor.uuid).brushes.get_mut(&item).ok_or("You have no brush bound to your current item. Try /brush sphere for a basic brush.")?;
        brush.pattern = Some(pattern);
        Ok(vec!["Brush material set.".into()])
    }

    /// `/brush none` and `/tool none`: an unbound item is a message, not an error.
    fn unbind(&mut self, actor: &Actor, tool: bool) -> R {
        let item = actor.held.clone().unwrap_or_default();
        let s = self.sessions.get(actor.uuid);
        let was_brush = s.brushes.remove(&item).is_some();
        let was_tool = s.tools.remove(&item).is_some();
        let bound = was_brush || was_tool || item == self.config.wand_item && tool;
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

    /// Replaces a player's selection with the cuboid between `a` and `b` (WorldGuard's `/rg select`);
    /// their outline is drawn again at the next tick.
    pub fn select(&mut self, player: u128, dimension: &str, a: Pos, b: Pos) {
        let s = self.sessions.get(player);
        s.pos1 = Some(a);
        s.pos2 = Some(b);
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

    /// The navigation wand (`navigation-wand.item`): a right click with that item, in the air or on a block,
    /// teleports the player to the block in sight, to a free spot on it (WorldEdit's NavigationWand). `None` when
    /// the held item is not the navigation wand.
    pub fn nav_use(&mut self, world: &dyn World, actor: &Actor, now_ms: i64) -> Option<Reply> {
        self.now_ms = now_ms;
        if actor.held.as_deref() != Some(self.config.nav_wand_item.as_str()) {
            return None;
        }
        if let Err(e) = self.permitted(actor, Some("worldedit.navigation.jumpto.tool")).and(self.allowed(actor)) {
            return Some(Reply::err(e));
        }
        let Some(target) = ops::raycast(world, actor.eye(), ops::look_vector(actor.yaw, actor.pitch), self.config.nav_wand_max_distance) else {
            return Some(Reply::err("No block in sight!".into()));
        };
        // WorldEdit's findFreePosition: the first spot above the block with two free blocks, feet and head.
        let free = |p: Pos| world.get(p).is_some_and(|s| world.is_air(s));
        let top = world.max_y() - 1;
        let spot = ((target[1] + 1)..=top).find(|&y| free([target[0], y, target[2]]) && free([target[0], y + 1, target[2]]));
        match spot {
            Some(y) => {
                let mut reply = Reply::default();
                reply.teleport = Some([f64::from(target[0]) + 0.5, f64::from(y), f64::from(target[2]) + 0.5]);
                Some(reply)
            }
            None => Some(Reply::err("No free spot above the block in sight!".into())),
        }
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
        let brush = self.sessions.get(actor.uuid).brushes.get(&item)?.clone();
        self.now_ms = now_ms;
        let node = match brush.shape {
            BrushShape::Sphere { .. } => "worldedit.brush.sphere",
            BrushShape::Cylinder { .. } => "worldedit.brush.cylinder",
            BrushShape::Smooth { .. } => "worldedit.brush.smooth",
        };
        if let Err(e) = self.permitted(actor, Some(node)).and(self.allowed(actor)) {
            return Some(Reply::err(e));
        }
        if self.queue.busy(actor.uuid) {
            return Some(Reply::err("Your last edit is still being written; wait a moment".into()));
        }
        let target = match pos {
            Some(p) => p,
            None => match ops::raycast(world, actor.eye(), ops::look_vector(actor.yaw, actor.pitch), self.config.brush_range) {
                Some(p) => p,
                None => return Some(Reply::err("No block in sight!".into())),
            },
        };
        if self.repeated(actor, target, false, now_ms) {
            return Some(Reply::default());
        }
        let mask = match brush.mask.as_deref().map(|m| Mask::parse(world, m, None)) {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => return Some(Reply::err(e)),
            None => None,
        };
        let salt = self.salt(actor);
        let (lo, hi) = (world.min_y(), world.max_y());
        let keep = |p: &Pos, world: &dyn World| (lo..=hi).contains(&p[1]) && mask.as_ref().is_none_or(|m| world.get(*p).is_some_and(|s| m.hits(world, s)));
        let ops: Vec<(Pos, StateId)> = match (&brush.shape, &brush.pattern) {
            (BrushShape::Sphere { radius, hollow }, Some(p)) => shapes::sphere(target, [*radius; 3], *hollow).into_iter().filter(|q| keep(q, world)).map(|q| (q, p.pick(salt, q))).collect(),
            (BrushShape::Cylinder { radius, height, hollow }, Some(p)) => shapes::cylinder(target, [*radius; 2], *height, *hollow).into_iter().filter(|q| keep(q, world)).map(|q| (q, p.pick(salt, q))).collect(),
            (BrushShape::Smooth { radius, iterations }, _) => {
                let r = radius.ceil() as i32;
                let region = Cuboid::new(sub(target, [r, r, r]), [target[0] + r, target[1] + r, target[2] + r]);
                ops::smooth(world, &region, *iterations, mask.as_ref())
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

const EXTENSIONS: [&str; 4] = [".schem", ".litematic", ".schematic", ".nbt"];

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
fn find_schematic(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = safe_name(name)?;
    for ext in std::iter::once("").chain(EXTENSIONS) {
        let path = dir.join(format!("{name}{ext}"));
        // No links out of the folder (WorldEdit's files.allow-symbolic-links: false).
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
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
fn list_schematics(dir: &Path, page: usize, oldest_first: bool, newest_first: bool) -> R {
    if oldest_first && newest_first {
        return Err("Cannot sort by oldest and newest.".into());
    }
    let mut files: Vec<(String, std::time::SystemTime)> = Vec::new();
    let mut walk = vec![dir.to_path_buf()];
    while let Some(d) = walk.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
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

/// The clipboard as a Sponge v3 file.
fn encode(world: &dyn World, clip: &Clipboard, env: &Env) -> Result<Vec<u8>, String> {
    let size = clip.bounds.size();
    let volume = size.iter().map(|&v| v as usize).product::<usize>();
    let air = world.resolve(&crate::block::BlockState::air()).unwrap_or(0);
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
    schem::write_sponge3(size, clip.bounds.min, origin, env.data_version, &palette, &cells, env.now_ms)
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

