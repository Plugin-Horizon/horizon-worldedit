//! WorldEdit's navigation commands: `//up`, `/ascend`, `/descend`, `/thru`, `/jumpto`, `/unstuck`, `/ceil`.
//! The searches follow WorldEdit's `AbstractPlayerActor` (`findFreePosition`, `ascendLevel`,
//! `descendLevel`, `ascendToCeiling`, `passThroughForwardWall`, `floatAt`). A move is the reply's
//! `teleport` (feet position); the glass platform of `//up` and `/ceil` is written at once.

use super::*;
use crate::horizon_worldedit_core::block::BlockState;

/// Whether a player walks through the block at `p` (WorldEdit: not a movement blocker). Above the
/// build height is open sky; an unloaded chunk or the void below the world stops a search.
pub(super) fn passable(world: &dyn World, p: Pos) -> bool {
    match world.get(p) {
        Some(s) => passes(&world.describe(s)),
        None => p[1] > world.max_y(),
    }
}

/// The blocks without a collision box that this server knows by name, and air, water and lava.
fn passes(b: &BlockState) -> bool {
    if b.is_air() {
        return true;
    }
    let n = b.name.strip_prefix("minecraft:").unwrap_or(&b.name);
    if n == "snow" {
        return b.properties.get("layers").is_some_and(|l| l == "1");
    }
    matches!(
        n,
        "water" | "lava" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "dead_bush" | "seagrass" | "tall_seagrass" | "kelp" | "kelp_plant" | "vine"
            | "glow_lichen" | "sculk_vein" | "torch" | "wall_torch" | "redstone_torch" | "redstone_wall_torch" | "soul_torch" | "soul_wall_torch" | "lever"
            | "ladder" | "fire" | "soul_fire" | "light" | "rail" | "powered_rail" | "detector_rail" | "activator_rail" | "redstone_wire" | "tripwire"
            | "tripwire_hook" | "brown_mushroom" | "red_mushroom" | "poppy" | "dandelion" | "cornflower" | "lily_of_the_valley" | "azure_bluet" | "oxeye_daisy"
            | "blue_orchid" | "allium" | "wither_rose" | "torchflower" | "sugar_cane" | "sweet_berry_bush"
    ) || ["_button", "_sign", "_wall_sign", "_hanging_sign", "_pressure_plate", "_tulip", "_sapling", "_banner", "_wall_banner", "_carpet_moss"].iter().any(|s| n.ends_with(s))
}

fn is_lava(world: &dyn World, p: Pos) -> bool {
    world.get(p).is_some_and(|s| world.describe(s).name.contains("lava"))
}

/// WorldEdit's `findFreePosition`: from the block `from` upward, the first spot with two free blocks
/// (feet and head); its feet block. Equal to `from` when the player already has room.
pub(super) fn find_free(world: &dyn World, from: Pos) -> Option<Pos> {
    let top = world.max_y() + 2;
    let mut y = from[1].max(world.min_y());
    let mut free = 0;
    while y <= top {
        if passable(world, [from[0], y, from[2]]) {
            free += 1;
        } else {
            free = 0;
        }
        if free == 2 {
            return Some([from[0], y - 1, from[2]]);
        }
        y += 1;
    }
    None
}

/// `ascendLevel`: the feet block of the next floor above, or `None`.
fn ascend_level(world: &dyn World, at: Pos) -> Option<Pos> {
    let (x, z) = (at[0], at[2]);
    let top = world.max_y() + 2;
    let mut y = at[1].max(world.min_y());
    // Out of the solid block the player is in.
    while y <= top && !passable(world, [x, y, z]) {
        y += 1;
    }
    let (mut free, mut spots) = (0, 0);
    while y <= top {
        if passable(world, [x, y, z]) {
            free += 1;
        } else {
            free = 0;
        }
        if free == 2 {
            spots += 1;
            if spots == 2 {
                // Never into lava.
                if is_lava(world, [x, y - 2, z]) {
                    return None;
                }
                return Some([x, y - 1, z]);
            }
        }
        y += 1;
    }
    None
}

/// `descendLevel`: the feet block of the next floor below, or `None`.
fn descend_level(world: &dyn World, at: Pos) -> Option<Pos> {
    let (x, z) = (at[0], at[2]);
    let floor = world.min_y();
    let mut y = at[1];
    if y <= floor {
        return None;
    }
    let mut free = 0;
    while y > floor {
        if passable(world, [x, y, z]) {
            free += 1;
        } else {
            free = 0;
        }
        if free == 2 {
            // A spot with room: drop to what is under it (not air, not lava).
            let mut py = y;
            while py >= floor {
                if let Some(s) = world.get([x, py, z]) {
                    let b = world.describe(s);
                    if !b.is_air() && !b.name.contains("lava") {
                        return Some([x, py + 1, z]);
                    }
                }
                py -= 1;
            }
            return None;
        }
        y -= 1;
    }
    None
}

/// The distinct blocks along the ray from `eye`, in steps of 0.2, up to `range` blocks (WorldEdit's `TargetBlock`).
fn ray_blocks(eye: [f64; 3], dir: [f64; 3], range: f64) -> Vec<Pos> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let mut out: Vec<Pos> = Vec::new();
    if len == 0.0 {
        return out;
    }
    let mut t = 0.0;
    while t <= range {
        let cell: Pos = [0, 1, 2].map(|a| (eye[a] + dir[a] / len * t).floor() as i32);
        if out.last() != Some(&cell) {
            out.push(cell);
        }
        t += 0.2;
    }
    out
}

/// `passThroughForwardWall`: the block to stand under on the far side of the wall ahead.
pub(super) fn pass_through(world: &dyn World, actor: &Actor, range: f64) -> Option<Pos> {
    let blocks = ray_blocks(actor.eye(), ops::look_vector(actor.yaw, actor.pitch), range);
    let (mut first, mut free_to_find, mut in_free, mut searched) = (true, 2, false, 0);
    for block in blocks {
        let free = passable(world, block);
        if first {
            first = false;
            if !free {
                free_to_find -= 1;
                continue;
            }
        }
        searched += 1;
        if searched > 20 {
            return None;
        }
        if in_free != free && free {
            free_to_find -= 1;
        }
        if free_to_find == 0 {
            // `setOnGround`: down to the first solid block, and stand on it.
            let mut y = block[1].max(world.min_y());
            while y >= world.min_y() {
                if !passable(world, [block[0], y, block[2]]) {
                    return Some([block[0], y + 1, block[2]]);
                }
                y -= 1;
            }
            return None;
        }
        in_free = free;
    }
    None
}

pub(super) fn center(p: Pos) -> [f64; 3] {
    [f64::from(p[0]) + 0.5, f64::from(p[1]), f64::from(p[2]) + 0.5]
}

impl Engine {
    /// `floatAt`: glass under the feet when the block there lets the player fall (or always with `-g`).
    fn float_at(&mut self, world: &mut dyn World, p: Pos, always_glass: bool) -> Reply {
        let below = [p[0], p[1] - 1, p[2]];
        if always_glass || !world.get(below).is_some_and(|s| !passes(&world.describe(s))) {
            if let Some(glass) = world.resolve(&BlockState::new("minecraft:glass", &[])) {
                world.write(&[(below, glass)]);
            }
        }
        let mut reply = Reply::default();
        reply.teleport = Some(center(p));
        reply
    }

    /// The navigation commands: `up`, `ascend`, `descend`, `thru`, `jumpto`, `unstuck`, `ceil` (aliases already
    /// resolved). `Err` is an error line; the reply carries the message and the teleport.
    pub(super) fn navigate(&mut self, world: &mut dyn World, actor: &Actor, name: &str, args: &Args) -> Result<Reply, String> {
        let here = actor.block();
        let say = |mut r: Reply, text: &str| {
            r.lines.push(Line { text: text.to_owned(), error: false });
            r
        };
        match name {
            "unstuck" => {
                let mut r = Reply::default();
                if let Some(feet) = find_free(world, here) {
                    if feet[1] != here[1] {
                        r.teleport = Some(center(feet));
                    }
                }
                Ok(say(r, "There you go!"))
            }
            "ascend" | "descend" => {
                let up = name == "ascend";
                let levels = args.int(0, 1)?.max(1);
                let (mut at, mut moved) = (here, 0);
                for _ in 0..levels {
                    let next = if up { ascend_level(world, at) } else { descend_level(world, at) };
                    match next {
                        Some(p) => {
                            at = p;
                            moved += 1;
                        }
                        None => break,
                    }
                }
                if moved == 0 {
                    return Err(format!("No free spot {} you found.", if up { "above" } else { "below" }));
                }
                let mut r = Reply::default();
                r.teleport = Some(center(at));
                let verb = if up { "Ascended" } else { "Descended" };
                Ok(say(r, &if moved == 1 { format!("{verb} a level.") } else { format!("{verb} {moved} levels.") }))
            }
            "thru" => match pass_through(world, actor, 6.0) {
                Some(p) => {
                    let mut r = Reply::default();
                    r.teleport = Some(center(p));
                    Ok(say(r, "Whoosh!"))
                }
                None => Err("No free spot ahead of you found.".into()),
            },
            "jumpto" => {
                let Some(hit) = ops::raycast(world, actor.eye(), ops::look_vector(actor.yaw, actor.pitch), 300.0) else {
                    return Err("No block in sight (or too far away)!".into());
                };
                let mut r = Reply::default();
                if let Some(feet) = find_free(world, hit) {
                    r.teleport = Some(center(feet));
                }
                Ok(say(r, "Poof!"))
            }
            "up" => {
                let Some(word) = args.get(0) else { return Err(usage("up")) };
                let distance: i32 = word.parse().map_err(|_| number_error(word))?;
                // -f (keep flying) needs a player-ability hook this server lacks: it is accepted and changes nothing.
                let y = (here[1] + distance).clamp(world.min_y(), world.max_y());
                let r = self.float_at(world, [here[0], y, here[2]], args.flag('g'));
                Ok(say(r, "Whoosh!"))
            }
            "ceil" => {
                let clearance = args.int(0, 0)?.max(0);
                let initial_y = here[1].max(world.min_y());
                let mut y = initial_y + 2;
                let open_above = world.get([here[0], y, here[2]]).is_some_and(|s| world.is_air(s));
                if open_above {
                    while y <= world.max_y() {
                        if !passable(world, [here[0], y, here[2]]) {
                            let platform = initial_y.max(y - 3 - clearance);
                            let r = self.float_at(world, [here[0], platform + 1, here[2]], args.flag('g'));
                            return Ok(say(r, "Whoosh!"));
                        }
                        y += 1;
                    }
                }
                Err("No free spot above you found.".into())
            }
            _ => Err(format!("The command '{name}' could not be found.")),
        }
    }
}

/// In `last_click`, the mark of the navigation wand's clicks: a right click or a left click on a block sets it, and the
/// arm swing that every click sends after it is then not taken for a left click in the air.
const NAV_MARK: Pos = [i32::MIN, 0, 0];

impl Engine {
    /// Whether a player with `held` in hand holds the navigation wand: the configured item, or an item bound with
    /// `/tool navwand`. Cheap, for the arm swing hook that hears every swing.
    pub fn holds_nav(&mut self, player: u128, held: Option<&str>) -> bool {
        let item = held.unwrap_or_default();
        matches!(self.sessions.get(player).tools.get(item), Some(Tool::NavWand)) || held == Some(self.config.nav_wand_item.as_str())
    }

    fn holds_nav_wand(&mut self, actor: &Actor) -> bool {
        self.holds_nav(actor.uuid, actor.held.as_deref())
    }

    /// Notes a click of the navigation wand that `nav_right_click` handled (a right click).
    pub(super) fn nav_clicked(&mut self, actor: &Actor, now_ms: i64) {
        self.last_click.insert(actor.uuid, (NAV_MARK, false, now_ms));
    }

    /// A left click with the navigation wand: the player jumps to the solid block in sight (`NavigationWand.actSecondary`,
    /// `getSolidBlockTrace` then `findFreePosition`). `air` is true for the arm swing of a click that hit no block; the swing
    /// that follows a click already handled is ignored. `None`: not the navigation wand, no `worldedit.navigation.jumpto.tool`,
    /// or the wand is switched off (`max-distance` 0): the click is left to the game.
    pub fn nav_left_click(&mut self, world: &dyn World, actor: &Actor, air: bool, now_ms: i64) -> Option<Reply> {
        if !self.holds_nav_wand(actor) || self.permitted(actor, Some("worldedit.navigation.jumpto.tool")).is_err() || self.allowed(actor).is_err() {
            return None;
        }
        let max = self.config.nav_wand_max_distance;
        if max <= 0.0 {
            return None;
        }
        self.now_ms = now_ms;
        let recent = self.last_click.get(&actor.uuid).is_some_and(|&(p, _, t)| p == NAV_MARK && now_ms - t < 150);
        if air && recent {
            return None;
        }
        self.last_click.insert(actor.uuid, (NAV_MARK, true, now_ms));
        let solid = |p: Pos| world.get(p).is_some() && !passable(world, p);
        let Some(hit) = super::tools::target_block(world, actor, max as i32, &solid, false) else {
            return Some(Reply::err("No block in sight (or too far away)!".into()));
        };
        let mut reply = Reply::default();
        if let Some(feet) = find_free(world, hit) {
            reply.teleport = Some(center(feet));
        }
        Some(reply)
    }

    /// A right click with the navigation wand: through the wall ahead (`NavigationWand.actPrimary`, `passThroughForwardWall` with
    /// the wand's range less ten). `None` when it is not the wand, `worldedit.navigation.thru.tool` is missing or the wand is off.
    pub fn nav_right_click(&mut self, world: &dyn World, actor: &Actor, now_ms: i64) -> Option<Reply> {
        self.now_ms = now_ms;
        if !self.holds_nav_wand(actor) || self.permitted(actor, Some("worldedit.navigation.thru.tool")).is_err() || self.allowed(actor).is_err() {
            return None;
        }
        let max = self.config.nav_wand_max_distance;
        if max <= 0.0 {
            return None;
        }
        self.nav_clicked(actor, now_ms);
        match pass_through(world, actor, (max - 10.0).max(1.0)) {
            Some(p) => {
                let mut reply = Reply::default();
                reply.teleport = Some(center(p));
                Some(reply)
            }
            None => Some(Reply::err("No free spot ahead of you found.".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    #[test]
    fn right_click_passes_through_and_left_click_jumps() {
        let mut e = Engine::default();
        let mut w = FakeWorld::new();
        w.fill([0, 63, 0], [0, 63, 6], "stone");
        w.fill([0, 64, 2], [0, 65, 2], "stone");
        let mut a = Actor { uuid: 4, name: "Builder".into(), permission: 4, dimension: "minecraft:overworld".into(), position: [0.5, 64.0, 0.5], ..Actor::default() };
        a.held = Some("minecraft:stone".into());
        assert!(e.nav_right_click(&w, &a, 1000).is_none());
        a.held = Some("minecraft:compass".into());
        // Right: through the wall at z = 2.
        assert_eq!(e.nav_right_click(&w, &a, 1000).unwrap().teleport, Some([0.5, 64.0, 3.5]));
        // The swing after a right click is not a left click.
        assert!(e.nav_left_click(&w, &a, true, 1050).is_none());
        // Left: to a free spot on the solid block in sight (looking down at the floor).
        a.pitch = 80.0;
        let r = e.nav_left_click(&w, &a, false, 5000).unwrap();
        assert!(r.teleport.is_some());
        // Nodes: thru for the right click, jumpto for the left.
        a.permission = 0;
        assert!(e.nav_right_click(&w, &a, 6000).is_none());
        assert!(e.nav_left_click(&w, &a, false, 6000).is_none());
        a.granted.insert("worldedit.navigation.thru.tool".into());
        assert!(e.nav_right_click(&w, &a, 7000).is_some());
        assert!(e.nav_left_click(&w, &a, false, 7000).is_none());
        a.granted.insert("worldedit.navigation.jumpto.tool".into());
        assert!(e.nav_left_click(&w, &a, false, 8000).is_some());
        e.config.nav_wand_max_distance = 0.0;
        assert!(e.nav_right_click(&w, &a, 9000).is_none());
    }
}
