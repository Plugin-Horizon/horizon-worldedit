//! WorldEdit as an arismoko native mod (trait mod on `mod_api`). The editor is the unchanged
//! `horizon-worldedit-core` crate (`core/`); this file is the glue that the Horizon bundle's
//! `glue.rs` and `*.rs` hook helpers used to be, rewritten onto arismoko's hooks and host methods.
//!
//! - Commands: every typed command goes through `player_command` (before vanilla or any mod runs it),
//!   so there are no shadowed-name workarounds. `register_commands` only adds the usages for completion.
//! - Clicks: the wand and tools on `left_click_block` / `right_click_block` (main hand), brushes and the
//!   navigation wand on `use_item` and `right_click_block`.
//! - Edits: `host.edit_blocks` (one bulk call per batch); `level_set_block` when it refuses.
//! - Big edits continue on `level_tick_post`; the editor is closed on `server_stopped`.
//! - Config: `config/worldedit.toml` (Server kind, re-read on save), or `plugins/WorldEdit/config.yml`
//!   when that exists.
//!
//! `TODO(arismoko-api: <name>)` marks behaviour the engine does not offer yet. See PORT_NOTES.md.
use horizon_worldedit_core as we;
use mod_api::{
    BlockEdit, BlockMetadata, BlockRun, BlockState as HostState, BlockUpdateFlags, Command, Config,
    ConfigKind, ConfigLoadingEvent, ConfigReloadingEvent, Flow, GameMode, Hand,
    HostBlockStates, HostSide, Hooks, Identifier, InteractionResult, ItemStack, LeftClickBlockEvent, LevelRef,
    LevelTickPostEvent, PlayerCommandEvent, PlayerRef, RegisterCommandsEvent,
    RegisterConfigsEvent, RightClickBlockEvent, ServerHost, ServerStoppedEvent, StateConstraint,
    Text, RightClickItemEvent,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const ID: &str = "worldedit";
const CONFIG_FILE: &str = "worldedit.toml";
const ORIGINAL_DIR: &str = "plugins/WorldEdit";

#[derive(Default)]
struct Mod;

fn log(line: &str) {
    eprintln!("[WorldEdit] {line}");
}

// --- block states -----------------------------------------------------------------------------
//
// The core numbers block states with `u32` ids that clipboards and history keep for good, so the
// ids are interned for the whole process: a host `BlockState` gets one id the first time it is seen.

#[derive(Default)]
struct States {
    by_id: Vec<HostState>,
    ids: HashMap<HostState, u32>,
    /// The core's name-and-properties form of each id, filled on first use.
    described: HashMap<u32, we::BlockState>,
}

thread_local! {
    static STATES: RefCell<States> = RefCell::new(States::default());
}

fn intern(state: HostState) -> u32 {
    STATES.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(&id) = s.ids.get(&state) {
            return id;
        }
        let id = s.by_id.len() as u32;
        s.by_id.push(state);
        s.ids.insert(state, id);
        id
    })
}

fn host_state(id: u32) -> Option<HostState> {
    STATES.with(|s| s.borrow().by_id.get(id as usize).copied())
}

// --- the level in place ------------------------------------------------------------------------

/// The level the editor edits, as the core's `World`. Built per call from the host, since the host
/// is borrowed for the duration of the edit.
struct GameWorld<'h> {
    host: &'h mut dyn ServerHost,
    level: LevelRef,
    dimension: String,
    min_y: i32,
    height: i32,
}

impl<'h> GameWorld<'h> {
    /// `None` when the level is not loaded on this server.
    fn new(host: &'h mut dyn ServerHost, dimension: &str) -> Option<Self> {
        let level = host.levels().list_levels().into_iter().find(|l| l.dimension_id() == dimension)?;
        let (min_y, height) = level.height(&*host)?;
        Some(Self { host, level, dimension: dimension.to_owned(), min_y, height })
    }
}

impl we::World for GameWorld<'_> {
    fn get(&self, p: we::Pos) -> Option<we::StateId> {
        if p[1] < self.min_y || p[1] >= self.min_y + self.height {
            return None;
        }
        // None for a chunk that is not loaded: an edit never loads one.
        self.level.block_state(&*self.host, p).map(intern)
    }

    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }

    fn describe(&self, state: we::StateId) -> we::BlockState {
        if let Some(d) = STATES.with(|s| s.borrow().described.get(&state).cloned()) {
            return d;
        }
        let Some(host_state) = host_state(state) else { return we::BlockState::air() };
        let meta = self.host.block_states();
        let Ok(name) = meta.name(host_state.block()) else { return we::BlockState::air() };
        let mut properties = BTreeMap::new();
        for property in meta.definition(host_state.block()).unwrap_or_default() {
            if let Ok(Some(value)) = host_state.property(&meta, &property.name) {
                properties.insert(property.name.clone(), value.to_owned());
            }
        }
        let described = we::BlockState { name: name.to_owned(), properties };
        STATES.with(|s| s.borrow_mut().described.insert(state, described.clone()));
        described
    }

    fn resolve(&self, state: &we::BlockState) -> Option<we::StateId> {
        let meta = self.host.block_states();
        let block = self.host.block_state_id(&state.name)?.block();
        let constraints: Vec<StateConstraint> =
            state.properties.iter().map(|(k, v)| StateConstraint::new(k.as_str(), v.as_str())).collect();
        HostState::of(&meta, block, &constraints).ok().map(intern)
    }

    fn block_names(&self) -> Vec<String> {
        let meta = self.host.block_states();
        let names: BTreeSet<String> = self
            .host
            .block_keys()
            .into_iter()
            .filter_map(|key| meta.name(key).ok().map(str::to_owned))
            .collect();
        names.into_iter().collect()
    }

    fn property_values(&self, block: &str, property: &str) -> Vec<String> {
        let meta = self.host.block_states();
        let Some(key) = self.host.block_state_id(block).map(|s| s.block()) else { return Vec::new() };
        meta.definition(key)
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.name == property)
            .map(|p| p.values)
            .unwrap_or_default()
    }

    fn write(&mut self, writes: &[(we::Pos, we::StateId)]) -> usize {
        if writes.is_empty() {
            return 0;
        }
        let (palette, runs) = we::edit::runs(writes);
        // A palette id without a host state would shift every later index: write nothing rather than that.
        let Some(palette) = palette.iter().map(|&s| host_state(s)).collect::<Option<Vec<HostState>>>() else {
            log("write skipped: unknown block state id");
            return 0;
        };
        let edit = BlockEdit {
            palette,
            filters: Vec::new(),
            runs: runs
                .iter()
                .map(|r| BlockRun { x: r.x, z: r.z, y: r.y, height: r.height, surface: None, block: r.block, only: None })
                .collect(),
            ..Default::default()
        };
        match self.host.edit_blocks(&self.dimension, &edit) {
            Ok(n) => n as usize,
            Err(e) => {
                // The bulk edit refuses e.g. a palette entry it cannot validate: write one by one.
                log(&format!("bulk edit refused ({e}); writing {} blocks one by one", writes.len()));
                let flags = BlockUpdateFlags::CLIENTS | BlockUpdateFlags::KNOWN_SHAPE | BlockUpdateFlags::SUPPRESS_DROPS;
                let mut changed = 0;
                for &(p, s) in writes {
                    let Some(state) = host_state(s) else { continue };
                    if let Ok(true) = self.level.set_block(&mut *self.host, p, state, flags) {
                        changed += 1;
                    }
                }
                changed
            }
        }
    }
}

// --- players -----------------------------------------------------------------------------------

fn player_by_uuid(host: &dyn ServerHost, uuid: u128) -> Option<PlayerRef> {
    host.players().server_players().into_iter().find(|p| p.uuid() == uuid)
}

fn acting(host: &dyn ServerHost) -> Option<PlayerRef> {
    host.players().acting_player()
}

/// The item id in the player's selected hotbar slot, `None` for an empty hand.
fn held_item(host: &dyn ServerHost, player: PlayerRef) -> Option<String> {
    let slot = player.selected_slot(host)?;
    let inventory = player.inventory(host)?;
    let id = inventory.get(slot)?.item().id(host).to_string();
    (id != "minecraft:air").then_some(id)
}

fn game_mode_id(mode: Option<GameMode>) -> i32 {
    match mode {
        Some(GameMode::Creative) => 1,
        Some(GameMode::Adventure) => 2,
        Some(GameMode::Spectator) => 3,
        _ => 0,
    }
}

/// The player as the core sees them.
fn actor_of(host: &dyn ServerHost, player: PlayerRef) -> we::Actor {
    let entity = player.entity();
    let permission = player.permission_level(host).unwrap_or(0);
    // Each worldedit node the player holds: a permission provider's answer (LuckPerms, through the
    // `player_permission_check` hook), else operator level 2. Operators at level 4 pass on their level
    // anyway (the core checks it first).
    let granted = we::NODES
        .iter()
        .filter(|&&n| player.has_permission(host, n))
        .map(|n| n.to_string())
        .collect();
    let [yaw, pitch] = entity.rotation(host).unwrap_or([0.0, 0.0]);
    we::Actor {
        uuid: player.uuid(),
        name: player.name(host).unwrap_or_default(),
        permission,
        dimension: entity.level(host).map(|l| l.dimension_id().to_string()).unwrap_or_default(),
        position: entity.position(host).unwrap_or_default(),
        yaw,
        pitch,
        game_mode: game_mode_id(player.game_mode(host)),
        held: held_item(host, player),
        granted,
    }
}

fn line(l: &we::Line) -> Text {
    // NOTE: the bundle coloured errors red and answers purple; the lines are plain here (TextStyle has no
    // Default, so every field has to be set; not an engine gap).
    Text::literal(l.text.clone())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Where `//schem` keeps its files: `schematics_dir` from the config, inside `config/` when relative.
fn env(data_version: i32) -> we::Env {
    let configured = PathBuf::from(we::with(|e| e.config.schematics_dir.clone()));
    let config_dir = PathBuf::from("config");
    let schematics_dir = if configured.is_absolute() { configured } else { config_dir.join(configured) };
    we::Env {
        schematics_dir,
        config_file: config_dir.join(CONFIG_FILE),
        // The game's own DataVersion (`server_data_version`), written into saved schematics.
        data_version,
        now_ms: now_ms(),
    }
}

/// Runs a typed line for `actor` against the level it stands in.
fn execute(host: &mut dyn ServerHost, actor: &we::Actor, command: &str) -> we::Reply {
    let env = env(host.server_data_version());
    let Some(mut world) = GameWorld::new(host, &actor.dimension) else { return we::Reply::default() };
    we::with(|e| e.command(&mut world, actor, command, &env))
}

/// Gives `reply`'s items, tells the player its lines and teleports them if asked.
fn finish(host: &mut dyn ServerHost, player: PlayerRef, actor: &we::Actor, reply: &we::Reply) {
    // The wand as /give gives it (`player_give_stack`: what does not fit drops for that player only).
    for item in &reply.give {
        let id = if item.contains(':') { item.clone() } else { format!("minecraft:{item}") };
        let stack = Identifier::parse(&id).ok().and_then(|id| host.item_key(&id)).map(ItemStack::of);
        let given = stack.is_some_and(|stack| matches!(player.give_stack(host, stack), Ok(true)));
        if !given {
            let _ = player.tell(&mut *host, Text::literal(format!("Could not give you {item}.")));
        }
    }
    for l in &reply.lines {
        let _ = player.tell(&mut *host, line(l));
    }
    if let Some([x, y, z]) = reply.teleport {
        let _ = player.entity().teleport_to(host, [x, y, z], Some([actor.yaw, actor.pitch]));
    }
}

/// The config, as the bundle read it: `plugins/WorldEdit/config.yml` when it exists, else `config/worldedit.toml`.
fn original_present() -> bool {
    Path::new(ORIGINAL_DIR).join("config.yml").is_file()
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register_configs(&self, _: HostSide, event: &mut RegisterConfigsEvent<'_>) {
        if original_present() {
            for problem in we::configure_original(Path::new(ORIGINAL_DIR)) {
                log(&problem);
            }
            return;
        }
        let mut config = Config::new(ID, ConfigKind::Server, we::config::DEFAULT);
        config.file = CONFIG_FILE.to_string();
        event.configs.push(config);
    }

    fn config_loading(&self, _: HostSide, event: &mut ConfigLoadingEvent<'_>) {
        if event.config.mod_id == ID {
            if let Some(problem) = we::configure(&event.config.text) {
                log(&problem);
            }
        }
    }

    fn config_reloading(&self, _: HostSide, event: &mut ConfigReloadingEvent<'_>) {
        if event.config.mod_id == ID {
            if let Some(problem) = we::configure(&event.config.text) {
                log(&problem);
            }
        }
    }

    fn register_commands(&self, _: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        // For completion only: execution happens in `player_command`, before vanilla looks at the name.
        for usage in we::usages() {
            event.commands.push(Command::new(&usage));
        }
    }

    fn player_command(&self, host: &mut dyn ServerHost, event: &mut PlayerCommandEvent<'_>) -> Flow {
        if event.cancelled || !we::is_command(event.command) {
            return Flow::Continue;
        }
        event.cancelled = true;
        let Some(player) = player_by_uuid(&*host, event.player) else {
            event.replies.push(Text::literal("WorldEdit cannot see you right now."));
            return Flow::Cancel;
        };
        let actor = actor_of(&*host, player);
        let reply = execute(&mut *host, &actor, event.command);
        event.replies.extend(reply.lines.iter().map(line));
        // The lines are answered through `replies` above (no player is swapped in here); the items are given now.
        for item in &reply.give {
            host.commands().run_command(&format!("give {} {} 1", actor.name, item));
        }
        Flow::Cancel
    }

    fn left_click_block(&self, host: &mut dyn ServerHost, event: &mut LeftClickBlockEvent) -> Flow {
        let Some(player) = acting(&*host) else { return Flow::Continue };
        let actor = actor_of(&*host, player);
        let pos = event.pos;
        let reply = {
            let Some(mut world) = GameWorld::new(&mut *host, &actor.dimension) else { return Flow::Continue };
            we::with(|e| match e.wand_click(&world, &actor, true, pos, now_ms()) {
                Some(r) => Some(r),
                None => e.tool_click(&mut world, &actor, true, pos, now_ms()),
            })
        };
        if let Some(reply) = reply {
            finish(host, player, &actor, &reply);
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn right_click_block(&self, host: HostSide, event: &mut RightClickBlockEvent) -> Flow {
        let HostSide::Server(host) = host else { return Flow::Continue };
        // Only the main hand counts: the off hand's turn is left to the game.
        if !matches!(event.click.hand, Hand::Main) {
            return Flow::Continue;
        }
        let Some(player) = acting(&*host) else { return Flow::Continue };
        let mut actor = actor_of(&*host, player);
        actor.held = event.click.stack.map(|s| s.item().id(&*host)).filter(|i| i != "minecraft:air");
        let pos = event.click.pos;
        let reply = {
            let Some(mut world) = GameWorld::new(&mut *host, &actor.dimension) else { return Flow::Continue };
            we::with(|e| match e.wand_click(&world, &actor, false, pos, now_ms()) {
                Some(r) => Some(r),
                None => match e.tool_click(&mut world, &actor, false, pos, now_ms()) {
                    Some(r) => Some(r),
                    None => match e.brush_use(&mut world, &actor, Some(pos), now_ms()) {
                        Some(r) => Some(r),
                        None => e.nav_use(&world, &actor, now_ms()),
                    },
                },
            })
        };
        match reply {
            Some(reply) => {
                // Fail ends the click, so the block under a wand or brush is not opened or placed on.
                *event.cancellation_result = InteractionResult::Fail;
                finish(host, player, &actor, &reply);
                Flow::Cancel
            }
            None => Flow::Continue,
        }
    }

    // `use_item` is an item part hook now; the air click with an item is `right_click_item`.
    fn right_click_item(&self, host: HostSide, event: &mut RightClickItemEvent<'_>) -> Flow {
        let HostSide::Server(host) = host else { return Flow::Continue };
        let Some(player) = acting(&*host) else { return Flow::Continue };
        let mut actor = actor_of(&*host, player);
        actor.held = Some(event.click.stack.item().id(&*host));
        let reply = {
            let Some(mut world) = GameWorld::new(&mut *host, &actor.dimension) else { return Flow::Continue };
            we::with(|e| match e.brush_use(&mut world, &actor, None, now_ms()) {
                Some(r) => Some(r),
                None => e.nav_use(&world, &actor, now_ms()),
            })
        };
        if let Some(reply) = reply {
            *event.cancellation_result = InteractionResult::Success;
            finish(host, player, &actor, &reply);
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn level_tick_post(&self, host: HostSide, event: &mut LevelTickPostEvent) {
        let HostSide::Server(host) = host else { return };
        let dimension = event.level.dimension_id().to_string();
        let done = {
            let Some(mut world) = GameWorld::new(&mut *host, &dimension) else { return };
            we::with(|e| e.tick(&mut world, &dimension))
        };
        for uuid in done {
            if let Some(player) = player_by_uuid(&*host, uuid) {
                let _ = player.tell(&mut *host, Text::literal("Your edit is written"));
            }
        }
    }

    fn server_stopped(&self, _: &mut dyn ServerHost, _: &mut ServerStoppedEvent) {
        // Selections, clipboards, history and queued writes go with the world.
        // TODO(arismoko-api: player_show_blocks): the selection outline (WorldEdit's CUI structure block, only the
        // player sees it) is not drawn, and nothing is cleared from it here.
        we::with(|e| e.closed());
    }
}
