# Status of the server bundle (2026-11-08, blind; WorldEdit source pass)

`written` = code exists and the core part has unit tests on an in-memory world; `partly` = works for the common
case, a known part is missing; `not started` = not in it. Nothing has run in a real Horizon server yet: the
engine was not built (blind port), so the glue (`glue.rs`, the helpers, `horizon-mixin.toml`) is uncompiled.

| WorldEdit feature | State | Notes |
|---|---|---|
| `//wand`, left/right click corners | written | via `left_click_block` / `right_click_block` (v1, no hand: may also see the off hand's turn; needs upstream `right_click_block_v2` for main hand only), click cancelled; 200 ms repeat filter for left clicks and brushes only |
| `//pos1`, `//pos2` (feet, `x,y,z`, `x y z`, `~`) | written | |
| `//hpos1`, `//hpos2` | partly | own ray walk, full cubes only (GAPS: pick helper) |
| `//sel`, `//desel`, `//size`, `//chunk` | written | cuboid only (no poly/ellipsoid/sphere selections) |
| `//expand`, `//contract` (reverse amount, `vert`), `//shift`, `//outset`, `//inset` | written | default direction = view (`me`) |
| `//set`, `//walls`, `//faces`/`//outline`, `//center` | written | |
| `//replace [from] <to>` (`//re`, `//rep`) | written | WorldEdit's one-argument form; aliases added after the public command list (2026-11-08, not built) |
| `/tool none` (`/tool unbind`), `/brush unbind` | written | unbinds a brush or a tool |
| `/tool selwand`, `info`, `repl`, `cycler`, `floodfill` | written | right click acts, left click acts the second way, as WorldEdit's `actPrimary`/`actSecondary`; `tree`, `deltree`, `stacker`, `navwand`, `farwand`, `lrbuild` refuse with the reason (GAPS) (2026-11-08, not built) |
| `//fill`, `//fillr`, `//drain [-w]`, `//fixwater`, `//fixlava`, `//snow [-s]`, `//thaw`, `//green [-f]`, `//extinguish` | written | `EditSession`'s algorithms; `//fill` is told from vanilla's by its arguments; `//snow` goes by a name list for solid blocks (2026-11-08, not built) |
| `//limit`, `//gmask`, `//toggleplace`, `//searchitem`, `//calc` | written | per player; the limit and the global mask apply to every edit (2026-11-08, not built) |
| `//help`, `/we version\|reload\|report\|trace\|cui\|tz\|help` | written | bare `//help` stays vanilla's; unknown sub-commands say so (2026-11-08, not built) |
| `worldedit_server::selection` for WorldGuard | written | the `[api]` package; reaches the engine through `horizon_client::worldedit_server::core` (a `lib` module in horizon-client-lib, as horizon-sentinel does it). Uncompiled (2026-11-08) |
| Flags: `-a -s` (stack, move), `-a -o -n -s` (paste), `-m` (copy, cut, paste), `-h` (sphere, cyl, pyramid, line, brush), `-r` (sphere), `-v` (outset, inset), `-f` (schem save) work; `-b -e` (copy, cut, paste, stack, move), `-r -m` on stack, `-m` on move, `-v` on paste and `//wand -n` are refused by name; unknown flags get the usage line (2026-11-08, not built) | partly | the chat wording follows the WorldEdit 7.4.6 lang file where it has a key |
| `//load <name>` (same as `//schem load <name>`) | written | the README and the chat table had it, the server did not dispatch it (2026-11-08, not built) |
| Chat completions of the server tree (`native_register_commands`): every sub-command with literal choices (directions, `vert`, `cuboid`, brush kinds, `none`/`unbind`), flags, typed numbers and `boolean`, `//undo [n] [player]` | written | 414 typed usages plus one catch-all per command (470 in all); the catch-all keeps any line the typed ones miss. Block and schematic names are not completed on a stock client (the chat table in the client bundle does it for `//`) (2026-11-08, not built) |
| `//undo [n] [player]`, `//redo [n] [player]` | partly | `[player]` works for players who used WorldEdit since the server started (GAPS: name lookup); permission `worldedit.history.undo` for others |
| `/brush smooth [r] [n] [mask]` | written | the mask replaces the brush's `/mask` (2026-11-08, not built) |
| `//hollow [n] [pattern]` | partly | empties the inner box; WorldEdit's hollow only hollows connected solids |
| `//overlay`, `//naturalize` | written | |
| `//smooth [n] [mask]` | partly | 5x5 Gaussian height map; raising copies the top and the block under it, not the column |
| `//line` | written | pos1 to pos2, thickness, `-h` |
| `//stack`, `//move` (`-a`, `-s`, leave pattern) | written | |
| `//sphere`, `//hsphere`, `//cyl`, `//hcyl`, `//pyramid`, `//hpyramid` | written | ellipsoid radii, `-r` raised |
| `//copy`, `//cut` | partly | blocks only: no block entity contents, no entities (GAPS) |
| `//paste` (`-a`, `-o`, `-s`, `-n`) | partly | same: blocks only |
| `//rotate`, `//rot`, `//flip` | written | multiples of 90 around y; states turned (facing, axis, rotation, rails, stairs, doors, slabs) |
| `//undo [n]`, `//redo [n]`, `//clearhistory` | written | per player, 15 edits, same level only |
| `/schem list/load/save/delete/formats` | partly | Sponge v1-3, Litematica, MCEdit, structure `.nbt` read; Sponge v3 written; no block entities; no data fixing of old names |
| Patterns: lists, weights, `hand`, `0`, properties | written | no `#clipboard`, `##tag`, random states, gradients |
| Masks: lists, `!`, `#existing`, partial properties | written | brush masks and `//gmask` |
| Brushes: sphere, cylinder, smooth; `/mask`, `/none` | written | right click on a block or in the air (`use_item`); range 300 |
| Big edits spread over ticks | written | `blocks_per_tick`, refuses a new edit until done |
| Limits: `max_blocks`, radius, brush radius, disallowed blocks | written | 10M positions hard cap per command |
| Config `config/worldedit.toml`, hot reload | written | via `register_configs` / `config_loading` / `config_reloading` |
| Server-side CUI (selection outline) | partly | structure block for one player, ≤ 48 a side, refreshed at the level tick |
| Per-player sessions | written | by UUID; cleared when the world closes (kept across log-outs) |
| Permissions `worldedit.*` | partly | a node per command (WorldEdit's names where the docs give them, guesses elsewhere); minimum level per node in `node_levels`, default `permission_level`; checked on commands, not on brush use; no LuckPerms provider yet (GAPS) |
| `//fast`/`//perf`, `//regen`, `//deform`, `//generate`, biomes, snapshots, navigation (`/jumpto`, `/thru`, `/up`), super pickaxe, `//toggleeditwand`, polygon/ellipsoid selections, WorldEdit CUI mod channel | not started | |

Tests: `cargo test` in `mixins/worldedit-server/core`: 64 passed at the last run, 2026-11-08 (patterns, masks, regions, shapes, clipboard
transforms, edit planning and runs, tick queue, every command end to end on a fake world, schematic round trips).

## Test first, once a build is possible

1. `horizon-mixins plan` against `a589a3dd9`: the eleven named hooks resolve, the bindings' types match, the
   `[[crate]]` builds inside the workspace with Horizon's `Cargo.lock` (flate2, serde, toml).
2. Compile the glue: `registry.resolve` with `BlockProperties::from(BTreeMap)`, `Tag::compound` with `&str` keys,
   `NativeBlockRun` fields, `game.lan.hub.encoder.entity_type`, `native_held` inside the click hooks.
3. On a throwaway dedicated server with a vanilla 26.3 client: `//wand`, left/right click (block not broken in
   creative, log not stripped), `//set stone` over 3x3x3 then `//undo` (check with `execute if block`), two players'
   selections apart.
4. `native_edit_blocks` from inside `perform_command`: is the guest's level the one in place (`self.dim`) in the
   Nether? Does the fallback per block trigger anywhere?
5. A 1,000,000-block `//set`: tick time, the queue's ticks, the "written" message, `//undo` afterwards.
6. `/schem save` a build, load it into WorldEdit 7.4.6 on a Paper test server and paste it, and the other way round:
   same place relative to the player (the offset rules in `core/src/schem/mod.rs` are from the format, unproven
   against a WorldEdit-written file).
7. `//rotate 90` in chat: reaches `player_command`, vanilla `/rotate @s 90` still works.
8. The selection outline: appears after a wand click, moves with `//expand`, the real block comes back.
9. Brush in the air and on a block with a blaze rod; off-hand double fire.
10. The server log after start: no "left out" warning for the typed usages (`native_register_commands`
    names any usage that does not parse or clashes with another kind). Then `/brush`, `/mask`, `/tool` and
    `//sel`, `//hollow`, `//replace`, `//load` in a stock client: the completions and the usage line match.

## 2026-11-08: pass against the WorldEdit source

Every command class in `worldedit-core/.../command/` was compared with `engine.rs`. Changed to match:

- Chat texts: every success and error line now reads as in `lang/strings.json` (set: `Operation completed (N blocks
  affected)`, replace/overlay/naturalize/smooth/move/stack/sphere/..., positions as `(x, y, z)` with the area,
  `//size` as the multi-line block, `//copy` and `//paste` with the three `affected` lines, schematic and brush texts,
  `Make a region selection first.`, `Your clipboard is empty. Use //copy first.`, block, property and mask errors).
- Permission nodes are the `@CommandPermissions` of each command (`worldedit.region.set`, `worldedit.generation.sphere`,
  `worldedit.history.clear`, `worldedit.analysis.count`, `worldedit.schematic.*`; `//undo` accepts
  `worldedit.history.undo` or `.self`, another player's history needs `.other`).
- `//hollow` is WorldEdit's flood fill from the selection's faces (default thickness 0, as WorldEdit); `//naturalize`
  starts at the first grass, dirt or stone block of a column; `//overlay` skips columns already covered; `//line`
  uses WorldEdit's line walk with a ball radius and `-h` shell; `//center` rounds like `roundHalfUp`; cylinders of
  height 0 are empty and negative heights reach below the feet.
- `//sphere` takes 1 or 3 radii, `//cyl` 1 or 2 (at least 1), `-r` is the only way to raise a sphere; `//cone` added.
- `//stack` has `-r` (block units) and `-m`; `//move` `-m`; offsets may be `x,y,z`; `//chunk` has `-s`, `-c` and
  coordinates and uses WorldEdit's 256-tall chunks; `//size -c`; `//distr -d -m -p`; `//sel extend`; positions say
  `Position already set.`; selection edits are clamped to the build height.
- Patterns: fractional weights (`12.5%stone`), `Block name 'x' was not recognized.`, unknown property and value
  errors. Masks: space-separated parts, `!` per part, `#air`, `#solid`, `^[...]`, `^=[...]`, `No match for 'x'.`
- Added `//removeabove`, `//removebelow`, `//removenear`, `//replacenear`, `/material`, `//middle`, `//mirror`,
  `//deselect`, `/schem list -p -d -n`, `/schem delete|d`, `/schem formats|f`, `/brush unbind`.
- Default `disallowed_blocks` is WorldEdit's list (applies below operator level 4).

Not compiled and not run: no cargo was used in this pass. The unit tests in `core/src/tests.rs`, `ops.rs`, `shapes.rs`
and `pattern.rs` were rewritten by hand for the new texts and have not been executed.

## 2026-11-08 wand and completions review

- Wand (`engine.rs` `wand_click`): same corner again is silent (WorldEdit's `SelectionWand` ignores a `selectPrimary/Secondary` that changes nothing; only `//pos1`/`//pos2` say "Position already set."), permission node `worldedit.selection.pos`, `wand_item` gets its `minecraft:` namespace. Not built in the engine.
- `//x` completion: not expressible with Horizon's command words (GAPS.md); every `/x` usage is valid (test).
- Command roots (needs the engine that accepts a leading `/` in a command name): double-slash commands register as `/set`, `/pos1`... (typed `//set`); `we`, `mask`, `tool`, `none`, `clearclipboard` stay plain; `brush`, `br`, `schem`, `schematic`, `undo`, `redo`, `clearhistory`, `material`, `removeabove/below/near`, `replacenear` register both ways, as WorldEdit does. Dispatch unchanged.

## Found by the live test run of 2026-11-08 (test server)

Both WorldEdit bundles were installed, so `//pos1` was answered by the upstream bundle (its own selection and
messages: `Position 1 set to ...`, `x,y,z` refused) and `//size`, `//stack`, `//inset` by this one, from a
selection it never saw: `Make a region selection first.`; `//set` and `//count` went to either. The upstream
bundle now stands aside on a dedicated server (`worldedit_defers_to_server`, an injection guard in `command.rs` and
`guest-command.rs`). Each of those commands has an end-to-end test here on one session (`one_selection_for_every_command`,
`shapes_change_a_fresh_area`); `//schem` prints a file name, never a path (`schematics_never_print_a_path`).
