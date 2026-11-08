// WorldEdit for Horizon, native. `horizon-mixins` stages this file into
// `crates/horizon-client/src/game.rs` as `mod worldedit`, so the editor sees
// `Game`'s private fields and methods.
//
// The editor belongs to the game that owns the world: single player's server
// thread, the machine hosting a LAN world, a dedicated server. It writes with
// `Game::level_set` (the `/setblock` path: neighbour updates, block entities,
// client sync, level events), so no command, packet or block log is involved
// and there is no command length or count limit.
//
// Commands reach it two ways: `run_command` (a command typed on this machine)
// and `guest_command` (a guest of a hosted world; queued unmodified so the
// server runs it with that player's state swapped in). The wand is the wooden
// axe `//wand` hands out: a click on the world with it held sets a position,
// taken in `play_mouse` before mining or item use starts. Any other wooden axe
// is an ordinary axe.
// A client of a server never edits: it sends the command and the server's own
// copy of this module does the work.
//
// One editor state per world (`world_root`), taken out of a thread-local map
// for the duration of a call and put back afterwards, so nothing of the mod is
// borrowed while game code runs and no field is added to `Game`.
use super::Game;
use super::worldedit_schem;
use horizon_client::chat::worldedit_paths;
use horizon_world::registry::BlockStateId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
type Pos = [i32; 3];
/// The most blocks one operation may touch, as WorldEdit's default limit.
const MAX_VOLUME: i64 = 250_000;
/// How many edits `//undo` remembers.
const MAX_HISTORY: usize = 10;
/// The wand: a wooden axe named `WorldEdit Wand`. The name is the mark, since
/// it is the one component that survives the item sync to the client, where
/// the click is handled; an ordinary wooden axe stays an axe.
const WAND_ITEM: &str = "minecraft:wooden_axe";
const WAND_NAME: &str = "WorldEdit Wand";
/// The commands this mod owns. Nothing else is taken from the game.
const COMMANDS: &[&str] = &[
    "we",
    "wand",
    "pos1",
    "pos2",
    "sel",
    "expand",
    "contract",
    "shift",
    "set",
    "replace",
    "walls",
    "outline",
    "hollow",
    "faces",
    "overlay",
    "center",
    "count",
    "distr",
    "sphere",
    "hsphere",
    "cyl",
    "hcyl",
    "pyramid",
    "hpyramid",
    "line",
    "copy",
    "cut",
    "paste",
    "move",
    "undo",
    "redo",
    "clearhistory",
    "schem",
    "load",
];
#[derive(Default)]
struct State {
    pos1: Option<Pos>,
    pos2: Option<Pos>,
    /// Clipboard blocks, relative to the paste origin.
    clipboard: Vec<(Pos, BlockStateId)>,
    /// What is in the clipboard, for `//paste`'s report.
    clipboard_name: String,
    /// Each edit's previous blocks, newest last.
    undo: Vec<Vec<(Pos, BlockStateId)>>,
    redo: Vec<Vec<(Pos, BlockStateId)>>,
    /// Bumped per edit, so a pattern spreads differently each time.
    salt: u64,
}
/// The salt of the next edit: one more than the last, scrambled.
fn next_salt(state: &mut State) -> u64 {
    state.salt = state
        .salt
        .wrapping_mul(0x5851_f42d_4c95_7f2d)
        .wrapping_add(1);
    let mut h = state.salt;
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 29;
    h
}
/// A stable number for `pos` under `salt`, so a block's pick never depends on
/// the order the region is walked in.
fn mix(salt: u64, pos: Pos) -> u64 {
    let mut h = salt ^ 0xcbf2_9ce4_8422_2325;
    for axis in pos {
        h ^= u64::from(axis as u32);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 29)
}
thread_local! {
    static STATES : std::cell::RefCell < BTreeMap < PathBuf, State >> = const {
    std::cell::RefCell::new(BTreeMap::new()) };
}
/// Runs `run` with this world's editor state. The state is taken out for the
/// call and put back afterwards, so game code never runs under the borrow.
fn with_state<T>(game: &mut Game<'_>, run: impl FnOnce(&mut Game<'_>, &mut State) -> T) -> T {
    struct Lease {
        key: PathBuf,
        state: State,
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            STATES.with(|states| {
                states
                    .borrow_mut()
                    .insert(self.key.clone(), std::mem::take(&mut self.state));
            });
        }
    }
    let key = game.world_root.clone();
    let state = STATES.with(|states| states.borrow_mut().remove(&key).unwrap_or_default());
    let mut lease = Lease { key, state };
    run(game, &mut lease.state)
}
fn message(game: &mut Game<'_>, text: impl Into<String>) {
    game.system_message(horizon_client::text::Text::literal(text.into()));
}
fn failure(game: &mut Game<'_>, text: impl Into<String>) {
    game.command_failure(horizon_client::text::Text::literal(text.into()));
}
fn too_big(game: &mut Game<'_>, volume: i64) {
    failure(
        game,
        format!("That region is {volume} blocks; the limit is {MAX_VOLUME}"),
    );
}
/// The acting player's feet: `~` is measured from here.
fn player_pos(game: &Game<'_>) -> Pos {
    let Some(player) = game.player.as_ref() else {
        return [0, 0, 0];
    };
    [
        player.position.x.floor() as i32,
        player.position.y.floor() as i32,
        player.position.z.floor() as i32,
    ]
}
/// `x y z`, each a number or `~` / `~offset`.
fn parse_triple(game: &Game<'_>, args: &[&str]) -> Option<Pos> {
    if args.len() < 3 {
        return None;
    }
    let base = player_pos(game);
    let mut out = base;
    for (i, arg) in args.iter().take(3).enumerate() {
        match arg.strip_prefix('~') {
            Some(rest) if rest.is_empty() => out[i] = base[i],
            Some(rest) => {
                let offset = rest.parse::<f64>().ok()?;
                out[i] = base[i].saturating_add(offset.floor() as i32);
            }
            None => out[i] = arg.parse::<f64>().ok()?.floor() as i32,
        }
    }
    Some(out)
}
/// A block name (`stone`, `minecraft:oak_stairs[facing=east]`) as a state.
fn block_state(game: &Game<'_>, spec: &str) -> Option<BlockStateId> {
    let registry = &game.pack.assets.registry;
    let (name, properties) = match spec.split_once('[') {
        Some((name, rest)) => (name.trim(), rest.strip_suffix(']')?),
        None => (spec.trim(), ""),
    };
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    let default = registry.default_state(&name)?;
    if properties.trim().is_empty() {
        return Some(default);
    }
    let mut state = registry.state(default)?.clone();
    for pair in properties.split(',') {
        let (key, value) = pair.split_once('=')?;
        state
            .properties
            .insert(key.trim().to_owned(), value.trim().to_owned());
    }
    registry.resolve(&state).ok()
}
fn air(game: &Game<'_>) -> BlockStateId {
    block_state(game, "minecraft:air").expect("minecraft:air is in the registry")
}
fn block_name(game: &Game<'_>, state: BlockStateId) -> String {
    game.res.facts().get(state).name.to_string()
}
/// Splits a pattern or mask on the commas outside `[...]` block properties, so
/// `oak_stairs[facing=east,half=top],stone` is two entries.
fn split_entries(spec: &str) -> Vec<String> {
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
/// An entry's weight and its text: `70%stone` is 70 and `stone`, `stone` is 1.
fn weighted(part: &str) -> Option<(u32, &str)> {
    match part.split_once('%') {
        Some((weight, rest)) => Some((weight.trim().parse::<u32>().ok()?.max(1), rest.trim())),
        None => Some((1, part.trim())),
    }
}
/// A block pattern: one block, or a weighted list (`stone,dirt`,
/// `70%stone,30%gravel`) picked at random for each block of a region.
struct Pattern {
    /// The text as typed, for messages.
    text: String,
    /// (weight, block), weight at least 1.
    entries: Vec<(u32, BlockStateId)>,
    total: u32,
}
impl Pattern {
    fn of(block: BlockStateId, text: String) -> Self {
        Self {
            text,
            entries: vec![(1, block)],
            total: 1,
        }
    }
    /// The block for `pos`: the only one, or a weighted pick.
    fn pick(&self, salt: u64, pos: Pos) -> BlockStateId {
        if self.entries.len() == 1 {
            return self.entries[0].1;
        }
        let mut roll = (mix(salt, pos) % u64::from(self.total)) as u32;
        for &(weight, block) in &self.entries {
            if roll < weight {
                return block;
            }
            roll -= weight;
        }
        self.entries[self.entries.len() - 1].1
    }
}
/// A pattern, or None when a name in it is not a block.
fn pattern(game: &Game<'_>, spec: &str) -> Option<Pattern> {
    let mut entries = Vec::new();
    let mut total = 0u32;
    for part in split_entries(spec) {
        let (weight, name) = weighted(&part)?;
        if name.is_empty() {
            return None;
        }
        let block = block_state(game, name)?;
        total += weight;
        entries.push((weight, block));
    }
    if entries.is_empty() {
        return None;
    }
    Some(Pattern {
        text: spec.trim().to_owned(),
        entries,
        total,
    })
}
/// What a report calls a pattern: the block, or the list as typed.
fn pattern_label(game: &Game<'_>, pattern: &Pattern) -> String {
    if pattern.entries.len() == 1 {
        block_name(game, pattern.entries[0].1)
    } else {
        pattern.text.clone()
    }
}
/// Which blocks a mask accepts: any entry of `stone,dirt`, an exact state when
/// properties were given.
struct Mask {
    parts: Vec<MaskPart>,
}
struct MaskPart {
    name: String,
    exact: Option<BlockStateId>,
}
impl Mask {
    fn hits(&self, game: &Game<'_>, state: BlockStateId) -> bool {
        self.parts.iter().any(|part| match part.exact {
            Some(exact) => state == exact,
            None => &*game.res.facts().get(state).name == part.name.as_str(),
        })
    }
}
fn mask(game: &Game<'_>, spec: &str) -> Option<Mask> {
    let mut parts = Vec::new();
    for entry in split_entries(spec) {
        let (_, name) = weighted(&entry)?;
        if name.is_empty() {
            return None;
        }
        let state = block_state(game, name)?;
        parts.push(MaskPart {
            name: block_name(game, state),
            exact: name.contains('[').then_some(state),
        });
    }
    if parts.is_empty() {
        return None;
    }
    Some(Mask { parts })
}
/// The selection's low corner, high corner and volume, clipped to the build
/// height.
fn region(game: &Game<'_>, state: &State) -> Option<(Pos, Pos, i64)> {
    let (a, b) = (state.pos1?, state.pos2?);
    let lo = [
        a[0].min(b[0]),
        a[1].min(b[1]).max(game.world.bottom_y()),
        a[2].min(b[2]),
    ];
    let hi = [
        a[0].max(b[0]),
        a[1].max(b[1]).min(game.world.top_y() - 1),
        a[2].max(b[2]),
    ];
    if hi[1] < lo[1] {
        return None;
    }
    let volume = (i64::from(hi[0] - lo[0]) + 1)
        * (i64::from(hi[1] - lo[1]) + 1)
        * (i64::from(hi[2] - lo[2]) + 1);
    Some((lo, hi, volume))
}
/// The selection, or a message saying there is none and how to make one.
fn selection(game: &mut Game<'_>, state: &State) -> Option<(Pos, Pos, i64)> {
    match region(game, state) {
        Some(region) => Some(region),
        None => {
            failure(
                game,
                "Select a region first: //pos1 and //pos2, or sneak + click with the wand",
            );
            None
        }
    }
}
/// Writes `ops`, remembering what was there; one `//undo` step for the lot.
fn place_ops(game: &mut Game<'_>, state: &mut State, ops: &[(Pos, BlockStateId)]) -> usize {
    let mut history: Vec<(Pos, BlockStateId)> = Vec::new();
    for &(pos, block) in ops {
        let old = game.world.block(pos[0], pos[1], pos[2]);
        if old == block {
            continue;
        }
        if game.level_set(pos, block).is_ok() {
            history.push((pos, old));
        }
    }
    let changed = history.len();
    if changed > 0 {
        state.undo.push(history);
        if state.undo.len() > MAX_HISTORY {
            state.undo.remove(0);
        }
        state.redo.clear();
    }
    changed
}
fn set_positions(
    game: &mut Game<'_>,
    state: &mut State,
    positions: &[Pos],
    pattern: &Pattern,
) -> usize {
    let salt = next_salt(state);
    let ops: Vec<(Pos, BlockStateId)> = positions
        .iter()
        .map(|&pos| (pos, pattern.pick(salt, pos)))
        .collect();
    place_ops(game, state, &ops)
}
fn report_set(game: &mut Game<'_>, name: &str, count: usize) {
    message(
        game,
        format!(
            "{count} block{} set to {name}",
            if count == 1 { "" } else { "s" }
        ),
    );
}
/// Every position of `lo..=hi` whose block `keep` accepts, skipping chunks
/// that are not loaded (an edit never loads one).
fn collect(
    game: &Game<'_>,
    lo: Pos,
    hi: Pos,
    keep: impl Fn(&Game<'_>, BlockStateId) -> bool,
) -> Vec<Pos> {
    let mut out = Vec::new();
    for x in lo[0]..=hi[0] {
        for z in lo[2]..=hi[2] {
            if !game.world.loaded(x, z) {
                continue;
            }
            for y in lo[1]..=hi[1] {
                let old = game.world.block(x, y, z);
                if keep(game, old) {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out
}
/// `up`, `down`, `north`, `south`, `east`, `west` as (axis, positive).
fn direction(word: &str) -> Option<(usize, bool)> {
    Some(match word {
        "up" => (1, true),
        "down" => (1, false),
        "east" => (0, true),
        "west" => (0, false),
        "south" => (2, true),
        "north" => (2, false),
        _ => return None,
    })
}
fn describe(game: &mut Game<'_>, lo: Pos, hi: Pos, volume: i64) {
    message(
        game,
        format!(
            "Region {}, {}, {} to {}, {}, {} ({}x{}x{}, {volume} blocks)",
            lo[0],
            lo[1],
            lo[2],
            hi[0],
            hi[1],
            hi[2],
            hi[0] - lo[0] + 1,
            hi[1] - lo[1] + 1,
            hi[2] - lo[2] + 1,
        ),
    );
}
/// The wooden axe `//wand` gives: named, and that name is what marks it.
fn wand_stack() -> horizon_client::items::ItemStack {
    let mut stack = horizon_client::items::ItemStack::new(WAND_ITEM, 1);
    stack.data_mut().custom_name = Some(horizon_world::nbt::Tag::String(WAND_NAME.into()));
    stack
}
/// Whether `stack` is the wand `//wand` handed out.
fn is_wand(stack: &horizon_client::items::ItemStack) -> bool {
    &*stack.item == WAND_ITEM
        && stack.custom_name().as_deref() == Some(WAND_NAME)
}
/// `//wand`: the wooden axe, into the inventory.
fn give_wand(game: &mut Game<'_>) -> bool {
    match game.give(wand_stack()) {
        None => message(
            game,
            "WorldEdit wand given: left click a block sets position 1, right click sets position 2",
        ),
        Some(_) => failure(game, "Your inventory is full: make room for the wand first"),
    }
    true
}
fn help(game: &mut Game<'_>) {
    for line in [
        "WorldEdit (native). //wand gives a wooden axe: left click sets position 1, right click position 2.",
        "//pos1, //pos2 [x y z] (~ for where you stand)  //sel [clear]",
        "//expand, //contract, //shift <amount> [up|down|north|south|east|west]",
        "//set <block>[,<block>], //replace <from>[,<from>] [to], //walls, //outline, //hollow, //faces, //overlay, //center",
        "Patterns: //set stone,dirt or //set 70%stone,30%gravel picks a block per block, at random.",
        "//sphere, //hsphere, //cyl, //hcyl, //pyramid, //hpyramid <block> <radius> [height]",
        "//line <block> [thickness], //copy, //cut, //paste, //move <amount> <dir>",
        "//count <block>, //distr, //undo, //redo, //clearhistory",
        "//schem opens the schematics menu: pick a file, Load & Paste, or Open Folder and drag one in.",
        "//schem list, //schem load <name>, //schem save <name>, //schem delete <name>, //schem folder.",
        "Schematics read .schem (Sponge), .litematic (Litematica) and .schematic (MCEdit).",
    ] {
        message(game, line);
    }
}
/// A vanilla command name: this mod may not take it.
fn vanilla_owns(name: &str) -> bool {
    let dispatcher = horizon_client::command::vanilla::game_dispatcher();
    dispatcher.nodes[0]
        .children
        .iter()
        .any(|&child| dispatcher.nodes[child].name() == name)
}

/// The command name of `//name ...`, or None when this mod does not own it.
/// The slashes are optional here: the chat input, the console and the guest
/// command packet each drop a different number of them before the command
/// reaches `run_command`, so only the name is matched.
fn parse_command(command: &str) -> Option<(String, Vec<&str>)> {
    let rest = command.trim_start_matches('/');
    let mut words = rest.split_whitespace();
    let name = words.next()?.to_ascii_lowercase();
    if !COMMANDS.contains(&name.as_str()) || vanilla_owns(&name) {
        return None;
    }
    Some((name, words.collect()))
}
/// Whether `command` (as typed, with its slashes) is one of this mod's.
pub(super) fn is_command(command: &str) -> bool {
    parse_command(command).is_some()
}
impl Game<'_> {
    /// A dedicated server's players are served by the server bundle (`worldedit_server`), which keeps a
    /// selection, clipboard and history per player; this window-side editor stays out of the way there,
    /// or both would answer every command from their own selection.
    pub(super) fn worldedit_defers_to_server(&self) -> bool {
        self.options.dedicated.is_some()
    }

    /// A command typed in chat, before the game parses it. `//name` reaches
    /// here as `/name`; anything else is left to the game.
    pub(super) fn worldedit_command(&mut self, command: &str) -> Option<bool> {
        let (name, args) = parse_command(command)?;
        Some(self.worldedit_run(&name, &args))
    }
    fn worldedit_run(&mut self, name: &str, args: &[&str]) -> bool {
        if self.remote.is_some() {
            return false;
        }
        if !matches!(name, "we" | "wand")
            && (!self.may_build() || self.ui.registries.permission < 2)
        {
            failure(self, "WorldEdit needs permission level 2 and build access");
            return false;
        }
        with_state(self, |game, state| match name {
            "we" => {
                help(game);
                true
            }
            "wand" => give_wand(game),
            "pos1" => set_position(game, state, true, args),
            "pos2" => set_position(game, state, false, args),
            "sel" => select(game, state, args),
            "expand" => resize(game, state, args, true),
            "contract" => resize(game, state, args, false),
            "shift" => shift(game, state, args),
            "set" => set(game, state, args),
            "replace" => replace(game, state, args),
            "walls" => shell(game, state, args, Shell::Walls),
            "outline" => shell(game, state, args, Shell::Outline),
            "hollow" => shell(game, state, args, Shell::Hollow),
            "faces" => shell(game, state, args, Shell::Faces),
            "overlay" => overlay(game, state, args),
            "center" => center(game, state, args),
            "count" => count(game, state, args),
            "distr" => distr(game, state),
            "sphere" => sphere(game, state, args, false),
            "hsphere" => sphere(game, state, args, true),
            "cyl" => cylinder(game, state, args, false),
            "hcyl" => cylinder(game, state, args, true),
            "pyramid" => pyramid(game, state, args, false),
            "hpyramid" => pyramid(game, state, args, true),
            "line" => line(game, state, args),
            "copy" => copy(game, state, false),
            "cut" => copy(game, state, true),
            "paste" => paste(game, state),
            "move" => move_selection(game, state, args),
            "undo" => undo(game, state),
            "redo" => redo(game, state),
            "schem" => schem(game, state, args),
            "load" => load(game, state, args),
            _ => {
                state.undo.clear();
                state.redo.clear();
                message(game, "History cleared");
                true
            }
        })
    }
    /// Left/right click with the wand held: the corners. Returns whether the
    /// click was the wand's, in which case the game does nothing else with it
    /// (no mining, no item use).
    pub(super) fn worldedit_wand(
        &mut self,
        button: winit::event::MouseButton,
        pressed: bool,
    ) -> bool {
        if !pressed {
            return false;
        }
        let name = match button {
            winit::event::MouseButton::Left => "pos1",
            winit::event::MouseButton::Right => "pos2",
            _ => return false,
        };
        if !self.holding_wand() {
            return false;
        }
        let Some(hit) = self.pick() else {
            return false;
        };
        self.handle_chat_input(&format!(
            "//{name} {} {} {}",
            hit.pos[0], hit.pos[1], hit.pos[2]
        ));
        true
    }
    /// Whether the held stack is the wand `//wand` gave.
    fn holding_wand(&self) -> bool {
        self.inventory
            .get(self.selected)
            .and_then(|slot| slot.as_ref())
            .is_some_and(is_wand)
    }
}
fn set_position(game: &mut Game<'_>, state: &mut State, first: bool, args: &[&str]) -> bool {
    let pos = if args.is_empty() {
        player_pos(game)
    } else if let Some(pos) = parse_triple(game, args) {
        pos
    } else {
        failure(game, "Usage: //pos1 [x y z] (~ for where you stand)");
        return false;
    };
    if first {
        state.pos1 = Some(pos);
    } else {
        state.pos2 = Some(pos);
    }
    message(
        game,
        format!(
            "Position {} set to {}, {}, {}",
            if first { 1 } else { 2 },
            pos[0],
            pos[1],
            pos[2]
        ),
    );
    if let Some((lo, hi, volume)) = region(game, state) {
        describe(game, lo, hi, volume);
    }
    true
}
fn select(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    if args.first() == Some(&"clear") {
        state.pos1 = None;
        state.pos2 = None;
        message(game, "Selection cleared");
        return true;
    }
    let (Some(a), Some(b)) = (state.pos1, state.pos2) else {
        message(game, "Selection: no corners yet (//pos1 and //pos2)");
        return true;
    };
    match region(game, state) {
        Some((lo, hi, volume)) => describe(game, lo, hi, volume),
        None => message(
            game,
            format!(
                "Selection: {}, {}, {} to {}, {}, {} (outside the build height)",
                a[0], a[1], a[2], b[0], b[1], b[2]
            ),
        ),
    }
    true
}
fn resize(game: &mut Game<'_>, state: &mut State, args: &[&str], grow: bool) -> bool {
    let Some((mut lo, mut hi, _)) = selection(game, state) else {
        return false;
    };
    let amount: i32 = args
        .first()
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(1)
        .abs()
        .max(1);
    let directions: Vec<(usize, bool)> = match args.get(1) {
        Some(word) => match direction(word) {
            Some(d) => vec![d],
            None => {
                failure(
                    game,
                    "Usage: //expand|//contract <amount> [up|down|north|south|east|west]",
                );
                return false;
            }
        },
        None => {
            vec![
                (0, true),
                (0, false),
                (1, true),
                (1, false),
                (2, true),
                (2, false),
            ]
        }
    };
    let delta = if grow { amount } else { -amount };
    for (axis, positive) in directions {
        if positive {
            hi[axis] = hi[axis].saturating_add(delta);
            if hi[axis] < lo[axis] {
                hi[axis] = lo[axis];
            }
        } else {
            lo[axis] = lo[axis].saturating_sub(delta);
            if lo[axis] > hi[axis] {
                lo[axis] = hi[axis];
            }
        }
    }
    lo[1] = lo[1].max(game.world.bottom_y());
    hi[1] = hi[1].min(game.world.top_y() - 1);
    state.pos1 = Some(lo);
    state.pos2 = Some(hi);
    if let Some((lo, hi, volume)) = region(game, state) {
        describe(game, lo, hi, volume);
    }
    true
}
fn shift(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some((lo, hi, _)) = selection(game, state) else {
        return false;
    };
    let amount: i32 = args
        .first()
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(1)
        .abs()
        .max(1);
    let Some(word) = args.get(1) else {
        failure(
            game,
            "Usage: //shift <amount> <up|down|north|south|east|west>",
        );
        return false;
    };
    let Some((axis, positive)) = direction(word) else {
        failure(
            game,
            "Usage: //shift <amount> <up|down|north|south|east|west>",
        );
        return false;
    };
    let delta = if positive { amount } else { -amount };
    let moved = |p: Pos| {
        let mut p = p;
        p[axis] = p[axis].saturating_add(delta);
        p
    };
    state.pos1 = Some(moved(lo));
    state.pos2 = Some(moved(hi));
    if let Some((lo, hi, volume)) = region(game, state) {
        describe(game, lo, hi, volume);
    }
    true
}
fn set(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some((lo, hi, volume)) = selection(game, state) else {
        return false;
    };
    let Some(spec) = args.first() else {
        failure(game, "Usage: //set <block>[,<block>]");
        return false;
    };
    if volume > MAX_VOLUME {
        too_big(game, volume);
        return false;
    }
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let positions = collect(game, lo, hi, |_, _| true);
    let label = pattern_label(game, &pattern);
    let changed = set_positions(game, state, &positions, &pattern);
    report_set(game, &label, changed);
    true
}
fn replace(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some((lo, hi, volume)) = selection(game, state) else {
        return false;
    };
    let Some(from) = args.first() else {
        failure(game, "Usage: //replace <from>[,<from>] [to]");
        return false;
    };
    if volume > MAX_VOLUME {
        too_big(game, volume);
        return false;
    }
    let Some(matches) = mask(game, from) else {
        failure(game, format!("Unknown block {from}"));
        return false;
    };
    let to = match args.get(1) {
        Some(spec) => match pattern(game, spec) {
            Some(pattern) => pattern,
            None => {
                failure(game, format!("Unknown block {spec}"));
                return false;
            }
        },
        None => Pattern::of(air(game), "minecraft:air".to_owned()),
    };
    let positions = collect(game, lo, hi, |game, old| matches.hits(game, old));
    let label = pattern_label(game, &to);
    let changed = set_positions(game, state, &positions, &to);
    report_set(game, &label, changed);
    true
}
enum Shell {
    Walls,
    Outline,
    Hollow,
    Faces,
}
fn shell(game: &mut Game<'_>, state: &mut State, args: &[&str], shape: Shell) -> bool {
    let Some((lo, hi, volume)) = selection(game, state) else {
        return false;
    };
    let Some(spec) = args.first() else {
        failure(
            game,
            "Usage: //walls|//outline|//hollow|//faces <block>[,<block>]",
        );
        return false;
    };
    if volume > MAX_VOLUME {
        too_big(game, volume);
        return false;
    }
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let salt = next_salt(state);
    let air = air(game);
    let mut ops: Vec<(Pos, BlockStateId)> = Vec::new();
    for x in lo[0]..=hi[0] {
        for z in lo[2]..=hi[2] {
            if !game.world.loaded(x, z) {
                continue;
            }
            for y in lo[1]..=hi[1] {
                let pos = [x, y, z];
                let on_x = x == lo[0] || x == hi[0];
                let on_y = y == lo[1] || y == hi[1];
                let on_z = z == lo[2] || z == hi[2];
                let interior = !on_x && !on_y && !on_z;
                let is_shell = match shape {
                    Shell::Walls => on_x || on_z,
                    Shell::Outline | Shell::Faces => on_x || on_y || on_z,
                    Shell::Hollow => !interior,
                };
                if is_shell {
                    ops.push((pos, pattern.pick(salt, pos)));
                } else if matches!(shape, Shell::Hollow) {
                    ops.push((pos, air));
                }
            }
        }
    }
    let label = pattern_label(game, &pattern);
    let changed = place_ops(game, state, &ops);
    report_set(game, &label, changed);
    true
}
fn overlay(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some((lo, hi, volume)) = selection(game, state) else {
        return false;
    };
    let Some(spec) = args.first() else {
        failure(game, "Usage: //overlay <block>[,<block>]");
        return false;
    };
    if volume > MAX_VOLUME {
        too_big(game, volume);
        return false;
    }
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let mut positions = Vec::new();
    for x in lo[0]..=hi[0] {
        for z in lo[2]..=hi[2] {
            if !game.world.loaded(x, z) {
                continue;
            }
            for y in (lo[1]..=hi[1]).rev() {
                if !game.res.facts().is_air(game.world.block(x, y, z)) {
                    positions.push([x, y, z]);
                    break;
                }
            }
        }
    }
    let label = pattern_label(game, &pattern);
    let changed = set_positions(game, state, &positions, &pattern);
    report_set(game, &label, changed);
    true
}
fn center(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some((lo, hi, _)) = selection(game, state) else {
        return false;
    };
    let Some(spec) = args.first() else {
        failure(game, "Usage: //center <block>[,<block>]");
        return false;
    };
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let middle = [
        (lo[0] + hi[0]) / 2,
        (lo[1] + hi[1]) / 2,
        (lo[2] + hi[2]) / 2,
    ];
    let label = pattern_label(game, &pattern);
    let changed = set_positions(game, state, &[middle], &pattern);
    report_set(game, &label, changed);
    true
}
fn count(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some((lo, hi, _)) = selection(game, state) else {
        return false;
    };
    let Some(spec) = args.first() else {
        failure(game, "Usage: //count <block>[,<block>]");
        return false;
    };
    let Some(matches) = mask(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let found = collect(game, lo, hi, |game, old| matches.hits(game, old)).len();
    message(
        game,
        format!(
            "{found} block{} of {spec} in the region",
            if found == 1 { "" } else { "s" }
        ),
    );
    true
}
fn distr(game: &mut Game<'_>, state: &mut State) -> bool {
    let Some((lo, hi, volume)) = selection(game, state) else {
        return false;
    };
    let mut counts: HashMap<String, i64> = HashMap::new();
    let mut total = 0i64;
    for x in lo[0]..=hi[0] {
        for z in lo[2]..=hi[2] {
            if !game.world.loaded(x, z) {
                continue;
            }
            for y in lo[1]..=hi[1] {
                let name = block_name(game, game.world.block(x, y, z));
                *counts.entry(name).or_default() += 1;
                total += 1;
            }
        }
    }
    let mut rows: Vec<(String, i64)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    message(
        game,
        format!("{total} blocks counted of {volume} in the region, by type:"),
    );
    for (name, count) in rows.into_iter().take(10) {
        let percent = if total > 0 {
            (count as f64) * 100.0 / (total as f64)
        } else {
            0.0
        };
        message(game, format!("  {name}: {count} ({percent:.1}%)"));
    }
    true
}
fn sphere(game: &mut Game<'_>, state: &mut State, args: &[&str], mut hollow: bool) -> bool {
    let center = player_pos(game);
    let mut args = args;
    if args.first() == Some(&"-h") {
        hollow = true;
        args = &args[1..];
    }
    let Some(spec) = args.first() else {
        failure(game, "Usage: //sphere [-h] <block>[,<block>] <radius>");
        return false;
    };
    let radius: i32 = args
        .get(1)
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(4)
        .clamp(1, 128);
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let inner = (radius - 1).max(0);
    let mut positions = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let (x, y, z) = (center[0] + dx, center[1] + dy, center[2] + dz);
                if y < game.world.bottom_y() || y > game.world.top_y() - 1 || !game.world.loaded(x, z) {
                    continue;
                }
                let distance = dx * dx + dy * dy + dz * dz;
                if distance > radius * radius || (hollow && distance <= inner * inner) {
                    continue;
                }
                positions.push([x, y, z]);
            }
        }
    }
    if positions.len() as i64 > MAX_VOLUME {
        failure(game, "That sphere is too big");
        return false;
    }
    let label = pattern_label(game, &pattern);
    let changed = set_positions(game, state, &positions, &pattern);
    report_set(game, &label, changed);
    true
}
fn cylinder(game: &mut Game<'_>, state: &mut State, args: &[&str], mut hollow: bool) -> bool {
    let center = player_pos(game);
    let mut args = args;
    if args.first() == Some(&"-h") {
        hollow = true;
        args = &args[1..];
    }
    let Some(spec) = args.first() else {
        failure(
            game,
            "Usage: //cyl [-h] <block>[,<block>] <radius> [height]",
        );
        return false;
    };
    let radius: i32 = args
        .get(1)
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(4)
        .clamp(1, 128);
    let height: i32 = args
        .get(2)
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(1)
        .clamp(1, 256);
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let inner = (radius - 1).max(0);
    let mut positions = Vec::new();
    for y in center[1]..(center[1] + height) {
        if y < game.world.bottom_y() || y > game.world.top_y() - 1 {
            continue;
        }
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let distance = dx * dx + dz * dz;
                if distance > radius * radius || (hollow && distance <= inner * inner) {
                    continue;
                }
                let (x, z) = (center[0] + dx, center[2] + dz);
                if !game.world.loaded(x, z) {
                    continue;
                }
                positions.push([x, y, z]);
            }
        }
    }
    if positions.len() as i64 > MAX_VOLUME {
        failure(game, "That cylinder is too big");
        return false;
    }
    let label = pattern_label(game, &pattern);
    let changed = set_positions(game, state, &positions, &pattern);
    report_set(game, &label, changed);
    true
}
fn pyramid(game: &mut Game<'_>, state: &mut State, args: &[&str], mut hollow: bool) -> bool {
    let base = player_pos(game);
    let mut args = args;
    if args.first() == Some(&"-h") {
        hollow = true;
        args = &args[1..];
    }
    let Some(spec) = args.first() else {
        failure(game, "Usage: //pyramid [-h] <block>[,<block>] <size>");
        return false;
    };
    let size: i32 = args
        .get(1)
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(4)
        .clamp(1, 64);
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let mut positions = Vec::new();
    for layer in 0..size {
        let y = base[1] + layer;
        if y > game.world.top_y() - 1 {
            break;
        }
        let radius = size - 1 - layer;
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if hollow && layer > 0 && dx.abs() < radius && dz.abs() < radius {
                    continue;
                }
                let (x, z) = (base[0] + dx, base[2] + dz);
                if !game.world.loaded(x, z) {
                    continue;
                }
                positions.push([x, y, z]);
            }
        }
    }
    if positions.len() as i64 > MAX_VOLUME {
        failure(game, "That pyramid is too big");
        return false;
    }
    let label = pattern_label(game, &pattern);
    let changed = set_positions(game, state, &positions, &pattern);
    report_set(game, &label, changed);
    true
}
fn line(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some(spec) = args.first() else {
        failure(game, "Usage: //line <block>[,<block>] [thickness]");
        return false;
    };
    let thickness: i32 = args
        .get(1)
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(0)
        .clamp(0, 16);
    let Some(pattern) = pattern(game, spec) else {
        failure(game, format!("Unknown block {spec}"));
        return false;
    };
    let from = player_pos(game);
    let Some(to) = game.pick().map(|hit| hit.pos) else {
        failure(game, "Look at a block within reach to end the line");
        return false;
    };
    let steps = (to[0] - from[0])
        .abs()
        .max((to[1] - from[1]).abs())
        .max((to[2] - from[2]).abs())
        .max(1);
    let mut seen: HashSet<Pos> = HashSet::new();
    let mut positions = Vec::new();
    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        let at = [
            from[0] + (((to[0] - from[0]) as f64) * t) as i32,
            from[1] + (((to[1] - from[1]) as f64) * t) as i32,
            from[2] + (((to[2] - from[2]) as f64) * t) as i32,
        ];
        for dx in -thickness..=thickness {
            for dy in -thickness..=thickness {
                for dz in -thickness..=thickness {
                    let pos = [at[0] + dx, at[1] + dy, at[2] + dz];
                    if pos[1] < game.world.bottom_y() || pos[1] > game.world.top_y() - 1 {
                        continue;
                    }
                    if seen.insert(pos) && game.world.loaded(pos[0], pos[2]) {
                        positions.push(pos);
                    }
                }
            }
        }
    }
    if positions.len() as i64 > MAX_VOLUME {
        failure(game, "That line is too big");
        return false;
    }
    let label = pattern_label(game, &pattern);
    let changed = set_positions(game, state, &positions, &pattern);
    report_set(game, &label, changed);
    true
}
fn copy(game: &mut Game<'_>, state: &mut State, cut: bool) -> bool {
    let Some((lo, hi, volume)) = selection(game, state) else {
        return false;
    };
    if volume > MAX_VOLUME {
        too_big(game, volume);
        return false;
    }
    let mut blocks = Vec::new();
    for x in lo[0]..=hi[0] {
        for z in lo[2]..=hi[2] {
            if !game.world.loaded(x, z) {
                continue;
            }
            for y in lo[1]..=hi[1] {
                blocks.push(([x - lo[0], y - lo[1], z - lo[2]], game.world.block(x, y, z)));
            }
        }
    }
    let copied = blocks.len();
    state.clipboard = blocks;
    state.clipboard_name = "the copied region".to_owned();
    if cut {
        let air = Pattern::of(air(game), "minecraft:air".to_owned());
        let positions = collect(game, lo, hi, |_, _| true);
        let removed = set_positions(game, state, &positions, &air);
        message(game, format!("Cut {removed} blocks"));
    } else {
        message(game, format!("Copied {copied} blocks"));
    }
    true
}
fn paste_at(game: &mut Game<'_>, state: &mut State, at: Pos) -> usize {
    let blocks = std::mem::take(&mut state.clipboard);
    let mut ops = Vec::with_capacity(blocks.len());
    for (offset, block) in &blocks {
        let pos = [at[0] + offset[0], at[1] + offset[1], at[2] + offset[2]];
        if pos[1] < game.world.bottom_y() || pos[1] > game.world.top_y() - 1 {
            continue;
        }
        if !game.world.loaded(pos[0], pos[2]) {
            continue;
        }
        ops.push((pos, *block));
    }
    let changed = place_ops(game, state, &ops);
    state.clipboard = blocks;
    changed
}
fn paste(game: &mut Game<'_>, state: &mut State) -> bool {
    if state.clipboard.is_empty() {
        failure(
            game,
            "The clipboard is empty (//copy, //cut or //schem load first)",
        );
        return false;
    }
    let at = player_pos(game);
    let changed = paste_at(game, state, at);
    if state.clipboard_name.is_empty() {
        message(game, format!("Pasted {changed} blocks"));
    } else {
        message(
            game,
            format!("Pasted {changed} blocks of {}", state.clipboard_name),
        );
    }
    true
}
fn move_selection(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let amount: i32 = args
        .first()
        .and_then(|a| a.parse::<i32>().ok())
        .unwrap_or(1)
        .abs()
        .max(1);
    let Some(word) = args.get(1) else {
        failure(
            game,
            "Usage: //move <amount> <up|down|north|south|east|west>",
        );
        return false;
    };
    let Some((axis, positive)) = direction(word) else {
        failure(
            game,
            "Usage: //move <amount> <up|down|north|south|east|west>",
        );
        return false;
    };
    if !copy(game, state, false) {
        return false;
    }
    let mut at = player_pos(game);
    at[axis] = at[axis].saturating_add(if positive { amount } else { -amount });
    let changed = paste_at(game, state, at);
    message(game, format!("Moved {changed} blocks"));
    true
}
fn undo(game: &mut Game<'_>, state: &mut State) -> bool {
    let Some(edit) = state.undo.pop() else {
        failure(game, "Nothing to undo");
        return false;
    };
    let mut redo = Vec::with_capacity(edit.len());
    for &(pos, old) in &edit {
        redo.push((pos, game.world.block(pos[0], pos[1], pos[2])));
        let _ = game.level_set(pos, old);
    }
    message(game, format!("Undid {} block changes", edit.len()));
    state.redo.push(redo);
    true
}
fn redo(game: &mut Game<'_>, state: &mut State) -> bool {
    let Some(edit) = state.redo.pop() else {
        failure(game, "Nothing to redo");
        return false;
    };
    let mut undo = Vec::with_capacity(edit.len());
    for &(pos, block) in &edit {
        undo.push((pos, game.world.block(pos[0], pos[1], pos[2])));
        let _ = game.level_set(pos, block);
    }
    message(game, format!("Redid {} block changes", edit.len()));
    state.undo.push(undo);
    true
}

/// This world's schematics: `<world>/schematics`, where `//schem save` writes
/// and `//schem load` looks first.
fn schematics_dir(game: &Game<'_>) -> PathBuf {
    // The window of a world on its own server thread has no folder of its own:
    // the world it opened is the one the helpers remember.
    let world = if game.world_root.as_os_str().is_empty() {
        worldedit_paths::remembered().unwrap_or_default()
    } else {
        game.world_root.clone()
    };
    if world.as_os_str().is_empty() {
        return PathBuf::new();
    }
    worldedit_paths::ensure_world(&world)
}
/// The extensions a schematic file can carry, in the order a name is tried.
const SCHEMATIC_EXTENSIONS: [&str; 4] = ["", ".schem", ".litematic", ".schematic"];
/// The file `name` means: a path as typed, else this world's `schematics/`.
fn schematic_path(game: &Game<'_>, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let typed = std::path::Path::new(name);
    let external = typed.is_absolute() || name.contains('/') || name.contains('\\');
    let mut candidates: Vec<PathBuf> = Vec::new();
    if external {
        for extension in SCHEMATIC_EXTENSIONS {
            candidates.push(PathBuf::from(format!("{name}{extension}")));
        }
    }
    let directory = schematics_dir(game);
    for extension in SCHEMATIC_EXTENSIONS {
        candidates.push(directory.join(format!("{name}{extension}")));
    }
    candidates.into_iter().find(|path| path.is_file())
}
/// A block as a schematic names it: `minecraft:oak_stairs[facing=east,half=top]`.
fn block_spec(game: &Game<'_>, state: BlockStateId) -> String {
    let Some(descriptor) = game.pack.assets.registry.state(state) else {
        return "minecraft:air".to_owned();
    };
    if descriptor.properties.is_empty() {
        return descriptor.name.clone();
    }
    let pairs: Vec<String> = descriptor
        .properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    format!("{}[{}]", descriptor.name, pairs.join(","))
}
fn schem(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    match args
        .first()
        .map(|word| word.to_ascii_lowercase())
        .as_deref()
    {
        Some("list") => list_schematics(game),
        Some("save") => save_schematic(game, state, &args[1..]),
        Some("load") => load_schematic(game, state, &args[1..]),
        Some("delete") | Some("remove") => delete_schematic(game, &args[1..]),
        Some("folder") | Some("open") => folder(game),
        Some(_) => load_schematic(game, state, args),
        // A window browses the folder; a console only has the list.
        None => {
            if super::worldedit_menu::open(game) {
                return true;
            }
            list_schematics(game)
        }
    }
}
/// `//schem folder`: the world's schematics folder, in the file manager, so
/// files can be dragged into it.
fn folder(game: &mut Game<'_>) -> bool {
    let dir = schematics_dir(game);
    if dir.as_os_str().is_empty() {
        failure(
            game,
            "No world folder yet: schematics live beside the world",
        );
        return false;
    }
    super::packs::open_folder(&dir);
    message(game, format!("Opened {}", dir.display()));
    true
}

/// `//schem` typed in the chat screen, before the message goes anywhere: a
/// window over its own world opens the menu, and everything else is left to
/// the world's server (`//schem list`, `//schem load <file>`, guests).
pub(super) fn schem_input(game: &mut Game<'_>, message: &str) -> bool {
    match parse_command(message) {
        Some((name, args)) if name == "schem" && args.is_empty() => {
            super::worldedit_menu::open(game)
        }
        _ => false,
    }
}

/// The schematics menu's hooks, from `horizon-mixin.toml`; each returns false
/// when no menu is open, which leaves the pause menu its own draw and input.
pub(super) fn menu_draw(game: &mut Game<'_>, batch: &mut horizon_client::hud::HudBatch) -> bool {
    super::worldedit_menu::draw(game, batch)
}
pub(super) fn menu_click(game: &mut Game<'_>) -> bool {
    super::worldedit_menu::click(game)
}
pub(super) fn menu_key(game: &mut Game<'_>, key: winit::keyboard::KeyCode) -> bool {
    super::worldedit_menu::key(game, key)
}
fn load(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    load_schematic(game, state, args)
}
fn load_schematic(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    if args.is_empty() {
        failure(
            game,
            format!(
                "Usage: //schem load <name>, files in {}",
                schematics_dir(game).display()
            ),
        );
        return false;
    }
    let name = args.join(" ");
    let found = schematic_path(game, &name).or_else(|| schematic_path(game, args[0]));
    let Some(path) = found else {
        failure(
            game,
            format!("No schematic {name} in {}", schematics_dir(game).display()),
        );
        return false;
    };
    let read = {
        let view: &Game<'_> = game;
        worldedit_schem::read(&path, &|spec: &str| block_state(view, spec))
    };
    let loaded = match read {
        Ok(loaded) => loaded,
        Err(error) => {
            failure(game, format!("Could not read {}: {error}", path.display()));
            return false;
        }
    };
    let label = path
        .file_name()
        .map(|file| file.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let (size, count, unknown) = (loaded.size, loaded.blocks.len(), loaded.unknown);
    let format = loaded.format;
    state.clipboard = loaded.blocks;
    state.clipboard_name = format!("{label} ({format})");
    message(
        game,
        format!(
            "Loaded {label} ({format}): {}x{}x{}, {count} blocks; //paste places it at your feet",
            size[0], size[1], size[2]
        ),
    );
    if unknown > 0 {
        message(
            game,
            format!("  {unknown} of its blocks have no match in this game and were left out"),
        );
    }
    true
}
fn save_schematic(game: &mut Game<'_>, state: &mut State, args: &[&str]) -> bool {
    let Some((lo, hi, volume)) = selection(game, state) else {
        return false;
    };
    if volume > MAX_VOLUME {
        too_big(game, volume);
        return false;
    }
    let given = args.join(" ");
    let given = given.trim();
    let name = if given.is_empty() {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        format!("build-{seconds}")
    } else {
        given.to_owned()
    };
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        failure(
            game,
            "A schematic's name cannot hold a path: //schem save <name>",
        );
        return false;
    }
    let file = if name.to_ascii_lowercase().ends_with(".schem") {
        name.clone()
    } else {
        format!("{name}.schem")
    };
    let directory = schematics_dir(game);
    if let Err(error) = std::fs::create_dir_all(&directory) {
        failure(
            game,
            format!("Could not make {}: {error}", directory.display()),
        );
        return false;
    }
    let size = [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1];
    let air = air(game);
    let mut order = Vec::with_capacity(volume as usize);
    for y in lo[1]..=hi[1] {
        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                order.push(if game.world.loaded(x, z) {
                    game.world.block(x, y, z)
                } else {
                    air
                });
            }
        }
    }
    let written = {
        let view: &Game<'_> = game;
        worldedit_schem::write_sponge(size, &order, &|state: BlockStateId| block_spec(view, state))
    };
    let bytes = match written {
        Ok(bytes) => bytes,
        Err(error) => {
            failure(game, format!("Could not write {file}: {error}"));
            return false;
        }
    };
    let path = directory.join(&file);
    if let Err(error) = std::fs::write(&path, &bytes) {
        failure(game, format!("Could not write {}: {error}", path.display()));
        return false;
    }
    message(
        game,
        format!(
            "Saved {}x{}x{} ({} blocks) as {}",
            size[0],
            size[1],
            size[2],
            order.len(),
            path.display()
        ),
    );
    true
}
fn list_schematics(game: &mut Game<'_>) -> bool {
    let directory = schematics_dir(game);
    if directory.as_os_str().is_empty() {
        message(
            game,
            "No world is open: schematics live in the world's own folder",
        );
        return true;
    }
    let entries = worldedit_paths::entries(&directory);
    message(
        game,
        format!(
            "Schematics in {} ({} file{}):",
            directory.display(),
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        ),
    );
    if entries.is_empty() {
        message(
            game,
            "  (none: //schem save <name> makes one, or copy a .schem, .litematic or .schematic there)",
        );
        return true;
    }
    for entry in entries.iter().take(20) {
        message(
            game,
            format!(
                "  {} ({} · {})",
                entry.file,
                entry.format,
                worldedit_paths::human_size(entry.bytes)
            ),
        );
    }
    if entries.len() > 20 {
        message(game, format!("  ... and {} more", entries.len() - 20));
    }
    true
}
fn delete_schematic(game: &mut Game<'_>, args: &[&str]) -> bool {
    if args.is_empty() {
        failure(game, "Usage: //schem delete <name>");
        return false;
    }
    let name = args.join(" ");
    let Some(path) = schematic_path(game, &name) else {
        failure(game, format!("No schematic {name}"));
        return false;
    };
    match std::fs::remove_file(&path) {
        Ok(()) => message(game, format!("Deleted {}", path.display())),
        Err(error) => {
            failure(
                game,
                format!("Could not delete {}: {error}", path.display()),
            );
            return false;
        }
    }
    true
}
