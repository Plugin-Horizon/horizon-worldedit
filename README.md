# WorldEdit for Horizon

> **Unofficial Rust port of WorldEdit for the Horizon server (https://github.com/arismoko/horizon). Not affiliated with or endorsed by the WorldEdit authors.**

In-game world editing for [Horizon](https://github.com/arismoko/horizon) (the Rust Minecraft 26.3) as a native
mod for **dedicated servers**. The repository is a fork of
[XnotCykoX/WorldEdit-Horizon](https://github.com/XnotCykoX/WorldEdit-Horizon) (MIT) with its history, licence and
author notice kept; the original bundle for the horizon-rust engine is on the `horizon-plain` branch.

Not affiliated with EngineHub. The server mod is **derived from WorldEdit**
([EngineHub/WorldEdit](https://github.com/EngineHub/WorldEdit), GPL-3.0): its commands, arguments, permission nodes
and chat messages follow the source 1:1 (see [Licence](#licence-and-sources)).

> **Built for and working only with [arismoko/horizon](https://github.com/arismoko/horizon)** (the Horizon fork that carries the mod API). [horizon-rust/horizon](https://github.com/horizon-rust/horizon) is experimental for mods and not supported: the earlier version for its native hooks / horizon-mixins is frozen on the `horizon-plain` branch (work stopped on 2026-11-12).

## Which Horizon

This is a native trait mod: `horizon-mod.toml`, a `Cargo.toml` that depends on `horizon-mod-api`, `src/lib.rs` with the hooks, and the plugin logic in the `core/` crate (plain Rust, no engine needed for its tests).

- Pin: `[native] horizon = "54090ee36c4e2c7b43b66db864220c577b94b7e0"` in `horizon-mod.toml` (game `26.3`). That is a development build of arismoko/horizon `main` plus mod API additions that are not all merged upstream yet, so a stock `main` may lack some host methods this mod calls until they land.
- It compiled and loaded on that build and was smoke-tested there; it has not seen a long run with real players. Treat it as experimental.
- Feature descriptions below were written for the earlier horizon-rust version; where they name engine calls (`native_*`) or old file paths, read them as descriptions only. The Known gaps list is current.

## The server mod

Plain vanilla (or Horizon) clients connect to a Horizon dedicated server built with this mod and use
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
completes the one-slash form only (see the Known gaps section). Three WorldEdit commands share their word with
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

## Build

Build it as a native trait mod with arismoko's launcher (see the "native mods" how-to in the arismoko/horizon docs):

```sh
horizon-launcher native-build <path to this folder> --server-output <dir> --version <tag> --trust-native-code
```

`--trust-native-code` is your consent to compile and run this code inside the server. The logic crate tests run without the engine: `(cd core && cargo test)`.

## Known gaps

What does not work yet on arismoko/horizon (each is marked `TODO(arismoko-api: ...)` in `src/lib.rs`):

- The selection outline (WorldEdit CUI) is not drawn.
- No selection API for other mods yet (WorldGuard on arismoko reports that it has no selection).

## Licence and sources

**Derived from WorldEdit (https://github.com/EngineHub/WorldEdit, GPL-3.0).** The server mod
(`core/`) ports WorldEdit's command classes (`worldedit-core/.../command/*Commands.java`), its
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
  the Known gaps section.
