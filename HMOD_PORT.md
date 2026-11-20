# hmod port: worldedit
Parity: before 42% -> after 49% (~430/~880 items; method: commands, sub-commands, flags, permission nodes, config keys, lang strings and listed features of the original counted, an item blocked by a missing engine API counts as not done). Blind-written, not compiled.

Round 3 (100 % round)
- Added: selectors `//sel poly|polyextend|ellipsoid|sphere|cyl|convex` (+ `-d`), their clicks and messages, `//size` info, `//expand|contract|shift|outset|inset` rules for shapes; commands that take the shape into account: `//set //walls //faces //replace //count //distr //copy //cut` (src/horizon_worldedit_core/selector.rs, engine/select.rs). Other region commands (hollow, overlay, naturalize, smooth, stack, move, paste target) still use the shape's bounding box.
- Added: `//generate` (`//gen`, `//g`, flags -hroc), `//deform` (-roc) with an expression language (engine/expr.rs: variables, assignments, `;`, `?:`, comparisons, math functions), `/butcher` (all flags, nodes `worldedit.butcher.*`, `limits.butcher-radius` keys), `/remove` (`/rem`, `/rement`), through the new World methods `entities_in` and `remove_entities`.
- Gaps (see GAPS_UPSTREAM.md): `//regen`, biome commands and `-b`, `//paste -e`, `//forest`/`//flora`/tree tool, server-side outline of shapes. Not done yet: `//curve`, `//snowsmooth`, `//revolve`, super pickaxe, `/tool tree|deltree|farwand|lrbuild|stack`, snapshots, `//timeout`, `//fast`, `//perf`. `//generate` has no `type`/`data` variables (always -1).
Round 1
- Fixed: `CommandArgumentType::suggest` takes the sender first (`_sender: Option<&CommandSender>`); the two internal calls pass `None` (src/lib.rs).
- Fixed: `IPayloadHandler::handle` returns `PayloadHandleError` (`CuiInbox`, src/lib.rs).
- Fixed: `has_permission` replaced by typed `permission_bool` queries; `may_use` and `actor_of` take `&mut dyn ServerHost` and all four callers pass `&mut *host` (src/lib.rs).
- Fixed: `give_stack` result matched as `Ok(true)`; now `.is_ok()` (`finish`, src/lib.rs).
- Added: `permission_gather_nodes` declares every name in `horizon_worldedit_core::NODES` as a Boolean node, default level 2 (src/lib.rs).
- Permission nodes: every `worldedit.*` name in `horizon_worldedit_core::NODES` (Boolean) -> default level 2, declared in `permission_gather_nodes` (src/lib.rs). Helper `fn node(name)` builds the typed node.
- Gaps: see 'Fix round 1' below
- Unsure:
  - Default level 2 is the config default of `permission_level`; a server that sets another value keeps 2 as the node default (gather runs before config is applied to the node defaults).
  - Tab-completion has no sender, so `suggest` gets `None`; `may_use` still filters the list for the caller.
  - The `worldedit.*` node check per command is unchanged; only the query path moved from strings to typed nodes.

## Fix round 1 (parity audit)
- Fixed: block entity data is now read (`block_entity` + `data`) and written back (`replace_data` after `edit_blocks`) by `//copy`, `//cut`, `//paste`, `//stack`, `//move`, `//undo`, `//redo` and `/schem save|load` (core: `World::block_entity_at`, `set_block_entities`, `upgrade_block_entity`).
- Added (round 1): `//placement <pos1|player|here|world|min|max> [multiplier] [offset]`, `//wand -n`, `//distr -c`, `//trim`, `/chunkinfo`, `/listchunks`, `/brush extinguish`, `//searchitem` over the item registry (`-b`, `-i`), nodes `worldedit.limit.unrestricted`, `worldedit.anyblock` (default level 4), `worldedit.placement`, `worldedit.selection.trim`, `worldedit.chunkinfo`, `worldedit.listchunks`, `worldedit.brush.ex`; config `max_blocks_maximum` (`limits.max-blocks-changed.maximum`).
- Gaps (still open, nothing invented): non-cuboid selectors (`//sel poly|ellipsoid|sphere|cyl|convex`), `//curve`, `//generate`, `//deform`, `//regen`, `//forest`, `//flora`, `//snowsmooth`, `//revolve`, `/butcher`, `/remove`, super pickaxe, tree/farwand/lrbuild tools, snapshots, biome commands and `-b`/`-e` flags (entities and biomes are not copied), `//timeout`, `//fast`, `//perf`, `//reorder`, `//drawsel`, `//world`, `//watchdog`, `//registry`, `/delchunks`, `/cs`. Scripting (`/cs`) needs a JS engine and is left out on purpose.
- Unsure: `block_entity` handles (`data`, `kind`, `replace_data`) are called as the SDK docs show them; the main session's compile will say if a signature differs. Block entity data is applied right after `edit_blocks` (new block entities are empty until then). The item registry name `minecraft:item` for `registry_entries` is a guess; when it is unknown `//searchitem` falls back to blocks.
