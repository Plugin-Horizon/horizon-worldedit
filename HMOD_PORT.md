# hmod port: worldedit
Round 1
- Fixed: `CommandArgumentType::suggest` takes the sender first (`_sender: Option<&CommandSender>`); the two internal calls pass `None` (src/lib.rs).
- Fixed: `IPayloadHandler::handle` returns `PayloadHandleError` (`CuiInbox`, src/lib.rs).
- Fixed: `has_permission` replaced by typed `permission_bool` queries; `may_use` and `actor_of` take `&mut dyn ServerHost` and all four callers pass `&mut *host` (src/lib.rs).
- Fixed: `give_stack` result matched as `Ok(true)`; now `.is_ok()` (`finish`, src/lib.rs).
- Added: `permission_gather_nodes` declares every name in `horizon_worldedit_core::NODES` as a Boolean node, default level 2 (src/lib.rs).
- Permission nodes: every `worldedit.*` name in `horizon_worldedit_core::NODES` (Boolean) -> default level 2, declared in `permission_gather_nodes` (src/lib.rs). Helper `fn node(name)` builds the typed node.
- Gaps: none
- Unsure:
  - Default level 2 is the config default of `permission_level`; a server that sets another value keeps 2 as the node default (gather runs before config is applied to the node defaults).
  - Tab-completion has no sender, so `suggest` gets `None`; `may_use` still filters the list for the caller.
  - The `worldedit.*` node check per command is unchanged; only the query path moved from strings to typed nodes.

## Fix round 1 (parity audit)
- Fixed: block entity data is now read (`block_entity` + `data`) and written back (`replace_data` after `edit_blocks`) by `//copy`, `//cut`, `//paste`, `//stack`, `//move`, `//undo`, `//redo` and `/schem save|load` (core: `World::block_entity_at`, `set_block_entities`, `upgrade_block_entity`).
