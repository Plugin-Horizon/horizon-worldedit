// WorldEdit for a Horizon dedicated server: the game side. Staged into
// `crates/horizon-client/src/game.rs` as `mod worldedit_server`, so it reads the level in place
// (`Game::world`) and the block registry directly; the hook helpers only call in here. Everything
// runs on the server thread, inside the hook that heard the player.
//
// Blind port (written 2026-11-06 against Horizon a589a3dd9 without a build): every Horizon item
// used here is named in GAPS.md / STATUS.md as "to check on the first build".
use super::Game;
use super::native_server::{NativeCommand, NativeServerPlayer};
use super::native_world_edit::{NativeBlockEdit, NativeBlockRun};
use horizon_client::text::{self, Text};
use horizon_world::registry::BlockStateId;
use horizon_client::worldedit_server::core as we;
use horizon_client::worldedit_server::core::World as _;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The config's mod id and file (`config/worldedit.toml`).
pub(super) const CONFIG_ID: &str = "worldedit_server";
pub(super) const CONFIG_FILE: &str = "worldedit.toml";

/// The level in place, as the editor sees it.
struct GameWorld<'a, 'g> {
    game: &'a mut Game<'g>,
    dimension: String,
}

impl we::World for GameWorld<'_, '_> {
    fn get(&self, p: we::Pos) -> Option<we::StateId> {
        let w = &self.game.world;
        if p[1] < w.bottom_y() || p[1] >= w.top_y() || !w.loaded(p[0], p[2]) {
            return None;
        }
        Some(w.block(p[0], p[1], p[2]).0)
    }
    fn min_y(&self) -> i32 {
        self.game.world.bottom_y()
    }
    fn max_y(&self) -> i32 {
        self.game.world.top_y() - 1
    }
    fn describe(&self, state: we::StateId) -> we::BlockState {
        match self.game.pack.assets.registry.state(BlockStateId(state)) {
            Some(s) => we::BlockState { name: s.name.clone(), properties: BTreeMap::from(s.properties.clone()) },
            None => we::BlockState::air(),
        }
    }
    fn resolve(&self, state: &we::BlockState) -> Option<we::StateId> {
        let descriptor = horizon_world::chunk::BlockState { name: state.name.clone(), properties: state.properties.clone().into() };
        self.game.pack.assets.registry.resolve(&descriptor).ok().map(|id| id.0)
    }
    fn is_air(&self, state: we::StateId) -> bool {
        self.game.res.facts().is_air(BlockStateId(state))
    }
    /// Horizon's bulk edit (`native_edit_blocks`: straight into the chunks, a section packed
    /// once, no neighbour updates or drops, light worked out again, chunks resent whole), and
    /// `native_set_block` one by one where the bulk edit refuses.
    fn block_names(&self) -> Vec<String> {
        let registry = &self.game.pack.assets.registry;
        let mut names = std::collections::BTreeSet::new();
        for id in 0..registry.len() {
            if let Some(s) = registry.state(BlockStateId(id as we::StateId)) {
                names.insert(s.name.clone());
            }
        }
        names.into_iter().collect()
    }
    fn property_values(&self, block: &str, property: &str) -> Vec<String> {
        self.game.pack.assets.registry.property_values(block, property).into_iter().map(str::to_owned).collect()
    }
    fn write(&mut self, writes: &[(we::Pos, we::StateId)]) -> usize {
        if writes.is_empty() {
            return 0;
        }
        let (palette, runs) = we::edit::runs(writes);
        let edit = NativeBlockEdit {
            palette: palette.iter().map(|&s| self.describe(s).to_string_full()).collect(),
            filters: Vec::new(),
            runs: runs
                .iter()
                .map(|r| NativeBlockRun { x: r.x, z: r.z, y: r.y, height: r.height, surface: None, block: r.block, only: None })
                .collect(),
            neighbour_updates: false,
        };
        match self.game.native_edit_blocks(&self.dimension, &edit) {
            Ok(n) => n as usize,
            Err(e) => {
                log::warn!("[WorldEdit] bulk edit refused ({e}); writing {} blocks one by one", writes.len());
                use horizon_world::level::flags;
                let f = flags::UPDATE_CLIENTS | flags::UPDATE_KNOWN_SHAPE | flags::UPDATE_SUPPRESS_DROPS;
                writes.iter().filter(|&&(p, s)| self.game.native_set_block(&self.dimension, p, BlockStateId(s), f)).count()
            }
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Where `//schem` keeps its files: `schematics_dir` from the config, inside `config/` when relative.
fn env() -> we::Env {
    let configured = we::with(|e| e.config.schematics_dir.clone());
    let path = PathBuf::from(&configured);
    let dir = if path.is_absolute() {
        path
    } else {
        Game::native_config_dir().map(|c| c.join(&path)).unwrap_or(path)
    };
    let config_file = Game::native_config_dir().map(|c| c.join(CONFIG_FILE)).unwrap_or_else(|_| PathBuf::from(CONFIG_FILE));
    we::Env { schematics_dir: dir, config_file, data_version: horizon_world::DATA_VERSION, now_ms: now_ms() }
}

fn actor(game: &Game<'_>, p: &NativeServerPlayer) -> we::Actor {
    we::Actor {
        uuid: p.uuid,
        name: p.name.clone(),
        permission: p.permission,
        dimension: p.dimension.clone(),
        position: p.position,
        yaw: p.yaw,
        pitch: p.pitch,
        game_mode: p.game_mode,
        held: game.native_held().map(|(item, _, _)| item).filter(|i| i != "minecraft:air"),
    }
}

fn line(l: &we::Line) -> Text {
    Text::literal(l.text.clone()).with_color(if l.error { text::RED } else { text::LIGHT_PURPLE })
}

/// The commands, in every player's command tree from the configured level up.
pub(super) fn register(commands: &mut Vec<NativeCommand>) {
    let level = we::with(|e| e.config.permission_level);
    for usage in we::usages() {
        commands.push(NativeCommand::op(&usage, level));
    }
}

/// The config file was read or changed.
pub(super) fn configure(text: &str) {
    if let Some(problem) = we::configure(text) {
        log::warn!("[WorldEdit] {problem}");
    }
}

/// A player's registered command (`set stone`, the slashes gone), their state swapped in.
pub(super) fn perform(game: &mut Game<'_>, player: &NativeServerPlayer, command: &str, ran: &mut bool) {
    if *ran || !we::is_command(command) {
        return;
    }
    *ran = true;
    let a = actor(game, player);
    let env = env();
    let reply = {
        let mut world = GameWorld { dimension: game.dim.id().to_owned(), game: &mut *game };
        we::with(|e| e.command(&mut world, &a, command, &env))
    };
    for item in &reply.give {
        let stack = horizon_client::items::ItemStack { item: item.as_str().into(), count: 1, damage: 0, extra: None };
        if game.native_give_stack(a.uuid, stack).is_some() {
            game.native_feedback(Text::literal("Your inventory is full: make room for the wand".to_owned()).with_color(text::RED));
        }
    }
    for l in &reply.lines {
        game.native_feedback(line(l));
    }
    outline_later(a.uuid);
}

/// Every command a player types, before anything reads it: `//rotate 90` arrives as `rotate 90`,
/// which vanilla's `/rotate <target> <rotation>` would take (a number first means WorldEdit's), and
/// `//fill stone 5` and `//help set` likewise (`we::is_shadowed`). The player is looked up among the
/// server's, so the command runs from where they stand; the held item is not known here.
pub(super) fn player_command(game: &mut Game<'_>, player: u128, name: &str, command: &str, canceled: &mut bool, replies: &mut Vec<Text>) {
    if *canceled || !we::is_shadowed(command) {
        return;
    }
    *canceled = true;
    let a = match game.native_server_players().into_iter().find(|p| p.uuid == player) {
        Some(p) => we::Actor { held: None, ..actor(game, &p) },
        None => we::Actor { uuid: player, name: name.to_owned(), permission: 4, game_mode: 1, dimension: game.dim.id().to_owned(), ..Default::default() },
    };
    let env = env();
    let reply = {
        let mut world = GameWorld { dimension: a.dimension.clone(), game: &mut *game };
        we::with(|e| e.command(&mut world, &a, command, &env))
    };
    replies.extend(reply.lines.iter().map(line));
    outline_later(a.uuid);
}

fn acting(game: &Game<'_>) -> Option<we::Actor> {
    let p = game.native_acting_player()?;
    Some(actor(game, &p))
}

fn tell(game: &mut Game<'_>, uuid: u128, reply: &we::Reply) {
    for l in &reply.lines {
        game.native_tell(uuid, line(l));
    }
}

/// A left click starting to dig `pos`: the wand's first corner.
pub(super) fn left_click(game: &mut Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    if *canceled {
        return;
    }
    let Some(a) = acting(game) else { return };
    let reply = {
        let mut world = GameWorld { dimension: a.dimension.clone(), game: &mut *game };
        we::with(|e| match e.wand_click(&world, &a, true, pos, now_ms()) {
            Some(r) => Some(r),
            None => e.tool_click(&mut world, &a, true, pos, now_ms()),
        })
    };
    if let Some(r) = reply {
        *canceled = true;
        tell(game, a.uuid, &r);
        outline_later(a.uuid);
    }
}

/// A right click on `pos`: the wand's second corner, the held item's tool or its brush there. `right_click_block`
/// (v1) names no hand, so this may be asked for the off hand's turn too; the held item read here is the
/// main hand's either way, and a wand or brush click ends the click before that turn.
pub(super) fn right_click(game: &mut Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    if *canceled {
        return;
    }
    let Some(a) = acting(game) else { return };
    let reply = {
        let mut world = GameWorld { dimension: a.dimension.clone(), game: &mut *game };
        we::with(|e| match e.wand_click(&world, &a, false, pos, now_ms()) {
            Some(r) => Some(r),
            None => match e.tool_click(&mut world, &a, false, pos, now_ms()) {
                Some(r) => Some(r),
                None => e.brush_use(&mut world, &a, Some(pos), now_ms()),
            },
        })
    };
    if let Some(r) = reply {
        *canceled = true;
        tell(game, a.uuid, &r);
        outline_later(a.uuid);
    }
}

/// A right click in the air: the held item's brush at the block in sight.
pub(super) fn use_item(game: &mut Game<'_>, item: &str, claimed: &mut bool) {
    if *claimed {
        return;
    }
    let Some(mut a) = acting(game) else { return };
    a.held = Some(item.to_owned());
    let reply = {
        let mut world = GameWorld { dimension: a.dimension.clone(), game: &mut *game };
        we::with(|e| e.brush_use(&mut world, &a, None, now_ms()))
    };
    if let Some(r) = reply {
        *claimed = true;
        tell(game, a.uuid, &r);
    }
}

/// After a level's tick: the share of big edits that is this tick's, written into that level.
pub(super) fn level_ticked(game: &mut Game<'_>, dimension: &str) {
    if game.dim.id() != dimension {
        return;
    }
    let done = {
        let mut world = GameWorld { dimension: dimension.to_owned(), game: &mut *game };
        we::with(|e| e.tick(&mut world, dimension))
    };
    for uuid in done {
        game.native_tell(uuid, Text::literal("Your edit is written".to_owned()).with_color(text::LIGHT_PURPLE));
        outline_later(uuid);
    }
    // Selections another mod set (`horizon_client::worldedit_server::core::with(|e| e.select(..))`).
    for uuid in we::with(|e| e.take_outline_due()) {
        outline_later(uuid);
    }
    let due: Vec<u128> = DUE.with(|d| d.borrow_mut().drain().collect());
    if due.is_empty() {
        return;
    }
    for p in game.native_server_players() {
        if !due.contains(&p.uuid) {
            continue;
        }
        if p.dimension == dimension {
            outline(game, p.uuid);
        } else {
            // Shown once they are in the selection's level, at that level's tick.
            outline_later(p.uuid);
        }
    }
}

thread_local! {
    /// Players whose outline is to be drawn again at the next level tick.
    static DUE: std::cell::RefCell<std::collections::HashSet<u128>> = std::cell::RefCell::new(Default::default());
    /// Where each player's outline block is (level, position), to put the real block back.
    static SHOWN: std::cell::RefCell<std::collections::HashMap<u128, (String, [i32; 3])>> = std::cell::RefCell::new(Default::default());
}

/// Draws the player's outline at the next level tick, where the hub is in place (click hooks
/// run inside a guest's packet, where it isn't).
fn outline_later(uuid: u128) {
    DUE.with(|d| d.borrow_mut().insert(uuid));
}

/// WorldEdit's server-side CUI: a structure block only this player sees, under the selection's
/// low corner, in save mode with its bounding box shown round the selection. The block that was
/// there before goes back to them first. A structure block shows at most 48 blocks a side.
fn outline(game: &mut Game<'_>, uuid: u128) {
    use horizon_protocol::packets::play::{self, Clientbound as C};
    use horizon_world::nbt::Tag;
    if let Some((dim, pos)) = SHOWN.with(|s| s.borrow_mut().remove(&uuid)) {
        if dim == game.dim.id() && game.world.loaded(pos[0], pos[2]) {
            let state = game.world.block(pos[0], pos[1], pos[2]).0 as i32;
            game.native_send(uuid, C::BlockUpdate(play::BlockUpdate { pos, state }));
        }
    }
    let Some((dim, c)) = we::with(|e| e.outline_of(uuid)) else { return };
    let size = c.size();
    if dim != game.dim.id() || size.iter().any(|&v| v > 48) {
        return;
    }
    let (bottom, top) = (game.world.bottom_y(), game.world.top_y());
    let pos = if c.min[1] > bottom { [c.min[0], c.min[1] - 1, c.min[2]] } else { [c.min[0], c.max[1] + 1, c.min[2]] };
    if pos[1] >= top || !game.world.loaded(pos[0], pos[2]) {
        return;
    }
    let structure = {
        let world = GameWorld { dimension: dim.clone(), game: &mut *game };
        we::World::resolve(&world, &we::BlockState::new("minecraft:structure_block", &[("mode", "save")]))
    };
    let Some(state) = structure else { return };
    let Some(kind) = game.lan.as_ref().and_then(|h| h.hub.encoder.entity_type(state)) else { return };
    let tag = Tag::compound([
        ("name", Tag::String("".into())),
        ("author", Tag::String("".into())),
        ("metadata", Tag::String("".into())),
        ("mode", Tag::String("SAVE".into())),
        ("posX", Tag::Int(c.min[0] - pos[0])),
        ("posY", Tag::Int(c.min[1] - pos[1])),
        ("posZ", Tag::Int(c.min[2] - pos[2])),
        ("sizeX", Tag::Int(size[0])),
        ("sizeY", Tag::Int(size[1])),
        ("sizeZ", Tag::Int(size[2])),
        ("rotation", Tag::String("NONE".into())),
        ("mirror", Tag::String("NONE".into())),
        ("ignoreEntities", Tag::Byte(1)),
        ("strict", Tag::Byte(0)),
        ("powered", Tag::Byte(0)),
        ("showair", Tag::Byte(0)),
        ("showboundingbox", Tag::Byte(1)),
        ("integrity", Tag::Float(1.0_f32.to_bits())),
        ("seed", Tag::Long(0)),
    ]);
    game.native_send(uuid, C::BlockUpdate(play::BlockUpdate { pos, state: state as i32 }));
    game.native_send(uuid, C::BlockEntityData(play::BlockEntityData { pos, kind, tag }));
    SHOWN.with(|s| s.borrow_mut().insert(uuid, (dim, pos)));
}

/// The world closed: selections, clipboards, history and queued writes go.
pub(super) fn closed() {
    we::with(|e| e.closed());
    DUE.with(|d| d.borrow_mut().clear());
    SHOWN.with(|s| s.borrow_mut().clear());
}
