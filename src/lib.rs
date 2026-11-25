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

pub mod horizon_worldedit_core;
use horizon_worldedit_core as we;
use mod_api::{
    BlockEdit, BlockMetadata, BlockRun, BlockState as HostState, BlockUpdateFlags, Command, CommandArgumentError,
    CommandArgumentKind, CommandArgumentType, CommandSuggestion, Config,
    ConfigKind, ConfigLoadingEvent, ConfigReloadingEvent, Flow, GameMode, Hand,
    HostBlockStates, HostSide, Hooks, Identifier, InteractionResult, ItemStack, LeftClickBlockEvent, LevelRef,
    LevelTickPostEvent, PlayerArmSwingEvent, PlayerCommandEvent, PlayerCommandsSentEvent, PlayerRef, RegisterCommandsEvent,
    RegisterConfigsEvent, RightClickBlockEvent, ServerHost, ServerStoppedEvent, StateConstraint,
    Suggestion, TabCompleteEvent, Text, RightClickItemEvent, Registrar, RegistrationContext,
};
use mod_api::Tag as HostTag;
use mod_api::{CommandSender, NodeDefault, PayloadHandleError, PermissionGatherNodesEvent, PermissionLevel, PermissionNode, PermissionValueType};
use mod_api::{IPayloadContext, IPayloadHandler, PayloadBytes, PayloadId};
use std::sync::Arc;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const ID: &str = "worldedit";
const CONFIG_FILE: &str = "worldedit.toml";
const ORIGINAL_DIR: &str = "plugins/WorldEdit";

/// Public interface for other mods (WorldGuard's `/rg define`, `redefine`, `claim`, `select`, `-s`):
/// `worldedit::api::{selection, select_cuboid}`. Each call takes the editor's lock for its own
/// duration only and calls nothing else while holding it.
pub mod api {
    use super::we;

    /// The player's cuboid selection: (level id, corner 1, corner 2) in click order. `None` while
    /// a corner is missing. This port has no polygon selections.
    pub fn selection(player: u128) -> Option<(String, [i32; 3], [i32; 3])> {
        we::with(|e| e.selection_of(player))
    }

    /// Replaces the player's selection with the cuboid between `a` and `b` in `dimension`.
    pub fn select_cuboid(player: u128, dimension: &str, a: [i32; 3], b: [i32; 3]) {
        we::with(|e| e.select(player, dimension, a, b));
    }

    /// Whether the engine hands the player-command hook the slash count (`//set x` arrives as `/set x`,
    /// `/set x` as `set x`). WorldGuard asks this before it refuses a double-slash form of its commands.
    pub fn slash_aware() -> bool {
        super::slash_strict()
    }
}

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

// --- tab completion ------------------------------------------------------------------------------
//
// The tree the game sends a player holds this mod's own argument kinds (`worldedit:pattern`...), whose
// words come from here. A block's id list is read once from the game and kept: a completion has no host.

static BLOCK_IDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Reads the game's block ids when they are not known yet.
fn ensure_block_ids(host: &dyn ServerHost) {
    let mut ids = BLOCK_IDS.lock().unwrap_or_else(|e| e.into_inner());
    if !ids.is_empty() {
        return;
    }
    let meta = host.block_states();
    let names: Vec<String> = host.block_keys().into_iter().filter_map(|key| meta.name(key).ok().map(str::to_owned)).collect();
    *ids = we::complete::sorted_ids(names);
}

#[derive(Clone, Copy)]
enum ArgKind {
    Pattern,
    Mask,
    Schematic,
    Command,
}

/// One of this mod's command argument kinds: a word, or the rest of the line.
struct WeArgument {
    kind: ArgKind,
    greedy: bool,
}

impl CommandArgumentType for WeArgument {
    fn kind(&self) -> CommandArgumentKind {
        if self.greedy {
            CommandArgumentKind::Greedy
        } else {
            CommandArgumentKind::Word
        }
    }

    // Never refuses: the editor reads the typed line itself (`player_command`) and words its own errors.
    fn parse(&self, input: &str) -> Result<String, CommandArgumentError> {
        Ok(input.to_owned())
    }

    fn suggest(&self, _sender: Option<&CommandSender>, prefix: &str) -> Vec<CommandSuggestion> {
        let words = match self.kind {
            ArgKind::Pattern => {
                let ids = BLOCK_IDS.lock().unwrap_or_else(|e| e.into_inner());
                we::complete::pattern_words(prefix, &ids)
            }
            ArgKind::Mask => {
                let ids = BLOCK_IDS.lock().unwrap_or_else(|e| e.into_inner());
                we::complete::mask_words(prefix, &ids)
            }
            ArgKind::Schematic => we::complete::schematic_words(&schematics_dir(), prefix),
            ArgKind::Command => we::complete::command_words(prefix),
        };
        words.into_iter().map(CommandSuggestion::new).collect()
    }
}

// --- selection outline (WorldEditCUI) ---------------------------------------------------------------
//
// A client with the WorldEditCUI mod says hello on `worldedit:cui` (`v|<protocol version>`; or runs `/we cui`); from
// then on the editor describes the player's selection to it (`s|<shape>`, points, radii, triangles: see
// `engine/cui.rs`) whenever it changes, checked every level tick. Older protocols get boxes.

const CUI_CHANNEL: &str = "worldedit:cui";

/// Receives the CUI's hello: that player is sent their selection from now on (again, if they say hello again).
struct CuiInbox;

impl IPayloadHandler for CuiInbox {
    fn handle(&self, payload: PayloadBytes, context: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        let player = context.player().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?;
        let uuid = player.uuid();
        let text = String::from_utf8_lossy(&payload.0).into_owned();
        we::with(|e| e.cui_hello(uuid, &text));
        Ok(())
    }
}

/// Sends each CUI player the description of their selection when it changed since the last send.
fn send_outlines(host: &mut dyn ServerHost) {
    let uuids = we::with(|e| e.cui_players());
    for uuid in uuids {
        let Some(player) = player_by_uuid(&*host, uuid) else {
            we::with(|e| e.cui_forget(uuid));
            continue;
        };
        let id = PayloadId(CUI_CHANNEL.to_string());
        for text in we::with(|e| e.cui_update(uuid)) {
            let _ = player.send_payload(&mut *host, &id, PayloadBytes(text.into_bytes()));
        }
    }
}

/// Whether the engine accepts command words that start with `/` (the `//set` forms). It does not yet
/// (`usage_nodes` in the engine's native_server.rs allows only `a-z 0-9 _ -`): flip this when it does, and every
/// `//command` completes through the same argument kinds as the single-slash ones.
const SLASH_ROOTS: bool = true;

// --- slash forms ----------------------------------------------------------------------------------
//
// WorldEdit's `//set` exists only with two slashes, `/we` only with one (`we::form_allows`). The player-command hook
// hears the line without its slashes, and the engine builds so far trimmed ALL of them (`trim_start_matches('/')` in
// lan_actions/commands.rs), so `/set` and `//set` look alike. An engine that trims only one slash delivers `/set x`
// for a typed `//set x` and `set x` for `/set x`: the first line that begins with `/` switches the strict check on
// (`SLASH_AWARE`), and so does the file `config/worldedit_strict_slash` (for a server that must refuse from the first
// command on). Until then every typed form runs, as before.

static SLASH_AWARE: AtomicBool = AtomicBool::new(false);

fn slash_strict() -> bool {
    SLASH_AWARE.load(Ordering::Relaxed) || Path::new("config/worldedit_strict_slash").exists()
}

// --- completions of whole lines ---------------------------------------------------------------------
//
// The engine's own completion walks the registered usages like Brigadier: the catch-all `<args:text>` usage that every
// command has swallows the rest of the line and wins whenever a space follows a typed word (`//expand 5 `), and the
// words of an argument kind are asked only for the word under the cursor. So this mod also answers the `tab_complete`
// hook: it matches the typed words against every usage itself and adds what the engine missed.

/// The usages split into words, built once.
fn usage_words() -> &'static Vec<Vec<String>> {
    static WORDS: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    WORDS.get_or_init(|| we::usages().iter().map(|u| u.split_whitespace().map(str::to_owned).collect()).collect())
}

/// Roots another mod or the game also answers to: never hidden, never completed here.
const SHARED_ROOTS: &[&str] = &["j", "info", "none", "ex", "ext", "l"];

/// The command roots this mod registers (with their leading `/` when typed with two slashes).
fn we_roots() -> &'static BTreeSet<String> {
    static ROOTS: OnceLock<BTreeSet<String>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        usage_words()
            .iter()
            .filter_map(|w| w.first())
            .filter(|r| !SHARED_ROOTS.contains(&r.as_str()))
            .map(|r| r.to_ascii_lowercase())
            .collect()
    })
}

/// A usage word `<name:kind>` or `[name:kind]`: its kind.
fn word_kind(word: &str) -> Option<&str> {
    let inner = word
        .strip_prefix('<')
        .and_then(|w| w.strip_suffix('>'))
        .or_else(|| word.strip_prefix('[').and_then(|w| w.strip_suffix(']')))?;
    inner.split_once(':').map(|(_, kind)| kind)
}

/// Kinds that take the rest of the line.
fn kind_is_greedy(kind: &str) -> bool {
    matches!(kind, "text" | "worldedit:pattern_text" | "worldedit:mask_text" | "worldedit:schematic" | "worldedit:command")
}

/// This mod's own kinds, as the argument that completes them.
fn own_kind(kind: &str) -> Option<ArgKind> {
    match kind {
        "worldedit:pattern" | "worldedit:pattern_text" => Some(ArgKind::Pattern),
        "worldedit:mask" | "worldedit:mask_text" => Some(ArgKind::Mask),
        "worldedit:schematic" => Some(ArgKind::Schematic),
        "worldedit:command" => Some(ArgKind::Command),
        _ => None,
    }
}

/// The typed permission node for one `worldedit.*` name (a Boolean, as the core asks it).
fn node(name: &str) -> PermissionNode {
    PermissionNode::new(name, PermissionValueType::Boolean)
}

/// Whether the player may use any WorldEdit command (holds one of the nodes, or is an operator).
fn may_use(host: &mut dyn ServerHost, uuid: u128) -> bool {
    host.player(uuid).is_some_and(|p| we::NODES.iter().any(|&n| p.permission_bool(&mut *host, &node(n)).unwrap_or(false)))
}

/// The completions for the word being typed at the end of `command` (the line without its first slash): pairs of
/// the byte offset the replacement starts at and the replacement. `None` when the line is not one of ours.
fn complete_line(host: &dyn ServerHost, command: &str) -> Option<Vec<(usize, String)>> {
    let first_space = command.find(' ')?;
    let root = command[..first_space].to_ascii_lowercase();
    if !we_roots().contains(&root) {
        return None;
    }
    let tokens: Vec<&str> = command.split(' ').collect();
    let mut starts = Vec::with_capacity(tokens.len());
    let mut at = 0;
    for t in &tokens {
        starts.push(at);
        at += t.len() + 1;
    }
    let n = tokens.len() - 1;
    ensure_block_ids(host);
    let mut out: Vec<(usize, String)> = Vec::new();
    let push = |out: &mut Vec<(usize, String)>, start: usize, word: String| {
        if !out.iter().any(|(s, w)| *s == start && *w == word) {
            out.push((start, word));
        }
    };
    for usage in usage_words() {
        if !usage[0].eq_ignore_ascii_case(&root) {
            continue;
        }
        for i in 1..=n {
            let Some(word) = usage.get(i) else { break };
            let kind = word_kind(word);
            if let Some(kind) = kind {
                if kind_is_greedy(kind) {
                    // The rest of the line is this argument (a pattern, a mask, a schematic name, a command).
                    if let Some(own) = own_kind(kind) {
                        let argument = WeArgument { kind: own, greedy: true };
                        for s in argument.suggest(None, &command[starts[i]..]) {
                            push(&mut out, starts[i], s.text);
                        }
                    }
                    break;
                }
            }
            if i == n {
                let prefix = tokens[n];
                match kind {
                    None => {
                        if word.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()) {
                            push(&mut out, starts[n], word.clone());
                        }
                    }
                    Some("player") | Some("player_name") => {
                        for p in host.players().server_players() {
                            if let Some(name) = p.name(host) {
                                if name.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()) {
                                    push(&mut out, starts[n], name);
                                }
                            }
                        }
                    }
                    Some(k) => {
                        if let Some(own) = own_kind(k) {
                            for s in (WeArgument { kind: own, greedy: false }).suggest(None, prefix) {
                                push(&mut out, starts[n], s.text);
                            }
                        }
                    }
                }
                break;
            }
            // A word typed before the cursor has to fit the usage's word at that place.
            let typed = tokens[i];
            let fits = match kind {
                None => typed.eq_ignore_ascii_case(word),
                Some("integer") => typed.parse::<i64>().is_ok(),
                Some("number") => typed.parse::<f64>().is_ok(),
                Some(_) => true,
            };
            if !fits {
                break;
            }
        }
    }
    Some(out)
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

    fn item_names(&self) -> Vec<String> {
        self.host.registry_entries("minecraft:item").unwrap_or_default().iter().map(|id| id.to_string()).collect()
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

    fn block_entity_at(&self, p: we::Pos) -> Option<we::nbt::Tag> {
        let entity = self.host.block_entity(self.level, p)?;
        let data = entity.data(&*self.host)?;
        let mut tag = from_host_tag(&HostTag::Compound(data));
        if let (we::nbt::Tag::Compound(map), Some(kind)) = (&mut tag, entity.kind(&*self.host)) {
            map.insert("id".into(), we::nbt::Tag::String(kind.id(&*self.host).to_string()));
        }
        Some(tag)
    }

    fn entities_in(&mut self, min: [f64; 3], max: [f64; 3]) -> Vec<we::EntityInfo> {
        let mut out = Vec::new();
        for e in self.level.entities_in(&*self.host, min, max).unwrap_or_default() {
            let (Some(kind), Some(pos)) = (e.kind(&*self.host), e.position(&*self.host)) else { continue };
            let living = self.host.living(e).is_some();
            let nbt = e.nbt(&mut *self.host);
            let has = |key: &str| nbt.as_ref().is_some_and(|c| c.iter().any(|(k, _)| k.to_string_lossy() == key));
            let (named, tamed) = (has("CustomName"), has("Owner") || has("owner"));
            out.push(we::EntityInfo { id: u64::from(e.id()), kind: kind.id(&*self.host).to_string(), pos, living, named, tamed });
        }
        out
    }

    fn name(&self) -> String {
        // The open world's folder (`level-name`): what `/snapshot` looks its backups up by.
        self.host.world_dir().and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "world".to_owned())
    }

    fn entities_data(&mut self, min: [f64; 3], max: [f64; 3]) -> Vec<we::EntityData> {
        let mut out = Vec::new();
        for e in self.level.entities_in(&*self.host, min, max).unwrap_or_default() {
            let (Some(kind), Some(pos)) = (e.kind(&*self.host), e.position(&*self.host)) else { continue };
            let kind = kind.id(&*self.host).to_string();
            // WorldEdit copies no player: a player has no saved state it could make again.
            if kind == "minecraft:player" {
                continue;
            }
            let Some(nbt) = e.nbt(&mut *self.host) else { continue };
            out.push(we::EntityData { id: u64::from(e.id()), kind, pos, nbt: from_host_tag(&HostTag::Compound(nbt)) });
        }
        out
    }

    fn spawn_entity(&mut self, kind: &str, pos: [f64; 3], nbt: &we::nbt::Tag) -> Option<u64> {
        let HostTag::Compound(data) = to_host_tag(nbt) else { return None };
        self.host.summon(&self.dimension, kind, pos, data).map(u64::from)
    }

    fn biome_at(&self, p: we::Pos) -> Option<String> {
        self.level.biome_at(&*self.host, p)
    }

    fn set_biome(&mut self, p: we::Pos, biome: &str) -> bool {
        self.level.set_biome(&mut *self.host, p, biome)
    }

    fn biome_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.host.registry_entries("minecraft:worldgen/biome").unwrap_or_default().iter().map(|id| id.to_string()).collect();
        names.sort();
        names
    }

    fn fill_biome(&mut self, min: we::Pos, max: we::Pos, biome: &str) -> Result<u32, String> {
        use mod_api::BiomeFillRefusal as Refusal;
        // The corners are inclusive here, the box call's maximum is not.
        let end = [max[0] + 1, max[1] + 1, max[2] + 1];
        self.level.fill_biome(&mut *self.host, min, end, biome).map_err(|e| match e {
            Refusal::NotLoaded => "a column of the selection is not loaded, or it leaves the build height".to_owned(),
            Refusal::UnknownBiome => format!("biome '{biome}' is not known to this world"),
            Refusal::NotInPlace => "this level is not the one the server is ticking".to_owned(),
            Refusal::TooManyCells { asked, max } => format!("{asked} biome cells in one call; at most {max}"),
            other => format!("{other:?}"),
        })
    }

    fn level_names(&self) -> Vec<String> {
        self.host.levels().list_levels().iter().map(|l| l.dimension_id().to_string()).collect()
    }

    // `rec-regen` (level.regenerate_chunks) and `rec-world-data` (level.delete_chunks): engine branches not merged yet.
    #[cfg(feature = "next-engine")]
    fn regenerate_chunks(&mut self, chunks: &[[i32; 2]]) -> Result<u32, String> {
        use mod_api::RegenRefusal as Refusal;
        self.level.regenerate_chunks(&mut *self.host, chunks).map_err(|e| match e {
            Refusal::NotLoaded => "a chunk of the selection is not loaded".to_owned(),
            Refusal::NoGenerator => "this level has no generator of the game's own to regenerate from".to_owned(),
            Refusal::NotInPlace => "this level is not the one the server is ticking".to_owned(),
            Refusal::TooManyChunks { asked, max } => format!("{asked} chunks in one call; at most {max}"),
            other => format!("{other:?}"),
        })
    }

    #[cfg(feature = "next-engine")]
    fn delete_chunks(&mut self, chunks: &[[i32; 2]]) -> Result<u32, String> {
        let list: Vec<(i32, i32)> = chunks.iter().map(|c| (c[0], c[1])).collect();
        self.level.delete_chunks(&mut *self.host, &list).map(|n| n as u32).ok_or_else(|| "the level is not the one in place, or the list is too long".to_owned())
    }

    fn generate_tree(&mut self, p: we::Pos, feature: &str) -> bool {
        self.level.generate_tree(&mut *self.host, p, feature).unwrap_or(false)
    }

    fn break_block(&mut self, p: we::Pos, drop: bool) -> bool {
        // `Level.destroyBlock` with no tool in hand: the drops (when asked) are spawned as the game does.
        matches!(self.level.destroy_block(&mut *self.host, p, drop, None, false), Ok(Some(_)))
    }

    fn block_light(&self, p: we::Pos) -> i32 {
        self.level.brightness(&*self.host, mod_api::LightLayer::Block, p).unwrap_or(0)
    }

    fn remove_entities(&mut self, ids: &[u64]) -> usize {
        let mut n = 0;
        for id in ids {
            if let Some(e) = u32::try_from(*id).ok().and_then(|i| self.host.entity(i)) {
                if e.discard(&mut *self.host) {
                    n += 1;
                }
            }
        }
        n
    }

    fn set_block_entities(&mut self, entities: &[(we::Pos, we::nbt::Tag)]) {
        for (p, tag) in entities {
            let Some(entity) = self.host.block_entity(self.level, *p) else { continue };
            let HostTag::Compound(data) = to_host_tag(tag) else { continue };
            // `id` and the position in the data are ignored; a refusal leaves that block entity empty.
            let _ = entity.replace_data(&mut *self.host, data);
        }
    }

    fn upgrade_block_entity(&self, data_version: i32, data: we::nbt::Tag) -> we::nbt::Tag {
        let HostTag::Compound(compound) = to_host_tag(&data) else { return data };
        match self.host.upgrade_block_entity_nbt(data_version, compound) {
            Ok(upgraded) => from_host_tag(&HostTag::Compound(upgraded)),
            Err(_) => data,
        }
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

/// The core's NBT as the game's, for block entity data.
fn to_host_tag(tag: &we::nbt::Tag) -> HostTag {
    use we::nbt::Tag as T;
    match tag {
        T::Byte(v) => HostTag::Byte(*v),
        T::Short(v) => HostTag::Short(*v),
        T::Int(v) => HostTag::Int(*v),
        T::Long(v) => HostTag::Long(*v),
        T::Float(v) => HostTag::Float(v.to_bits()),
        T::Double(v) => HostTag::Double(v.to_bits()),
        T::ByteArray(v) => HostTag::ByteArray(v.iter().map(|&b| b as u8).collect()),
        T::String(v) => HostTag::String(v.as_str().into()),
        T::List(v) => HostTag::List(v.iter().map(to_host_tag).collect()),
        T::Compound(m) => HostTag::Compound(m.iter().map(|(k, v)| (k.as_str().into(), to_host_tag(v))).collect()),
        T::IntArray(v) => HostTag::IntArray(v.clone()),
        T::LongArray(v) => HostTag::LongArray(v.clone()),
    }
}

/// The game's NBT as the core's.
fn from_host_tag(tag: &HostTag) -> we::nbt::Tag {
    use we::nbt::Tag as T;
    match tag {
        HostTag::End => T::Byte(0),
        HostTag::Byte(v) => T::Byte(*v),
        HostTag::Short(v) => T::Short(*v),
        HostTag::Int(v) => T::Int(*v),
        HostTag::Long(v) => T::Long(*v),
        HostTag::Float(v) => T::Float(f32::from_bits(*v)),
        HostTag::Double(v) => T::Double(f64::from_bits(*v)),
        HostTag::ByteArray(v) => T::ByteArray(v.iter().map(|&b| b as i8).collect()),
        HostTag::String(v) => T::String(v.to_string_lossy()),
        HostTag::List(v) => T::List(v.iter().map(from_host_tag).collect()),
        HostTag::Compound(m) => T::Compound(m.iter().map(|(k, v)| (k.to_string_lossy(), from_host_tag(v))).collect()),
        HostTag::IntArray(v) => T::IntArray(v.clone()),
        HostTag::LongArray(v) => T::LongArray(v.clone()),
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
fn actor_of(host: &mut dyn ServerHost, player: PlayerRef) -> we::Actor {
    let entity = player.entity();
    let permission = player.permission_level(&*host).unwrap_or(0);
    // Each worldedit node the player holds: a permission provider's answer (LuckPerms, through the
    // `player_permission_check` hook), else the node's default (operator level 2, declared in
    // `permission_gather_nodes`). Operators at level 4 pass on their level anyway (the core checks it first).
    let granted = we::NODES
        .iter()
        .filter(|&&n| player.permission_bool(&mut *host, &node(n)).unwrap_or(false))
        .map(|n| n.to_string())
        .collect();
    let [yaw, pitch] = entity.rotation(host).unwrap_or([0.0, 0.0]);
    we::Actor {
        uuid: player.uuid(),
        name: player.name(host).unwrap_or_default(),
        permission,
        // `//world` sends the player's commands to another level.
        dimension: we::world_override(player.uuid()).unwrap_or_else(|| entity.level(host).map(|l| l.dimension_id().to_string()).unwrap_or_default()),
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
fn schematics_dir() -> PathBuf {
    let configured = PathBuf::from(we::with(|e| e.config.schematics_dir.clone()));
    if configured.is_absolute() {
        configured
    } else {
        PathBuf::from("config").join(configured)
    }
}

fn env(data_version: i32) -> we::Env {
    let config_dir = PathBuf::from("config");
    let schematics_dir = schematics_dir();
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
        let given = stack.is_some_and(|stack| player.give_stack(host, stack).is_ok());
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

thread_local! {
    /// Teleports a typed command asked for (`//up`, `/jumpto`...): the command hook cannot move a player, the next
    /// level tick does. UUID, feet position, yaw and pitch.
    static TELEPORTS: RefCell<Vec<(u128, [f64; 3], [f32; 2])>> = RefCell::new(Vec::new());
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn permission_gather_nodes(&self, event: &mut PermissionGatherNodesEvent<'_>) {
        // Every node the core asks about, at the operator level the old string nodes fell back to (`permission_level`).
        for &name in we::NODES {
            // The nodes that lift a limit need level 4 (`FULL_OPERATOR_NODES`); every other one level 2.
            let level = if we::FULL_OPERATOR_NODES.contains(&name) { 4 } else { 2 };
            let _ = event.nodes.add(node(name), NodeDefault::Level(PermissionLevel::from_level(level)));
        }
    }

    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for (name, kind, greedy) in [
            ("pattern", ArgKind::Pattern, false),
            ("pattern_text", ArgKind::Pattern, true),
            ("mask", ArgKind::Mask, false),
            ("mask_text", ArgKind::Mask, true),
            ("schematic", ArgKind::Schematic, true),
            ("command", ArgKind::Command, true),
        ] {
            r.register_command_argument(name, WeArgument { kind, greedy });
        }
        // The selection outline for clients with the WorldEditCUI mod: its handshake arrives on this channel.
        let mut handlers = r.payload_handlers();
        let mut registrar = handlers.registrar("1");
        let inbox: Arc<dyn IPayloadHandler> = Arc::new(CuiInbox);
        registrar.play_raw(PayloadId(CUI_CHANNEL.to_string()), Some(inbox));
    }

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

    fn register_commands(&self, host: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        ensure_block_ids(&*host);
        // For completion only: execution happens in `player_command`, before vanilla looks at the name.
        // The engine accepts only [a-z0-9_-] command words and has no `player_name` kind: the `//name` forms
        // (root starting with `/`) are left out (they logged thousands of warnings), `player_name` is a `word`.
        for usage in we::usages() {
            if usage.starts_with('/') && !SLASH_ROOTS {
                continue;
            }
            event.commands.push(Command::new(&usage));
        }
    }

    fn player_command(&self, host: &mut dyn ServerHost, event: &mut PlayerCommandEvent<'_>) -> Flow {
        if event.cancelled {
            return Flow::Continue;
        }
        // `//set x` arrives as `/set x` once the engine keeps the second slash (see `slash_strict`).
        let double = event.command.starts_with('/');
        if double {
            SLASH_AWARE.store(true, Ordering::Relaxed);
        }
        if !we::is_command(event.command) {
            return Flow::Continue;
        }
        // A form this command does not exist in (`/set`, `//we`) is not ours: the engine answers it as unknown.
        if slash_strict() && !we::form_allows(event.command, double) {
            return Flow::Continue;
        }
        event.cancelled = true;
        let Some(player) = player_by_uuid(&*host, event.player) else {
            event.replies.push(Text::literal("WorldEdit cannot see you right now."));
            return Flow::Cancel;
        };
        ensure_block_ids(&*host);
        let actor = actor_of(&mut *host, player);
        let reply = execute(&mut *host, &actor, event.command);
        event.replies.extend(reply.lines.iter().map(line));
        if let Some(to) = reply.teleport {
            TELEPORTS.with(|t| t.borrow_mut().push((actor.uuid, to, [actor.yaw, actor.pitch])));
        }
        // The lines are answered through `replies` above (no player is swapped in here); the items are given now.
        for item in &reply.give {
            host.commands().run_command(&format!("give {} {} 1", actor.name, item));
        }
        Flow::Cancel
    }

    fn player_commands_sent(&self, host: &mut dyn ServerHost, event: &mut PlayerCommandsSentEvent<'_>) {
        // Players who may use no WorldEdit command do not get its roots in their tab list. A player the host cannot
        // resolve yet (still joining) keeps the full list rather than losing it for an operator.
        if host.player(event.player).is_none() || may_use(&mut *host, event.player) {
            return;
        }
        let roots = we_roots();
        event.names.retain(|n| !roots.contains(&n.to_ascii_lowercase()));
    }

    fn tab_complete(&self, host: &mut dyn ServerHost, event: &mut TabCompleteEvent<'_>) {
        let command: &str = event.command;
        let Some(first_space) = command.find(' ') else {
            // A root word alone (`/s`): WorldEdit's roots leave the list of a player who may use none of it.
            if !may_use(&mut *host, event.player) {
                let roots = we_roots();
                event.suggestions.retain(|s| !roots.contains(&s.text.trim_start_matches('/').to_ascii_lowercase()));
            }
            return;
        };
        if !we_roots().contains(&command[..first_space].to_ascii_lowercase()) {
            return;
        }
        // Nothing of WorldEdit (not even a schematic name) for a player who may not use it.
        if !may_use(&mut *host, event.player) {
            event.suggestions.clear();
            return;
        }
        let Some(words) = complete_line(&*host, command) else { return };
        let (from, to) = (event.range[0], event.range[1]);
        for (start, word) in words {
            // The engine's range may begin before the word (it took the rest of the line for one argument).
            let text = if start >= from { format!("{}{}", &command[from..start], word) } else { word };
            if !event.suggestions.iter().any(|s| s.text == text) {
                event.suggestions.push(Suggestion::new((from, to), text));
            }
        }
    }

    fn left_click_block(&self, host: &mut dyn ServerHost, event: &mut LeftClickBlockEvent) -> Flow {
        let Some(player) = acting(&*host) else { return Flow::Continue };
        let actor = actor_of(&mut *host, player);
        let pos = event.pos;
        let reply = {
            let Some(mut world) = GameWorld::new(&mut *host, &actor.dimension) else { return Flow::Continue };
            we::with(|e| match e.wand_click(&world, &actor, true, pos, now_ms()) {
                Some(r) => Some(r),
                None => match e.tool_click(&mut world, &actor, true, pos, now_ms()) {
                    Some(r) => Some(r),
                    // The navigation wand's left click passes through the wall in front of the player.
                    None => e.nav_thru(&world, &actor, false, now_ms()),
                },
            })
        };
        if let Some(reply) = reply {
            finish(host, player, &actor, &reply);
            return Flow::Cancel;
        }
        Flow::Continue
    }

    // A left click in the air sends no click event to the server, only the arm swing every click sends; the swing that
    // follows a click `nav_thru` or `nav_use` already handled is ignored there.
    fn player_arm_swing(&self, host: &mut dyn ServerHost, event: &mut PlayerArmSwingEvent<'_>) {
        if event.hand != "mainhand" {
            return;
        }
        let Some(player) = player_by_uuid(&*host, event.player) else { return };
        // Every swing comes here: only the navigation wand's holder is worth building an actor for.
        let held = held_item(&*host, player);
        if !we::with(|e| e.holds_nav(event.player, held.as_deref()) || e.holds_trace_tool(event.player, held.as_deref())) {
            return;
        }
        let actor = actor_of(&mut *host, player);
        let reply = {
            let Some(mut world) = GameWorld::new(&mut *host, &actor.dimension) else { return };
            // The far wand and the long-range build tool answer a left click in the air; the navigation wand has its own.
            we::with(|e| match e.swing_click(&mut world, &actor, now_ms()) {
                Some(r) => Some(r),
                None => e.nav_thru(&world, &actor, true, now_ms()),
            })
        };
        if let Some(reply) = reply {
            finish(host, player, &actor, &reply);
        }
    }

    fn right_click_block(&self, host: HostSide, event: &mut RightClickBlockEvent) -> Flow {
        let HostSide::Server(host) = host else { return Flow::Continue };
        // Only the main hand counts: the off hand's turn is left to the game.
        if !matches!(event.click.hand, Hand::Main) {
            return Flow::Continue;
        }
        let Some(player) = acting(&*host) else { return Flow::Continue };
        let mut actor = actor_of(&mut *host, player);
        actor.held = event.click.stack.map(|s| s.item().id(&*host)).filter(|i| i != "minecraft:air");
        let pos = event.click.pos;
        // The side that was clicked (`[0, 1, 0]` the top): the stack tool goes that way.
        let face = event.click.hit.face;
        let reply = {
            let Some(mut world) = GameWorld::new(&mut *host, &actor.dimension) else { return Flow::Continue };
            we::with(|e| match e.wand_click(&world, &actor, false, pos, now_ms()) {
                Some(r) => Some(r),
                None => match e.tool_click_face(&mut world, &actor, false, pos, Some(face), now_ms()) {
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
        let mut actor = actor_of(&mut *host, player);
        actor.held = Some(event.click.stack.item().id(&*host));
        let reply = {
            let Some(mut world) = GameWorld::new(&mut *host, &actor.dimension) else { return Flow::Continue };
            we::with(|e| match e.far_wand_air(&mut world, &actor, now_ms()) {
                Some(r) => Some(r),
                None => match e.brush_use(&mut world, &actor, None, now_ms()) {
                    Some(r) => Some(r),
                    None => e.nav_use(&world, &actor, now_ms()),
                },
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
        ensure_block_ids(&*host);
        send_outlines(&mut *host);
        let moves: Vec<_> = TELEPORTS.with(|t| t.borrow_mut().drain(..).collect());
        for (uuid, to, [yaw, pitch]) in moves {
            if let Some(player) = player_by_uuid(&*host, uuid) {
                let _ = player.entity().teleport_to(&mut *host, to, Some([yaw, pitch]));
            }
        }
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
        // TODO(arismoko-api: player_show_block_entity): the outline for clients WITHOUT the WorldEditCUI mod (WorldEdit's
        // `ServerCUIHandler`: a structure block with the selection's box, shown to that player only) needs a block entity
        // shown to one player; `player_show_block` sends a block state without its data. Nothing is drawn for them.
        we::with(|e| e.closed());
    }
}
