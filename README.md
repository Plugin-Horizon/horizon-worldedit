# WorldEdit for Horizon

> **Unofficial Rust port of WorldEdit for the Horizon server (https://github.com/horizon-rust/horizon). Not affiliated with or endorsed by the WorldEdit authors.**

In-game world editing for [Horizon](https://github.com/horizon-rust/horizon) (the Rust Minecraft 26.3), as
native mods. This is a fork of [XnotCykoX/WorldEdit-Horizon](https://github.com/XnotCykoX/WorldEdit-Horizon)
(MIT), with its history, licence and author notice kept, plus a second bundle for **dedicated servers**.

Not affiliated with EngineHub. The server bundle is **derived from WorldEdit**
([EngineHub/WorldEdit](https://github.com/EngineHub/WorldEdit), GPL-3.0): its commands, arguments, permission nodes
and chat messages follow the source 1:1 (see [Licence](#licence-and-sources)).

| Bundle | Where it runs | Who it serves |
|---|---|---|
| [`mixins/worldedit`](mixins/worldedit) (upstream, unchanged) | single player, a LAN host, a guest of one | the person at the window; schematics menu, chat completions |
| [`mixins/worldedit-server`](mixins/worldedit-server) (this fork) | a dedicated server (and a LAN host) | every player of the server, each with their own selection, clipboard and history |

Install one of the two, not both: both answer `//set`. On a dedicated server the upstream bundle now stands
aside (it checks `--dedicated`), so a server that has both still answers every command from the server bundle's
per-player session; before that, `//pos1` was answered by the upstream bundle and `//size` by this one, from two
different selections.

## The server bundle

Plain vanilla (or Horizon) clients connect to a Horizon dedicated server built with this bundle and use
WorldEdit there. Nothing is needed on the client.

```
//wand                         a wooden axe: left click position 1, right click position 2
//set 70%stone,30%andesite     fill the selection
//copy   //paste   //undo      the clipboard keeps where you stood when you copied
/schem save house  /schem load house
/brush sphere stone 3          then right click with the held item
```

### Commands

| Area | Commands |
|---|---|
| Selection | `//wand` `//pos1` `//pos2 [x,y,z]` `//hpos1` `//hpos2` `//sel [cuboid\|extend]` `//desel` (`//deselect`) `//size [-c]` `//expand <n> [reverse] [dir]` `//expand vert` `//contract <n> [reverse] [dir]` `//shift <n> [dir]` `//outset` `//inset [-h] [-v] <n>` `//chunk [-s] [-c] [x,y,z]` `//count <mask>` `//distr [-d] [-m mask] [-p page]` |
| Region | `//set` `//replace [from] <to>` (`//re`, `//rep`) `//walls` `//faces` (`//outline`) `//center` (`//middle`) `//hollow [thickness] [pattern]` `//overlay` `//naturalize` `//smooth [n] [mask]` `//line <pattern> [thickness] [-h]` `//stack [n] [offset] [-a] [-s] [-r] [-m mask]` `//move [n] [offset] [leave] [-a] [-s] [-m mask]` |
| Shapes | `//sphere` `//hsphere <pattern> <r>[,y,z] [-r]` `//cyl` `//hcyl <pattern> <r>[,z] [height]` `//cone <pattern> <r>[,z] [height] [thickness] [-h]` `//pyramid` `//hpyramid <pattern> <size>` |
| Around you | `//removeabove [size] [height]` `//removebelow [size] [height]` `//removenear <mask> [radius]` `//replacenear <radius> [from] <to>` |
| Clipboard | `//copy [-m mask]` `//cut [leave] [-m mask]` `//paste [-a] [-o] [-s] [-n] [-m mask]` `//rotate <degrees>` (or `//rot`) `//flip [dir]` (`//mirror`) `/clearclipboard` |
| History | `//undo [n] [player]` `//redo [n] [player]` `//clearhistory` |
| Schematics | `/schem list [-p page] [-d\|-n]` `/schem load <name>` `/schem save <name> [-f]` `/schem delete <name>` `/schem formats` |
| Brushes | `/brush sphere [-h] <pattern> [r]` `/brush cylinder [-h] <pattern> [r] [height]` `/brush smooth [r] [n] [mask]` `/mask [mask]` `/material <pattern>` `/brush none` (`/brush unbind`) (`/br` for `/brush`) `/tool none` (`/tool unbind`) |
| Utility | `//fill <pattern> <radius> [depth]` `//fillr <pattern> <radius> [depth]` `//drain [-w] <radius>` `//fixwater <radius>` `//fixlava <radius>` `//snow [-s] [size] [height]` `//thaw [size] [height]` `//green [-f] [size] [height]` `//extinguish [radius]` (`//ex`, `//ext`) |
| Session | `//limit [n]` `//gmask [mask]` `//toggleplace` `//searchitem [-b] [-p page] <query>` (`//search`, `//l`) `//calc <expression>` (`//eval`, `//solve`) |
| Tools | `/tool selwand` `/tool info` `/tool repl <pattern>` `/tool cycler` `/tool floodfill <pattern> <range>` `/tool none` |
| Help | `/we` `/we version` `/we help [-s] [-p page] [command]` `/we reload` `/we report` `/we trace [active\|inactive]` `/we tz <offset>` `/we cui` |

An offset is a direction word or `x,y,z`. Patterns: `stone`, `70%stone,30%gravel` (weights may be fractional),
`oak_stairs[facing=east]`, `hand` (the held block), `0` (air). Masks (space-separated parts must all match; `!` turns
a part round): `stone,dirt`, `!air`, `#existing`, `#air`, `#solid`, `^[facing=east]` / `^=[...]` (strict),
`oak_stairs[facing=east]` (matches the named properties only). Directions:
`north` `south` `east` `west` `up` `down`, their first letters, and `me` (the default), `back`, `left`, `right`
from where you look.

Slashes don't matter: the server strips them, so `//set` and `/set` are the same command. The vanilla client
completes the one-slash form only (see [GAPS.md](GAPS.md)). Three WorldEdit commands share their word with
vanilla's, so they are told apart by their arguments (`is_shadowed`): `//rotate 90` (a number first), `//fill
<pattern> <radius> [depth]` (vanilla's `/fill` takes seven words or more) and `//help <command>` / `//help -p 2`
(a bare `//help` stays the game's; `/we help` is always WorldEdit's). They have no completions.

### How it behaves

- **Per player.** Selections, clipboards, history (15 edits) and brushes belong to the player's UUID.
- **WorldEdit's semantics.** `//replace stone` replaces every non-air block; `//overlay` lays the pattern on top
  of the surface; `//copy` remembers where you stood and `//paste` lays the copy around you the same way; `//paste
  -o` puts it back where it was copied; spheres and cylinders are measured with half a block added.
- **Bulk writes.** Edits go to Horizon's `native_edit_blocks` as column runs (a chunk section packed once, no
  neighbour updates, no drops, light recomputed, chunks resent whole), with `native_set_block` as the fallback.
- **No freeze.** An edit bigger than `blocks_per_tick` (200,000) writes that many blocks now and the rest over the
  next ticks; the player is told when it is done, and can't start another edit until then.
- **Selection outline.** The selection is outlined by a structure block only its player sees (WorldEdit's
  server-side CUI), for boxes up to 48 blocks a side.
- **Schematics stay in their folder.** Names may have subfolders but no `..`, no absolute path, no symlink.

### Config: `config/worldedit.toml`

Written with comments on first start, read again whenever it is saved:

| Key | Default | WorldEdit's key |
|---|---|---|
| `permission_level` | 2 | (the default minimum level of every `worldedit.*` node) |
| `node_levels` | `{}` | (per-node minimum levels, `worldedit.*` names as WorldEdit's) |
| `max_blocks` | -1 (no limit) | `limits.max-blocks-changed` |
| `blocks_per_tick` | 200000 | (FAWE's queue, roughly) |
| `history_size` | 15 | `history.size` |
| `wand_item` | `minecraft:wooden_axe` | `wand-item` |
| `max_brush_radius` / `brush_range` | 5 / 300 | `limits.max-brush-radius` |
| `max_radius` | -1 | `limits.max-radius` |
| `schematics_dir` | `worldedit/schematics` (inside `config/`) | `saving.dir` |
| `disallowed_blocks` | WorldEdit's list | `limits.disallowed-blocks` (applies below operator level 4) |
| `creative_only` | false | |
| `server_side_cui` | true | `server-side-cui` |

Point `schematics_dir` at a Paper server's `plugins/WorldEdit/schematics` and both servers share the files:
`/schem save` writes Sponge v3, the format WorldEdit 7.3+ writes, with the copy's origin in
`Metadata.WorldEdit.Origin`; loading reads Sponge v1 to v3 (with v2's `WEOffset` metadata), Litematica,
MCEdit/legacy `.schematic` and vanilla structure `.nbt`.

One command looks at no more than 10,000,000 positions, whatever `max_blocks` says.

### Build

A [horizon-mixins](https://github.com/horizon-rust/horizon-mixins) bundle with its own crate:

```
mixins/worldedit-server/
  horizon-mixin.toml   one [[crate]], two modules, an [api] package, eleven named hooks (no raw function targets)
  api/                 the trait-mod half: `worldedit_server::selection`, which WorldGuard reads (see below)
  glue.rs              `mod worldedit_server` in game.rs: the level as the editor's World, the hooks' work
  *.rs                 the hook helpers, one call each
  core/                horizon-worldedit-core: everything without game types, with its tests
```

```sh
cargo test --manifest-path mixins/worldedit-server/core/Cargo.toml     # 64 tests on a world in memory
horizon-launcher native-build mixins/worldedit-server --server-output DIR --version <tag> --trust-native-code
```

The bundle pins Horizon `a589a3dd9`. It was **written without building the engine**: the core crate is
tested, the glue has not been compiled yet. [STATUS.md](STATUS.md) lists what to test first;
[GAPS.md](GAPS.md) what Horizon lacks for the rest.

### Selection for other mods (WorldGuard)

The bundle has an `[api]` package (`api/`) with `worldedit_server::selection::{selection_of, set_selection}`,
the interface [horizon-worldguard's SELECTION.md](https://github.com/Detimes/horizon-worldguard) asks for.
Both reach the one engine through `horizon_client::worldedit_server::core`, a `pub` module in
horizon-client-lib that re-exports the core crate (the way horizon-sentinel publishes its core). A cuboid
selection only; `/rg define` and the like read what `//pos1` and `//pos2` (or the wand) set, and `/rg select`
writes the player's selection and has its outline drawn again.

## Improvements that could go back upstream

The server bundle shares its ideas with the upstream bundle; these would be pull requests to
XnotCykoX/WorldEdit-Horizon, so the client bundle gets them and stays maintained by its author:

1. **Editor state per player, not per world.** Upstream keys the state by `world_root`: on a LAN host every guest
   shares one selection, clipboard and `//undo`. Keying by the player's UUID fixes that.
2. **`//replace <to>` with one argument** replaces every non-air block (WorldEdit), not "`<from>` to air".
3. **`//overlay`** sets the block above the surface, not the surface block itself.
4. **Clipboard origin.** Copy relative to where the player stands and paste around them (WorldEdit), with
   `-a` (skip air), `-o` (original place), `-s`/`-n` (select the paste); upstream pastes the low corner at the feet.
5. **Schematic offsets.** A Sponge v3 `Offset` is the box's corner relative to the origin (add it, don't subtract);
   v2 files keep that in `Metadata.WEOffsetX/Y/Z` while `Offset` is the absolute corner; legacy `.schematic` uses
   `WEOffsetX/Y/Z` ints (upstream looks for an array `WEOffset`). Today a WorldEdit-made file pastes far off.
6. **`/schem save` writes the clipboard as Sponge v3** with WorldEdit's `Origin` metadata, and refuses to
   overwrite without `-f`; upstream writes the selection as v2 with a zero offset.
7. **Paths.** On anything that isn't the player's own machine, `//schem load/delete <any path>` is a file
   read/delete outside the folder: keep names inside `schematics/` there (no `..`, absolute paths, symlinks).
8. **Rounder shapes.** Radius + 0.5, as WorldEdit measures, so `//sphere stone 3` looks like WorldEdit's.
9. **Missing commands:** `//stack`, `//rotate`, `//flip`, `//smooth`, `//naturalize`, `//hpos1/2`, `//chunk`,
   `//outset/inset`, `//expand vert` and the reverse amount, `//undo <n>`, brushes, `/mask`.
10. **Bulk writes** through `native_edit_blocks` instead of `level_set` per block, and big edits spread over ticks.
11. **Server mode through named hooks** (`register_commands`, `perform_command`, `left_click_block`,
    `right_click_block`, `use_item`): the wand and commands then work on a dedicated server, where the click is a
    server packet and not the window's mouse.
12. **A core crate without game types** (`[[crate]]`), so patterns, shapes and file formats are unit-tested; the
    schematic readers here are upstream's, moved onto plain NBT, and could flow back the same way.
13. Vanilla structure `.nbt` loading; a selection outline through a client-only structure block.

## Licence and sources

**Derived from WorldEdit (https://github.com/EngineHub/WorldEdit, GPL-3.0).** The server bundle
(`mixins/worldedit-server`) ports WorldEdit's command classes (`worldedit-core/.../command/*Commands.java`), its
`lang/strings.json` messages, its pattern and mask parsers, its selection logic and the shape and region algorithms of
`EditSession` to Rust, working from the source (checked out 2026-11-08). The whole repository is therefore distributed
under the **GNU General Public License v3** ([LICENSE](LICENSE), WorldEdit's own licence file); the copyright holders
are kept in [NOTICE](NOTICE). The MIT-licensed parts keep their notices (also in NOTICE).

- Upstream bundle (XnotCykoX/WorldEdit-Horizon, MIT): read in full; its pattern syntax, position hash, schematic
  readers and the 1.12 block table (`legacy_ids.rs`) are reused in `core/` and say so in their headers.
- [keksi2k/horizonmatica](https://github.com/keksi2k/horizonmatica) (MIT, Copyright (c) 2026 keksi2k): `core/src/nbt.rs`
  is its NBT reader/writer unchanged, `core/src/block.rs` its block state type and rotation/mirror code.
- [Kowge/WorldEdit_Horizon](https://github.com/Kowge/WorldEdit_Horizon) (MIT): read for ideas (server hooks,
  sessions by UUID, the view direction as default); no code taken.
- WorldEdit itself (EngineHub, GPL-3.0): command syntax, permission nodes, messages (`lang/strings.json`), the
  hollow, naturalize, overlay, sphere, cylinder, cone, pyramid and line algorithms, selection explain texts and
  brush behaviour. Where this port differs (no CUI channel, no scripting, no entity or biome support) it says so in
  [GAPS.md](GAPS.md) and [STATUS.md](STATUS.md).

The upstream bundle's own README is kept at [mixins/worldedit/UPSTREAM-README.md](mixins/worldedit/UPSTREAM-README.md).

## Contributing

Pull requests and improvements are always welcome! Open an issue or a PR -- direct pushes to main are disabled, every change goes through review.
