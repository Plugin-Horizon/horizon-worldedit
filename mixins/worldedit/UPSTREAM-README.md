# WorldEdit for Horizon, native

The WorldEdit editor as a **native mod** for [Horizon](https://github.com/horizon-rust/horizon): a
[horizon-mixins](https://github.com/horizon-rust/horizon-mixins) bundle that is compiled into the game binary
instead of running as a sandboxed WebAssembly mod. It edits the world the game owns directly — a fill writes
blocks through the game's own `set_block`, with no command, no packet, no block log and no command-length
limit — and it works in single player (server thread or in-frame), on a LAN host and on a guest of one.

```
//wand                       a wooden axe called "WorldEdit Wand"
left click / right click    position 1 / position 2, with that axe in hand
//pos1 //pos2 [x y z]        or without it, `~` for where you stand
//set stone,dirt,70%gravel   a region, a shape, a pattern
//cyl acacia_wood,minecraft:stone 10 20
//schem load castle          a .schem, .litematic or .schematic file...
//paste                      ...placed at your feet
//copy //paste //undo //redo
```

## Commands

```
//pos1 //pos2 [x y z]   //sel [clear]   //wand   //we
//expand //contract <amount> [up|down|north|south|east|west]   //shift <amount> <dir>
//set <pattern>         //replace <mask> [pattern]
//walls //outline //hollow //faces //overlay //center <pattern>
//sphere //hsphere <pattern> <radius>      //cyl //hcyl <pattern> <radius> [height]
//pyramid //hpyramid <pattern> <size>      //line <pattern> [thickness]
//copy //cut //paste //move <amount> <dir>
//count <mask> //distr                    //undo //redo //clearhistory
//schem list   //schem load <name>   //load <name>   //schem save [name]   //schem delete <name>
```

A **pattern** is one block (`stone`, `minecraft:oak_stairs[facing=east]`) or a comma-separated list picked at
random for every block: `//set stone,dirt`, `//set 70%stone,30%gravel`, `//cyl acacia_wood,minecraft:stone 4 3`.
Commas inside `[...]` stay with the block's properties. A mask (`//replace`) takes a list too.

One edit touches at most 250,000 blocks, the last 10 edits undo, an edit never loads a chunk, and editing
needs build access and permission level 2 (`//we` and `//wand` do not).

`//` completes in the chat screen: names, block names from the game's registry, directions, and the usage
line instead of vanilla's "Unknown or incomplete command".

## Schematics

`//schem load <file>` reads a schematic into the clipboard and `//paste` places it where you stand; `//undo`
takes it back.

```
//schem list                         what this world has, and where the folder is
//schem load castle                  a name in <world>/schematics/
//schem load castle.litematic        the file name, extension and all
//schem load C:\builds\castle.schem  or a full path, spaces and all
//paste
//schem save castle                  the selection, written as Sponge v2
//schem delete castle
```

`//schem` with no argument opens a menu over the world instead, listing the world's `schematics/` folder —
pick a file, **Load & Paste** (or `Enter`), **Open Folder** to drag files in, **Refresh** (`F5`), **Done**
(`Esc`). The folder is made when the world opens, so it is there before the first schematic; `//schem folder`
opens it from the chat line too.

| File | Format | What is read |
|---|---|---|
| `.schem` | Sponge v1, v2, v3 (WorldEdit, FAWE, Axiom) | the palette and varint `BlockData`, wherever that version keeps them |
| `.litematic` | Litematica | every region with its palette and packed `BlockStates`; a region's negative `Size` (a mirrored one) is honored, and the regions are laid out relative to each other |
| `.schematic` | MCEdit, legacy WorldEdit | `Blocks`/`Data`/`AddBlocks` as 1.12 numeric block ids, through the 1.12→1.13 table in `legacy_ids.rs` |

The format is recognized by the file's contents, not its extension. A Sponge `Offset` (a legacy `WEOffset`)
is subtracted, so the origin recorded when the schematic was made lands on you when you paste. A palette
entry this game has no block for is left out, and the load reports how many.

Saving writes Sponge v2, which WorldEdit, FAWE, Axiom and Litematica all read.

Not yet: block entities and entities inside a file (chest contents, signs, armour stands) — the blocks come
across, not what is in them. A file over the 250,000-block edit limit is refused rather than half-pasted.

## Install with the launcher

1. Download `worldedit-native-1.2.1.hnmod` (or pack it: `python3 tools/pack.py`).
2. Launcher → Mods → install the `.hnmod`, then Settings → Game → **Native mods (experimental)**, then **Build**.
3. Play the **Native mods** instance the build creates.

Native mods are native code with the player's rights: the launcher asks you to trust them before it builds.

## Build it into your own Horizon tree

```sh
git clone https://github.com/horizon-rust/horizon-mixins
cargo build --manifest-path horizon-mixins/Cargo.toml
horizon-mixins/target/debug/horizon-mixins plan \
  --workspace ~/horizon --mods "$PWD/mixins/worldedit" --out plan.json
horizon-mixins/target/debug/horizon-mixins check \
  --workspace ~/horizon --mods "$PWD/mixins/worldedit" --out ~/horizon-we --trust-native-mods
cd ~/horizon-we && cargo build --profile fast -p horizon-client
```

`plan` validates every hook against the tree without writing to it; `check` stages a patched copy and runs
its tests; the last line is a game with WorldEdit in it.

`legacy_ids.rs` is generated from the game's own block table; it changes only with a new game version:

```sh
python3 tools/gen_legacy.py ~/horizon/crates/horizon-world/src/datafix/block_state_data.txt
```

## Layout

```
horizon-native.json          the .hnmod manifest: id, version, game, the Horizon and horizon-mixins commits
mixins/worldedit/
  horizon-mixin.toml         four modules and four hooks
  runtime.rs                 `mod worldedit`: selection, patterns, masks, shapes, clipboard, schematics, undo
  schematic.rs               `mod worldedit_schem`: Sponge, Litematica and MCEdit files, read and written
  legacy_ids.rs              `mod worldedit_legacy`: the 1.12 block table, generated
  command.rs                 a `//command` typed here (HEAD on `run_command`)
  guest-command.rs           a `//command` from a LAN guest or a server client (HEAD on `guest_command`)
  wand.rs                    the wand's clicks (HEAD on `play_mouse`)
  chat-suggestions.rs        `mod worldedit_chat`: the `//` command table and its completions
  chat-hook.rs               the chat screen's completion pass (HEAD on `update_command_info`)
tools/pack.py                packs the .hnmod, checking the manifest against the bundle
tools/gen_legacy.py          regenerates legacy_ids.rs from Horizon's block table
tools/make_fixtures.py       test schematics: Sponge v2 and v3, Litematica, MCEdit
tools/verify_schem.py        counts the blocks of a saved .schem
tools/e2e_schematics.sh      drives a built game through load, paste and save of each format
```

## Verified

In an offscreen game on its server thread: `//pos1 0 64 0`, `//pos2 2 66 2` and `//set stone` changed 27
blocks (checked with vanilla `execute if block`), `//count stone` found them, `//undo` put them back; the
wand's left click set position 1 to the block under the crosshair and did not mine it, while a plain wooden
axe mined; `//sphere glass 3` placed 123 blocks; `//set oak_stairs[facing=east]` wrote the property; `//po` +
Tab became `//pos1` and `//set sto` + Tab became `//set stone`; `//set stone,dirt` over 5x1x5 wrote 13 stone
and 12 dirt, `//set 90%glass,10%gold_block` wrote 20 glass and 5 gold, and `//cyl acacia_wood,minecraft:stone
4 3` wrote 147 blocks as 76 acacia and 71 stone.

Schematics, each loaded, pasted and the paste saved again with `//schem save` and counted:

| File | Loaded as | The saved box held |
|---|---|---|
| Sponge v2, 3x2x3 | `Sponge v2`, 18 blocks | 8 stone, 8 glass, 1 dirt, 1 `oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]` |
| Sponge v3, 3x2x3 | `Sponge v3`, 18 blocks | the same 18 |
| Litematica, 7x2x3, two regions, one with `Size.x = -2` | `Litematica v6`, 26 blocks | 17 stone (the mirrored region's 8 among them), 1 stairs, the rest air |
| MCEdit, 2x2x2 | `MCEdit`, 8 blocks | 4 stone, 4 oak planks (1.12 ids 1 and 5) |

`//schem list` printed the folder and the files, `//schem load <a Windows path>` read a file outside the
folder, and `//paste` placed each at the player's feet.

## Compatibility

Native hooks are raw function paths, so a bundle is tied to the Horizon source it is planned against. This
one pins **Nightly 2026-10-28** (`83f15fa`), where `plan` resolves its four modules and four hooks; it was
compiled and played on a tree of that Nightly. Horizon moves fast: on another commit, change
`compatibility.horizon_revision` in `mixins/worldedit/horizon-mixin.toml` (and `horizon` in
`horizon-native.json`) to that commit, and re-run `plan` — a hook whose function moved or whose signature
changed is refused, never silently skipped.

## Not in it

Brushes and tool bindings, `#clipboard`-style pattern sources, probability or expression masks, `//deform`,
`//smooth`, `//naturalize`, `//stack`, `//regen`, biome tools, `//schem move`/`rename`, block entities and
entities inside a schematic, and drawing the selection in the world (there is no world-space line renderer to
hook): `//sel`, `//pos1` and `//pos2` report the selection in chat.

## License

MIT — see [LICENSE](LICENSE).

## Written with an agent

The bundle was written with an AI coding agent (Codebuff) against a Horizon 26.3 tree, at XnotCykoX's
direction: the commands, the patterns, the wand, the completions and the schematic formats were each checked
in an offscreen game before landing here.
