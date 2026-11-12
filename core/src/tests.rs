//! The commands end to end, on a world in memory: what a builder types and what the world holds
//! afterwards.

use crate::engine::{Actor, Engine, Env};
use crate::world::fake::FakeWorld;
use crate::world::World;
use crate::Config;
use std::path::PathBuf;

fn actor(uuid: u128) -> Actor {
    Actor {
        uuid,
        name: format!("builder{uuid}"),
        permission: 2,
        dimension: "minecraft:overworld".into(),
        position: [0.5, 64.0, 0.5],
        yaw: 0.0,
        pitch: 0.0,
        game_mode: 1,
        held: None,
        granted: Default::default(),
    }
}

fn env(tag: &str) -> Env {
    let base = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let dir = base.join("we-tests").join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let config_file = dir.join("worldedit.toml");
    Env { schematics_dir: dir, config_file, data_version: 4440, now_ms: 0 }
}

fn run(e: &mut Engine, w: &mut FakeWorld, a: &Actor, line: &str) -> String {
    let r = e.command(w, a, line, &env("none"));
    let text = r.text();
    assert!(r.lines.iter().all(|l| !l.error), "`{line}` failed: {text}");
    text
}

fn fails(e: &mut Engine, w: &mut FakeWorld, a: &Actor, line: &str) -> String {
    let r = e.command(w, a, line, &env("none"));
    assert!(r.lines.iter().any(|l| l.error), "`{line}` should fail: {}", r.text());
    r.text()
}

#[test]
fn wand_corners_and_set() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    let r = e.command(&mut w, &a, "wand", &env("none"));
    assert_eq!(r.give, vec!["minecraft:wooden_axe".to_string()]);
    a.held = Some("minecraft:wooden_axe".into());
    assert!(e.wand_click(&w, &a, true, [0, 64, 0], 0).is_some());
    // A dig packet sent twice at once: the corner is not said again.
    assert_eq!(e.wand_click(&w, &a, true, [0, 64, 0], 50).unwrap().lines.len(), 0);
    let second = e.wand_click(&w, &a, false, [2, 66, 2], 1000).unwrap();
    assert_eq!(second.text(), "Second position set to (2, 66, 2) (27).");
    // The same corner again (a held button, a click heard twice): the wand takes the click and says
    // nothing, as WorldEdit's SelectionWand does when `selectSecondary` reports no change. Only
    // `//pos2` answers "Position already set.".
    let again = e.wand_click(&w, &a, false, [2, 66, 2], 1050).unwrap();
    assert!(again.lines.is_empty(), "{}", again.text());
    let again = e.wand_click(&w, &a, true, [0, 64, 0], 5000).unwrap();
    assert!(again.lines.is_empty(), "{}", again.text());
    assert_eq!(fails(&mut e, &mut w, &a, "//pos2 2,66,2"), "Position already set.");
    // Without the wand the click is the game's.
    a.held = Some("minecraft:stone".into());
    assert!(e.wand_click(&w, &a, true, [9, 9, 9], 2000).is_none());
    assert_eq!(run(&mut e, &mut w, &a, "//set stone"), "Operation completed (27 blocks affected)");
    assert_eq!(w.count("stone"), 27);
    // Only the changed blocks count the second time.
    assert_eq!(run(&mut e, &mut w, &a, "set stone"), "Operation completed (0 blocks affected)");
    a.held = Some("minecraft:wooden_axe".into());
    // A new place is said, left click first, right click second, and the corners are the clicked blocks.
    assert_eq!(e.wand_click(&w, &a, true, [5, 64, 5], 6000).unwrap().text(), "First position set to (5, 64, 5) (48).");
    assert_eq!(e.wand_click(&w, &a, false, [7, 64, 5], 6001).unwrap().text(), "Second position set to (7, 64, 5) (3).");
    assert_eq!(e.outline_of(1).unwrap().1, crate::region::Cuboid::new([5, 64, 5], [7, 64, 5]));
}

#[test]
fn permission_and_creative_only() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    a.permission = 1;
    assert!(fails(&mut e, &mut w, &a, "//pos1").contains("not permitted"));
    assert!(e.command(&mut w, &a, "wand", &env("none")).give.is_empty());
    a.held = Some("minecraft:wooden_axe".into());
    assert!(e.wand_click(&w, &a, true, [0, 0, 0], 0).is_none());
    // The wand's own node: a server may lower it for the wand alone.
    let mut e = Engine::new(Config { node_levels: [("worldedit.selection.pos".to_string(), 1u8)].into(), ..Config::default() });
    assert!(e.wand_click(&w, &a, true, [0, 0, 0], 0).is_some());
    let mut e = Engine::new(Config { creative_only: true, ..Config::default() });
    a.permission = 2;
    a.game_mode = 0;
    assert!(fails(&mut e, &mut w, &a, "//pos1").contains("creative"));
}

#[test]
fn sessions_are_per_player() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let (a, b) = (actor(1), actor(2));
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 1 64 1");
    assert!(fails(&mut e, &mut w, &b, "//set stone").contains("selection"));
    run(&mut e, &mut w, &a, "//set dirt");
    assert!(fails(&mut e, &mut w, &b, "//undo").contains("Nothing"));
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(w.count("dirt"), 0);
}

#[test]
fn replace_follows_worldedit() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    w.fill([0, 64, 0], [3, 64, 0], "stone");
    w.fill([0, 64, 0], [0, 64, 0], "dirt");
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 4,65,0");
    // One argument: every block that isn't air becomes it.
    assert!(run(&mut e, &mut w, &a, "//replace glass").starts_with("4 blocks"));
    assert_eq!(w.count("glass"), 4);
    run(&mut e, &mut w, &a, "//undo");
    // Two: only the first's blocks.
    assert!(run(&mut e, &mut w, &a, "//replace dirt gold_block").starts_with("1 block"));
    assert!(run(&mut e, &mut w, &a, "//replace !air,stone sand").starts_with("1 block"));
}

#[test]
fn walls_faces_hollow_overlay_center() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 4,68,4");
    run(&mut e, &mut w, &a, "//walls stone");
    assert_eq!(w.count("stone"), 16 * 5);
    run(&mut e, &mut w, &a, "//undo");
    run(&mut e, &mut w, &a, "//faces glass");
    assert_eq!(w.count("glass"), 125 - 27);
    run(&mut e, &mut w, &a, "//set dirt");
    // WorldEdit's hollow floods in from the selection's faces through non-solid blocks, so the
    // selection must reach air around the object.
    run(&mut e, &mut w, &a, "//outset 1");
    assert!(run(&mut e, &mut w, &a, "//hollow").starts_with("27 blocks"));
    assert_eq!(w.count("dirt"), 125 - 27);
    assert_eq!(run(&mut e, &mut w, &a, "//center gold_block"), "Center set. (1 blocks changed)");
    assert_eq!(w.count("gold_block"), 1);
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 4,70,4");
    assert_eq!(run(&mut e, &mut w, &a, "//overlay sand"), "25 blocks have been overlaid.");
    assert_eq!(w.count("sand"), 25);
    assert_eq!(w.name_at([2, 69, 2]), "minecraft:sand");
}

#[test]
fn patterns_and_unknown_blocks() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 9,64,9");
    run(&mut e, &mut w, &a, "//set 50%stone,50%dirt");
    let (s, d) = (w.count("stone"), w.count("dirt"));
    assert_eq!(s + d, 100);
    assert!(s > 30 && d > 30, "{s} {d}");
    assert!(fails(&mut e, &mut w, &a, "//set unobtainium").contains("Block name 'unobtainium' was not recognized."));
    a.held = Some("minecraft:glass".into());
    run(&mut e, &mut w, &a, "//set hand");
    assert_eq!(w.count("glass"), 100);
    run(&mut e, &mut w, &a, "//set oak_stairs[facing=east]");
    assert_eq!(w.name_at([0, 64, 0]), "minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]");
}

#[test]
fn expand_contract_shift_by_view() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 0,64,0");
    a.yaw = 180.0; // north
    assert_eq!(run(&mut e, &mut w, &a, "//expand 5"), "Region expanded 5 blocks");
    let size = run(&mut e, &mut w, &a, "//size");
    assert!(size.contains("Position 1: (0, 64, -5)") && size.contains("Size: (1, 1, 6)"), "{size}");
    assert_eq!(run(&mut e, &mut w, &a, "//expand 2 1 up"), "Region expanded 18 blocks");
    assert_eq!(run(&mut e, &mut w, &a, "//contract 1 down"), "Region contracted 6 blocks.");
    assert!(run(&mut e, &mut w, &a, "//size").contains("Size: (1, 3, 6)"));
    assert_eq!(run(&mut e, &mut w, &a, "//shift 10 east"), "Region shifted.");
    assert!(run(&mut e, &mut w, &a, "//size").contains("Position 1: (10, 63, -5)"));
    assert!(run(&mut e, &mut w, &a, "//expand vert").ends_with("(top-to-bottom)."));
    assert!(run(&mut e, &mut w, &a, "//size").contains("Position 1: (10, -64, -5)"));
    assert_eq!(run(&mut e, &mut w, &a, "//inset -h 0"), "Region inset.");
    assert!(run(&mut e, &mut w, &a, "//size").contains("# of blocks: 2304"));
    // WorldEdit's chunk is 256 blocks tall, counted in steps of 256.
    assert_eq!(run(&mut e, &mut w, &a, "//chunk"), "Chunk selected: 0, 0, 0");
    assert!(run(&mut e, &mut w, &a, "//size").contains("Size: (16, 256, 16)"));
    assert_eq!(run(&mut e, &mut w, &a, "//sel"), "Selection cleared.");
    assert!(fails(&mut e, &mut w, &a, "//size").contains("Make a region selection first."));
}

#[test]
fn copy_paste_is_relative_to_where_you_stand() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    w.fill([5, 64, 5], [6, 65, 5], "stone");
    w.fill([5, 66, 5], [5, 66, 5], "oak_stairs[facing=north]");
    run(&mut e, &mut w, &a, "//pos1 5,64,5");
    run(&mut e, &mut w, &a, "//pos2 6,66,5");
    a.position = [5.5, 64.0, 3.5]; // two blocks north of the build
    assert_eq!(run(&mut e, &mut w, &a, "//copy"), "6 blocks affected\n0 biomes affected\n0 entities affected");
    a.position = [100.5, 70.0, 100.5];
    assert!(run(&mut e, &mut w, &a, "//paste").starts_with("The clipboard has been pasted at (100, 70, 100)"));
    assert_eq!(w.name_at([100, 70, 102]), "minecraft:stone");
    assert_eq!(w.name_at([100, 72, 102]), w.describe(w.id("oak_stairs[facing=north]")).to_string_full());
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(w.name_at([100, 70, 102]), "minecraft:air");
    // Turned a quarter clockwise: what lay south of the player lies west.
    assert_eq!(run(&mut e, &mut w, &a, "rotate 90"), "The clipboard copy has been rotated.");
    run(&mut e, &mut w, &a, "//paste -s");
    assert_eq!(w.name_at([98, 70, 100]), "minecraft:stone");
    assert!(w.name_at([98, 72, 100]).contains("facing=east"));
    assert!(run(&mut e, &mut w, &a, "//size").contains("Size: (1, 3, 2)"));
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(run(&mut e, &mut w, &a, "//flip up"), "The clipboard copy has been flipped.");
    // Mirrored through the feet: what stood above now hangs below, stairs upside down.
    run(&mut e, &mut w, &a, "//paste -a");
    assert_eq!(w.name_at([98, 69, 100]), "minecraft:stone");
    assert!(w.name_at([98, 68, 100]).contains("half=top"), "{}", w.name_at([98, 68, 100]));
    run(&mut e, &mut w, &a, "//undo");
    // Flipped back and turned back, -o puts it exactly where it was copied: nothing changes.
    a.position = [0.5, 64.0, 0.5];
    run(&mut e, &mut w, &a, "//flip up");
    run(&mut e, &mut w, &a, "//rotate -90");
    assert!(run(&mut e, &mut w, &a, "//paste -o").starts_with("The clipboard has been pasted at"));
    assert_eq!(w.count("stone"), 4);
    run(&mut e, &mut w, &a, "/clearclipboard");
    assert_eq!(fails(&mut e, &mut w, &a, "//paste"), "Your clipboard is empty. Use //copy first.");
    run(&mut e, &mut w, &a, "//copy");
    assert!(fails(&mut e, &mut w, &a, "//rotate 45").contains("90"));
}

#[test]
fn cut_move_stack() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    w.fill([0, 64, 0], [1, 64, 0], "stone");
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 1,64,0");
    a.yaw = -90.0; // east
    assert_eq!(run(&mut e, &mut w, &a, "//stack 3"), "6 blocks changed. Undo with //undo");
    assert_eq!(w.count("stone"), 8);
    assert_eq!(w.name_at([7, 64, 0]), "minecraft:stone");
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(run(&mut e, &mut w, &a, "//move 2 up -s"), "2 blocks moved.");
    assert_eq!((w.name_at([0, 64, 0]).as_str(), w.name_at([0, 66, 0]).as_str()), ("minecraft:air", "minecraft:stone"));
    assert!(run(&mut e, &mut w, &a, "//size").contains("Position 1: (0, 66, 0)"));
    run(&mut e, &mut w, &a, "//cut");
    assert_eq!(w.count("stone"), 0);
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(w.count("stone"), 2);
    run(&mut e, &mut w, &a, "//redo");
    assert_eq!(w.count("stone"), 0);
}

#[test]
fn shapes_at_the_feet() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    let full: usize = run(&mut e, &mut w, &a, "//sphere stone 3").split(' ').next().unwrap().parse().unwrap();
    assert_eq!(full, w.count("stone"));
    run(&mut e, &mut w, &a, "//undo");
    let hollow: usize = run(&mut e, &mut w, &a, "//hsphere glass 3").split(' ').next().unwrap().parse().unwrap();
    assert!(hollow < full && w.name_at([0, 64, 0]) == "minecraft:air");
    run(&mut e, &mut w, &a, "//cyl dirt 2 3");
    assert_eq!(w.name_at([2, 66, 0]), "minecraft:dirt");
    run(&mut e, &mut w, &a, "//pyramid sand 3");
    assert_eq!(w.name_at([0, 66, 0]), "minecraft:sand");
    run(&mut e, &mut w, &a, "//pos1 0,80,0");
    run(&mut e, &mut w, &a, "//pos2 5,80,0");
    assert_eq!(run(&mut e, &mut w, &a, "//line gold_block"), "6 blocks have been changed.");
    assert!(fails(&mut e, &mut w, &a, "//sphere stone").contains("Usage"));
    assert_eq!(fails(&mut e, &mut w, &a, "//sphere stone 1,2"), "You must either specify 1 or 3 radius values.");
    assert_eq!(fails(&mut e, &mut w, &a, "//cyl stone 1,2,3"), "You must either specify 1 or 2 radius values.");
    assert!(run(&mut e, &mut w, &a, "//cone stone 3 4").ends_with("blocks have been created."));
}

#[test]
fn undo_redo_and_history_size() {
    let mut e = Engine::new(Config { history_size: 2, ..Config::default() });
    let mut w = FakeWorld::new();
    let a = actor(1);
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 0,64,0");
    for b in ["stone", "dirt", "glass"] {
        run(&mut e, &mut w, &a, &format!("//set {b}"));
    }
    assert!(run(&mut e, &mut w, &a, "//undo 5").contains("Undid 2 available edits"));
    assert_eq!(w.name_at([0, 64, 0]), "minecraft:stone");
    run(&mut e, &mut w, &a, "//redo");
    assert_eq!(w.name_at([0, 64, 0]), "minecraft:dirt");
    run(&mut e, &mut w, &a, "//clearhistory");
    assert!(fails(&mut e, &mut w, &a, "//undo").contains("Nothing"));
    // An edit in another level is undone there only.
    run(&mut e, &mut w, &a, "//set sand");
    let mut nether = a.clone();
    nether.dimension = "minecraft:the_nether".into();
    assert!(fails(&mut e, &mut w, &nether, "//undo").contains("overworld"));
    run(&mut e, &mut w, &a, "//undo");
}

#[test]
fn limit_and_big_edits_over_ticks() {
    let mut e = Engine::new(Config { max_blocks: 10, ..Config::default() });
    let mut w = FakeWorld::new();
    let a = actor(1);
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 3,64,3");
    assert_eq!(fails(&mut e, &mut w, &a, "//set stone"), "Max blocks changed in an operation reached (10).");
    let mut e = Engine::new(Config { blocks_per_tick: 1000, ..Config::default() });
    e.config.blocks_per_tick = 1000;
    run(&mut e, &mut w, &a, "//pos1 0,0,0");
    run(&mut e, &mut w, &a, "//pos2 49,0,49");
    assert!(run(&mut e, &mut w, &a, "//set stone").contains("next 2 ticks"));
    assert_eq!(w.count("stone"), 1000);
    assert!(fails(&mut e, &mut w, &a, "//set dirt").contains("still being written"));
    // Another level's tick writes nothing of it.
    assert!(e.tick(&mut w, "minecraft:the_end").is_empty());
    assert!(e.tick(&mut w, "minecraft:overworld").is_empty());
    assert_eq!(e.tick(&mut w, "minecraft:overworld"), vec![1]);
    assert_eq!(w.count("stone"), 2500);
    run(&mut e, &mut w, &a, "//undo");
}

#[test]
fn schematics_round_trip_and_stay_in_their_folder() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    let env = env("schem");
    w.fill([0, 64, 0], [2, 64, 0], "stone");
    w.fill([1, 65, 0], [1, 65, 0], "oak_stairs[facing=west,half=top]");
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 2,65,0");
    a.position = [1.5, 64.0, -1.5];
    run(&mut e, &mut w, &a, "//copy");
    let r = e.command(&mut w, &a, "/schem save house", &env);
    assert_eq!(r.text(), "house saved.");
    assert!(env.schematics_dir.join("house.schem").is_file());
    assert_eq!(e.command(&mut w, &a, "/schem save house", &env).text(), "That schematic already exists. Use the -f flag to overwrite it.");
    assert_eq!(e.command(&mut w, &a, "/schem save house -f", &env).text(), "house saved (overwriting previous file).");
    let r = e.command(&mut w, &a, "/schem save sub/tower", &env);
    assert!(!r.lines[0].error, "{}", r.text());
    for bad in ["/schem load ../secret", "/schem load /etc/passwd", "/schem save a\\b", "/schem delete ./x"] {
        assert!(e.command(&mut w, &a, bad, &env).lines[0].error, "{bad}");
    }
    let list = e.command(&mut w, &a, "/schem list", &env).text();
    assert!(list.starts_with("Available schematics") && list.contains("[L] house.schem") && list.contains("[L] sub/tower.schem"), "{list}");
    run(&mut e, &mut w, &a, "/clearclipboard");
    let r = e.command(&mut w, &a, "/schem load house", &env);
    assert_eq!(r.text(), "house loaded. Paste it with //paste");
    a.position = [50.5, 64.0, 49.5];
    run(&mut e, &mut w, &a, "//paste -a");
    assert_eq!(w.name_at([49, 64, 51]), "minecraft:stone");
    assert_eq!(w.name_at([51, 64, 51]), "minecraft:stone");
    assert!(w.name_at([50, 65, 51]).contains("facing=west"));
    assert_eq!(e.command(&mut w, &a, "/schem delete house", &env).text(), "house has been deleted.");
    assert_eq!(e.command(&mut w, &a, "/schem load house", &env).text(), "Schematic house does not exist!");
}

#[test]
fn brushes_paint_where_you_look() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    w.fill([-10, 60, -10], [10, 60, 30], "stone");
    a.position = [0.5, 61.0, 0.5];
    a.pitch = 45.0; // down, south
    a.held = Some("minecraft:wooden_axe".into());
    assert!(fails(&mut e, &mut w, &a, "/brush sphere dirt 2").contains("wand"));
    a.held = Some("minecraft:blaze_rod".into());
    assert_eq!(fails(&mut e, &mut w, &a, "/brush sphere dirt 9"), "Maximum brush radius (in configuration): 5");
    assert_eq!(run(&mut e, &mut w, &a, "/br sphere dirt 1"), "Sphere brush shape equipped (1).\nRun /brush unbind while holding the item to unbind it.");
    let r = e.brush_use(&mut w, &a, None, 0).unwrap();
    assert!(r.lines.is_empty(), "{}", r.text());
    assert!(w.count("dirt") > 0);
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(w.count("dirt"), 0);
    run(&mut e, &mut w, &a, "/mask air");
    e.brush_use(&mut w, &a, Some([0, 60, 5]), 1000).unwrap();
    // Only air was painted: the stone floor stays stone.
    assert_eq!(w.name_at([0, 60, 5]), "minecraft:stone");
    assert_eq!(w.name_at([0, 61, 5]), "minecraft:dirt");
    assert_eq!(run(&mut e, &mut w, &a, "/brush none"), "Brush unbound from your current item.");
    assert!(e.brush_use(&mut w, &a, None, 2000).is_none());
    assert_eq!(run(&mut e, &mut w, &a, "/tool none"), "Your current item is not bound.");
}

#[test]
fn rotate_is_ours_only_with_degrees() {
    assert!(crate::is_rotate("rotate 90") && crate::is_rotate("//rotate -90"));
    assert!(!crate::is_rotate("rotate @s 90") && !crate::is_rotate("rotate Steve 90 0"));
    assert!(crate::is_command("//set stone") && crate::is_command("schem list") && !crate::is_command("give @s stone"));
    for u in crate::usages() {
        let name = u.split(' ').next().unwrap();
        assert!(name.strip_prefix('/').unwrap_or(name).chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()), "{u}");
    }
}

#[test]
fn outline_follows_the_corners() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(7);
    assert!(e.outline_of(7).is_none());
    run(&mut e, &mut w, &a, "//pos1 1,2,3");
    assert_eq!(e.outline_of(7).unwrap().1, crate::region::Cuboid::new([1, 2, 3], [1, 2, 3]));
    run(&mut e, &mut w, &a, "//pos2 4,5,6");
    assert_eq!(e.outline_of(7).unwrap(), ("minecraft:overworld".to_string(), crate::region::Cuboid::new([1, 2, 3], [4, 5, 6])));
    e.config.server_side_cui = false;
    assert!(e.outline_of(7).is_none());
}

#[test]
fn near_commands_and_selection_modes() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    w.fill([-2, 64, -2], [2, 66, 2], "stone");
    assert_eq!(run(&mut e, &mut w, &a, "//removeabove 1 2"), "2 blocks have been removed.");
    assert_eq!(run(&mut e, &mut w, &a, "//replacenear 2 stone dirt"), "73 blocks have been replaced.");
    assert_eq!(run(&mut e, &mut w, &a, "//removenear dirt 1"), "0 blocks have been removed.");
    assert_eq!(fails(&mut e, &mut w, &a, "//replacenear 2"), "Usage: //replacenear <radius> [from] <to>");
    // `//sel extend`: the second click grows the box instead of replacing the first corner.
    assert_eq!(run(&mut e, &mut w, &a, "//sel extend"), "Cuboid: left click for a starting point, right click to extend");
    assert_eq!(run(&mut e, &mut w, &a, "//pos1 0,64,0"), "Started selection at (0, 64, 0) (1).");
    assert_eq!(run(&mut e, &mut w, &a, "//pos2 3,64,0"), "Extended selection to encompass (3, 64, 0) (4).");
    assert_eq!(fails(&mut e, &mut w, &a, "//pos2 1,64,0"), "Position already set.");
    assert_eq!(run(&mut e, &mut w, &a, "//sel cuboid"), "Cuboid: left click for point 1, right click for point 2");
    // A distribution, paged.
    assert!(run(&mut e, &mut w, &a, "//distr").starts_with("Block Distribution\nTotal Block Count: 4\n"));
    assert_eq!(fails(&mut e, &mut w, &a, "//distr -p 9"), "Invalid page number");
    assert_eq!(fails(&mut e, &mut w, &a, "//set stone,%"), "Block name '%' was not recognized.");
    assert_eq!(fails(&mut e, &mut w, &a, "//count nonsense"), "No match for 'nonsense'.");
}

/// Horizon's `usage_nodes` / `command_tree` rules (game/native_server.rs), replayed: a usage Horizon
/// would leave out never reaches a player's tab completion. Returns the reasons.
fn tree_problems(usages: &[String], vanilla: &[&str]) -> Vec<String> {
    use std::collections::HashMap;
    // (kind of the node at a path) per full path of keys.
    let mut tree: HashMap<String, String> = HashMap::new();
    let mut problems = Vec::new();
    'usage: for usage in usages {
        let (mut optional_seen, mut greedy_seen) = (false, false);
        let mut path = String::new();
        let mut first = true;
        for word in usage.split_whitespace() {
            if greedy_seen {
                problems.push(format!("`{usage}`: text must be last"));
                continue 'usage;
            }
            let (key, kind) = if let Some(w) = word.strip_prefix('<') {
                let Some((n, k)) = w.strip_suffix('>').and_then(|w| w.split_once(':')) else {
                    problems.push(format!("`{usage}`: bad `{word}`"));
                    continue 'usage;
                };
                if optional_seen {
                    problems.push(format!("`{usage}`: `{word}` follows an optional argument"));
                    continue 'usage;
                }
                (n.to_owned(), format!("arg:{}", if k == "text" { "greedy" } else { k }))
            } else if let Some(w) = word.strip_prefix('[') {
                let Some((n, k)) = w.strip_suffix(']').and_then(|w| w.split_once(':')) else {
                    problems.push(format!("`{usage}`: bad `{word}`"));
                    continue 'usage;
                };
                optional_seen = true;
                (n.to_owned(), format!("arg:{}", if k == "text" { "greedy" } else { k }))
            } else {
                // The first word of a command may start with slashes (`/set`, typed `//set`).
                let body = if first { word.trim_start_matches('/') } else { word };
                let literal = !body.is_empty() && body.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c));
                if !literal || optional_seen {
                    problems.push(format!("`{usage}`: `{word}` is not a command word (or follows an optional argument)"));
                    continue 'usage;
                }
                (word.to_owned(), "literal".to_owned())
            };
            if first && vanilla.contains(&key.as_str()) {
                problems.push(format!("`{usage}`: vanilla has /{key}"));
                continue 'usage;
            }
            first = false;
            greedy_seen |= kind == "arg:greedy";
            path = format!("{path}/{key}");
            match tree.get(&path) {
                Some(k) if *k != kind => {
                    problems.push(format!("`{usage}`: `{key}` is another kind in an earlier usage"));
                    continue 'usage;
                }
                Some(_) => {}
                None => {
                    tree.insert(path.clone(), kind);
                }
            }
        }
    }
    problems
}

/// Vanilla's root commands (26.x), which Horizon leaves a mod's command out for.
const VANILLA_ROOTS: &[&str] = &[
    "advancement", "attribute", "ban", "ban-ip", "banlist", "bossbar", "clear", "clone", "damage", "data", "datapack", "debug", "defaultgamemode",
    "deop", "dialog", "difficulty", "effect", "enchant", "execute", "experience", "xp", "fetchprofile", "fill", "fillbiome", "forceload", "function",
    "gamemode", "gamerule", "give", "help", "item", "jfr", "kick", "kill", "list", "locate", "loot", "me", "msg", "tell", "w", "op", "pardon",
    "pardon-ip", "particle", "perf", "place", "playsound", "publish", "random", "recipe", "reload", "return", "ride", "rotate", "save-all",
    "save-off", "save-on", "say", "schedule", "scoreboard", "seed", "setblock", "setidletimeout", "setworldspawn", "spawnpoint", "spectate",
    "spreadplayers", "stop", "stopsound", "stopwatch", "summon", "swing", "tag", "team", "teammsg", "tm", "teleport", "tp", "tellraw", "test", "tick",
    "time", "title", "transfer", "trigger", "version", "waypoint", "weather", "whitelist", "worldborder", "chase", "dialog",
];

#[test]
fn every_usage_reaches_the_command_tree() {
    let usages = crate::usages();
    let problems = tree_problems(&usages, VANILLA_ROOTS);
    assert!(problems.is_empty(), "{} usages would be left out of the tree:\n{}", problems.len(), problems.join("\n"));
    // Every command word has at least one usage, under the root WorldEdit registers it with.
    for (name, _) in crate::engine::COMMANDS {
        let root = |r: &str| usages.iter().any(|u| u.split(' ').next() == Some(r));
        let slashed = format!("/{name}");
        if crate::engine::SINGLE_SLASH.contains(name) {
            assert!(root(name) && !root(&slashed), "{name}");
        } else if crate::engine::BOTH_FORMS.contains(name) {
            assert!(root(name) && root(&slashed), "{name}");
        } else {
            assert!(root(&slashed) && !root(name), "{name}");
        }
    }
    for root in ["/set", "/pos1", "/wand", "/copy", "/sel", "/size"] {
        assert!(usages.iter().any(|u| u.split(' ').next() == Some(root)), "{root}");
    }
}

#[test]
fn wand_item_gets_its_namespace() {
    let (c, problem) = Config::parse("wand_item = \"Wooden_Axe\"\n");
    assert!(problem.is_none());
    assert_eq!(c.wand_item, "minecraft:wooden_axe");
    assert_eq!(Config::parse("wand_item = \"minecraft:stick\"\n").0.wand_item, "minecraft:stick");
}

// ---- the live test run's selection and shape bugs: a selection made with //pos1 and //pos2 is the
// one every command reads, whatever the argument form, and a fresh area is changed in full ----

#[test]
fn one_selection_for_every_command() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    a.position = [44.5, 65.0, 60.5];
    assert_eq!(run(&mut e, &mut w, &a, "//pos1 42,65,58"), "First position set to (42, 65, 58).");
    assert_eq!(run(&mut e, &mut w, &a, "//pos2 46 69 62"), "Second position set to (46, 69, 62) (125).");
    let size = run(&mut e, &mut w, &a, "//size");
    assert!(size.contains("Position 1: (42, 65, 58)") && size.contains("# of blocks: 125"), "{size}");
    assert_eq!(run(&mut e, &mut w, &a, "//set stone"), "Operation completed (125 blocks affected)");
    assert!(run(&mut e, &mut w, &a, "//count stone").contains("125"));
    assert_eq!(run(&mut e, &mut w, &a, "//faces glass"), "98 blocks have been changed.");
    assert_eq!(w.count("glass"), 98);
    assert_eq!(run(&mut e, &mut w, &a, "//stack 2 up"), "250 blocks changed. Undo with //undo");
    assert_eq!(run(&mut e, &mut w, &a, "//inset 1"), "Region inset.");
    assert_eq!(run(&mut e, &mut w, &a, "//outset 1"), "Region outset.");
    assert!(run(&mut e, &mut w, &a, "//size").contains("# of blocks:"));
}

#[test]
fn shapes_change_a_fresh_area() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    a.position = [44.5, 65.0, 60.5];
    let n = |t: String| -> usize { t.split(' ').next().unwrap().parse().unwrap() };
    // WorldEdit widens a radius by one half: 3 is a sphere of 179 blocks.
    let full = n(run(&mut e, &mut w, &a, "//sphere stone 3"));
    assert_eq!(full, 179);
    run(&mut e, &mut w, &a, "//undo");
    // -h hollows the sphere: the same shell as //hsphere makes.
    let hollow = n(run(&mut e, &mut w, &a, "//sphere -h stone 3"));
    assert!(hollow > 0 && hollow < full, "{hollow}");
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(n(run(&mut e, &mut w, &a, "//hsphere stone 3")), hollow);
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(n(run(&mut e, &mut w, &a, "//pyramid stone 3")), 25 + 9 + 1);
    run(&mut e, &mut w, &a, "//undo");
    assert!(n(run(&mut e, &mut w, &a, "//hpyramid glass 3")) > 0);
    run(&mut e, &mut w, &a, "//undo");
    assert!(n(run(&mut e, &mut w, &a, "//cone stone 3 4")) > 0);
    run(&mut e, &mut w, &a, "//undo");
    assert!(n(run(&mut e, &mut w, &a, "//hcyl stone 3 4")) > 0);
    run(&mut e, &mut w, &a, "//undo");
    run(&mut e, &mut w, &a, "//pos1 42,65,58");
    run(&mut e, &mut w, &a, "//pos2 46,69,62");
    assert_eq!(n(run(&mut e, &mut w, &a, "//line stone")), 5);
}

#[test]
fn schematics_never_print_a_path() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    let env = env("nopath");
    let dir = env.schematics_dir.display().to_string();
    let say = |e: &mut Engine, w: &mut FakeWorld, line: &str| {
        let r = e.command(w, &a, line, &env);
        let t = r.text();
        assert!(!t.contains(&dir) && !t.contains("/tmp"), "`{line}` printed a path: {t}");
        t
    };
    w.fill([0, 64, 0], [2, 66, 2], "stone");
    say(&mut e, &mut w, "//pos1 0,64,0");
    say(&mut e, &mut w, "//pos2 2,66,2");
    say(&mut e, &mut w, "//copy");
    assert_eq!(say(&mut e, &mut w, "//schem save house"), "house saved.");
    assert!(say(&mut e, &mut w, "//schem list").contains("[L] house.schem"));
    assert_eq!(say(&mut e, &mut w, "//schem load house"), "house loaded. Paste it with //paste");
    assert_eq!(say(&mut e, &mut w, "//schem delete house"), "house has been deleted.");
    assert_eq!(say(&mut e, &mut w, "//schem load nowhere"), "Schematic nowhere does not exist!");
}

// ---- other mods read and write the selection (WorldGuard's /rg define) ----

#[test]
fn selection_is_shared_with_other_mods() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(7);
    assert_eq!(e.selection_of(7), None);
    run(&mut e, &mut w, &a, "//pos1 5,64,5");
    assert_eq!(e.selection_of(7), None, "one corner is no selection");
    run(&mut e, &mut w, &a, "//pos2 1,70,9");
    assert_eq!(e.selection_of(7), Some(("minecraft:overworld".to_owned(), [5, 64, 5], [1, 70, 9])));
    e.select(7, "minecraft:the_nether", [0, 0, 0], [3, 3, 3]);
    assert_eq!(e.selection_of(7), Some(("minecraft:the_nether".to_owned(), [0, 0, 0], [3, 3, 3])));
    assert_eq!(e.take_outline_due(), vec![7]);
    assert!(e.take_outline_due().is_empty());
    // The same engine behind the module-level handle the `[api]` package calls.
    crate::with(|e| e.select(9, "minecraft:overworld", [1, 2, 3], [4, 5, 6]));
    assert_eq!(crate::with(|e| e.selection_of(9)), Some(("minecraft:overworld".to_owned(), [1, 2, 3], [4, 5, 6])));
}

// ---- the utility commands ----

#[test]
fn fill_and_fillr_fill_holes() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    // Radius 1, depth 1 on open air: the 3 x 3 layer round the block you stand in.
    assert_eq!(run(&mut e, &mut w, &a, "//fill stone 1"), "9 blocks have been filled.");
    run(&mut e, &mut w, &a, "//undo");
    // A pit: a floor and a ring of wall, 3 x 3 inside.
    w.fill([-2, 60, -2], [2, 60, 2], "stone");
    for (x, z) in (-2i32..=2).flat_map(|x| (-2i32..=2).map(move |z| (x, z))).filter(|&(x, z)| x.abs() == 2 || z.abs() == 2) {
        w.fill([x, 61, z], [x, 62, z], "stone");
    }
    let mut a = actor(1);
    a.position = [0.5, 62.0, 0.5];
    // Depth 2 goes down two layers, never out through the wall.
    assert_eq!(run(&mut e, &mut w, &a, "//fill gold_block 6 2"), "18 blocks have been filled.");
    assert_eq!(w.count("gold_block"), 18);
    run(&mut e, &mut w, &a, "//undo");
    // Recursive: all the way down (no depth), and the radius keeps it in.
    assert_eq!(run(&mut e, &mut w, &a, "//fillr gold_block 6"), "18 blocks have been filled.");
    assert_eq!(w.name_at([3, 62, 0]), "minecraft:air");
    // Bad input.
    assert!(fails(&mut e, &mut w, &a, "//fillr gold_block").contains("Usage"));
    assert_eq!(fails(&mut e, &mut w, &a, "//fill stone x"), "Number expected; string \"x\" given.");
}

#[test]
fn vanilla_fill_is_left_alone() {
    // `//fill stone 5` reaches the hook as `fill stone 5`; vanilla's `/fill` has seven words or more.
    assert!(crate::is_shadowed("fill stone 5"));
    assert!(crate::is_shadowed("//fill stone 5 2"));
    assert!(!crate::is_shadowed("fill 0 0 0 5 5 5 stone"));
    assert!(!crate::is_shadowed("fill stone"));
    assert!(crate::is_shadowed("help set") && crate::is_shadowed("help -p 2") && crate::is_shadowed("rotate 90"));
    assert!(!crate::is_shadowed("help") && !crate::is_shadowed("help tp"));
}

#[test]
fn drain_and_fix_liquids() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    w.fill([-3, 60, -3], [3, 60, 3], "stone");
    w.fill([-2, 61, -2], [2, 62, 2], "water");
    a.position = [0.5, 62.0, 0.5];
    assert_eq!(run(&mut e, &mut w, &a, "//drain 10"), "50 blocks have been drained.");
    assert_eq!(w.count("water"), 0);
    run(&mut e, &mut w, &a, "//undo");
    // Radius 1: the pool is drained within reach of a sphere, not beyond.
    let n: usize = run(&mut e, &mut w, &a, "//drain 1").split(' ').next().unwrap().parse().unwrap();
    assert!(n > 0 && n < 50, "{n}");
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(run(&mut e, &mut w, &a, "//drain -w 10"), "50 blocks have been drained.");
    // A single water block spreads sideways over the floor, never up, never into stone.
    run(&mut e, &mut w, &a, "//undo");
    run(&mut e, &mut w, &a, "//drain 10");
    w.fill([0, 61, 0], [0, 61, 0], "water");
    a.position = [0.5, 61.0, 0.5];
    // Radius 2: the 21 columns of the flat circle of radius 2.5, minus the water that is already there.
    assert_eq!(run(&mut e, &mut w, &a, "//fixwater 2"), "20 blocks have been fixed.");
    assert_eq!(w.count("water"), 21);
    assert_eq!(run(&mut e, &mut w, &a, "//fixlava 2"), "0 blocks have been fixed.");
    assert_eq!(fails(&mut e, &mut w, &a, "//drain"), "Usage: //drain [-w] <radius>");
}

#[test]
fn snow_thaw_green_extinguish() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    w.fill([-4, 60, -4], [4, 60, 4], "stone");
    a.position = [0.5, 61.0, 0.5];
    // The 21 columns of the circle of radius 2.5 get a layer on top of the stone.
    assert_eq!(run(&mut e, &mut w, &a, "//snow 2 5"), "21 surfaces have been covered.");
    assert_eq!(w.name_at([0, 61, 0]), "minecraft:snow[layers=1]");
    assert_eq!(w.name_at([3, 61, 0]), "minecraft:air");
    // Again: the layer is no ground, so nothing new; with -s the layers stack.
    assert_eq!(run(&mut e, &mut w, &a, "//snow 2 5"), "0 surfaces have been covered.");
    assert_eq!(run(&mut e, &mut w, &a, "//snow -s 2 5"), "21 surfaces have been covered.");
    assert_eq!(w.name_at([0, 61, 0]), "minecraft:snow[layers=2]");
    // Thaw takes the snow off the top block of each column.
    assert_eq!(run(&mut e, &mut w, &a, "//thaw 2 5"), "13 blocks have been thawed.");
    assert_eq!(w.count("snow_block"), 0);
    assert_eq!(w.name_at([0, 61, 0]), "minecraft:air");
    // Ice on top of water becomes water.
    w.fill([0, 61, 0], [1, 61, 0], "ice");
    // (and the snow of the ring that radius 2 left)
    assert_eq!(run(&mut e, &mut w, &a, "//thaw 3"), "10 blocks have been thawed.");
    assert_eq!(w.name_at([1, 61, 0]), "minecraft:water[level=0]");
    // Still water freezes, the stone next to it gets a layer.
    assert_eq!(run(&mut e, &mut w, &a, "//snow 1 5"), "9 surfaces have been covered.");
    assert_eq!(w.count("ice"), 2);
    // Green: dirt in the open becomes grass, dirt under stone stays; coarse dirt needs -f.
    w.fill([2, 61, 2], [2, 61, 2], "dirt");
    w.fill([-2, 61, -2], [-2, 62, -2], "dirt");
    w.fill([-2, 63, -2], [-2, 63, -2], "stone");
    w.fill([2, 61, -2], [2, 61, -2], "coarse_dirt");
    assert_eq!(run(&mut e, &mut w, &a, "//green 4 5"), "1 blocks have been greened.");
    assert_eq!(run(&mut e, &mut w, &a, "//green -f 4 5"), "1 blocks have been greened.");
    assert_eq!(w.name_at([-2, 62, -2]), "minecraft:dirt");
    // Fire in the cube round you goes; further away stays.
    w.fill([1, 62, 1], [2, 62, 1], "fire");
    w.fill([30, 62, 1], [30, 62, 1], "fire");
    assert_eq!(run(&mut e, &mut w, &a, "//extinguish 5"), "2 fires have been extinguished.");
    assert_eq!(w.count("fire"), 1);
    assert_eq!(run(&mut e, &mut w, &a, "//ex"), "1 fires have been extinguished.");
    assert_eq!(w.count("fire"), 0);
}

#[test]
fn limit_gmask_and_toggleplace() {
    let mut e = Engine::new(Config { max_blocks: 100, ..Config::default() });
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    a.permission = 2;
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 9,64,9");
    assert_eq!(run(&mut e, &mut w, &a, "//limit"), "Block change limit set to 100.");
    assert_eq!(run(&mut e, &mut w, &a, "//limit 50"), "Block change limit set to 50. (Use //limit to go back to the default.)");
    assert!(fails(&mut e, &mut w, &a, "//set stone").contains("(50)"));
    assert_eq!(fails(&mut e, &mut w, &a, "//limit 500"), "Your maximum allowable limit is 100.");
    assert_eq!(fails(&mut e, &mut w, &a, "//limit -1"), "Your maximum allowable limit is 100.");
    run(&mut e, &mut w, &a, "//limit");
    assert_eq!(run(&mut e, &mut w, &a, "//set stone"), "Operation completed (100 blocks affected)");
    // An operator of the node's level may lift it.
    let mut e = Engine::new(Config { max_blocks: 100, node_levels: [("worldedit.limit.unrestricted".to_string(), 2u8)].into(), ..Config::default() });
    assert_eq!(run(&mut e, &mut w, &a, "//limit -1"), "Block change limit set to -1. (Use //limit to go back to the default.)");
    // Global mask: only stone is changed, by every edit.
    let mut e = Engine::default();
    w.fill([0, 64, 0], [9, 64, 9], "dirt");
    w.fill([0, 64, 0], [4, 64, 9], "stone");
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 9,64,9");
    assert_eq!(run(&mut e, &mut w, &a, "//gmask stone"), "Global mask set.");
    assert_eq!(run(&mut e, &mut w, &a, "//set gold_block"), "Operation completed (50 blocks affected)");
    assert_eq!(w.count("dirt"), 50);
    assert_eq!(run(&mut e, &mut w, &a, "//gmask"), "Global mask disabled.");
    assert_eq!(run(&mut e, &mut w, &a, "//set gold_block"), "Operation completed (50 blocks affected)");
    assert_eq!(w.count("gold_block"), 100);
    assert!(fails(&mut e, &mut w, &a, "//gmask nonsense").contains("No match"));
    // Toggleplace: shapes are made at position 1.
    run(&mut e, &mut w, &a, "//pos1 20,70,20");
    assert_eq!(run(&mut e, &mut w, &a, "//toggleplace"), "Now placing at pos #1.");
    run(&mut e, &mut w, &a, "//pyramid sand 2");
    assert_eq!(w.name_at([20, 70, 20]), "minecraft:sand");
    assert_eq!(run(&mut e, &mut w, &a, "//toggleplace"), "Now placing at the block you stand in.");
    run(&mut e, &mut w, &a, "//pyramid dirt 1");
    assert_eq!(w.name_at([0, 64, 0]), "minecraft:dirt");
}

#[test]
fn searchitem_and_calc() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    assert_eq!(fails(&mut e, &mut w, &a, "//searchitem st"), "Enter a longer search string (len > 2).");
    assert_eq!(fails(&mut e, &mut w, &a, "//searchitem -b -i stone"), "You cannot use both the 'b' and 'i' flags simultaneously.");
    let r = run(&mut e, &mut w, &a, "//searchitem stair");
    assert!(r.contains("Search results for 'stair'") && r.contains("Oak Stairs (minecraft:oak_stairs)"), "{r}");
    assert_eq!(run(&mut e, &mut w, &a, "//search zzzz"), "No results found.");
    assert_eq!(fails(&mut e, &mut w, &a, "//searchitem -p 9 stone"), "Invalid page number");
    // Two plus two, printed as WorldEdit does (the words as a list).
    assert_eq!(run(&mut e, &mut w, &a, "//calc 2+2"), "[2+2] = 4");
    assert_eq!(run(&mut e, &mut w, &a, "//calc 1000 * 1000 / 3"), "[1000, *, 1000, /, 3] = 333,333.33333");
    assert_eq!(run(&mut e, &mut w, &a, "//eval sqrt(16)"), "[sqrt(16)] = 4");
    assert!(fails(&mut e, &mut w, &a, "//calc 2 +").starts_with("'2 +' could not be parsed as a valid expression"));
    assert!(fails(&mut e, &mut w, &a, "//calc").contains("Usage"));
}

#[test]
fn help_and_worldedit_subcommands() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(1);
    // `/we help` lists the commands, eight a page, sorted; a command gets its description and usage.
    let page1 = run(&mut e, &mut w, &a, "/we help");
    assert!(page1.starts_with("---") && page1.contains("Help") && page1.contains("Page 1 of "), "{page1}");
    assert_eq!(page1.lines().count(), 1 + 8 + 1);
    assert!(run(&mut e, &mut w, &a, "/we help -p 2").contains("Page 2 of "));
    assert_eq!(fails(&mut e, &mut w, &a, "/we help -p 99"), "Invalid page number");
    let set = run(&mut e, &mut w, &a, "help set");
    assert!(set.contains("Help for //set") && set.contains("Sets all the blocks in the region") && set.contains("Usage: //set <pattern>"), "{set}");
    assert!(run(&mut e, &mut w, &a, "help -s schem").contains("load: Load a schematic into your clipboard"));
    assert!(fails(&mut e, &mut w, &a, "help nonsense").contains("The command 'nonsense' could not be found."));
    assert!(fails(&mut e, &mut w, &a, "help schem nonsense").contains("The sub-command 'nonsense' under '/schematic' could not be found."));
    // Sub-commands of /we.
    let v = run(&mut e, &mut w, &a, "/we version");
    assert!(v.starts_with("WorldEdit version ") && v.contains("Platforms") && v.contains("Capabilities"), "{v}");
    assert_eq!(run(&mut e, &mut w, &a, "/worldedit trace"), "Trace mode now active.");
    assert_eq!(fails(&mut e, &mut w, &a, "/worldedit trace active"), "Trace mode already active.");
    assert_eq!(run(&mut e, &mut w, &a, "/we trace inactive"), "Trace mode now inactive.");
    assert_eq!(run(&mut e, &mut w, &a, "/we cui"), "");
    assert_eq!(fails(&mut e, &mut w, &a, "/we selector"), "The sub-command 'selector' under 'worldedit' could not be found.");
    assert_eq!(fails(&mut e, &mut w, &a, "/we limit"), "The sub-command 'limit' under 'worldedit' could not be found.");
    let tz = run(&mut e, &mut w, &a, "/we tz +01:00");
    assert!(tz.starts_with("Timezone set for this session to: +01:00\nThe current time in that timezone is: 1970-01-01 01:00:00"), "{tz}");
    assert_eq!(fails(&mut e, &mut w, &a, "/we tz bogus"), "Invalid timezone");
    // reload reads the config file; report writes a file and names it without its folder path.
    let env = env("reload");
    std::fs::write(&env.config_file, "max_blocks = 77\n").unwrap();
    let r = e.command(&mut w, &a, "/we reload", &env);
    assert_eq!(r.text(), "Configuration reloaded!");
    assert_eq!(e.config.max_blocks, 77);
    std::fs::write(&env.config_file, "max_blocks = \"x\"").unwrap();
    assert!(e.command(&mut w, &a, "/we reload", &env).lines[0].error);
    assert_eq!(e.config.max_blocks, 77);
    let r = e.command(&mut w, &a, "/we report", &env).text();
    assert!(r.starts_with("WorldEdit report written to ") && r.ends_with("report.txt") && !r.contains("/tmp"), "{r}");
}

#[test]
fn every_help_entry_is_a_command_of_ours() {
    // The help lists only what this server answers: each entry's names register a usage.
    let usages = crate::usages();
    for e in crate::engine::help_entries() {
        let name = e.name.trim_start_matches('/');
        // `//fill`, `//help` and `//rotate` share their names with vanilla's: heard by `is_shadowed`.
        if matches!(name, "fill" | "help" | "rotate") {
            continue;
        }
        let root = |r: &str| usages.iter().any(|u| u.split(' ').next() == Some(r));
        assert!(root(&format!("/{name}")) || root(name), "`{}` has no usage", e.name);
    }
}

// ---- tools ----

#[test]
fn tools_bind_and_click() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(1);
    a.held = Some("minecraft:stick".into());
    assert_eq!(run(&mut e, &mut w, &a, "/tool selwand"), "Selection wand bound to Stick.\nRun /tool none while holding the item to unbind it.");
    // Left click is position 1, right click position 2, on any item.
    assert_eq!(e.tool_click(&mut w, &a, true, [1, 64, 1], 0).unwrap().text(), "First position set to (1, 64, 1).");
    assert_eq!(e.tool_click(&mut w, &a, false, [3, 64, 3], 1000).unwrap().text(), "Second position set to (3, 64, 3) (9).");
    assert_eq!(e.selection_of(1), Some(("minecraft:overworld".to_owned(), [1, 64, 1], [3, 64, 3])));
    assert_eq!(run(&mut e, &mut w, &a, "/tool none"), "Tool unbound from your current item.");
    assert!(e.tool_click(&mut w, &a, true, [1, 64, 1], 5000).is_none());
    assert_eq!(run(&mut e, &mut w, &a, "/tool none"), "Your current item is not bound.");
    // Info.
    w.fill([5, 64, 5], [5, 64, 5], "oak_stairs[facing=east,half=bottom]");
    run(&mut e, &mut w, &a, "/tool info");
    assert_eq!(e.tool_click(&mut w, &a, false, [5, 64, 5], 6000).unwrap().text(), "@(5, 64, 5): Oak Stairs (minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false])");
    assert!(e.tool_click(&mut w, &a, true, [5, 64, 5], 7000).is_none(), "the left click digs");
    // Replacer: right click places, left click takes the block as the pattern.
    run(&mut e, &mut w, &a, "/tool repl gold_block");
    assert!(e.tool_click(&mut w, &a, false, [6, 64, 6], 8000).unwrap().lines.is_empty());
    assert_eq!(w.name_at([6, 64, 6]), "minecraft:gold_block");
    w.fill([7, 64, 7], [7, 64, 7], "dirt");
    assert_eq!(e.tool_click(&mut w, &a, true, [7, 64, 7], 9000).unwrap().text(), "Replacer tool switched to: Dirt");
    e.tool_click(&mut w, &a, false, [8, 64, 8], 10_000);
    assert_eq!(w.name_at([8, 64, 8]), "minecraft:dirt");
    run(&mut e, &mut w, &a, "//undo");
    assert_eq!(w.name_at([8, 64, 8]), "minecraft:air");
    // Cycler: left click picks the next property, right click the next value.
    run(&mut e, &mut w, &a, "/tool cycler");
    assert_eq!(e.tool_click(&mut w, &a, false, [5, 64, 5], 11_000).unwrap().text(), "Value of facing is now south.");
    assert!(w.name_at([5, 64, 5]).contains("facing=south"));
    assert_eq!(e.tool_click(&mut w, &a, true, [5, 64, 5], 12_000).unwrap().text(), "Now cycling half.");
    assert_eq!(e.tool_click(&mut w, &a, false, [5, 64, 5], 13_000).unwrap().text(), "Value of half is now top.");
    assert_eq!(e.tool_click(&mut w, &a, false, [20, 64, 20], 14_000).unwrap().text(), "That block's data cannot be cycled!");
    // Flood fill: the connected blocks of the type within the range.
    w.fill([0, 70, 0], [4, 70, 0], "stone");
    run(&mut e, &mut w, &a, "/tool floodfill gold_block 3");
    e.tool_click(&mut w, &a, false, [0, 70, 0], 15_000);
    assert_eq!(w.count("gold_block"), 1 + 4, "the one before and the four within distance 3 of the corner");
    assert_eq!(fails(&mut e, &mut w, &a, "/tool floodfill gold_block 6"), "Maximum range is 5.");
    // A tool and a brush never share an item.
    run(&mut e, &mut w, &a, "/brush sphere stone 2");
    assert!(e.tool_click(&mut w, &a, false, [0, 70, 0], 20_000).is_none());
    // What this server cannot do says why.
    assert!(fails(&mut e, &mut w, &a, "/tool tree").contains("not available"));
    assert_eq!(fails(&mut e, &mut w, &a, "/tool bogus"), "The sub-command 'bogus' under 'tool' could not be found.");
    a.held = None;
    assert_eq!(fails(&mut e, &mut w, &a, "/tool info"), "Can't bind tool to Air: Blocks can't be used.");
}

#[test]
fn only_operators_and_granted_nodes_get_worldedit() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let mut a = actor(7);
    a.permission = 0;
    // No node: refused everything, the wand and the commands without a node included.
    assert!(fails(&mut e, &mut w, &a, "//pos1").contains("not permitted"));
    assert!(fails(&mut e, &mut w, &a, "wand").contains("not permitted"));
    assert!(fails(&mut e, &mut w, &a, "//version").contains("not permitted"));
    // The selection node gives the selection, and not a change of the region.
    a.granted.insert("worldedit.selection.pos".into());
    run(&mut e, &mut w, &a, "//pos1 0,64,0");
    run(&mut e, &mut w, &a, "//pos2 1 64 1");
    assert!(fails(&mut e, &mut w, &a, "//set stone").contains("not permitted"));
    assert_eq!(w.count("stone"), 0);
    a.granted.insert("worldedit.region.set".into());
    assert_eq!(run(&mut e, &mut w, &a, "//set stone"), "Operation completed (4 blocks affected)");
    // Creative only still applies to a granted player.
    let mut e = Engine::new(Config { creative_only: true, ..Config::default() });
    a.game_mode = 0;
    assert!(fails(&mut e, &mut w, &a, "//pos1 0,64,0").contains("creative"));
}

#[test]
fn history_expires_after_its_minutes() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let a = actor(8);
    let at = |ms: i64| Env { now_ms: ms, ..env("history") };
    let minute = 60_000;
    e.command(&mut w, &a, "//pos1 0,64,0", &at(0));
    e.command(&mut w, &a, "//pos2 0,64,0", &at(0));
    // Nine minutes after the edit it can still be undone (and redone).
    e.command(&mut w, &a, "//set stone", &at(minute));
    let r = e.command(&mut w, &a, "//undo", &at(10 * minute));
    assert!(r.lines.iter().all(|l| !l.error), "{}", r.text());
    assert_eq!(w.count("stone"), 0);
    // An edit at minute 20, undone at minute 31: forgotten (expiration 10 minutes).
    e.command(&mut w, &a, "//set dirt", &at(20 * minute));
    let r = e.command(&mut w, &a, "//undo", &at(31 * minute));
    assert!(r.text().contains("Nothing left to undo"), "{}", r.text());
    assert_eq!(w.count("dirt"), 1);
}

#[test]
fn navigation_wand_teleports_to_the_block_in_sight() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    w.fill([-2, 63, -2], [2, 63, 2], "stone");
    let mut a = actor(9);
    a.position = [0.5, 64.0, 0.5];
    a.pitch = 90.0;
    a.held = Some("minecraft:compass".into());
    let r = e.nav_use(&w, &a, 0).expect("the compass is the navigation wand");
    assert_eq!(r.teleport, Some([0.5, 64.0, 0.5]));
    // Another item is left to the game.
    a.held = Some("minecraft:stone".into());
    assert!(e.nav_use(&w, &a, 0).is_none());
    // Without the node, and not an operator: refused.
    a.held = Some("minecraft:compass".into());
    a.permission = 0;
    assert!(e.nav_use(&w, &a, 0).unwrap().text().contains("not permitted"));
    a.granted.insert("worldedit.navigation.jumpto.tool".into());
    assert_eq!(e.nav_use(&w, &a, 0).unwrap().teleport, Some([0.5, 64.0, 0.5]));
}

#[test]
fn players_with_server_cui_off_get_no_outline() {
    let mut e = Engine::default();
    let mut w = FakeWorld::new();
    let (a, b) = (actor(1), actor(2));
    for p in [&a, &b] {
        run(&mut e, &mut w, p, "//pos1 0,64,0");
        run(&mut e, &mut w, p, "//pos2 1 64 1");
    }
    e.set_cui_off([1u128].into());
    assert!(e.outline_of(1).is_none());
    assert!(e.outline_of(2).is_some());
}

