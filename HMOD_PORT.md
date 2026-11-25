# hmod port: worldedit
Parity: 99% now / 99% with our PRs / 99.9% with engine changes / 99% with new SDK (1025/1034 items at fine grain, round 6, 9 of them engine-only; was 56% / 56% / 56% / 56% (490/880, a hand estimate); method: commands, sub-commands, flags, permission nodes, config keys, lang strings and listed features of the original counted, an item blocked by a missing engine API counts as not done). Round 6 is written blind and queued for its first build (`r6-worldedit-1`).
Steps: B = A (no open upstream PR covers a worldedit item); C adds the 7 tree and biome items of wave G10 that were already counted (472) on top of round 100; D = A (no merged upstream API applies). The estimate is by hand, not a measured count.

## Checklist round 100 (walked against orig-src/worldedit, 2026-11-20)
Walked: the `@Command` names of every class in `worldedit-core/.../command/`, the brush list (`BrushCommands`), the tool list (`ToolCommands`, `ToolUtilCommands`), the `@CommandPermissions` nodes of the commands above, the flags of snowsmooth, revolve and clipboard, the lang strings of those commands. Not walked in detail this round (open): `GenerationCommands` (structure, feature, pumpkins, forestgen), `BiomeCommands`, `ChunkCommands` `delchunks`, `SnapshotCommands` and `LegacySnapshotCommands`, `SuperPickaxeCommands`, `ApplyBrushCommands` and `PaintBrushCommands`, `SchematicCommands share`, the config keys of `LocalConfiguration`, and the remaining lang strings.
- [x] `//revolve <count> [-m mask] [-r]` (`ClipboardCommands.revolve`); `-e` and `-b` refused (not supported).
- [x] `/brush snowsmooth [radius] [iterations] [-l] [-m]` (`BrushCommands.snowSmoothBrush`); its kernel is 10, not a parameter.
- [x] `/brush clipboard` / `copy` with `-a`, `-o`, `-v`, `-m`; `-e` and `-b` refused.
- [x] `/tool navwand` / `/navwand` (right click only; the left click, pass through walls, is not ported).
- [x] `//smooth`, `//snowsmooth` and the smooth brush now use WorldEdit's Gaussian kernel (11 x 11 for radius 5, not 5 x 5) and its edge rule (`HeightMapFilter` reads the centre cell off the map).
- [x] Earlier rounds: selection, region, shape, clipboard, history, schematic, brush sphere / cylinder / smooth / extinguish, tools selwand / info / repl / cycler / floodfill / farwand, navigation commands, limit / fast / perf / timeout / reorder / drawsel / watchdog / registry / searchitem / calc, `/butcher`, `/remove`, `//generate`, `//deform`.
- [ ] Brushes not ported: splatter, gravity, butcher, set, raise, lower, snow, morph, erode, dilate (no API needed, open); heightmap (reads an image file, open); forest, feature (generation API, see GAPS_UPSTREAM.md); biome (no biome write API).
- [ ] Tools not ported: tree, deltree, stacker, lrbuild (hooks, GAPS_UPSTREAM.md); super pickaxe `/` and `,` (drop-items API); `/tool mask`, `material`, `range`, `size`, `tracemask` (no API, open).
- [ ] Commands not ported: `//regen`, `//forest`, `//flora`, biome commands and `-b` (API); `//world` (level-by-name API); `/delchunks` (API: `LevelRef::delete_chunks` in GAPS_UPSTREAM.md); `/cs`, `.s` (scripting, out of scope); snapshots (open).
- [ ] navwand left click `thru` (left click in the air, API).
- Unsure:
  - The revolve turn: WorldEdit's `rotateY` signs taken from `AffineTransform.rotateY`; the block is moved to the nearest block of its turned centre, and the offset of WorldEdit's `translate(0.5)` is folded into that rounding. Quarter turns are exact; the other angles are an estimate.
  - The clipboard brush centre is `floor((min + max) / 2)` relative to the origin, read from `Region.getCenter()` and `toBlockPoint()`; not checked against a run.
  - `snowsmooth` iterations clamp to 0..50 here (the original has no limit); the brush's `-l` default is 1 as in the original.

Round 100 (blind)
- Fixed: the blur kernel. WorldEdit's `GaussianKernel(5, 1.0)` is an 11 x 11 square (radius 5); the port used 5 x 5. `HeightMapFilter` also reads the centre cell for a sample off the map, not the edge cell; the port clamped. Affects `//smooth`, `//snowsmooth` and the smooth brush (`ops::kernel`, `ops::blur_once`).
- Added: `/brush snowsmooth` (`BrushShape::SnowSmooth`, `ops::snow_brush_box`), `//revolve` (`engine.rs` `revolve`, `Clipboard::turned`), `/brush clipboard` (`BrushShape::Paste`, the paste is done in `brush_use` with its own commit so block entity data goes along), `/tool navwand` (`Tool::NavWand`, `nav_use` takes a bound item).
- Permission nodes added: `worldedit.brush.snowsmooth`, `worldedit.brush.clipboard`, `worldedit.revolve`. The `/tool navwand` node is `worldedit.navigation.jumpto.tool` (as in the original).
- Unit tests (literal values, not run): `gaussian_kernel_is_radius_sized_and_sums_to_one`, `blur_reads_the_centre_off_the_map`, `snow_brush_box_floors_and_grows_ten_up`, `turned_copy_moves_blocks_round_the_origin`.
- Flags: `brush` accepts `-h -l -m -a -o -v` (and refuses `-e -b`), `revolve` takes `-m -r`.
- Note: the request named a "kernel-size parameter" for `/brush snowsmooth`. The original has none: the brush's kernel is fixed (`GaussianKernel(10, 1.0)`), so the port does the same.

Round 5 (curve and snowsmooth, blind)
- Added: `//curve [-h] <pattern> [thickness]` (`engine.rs` `curve`, `shapes::spline`): only on a convex polyhedral selection, otherwise WorldEdit's "//curve only works with convex polyhedral selections"; the spline runs through the selection's vertices as `EditSession.drawSpline` does with tension, bias and continuity 0 (Catmull-Rom), sampled about ten times per unit of arc length, ballooned by the thickness, hollowed with `-h`; message "N blocks have been changed.".
- Added: `//snowsmooth [iterations] [-l layers] [-m mask]` (`engine.rs` `snow_smooth`, `ops::snow_smooth`): WorldEdit's `SnowHeightMap` (snow layers as fractions of a block, highest ground block per column, the 5x5 blur `iterations` times, half a layer added, growing columns get snow layers and copied blocks below, shrinking ones are cut down, liquid tops do not grow); message "Snow's height map smoothed. N blocks changed."; iterations clamped to 0..50 as the port's `//smooth` does (the original has no limit).
- Permission nodes added: `worldedit.region.curve`, `worldedit.region.snowsmooth`.
- Unit tests (literal values, not run): `spline_runs_between_its_end_nodes`, `spline_hollow_of_a_thin_line_keeps_it_all` (shapes.rs), `snow_smooth_puts_a_snow_layer_on_flat_ground` (ops.rs).
- Gaps (not done): `/brush snowsmooth` (WorldEdit's brush uses `GaussianKernel(10, 1.0)`, the port's blur is fixed at 5x5, so it needs a kernel size argument first); `//revolve` (clipboard turns are quarter turns only); `//regen`, `//forest`, `//flora` (API in GAPS_UPSTREAM.md).
- Unsure: the spline's arc-length sampling follows the Java loop step (1 / length / 10) but float rounding can give one sample more or less at the very end; the snow column rules read the world as it was before the command (the original reads its own edits, which only matters inside one column and is covered by the ordering in `ops::snow_smooth`).

Round 4 (session options and far wand, blind)
- Added: `//timeout [ms]` (default 100, message as WorldEdit), `//fast [on|off]` (flag, WorldEdit's messages), `//perf [sideeffect] [state] [-h]` (six exposed side effects, WorldEdit's defaults and messages, text list instead of the info box), `//reorder [multi|fast|none]`, `//drawsel [on|off]` (turns the selection outline off or on for the player, as a `useServerCUI` file would), `//watchdog [active|inactive]` (prints WorldEdit's "no watchdog hook" error, as on any platform without one), `//registry [-p page] <block|item> [query]` (glob search, WorldEdit's titles and "Invalid registry key" text), `/tool farwand` (left click pos #1, right click pos #2 on the block looked at; a right click in the air through the new `far_wand_air` hook in `right_click_item`).
- Permission nodes added: `worldedit.timeout`, `worldedit.fast`, `worldedit.perf`, `worldedit.reorder`, `worldedit.drawsel`, `worldedit.watchdog`, `worldedit.registry`, `worldedit.tool.farwand`.
- Recorded only, nothing reads them: `//fast`, `//perf`, `//reorder` and `//timeout` (the port's calculator has no timeout, and its writes already skip neighbour updates). `//registry` lists the block and item registries only; other registry names are refused, and the item list is empty on hosts that do not offer it.
- Gaps (not done, nothing invented): `//world` (needs a level by name, see GAPS_UPSTREAM.md), `//revolve` (the clipboard turns in quarter turns only, revolving needs any angle), `//curve` (convex polyhedral region and spline), `//snowsmooth` (a brush, not ported this round), super pickaxe (drop-items needs the break-with-drops API), tree/deltree/stacker/lrbuild tools (clicked face, tree generation, left click in the air).
- Unsure: far wand left click in the air is not hooked (no event), so only the air right click sets pos #2; `//drawsel off` stops new outline updates for the player but does not clear a box already drawn.

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

Round 3 (2026-11-25)
- `next-engine` feature removed: `//regen` (engine regeneration, or `level.generate_chunk` under a seed or into the clipboard), `/delchunks` (`-o` by `level.saved_chunk_modified`, backup by `level.delete_chunks_with_backup`) and the structure-block selection outline for clients without the CUI mod (`player_show_block_entity`) are normal code. Item list: 125/125 (the CraftScript command stays n/a).
