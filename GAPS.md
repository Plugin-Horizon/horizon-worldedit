# What Horizon lacks for the server bundle

Written against Horizon `a589a3dd9` (`crates/horizon-client/src/game/native_hooks.rs`, `native_server.rs`,
`native_world_edit.rs`, `lan_actions.rs`). Each line is one item for an upstream PR.

## Commands

- [ ] `NativeCommand::new("/set <pattern:text>")`: allow a leading `/` in a command's first literal (today
  `usage_nodes` takes `[a-z0-9_-]` only) — needed for `//set`, `//copy`... to complete and show as valid in the
  vanilla chat box (WorldEdit registers the literal `/set`, so the client sees `//set`) — today: registered as
  `set`; `//set` still runs because `guest_command` strips every slash, but the client marks it red and
  completes only `/set`. Why nothing can be done on the mod side: the client completes from the tree it is sent
  and asks the server only for `ask_server` argument nodes, never for an unknown root word, so a typed `//` is
  matched against root literals that start with `/` and there are none (`native_server.rs` `usage_nodes` rejects
  `/` with "is not a command word"; a root argument node is refused too: "a command starts with its name").
  `core` test `every_usage_reaches_the_command_tree` replays those rules: all usages register as `/x`. Fix upstream:
  accept `/` in the first literal (and match `native_command_level` on it), or a per-mod catch-all root.
- [ ] `native_player_command(player, name, command, canceled, replies)` with the command as typed, slashes kept
  (or a `slashes: u8` argument) — needed for `//rotate`, which vanilla's `/rotate` shadows, and any future
  `//name` vanilla also has — today: `rotate <number>` is taken as WorldEdit's (vanilla's first argument is an
  entity), `//rot` is registered as an alias.
- [ ] `fn native_has_permission(&self, player: u128, node: &str, default_level: u8) -> bool` plus a provider hook
  (the same item as in the LuckPerms port's GAPS.md) — needed for `worldedit.*` permissions per command, so
  builders need not be operators — today: one operator level (`permission_level`) for everything.

- [ ] `fn native_player_uuid(&self, name: &str) -> Option<u128>` (online or saved players) — needed for `//undo <player>` and `//redo <player>` to reach a player who has not used WorldEdit since the server started — today: only the names WorldEdit has seen since start are known.

## Blocks and entities

- [ ] Block entity data of vanilla blocks: `fn native_block_entity_tag(&self, dimension: &str, pos: [i32; 3]) ->
  Option<Tag>` and either NBT in `NativeBlockEdit.palette` (refused today) or `fn native_set_block_tag(&mut self,
  dimension: &str, pos: [i32; 3], tag: &Tag) -> bool` — needed for copying, pasting and saving chests, signs,
  banners, heads, spawners (Sponge `BlockEntities`) — today: blocks only, their contents are lost and a file's
  `BlockEntities` are skipped.
- [ ] `fn native_entities_in(&self, dimension: &str, min: [f64; 3], max: [f64; 3]) -> Vec<(String, [f64; 3],
  Tag)>` and `fn native_spawn_entity(&mut self, dimension: &str, kind: &str, pos: [f64; 3], nbt: &Tag) ->
  Option<u32>` — needed for `//copy -e`, `//paste -e` and a schematic's `Entities` (armour stands, item frames,
  paintings) — today: not possible.
- [ ] Block state data fixing for mods: `fn native_upgrade_block_state(name: &str, properties: &BTreeMap<String,
  String>, data_version: i32) -> (String, BTreeMap<String, String>)` over `horizon_world::datafix` — needed for
  schematics saved by older versions (renamed blocks such as `grass` to `short_grass`) — today: a name this game
  lacks is left out and counted in the load message.
- [ ] `NativeBlockEdit { neighbour_updates: bool, .. }` (or flags as `native_set_block` takes) — needed for
  WorldEdit's side effects when a builder wants them (`//perf neighbors on`: fences connecting, sand falling) —
  today: bulk edits never update neighbours (WorldEdit's `//fast`).
- [ ] `native_edit_blocks` for a level that isn't the one in place (`dimension` other than `self.dim`) — needed
  for `//undo` of an edit in another level and for writing queued edits from a server-wide tick — today: undo
  asks the player to go there; queued writes wait for that level's own `level_tick_post`.
- [ ] A cost-bounded or background bulk edit (section packing off the server thread) — needed for million-block
  edits without spreading them over many ticks — today: `blocks_per_tick` (200,000) per tick.

## Clicks and players

- [ ] The hand in `native_right_click_block` (`hand: Hand` argument) — needed to answer the main hand only
  (both fire, once per hand) — needs upstream hook `right_click_block_v2` (on the unmerged `hooks-b-all` branch,
  not on main); today: v1 `right_click_block`, which names no hand. The 200 ms repeat filter stays only for doubled
  dig packets and a brush's block click followed by its use in the air.
- [ ] `fn native_pick_block(&self, eye: [f64; 3], dir: [f64; 3], range: f64, fluids: bool) -> Option<([i32; 3],
  [i32; 3])>` with the game's block shapes — needed for `//hpos1`, `//hpos2` and brushes to hit what the player
  sees (slabs, plants, glass panes) — today: a grid walk that stops at the first non-air block.
- [ ] `native_held()` with the stack's components, or `native_held_stack() -> Option<ItemStack>` — needed to tell
  a named or enchanted tool from a plain one (brushes bound to one stack, as WorldEdit's tools are to an item type
  only, would allow both) — today: brushes and the wand go by item id, as in WorldEdit.

## Selection display

- [ ] `fn native_show_block(&mut self, uuid: u128, pos: [i32; 3], state: Option<BlockStateId>, tag: Option<Tag>)`
  (a client-only block for one player, `None` puts the real one back, the block entity type looked up inside) —
  needed for WorldEdit's server-side CUI (the selection outlined by a structure block) — today: raw
  `BlockUpdate`/`BlockEntityData` packets and `game.lan.hub.encoder.entity_type`, drawn at the next level tick
  because the hub is out of `self.lan` inside a guest's click.
- [ ] Custom payload in and out (`worldedit:cui` plugin channel; the same hooks the voice chat port needs) —
  needed for the WorldEdit CUI client mod's outlines of any size — today: not possible; structure block only,
  up to 48 blocks a side.

## Compared with WorldEdit's command classes (2026-11-08, WorldEdit `f5ad5a6`)

Not ported, and why (nothing is faked):

- Selection types `poly`, `polyextend`, `ellipsoid`, `sphere`, `cyl`, `convex` and `//pos`, `//trim`, `toggleeditwand`,
  `//curve`: the session holds one cuboid (two corners); the others need region types and the CUI channel.
- `//fast`, `//perf`, `//reorder`, `//update`: need the side-effect hooks listed above.
- `//placement`, `//timeout`, `//drawsel`, `//world`, `//watchdog`, `//registry`: session options, not written.
  Written (2026-11-08): `//limit`, `//gmask`, `//toggleplace`, `//searchitem` (blocks only: the item list is not
  something the editor sees; `-i` says so), `//calc` (arithmetic, `^`, `%`, the math functions; WorldEdit's
  variables, conditions and ternaries are not), `/we version|reload|report|trace|cui|tz|help` (`/we cui` answers
  nothing: no plugin channel; `/we tz` takes `UTC`, `Z` and offsets: zone names need a time zone database;
  `/we report -p` is refused), `//help` (a bare `//help` stays vanilla's, see README).
- `//generate`, `//generatebiome`, `//deform`, `//regen`, `//forestgen`, `//pumpkins`, `//forest`, `//flora`,
  `//feature`, `//structure`, `//snowsmooth`, `//butcher`, `//remove`: block ones are not written yet; tree,
  feature, structure and chunk regeneration need a world-generation hook; `//butcher` and `//remove` need entities.
  Written (2026-11-08): `//fill`, `//fillr`, `//drain`, `//fixwater`, `//fixlava`, `//snow`, `//thaw`, `//green`,
  `//extinguish`. Two differences: `//snow` puts a layer on any full block the name list calls solid (the game's
  block shapes are not exposed: stairs, slabs and fences are skipped by name), and `//fixwater`/`//fixlava` do not
  re-level flowing liquid because the writes are source blocks.
- Biomes (`/biomelist`, `/biomeinfo`, `//setbiome`, `//replacebiome`, `-b` on copy/cut/paste/stack/move) need a biome
  read and write hook.
- Entities (`-e` on copy/cut/paste/stack/move) are listed above.
- Navigation commands (`/unstuck`, `/ascend`, `/descend`, `/ceil`, `/thru`, `/jumpto`, `/up`) need a teleport hook with a
  safe-position search (`Player.findFreePosition` also follows `//sphere` and `//pyramid` in WorldEdit).
- Tools written: `/tool selwand|info|repl|cycler|floodfill` (no light levels in the info line, no legacy ids).
  Not written: `/tool tree|deltree` (tree generation), `stacker` (the clicked face), `navwand` (teleport), `farwand`,
  `lrbuild` (a left click in the air); each says why when asked. Also not written: the super pickaxe
  (`/`, `//`, `/single`, `/area`, `/recursive`), brush kinds other than sphere, cylinder and smooth (splatter, clipboard,
  snowsmooth, extinguish, gravity, butcher, heightmap, deform, set, forest, feature, raise, lower, snow, biome, morph,
  erode, dilate), `/range`, `/size` (clashes with `//size` once slashes are stripped), `/tracemask`: not written yet;
  tools need item-use hooks with the clicked face, which `right_click_block_v2` does give.
- `//revolve`, snapshots (`/restore`, `/snapshot`), `//schem share`, `cs`/`.s` scripting, `//fast`: out of scope.
- Patterns `#clipboard`, `#copy`, `#simplex`, `#random`, `##tag`, `^type` / `^[state]` (apply to the existing block),
  `*` (random state): the pattern has no access to the world while it picks; masks `#exposed`, `#fullcube`, `#region`,
  `#lazyregion`, `#offset`, `#simplex`, `#expr`, `##tag`, `$biome`, `%percent`, `#clipboard`: not written yet.
- `#solid` and the hollow flood use a name list for "solid" (the game's collision shapes are not exposed here), see
  `pattern::is_solid_name`.
- Item and block names are looked up by the game's registry only: WorldEdit's legacy numeric ids (`//set 1`) and
  fuzzy name matching (`//set oak` for a unique match) are not, except `0` for air.
