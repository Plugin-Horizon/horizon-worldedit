//! The selection outline for clients with the WorldEditCUI mod: the messages WorldEdit sends on the `worldedit:cui`
//! plugin channel (`LocalSession.dispatchCUISelection` and every selector's `describeCUI` and `describeLegacyCUI`).
//! The client says hello with `v|<protocol version>`; from then on the player's selection is described to it
//! whenever it changes.
//!
//! The messages are `<type>|<argument>|...`: `s|<shape>` starts a selection of that shape, `p|<id>|<x>|<y>|<z>|<volume>`
//! is a point, `p2|<id>|<x>|<z>|<volume>` a polygon's point, `mm|<min y>|<max y>` the height of a polygon or a
//! cylinder, `e|<id>|<x>|<y>|<z>` the centre (id 0) and the radii (id 1) of an ellipsoid, `cyl|<x>|<y>|<z>|<rx>|<rz>`
//! a cylinder and `poly|<a>|<b>|<c>` a triangle of a polyhedron. A client of an older protocol gets the boxes only
//! (the shapes' `describeLegacyCUI`).

use super::*;
use crate::horizon_worldedit_core::selector::{hull_planes, SelKind, Shape};
use crate::horizon_worldedit_core::session::CuiClient;

/// The protocol version a client that announced itself with `/we cui` and no version is taken to speak
/// (`LocalSession.CUI_VERSION_UNINITIALIZED`): it gets boxes.
const UNINITIALIZED: i32 = -1;

fn point(id: usize, p: Pos, volume: i64) -> String {
    format!("p|{id}|{}|{}|{}|{volume}", p[0], p[1], p[2])
}

/// The two corner points of a box, as the older protocol shows any shape.
fn box_points(c: &Cuboid, volume: i64) -> Vec<String> {
    vec![point(0, c.min, volume), point(1, c.max, volume)]
}

/// The triangles of the hull of `points`, as indexes into it: each face is the points on one supporting plane,
/// ordered round their centre and fanned from the first.
fn triangles(points: &[Pos]) -> Vec<[usize; 3]> {
    let dot = |a: [i64; 3], b: [i64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let at = |i: usize| [i64::from(points[i][0]), i64::from(points[i][1]), i64::from(points[i][2])];
    let mut faces: Vec<Vec<usize>> = Vec::new();
    let mut out = Vec::new();
    for (normal, offset) in hull_planes(points) {
        let on: Vec<usize> = (0..points.len()).filter(|&i| dot(normal, at(i)) == offset).collect();
        if on.len() < 3 || faces.contains(&on) {
            continue;
        }
        faces.push(on.clone());
        let centre = [0, 1, 2].map(|a| on.iter().map(|&i| at(i)[a] as f64).sum::<f64>() / on.len() as f64);
        let n = normal.map(|v| v as f64);
        let first = [0, 1, 2].map(|a| at(on[0])[a] as f64 - centre[a]);
        // The second axis of the face's plane: the normal crossed with the first.
        let second = [n[1] * first[2] - n[2] * first[1], n[2] * first[0] - n[0] * first[2], n[0] * first[1] - n[1] * first[0]];
        let angle = |i: usize| {
            let d = [0, 1, 2].map(|a| at(i)[a] as f64 - centre[a]);
            let (x, y) = ((0..3).map(|a| d[a] * first[a]).sum::<f64>(), (0..3).map(|a| d[a] * second[a]).sum::<f64>());
            y.atan2(x)
        };
        let mut order = on;
        order.sort_by(|&a, &b| angle(a).total_cmp(&angle(b)));
        for k in 1..order.len() - 1 {
            out.push([order[0], order[k], order[k + 1]]);
        }
    }
    out
}

/// What a client of protocol `version` is told about a selection: the shape's start and its points, and the
/// corner boxes for a client too old for the shape.
pub fn describe(kind: SelKind, shape: Option<&Shape>, pos1: Option<Pos>, pos2: Option<Pos>, version: i32) -> Vec<String> {
    let mut out = Vec::new();
    match (kind, shape) {
        (SelKind::Cuboid | SelKind::Extend, _) => {
            out.push("s|cuboid".to_owned());
            let volume = match (pos1, pos2) {
                (Some(a), Some(b)) => Cuboid::new(a, b).volume() as i64,
                _ => -1,
            };
            out.extend(pos1.map(|p| point(0, p, volume)));
            out.extend(pos2.map(|p| point(1, p, volume)));
        }
        (SelKind::Poly | SelKind::PolyExtend, shape) => {
            out.push("s|polygon2d".to_owned());
            if let Some(Shape::Poly { points, min_y, max_y }) = shape {
                let volume = shape.map_or(0, |s| s.volume() as i64);
                for (i, p) in points.iter().enumerate() {
                    out.push(format!("p2|{i}|{}|{}|{volume}", p[0], p[1]));
                }
                out.push(format!("mm|{min_y}|{max_y}"));
            }
        }
        (SelKind::Ellipsoid | SelKind::Sphere, shape) => {
            if version >= 1 {
                out.push("s|ellipsoid".to_owned());
                if let Some(Shape::Ellipsoid { center, radius }) = shape {
                    out.push(format!("e|0|{}|{}|{}", center[0], center[1], center[2]));
                    out.push(format!("e|1|{}|{}|{}", radius[0].floor() as i64, radius[1].floor() as i64, radius[2].floor() as i64));
                }
            } else {
                out.push("s|cuboid".to_owned());
                if let Some(s) = shape {
                    out.extend(s.bounds().map(|b| box_points(&b, s.volume() as i64)).unwrap_or_default());
                }
            }
        }
        (SelKind::Cyl, shape) => {
            let defined = matches!(shape, Some(Shape::Cyl { radius, .. }) if radius[0] > 0.0 || radius[1] > 0.0);
            if version >= 1 {
                out.push("s|cylinder".to_owned());
                if let Some(Shape::Cyl { center, radius, min_y, max_y }) = shape {
                    let y = (min_y + max_y).div_euclid(2);
                    out.push(format!("cyl|{}|{y}|{}|{}|{}", center[0], center[1], java_double(radius[0]), java_double(radius[1])));
                    out.push(format!("mm|{min_y}|{max_y}"));
                }
            } else {
                out.push("s|cuboid".to_owned());
                if let (true, Some(s)) = (defined, shape) {
                    out.extend(s.bounds().map(|b| box_points(&b, s.volume() as i64)).unwrap_or_default());
                }
            }
        }
        (SelKind::Convex, shape) => {
            let points: &[Pos] = match shape {
                Some(Shape::Convex { points }) => points,
                _ => &[],
            };
            if version >= 3 {
                out.push("s|polyhedron".to_owned());
                let volume = shape.map_or(0, |s| s.volume() as i64);
                for (i, p) in points.iter().enumerate() {
                    out.push(point(i, *p, volume));
                }
                for [a, b, c] in triangles(points) {
                    out.push(format!("poly|{a}|{b}|{c}"));
                }
            } else {
                out.push("s|cuboid".to_owned());
                if let (true, Some(s)) = (points.len() >= 4, shape) {
                    out.extend(s.bounds().map(|b| box_points(&b, s.volume() as i64)).unwrap_or_default());
                }
            }
        }
    }
    out
}

/// The most blocks a structure block outlines along an axis (`ServerCUIHandler.getMaxServerCuiSize`).
pub const SERVER_CUI_MAX: i32 = 48;
/// How far (per axis) the outlined box may be from the structure block that draws it.
const SERVER_CUI_DISTANCE: i32 = 32;

/// The structure block that draws a cuboid selection for a player without the CUI mod
/// (`ServerCUIHandler.createStructureBlock`): where it stands, and the box it outlines as an offset and a size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureOutline {
    pub block: Pos,
    pub offset: Pos,
    pub size: Pos,
}

/// The structure block for `c` as seen by a player at `position` looking along `yaw` and `pitch`: 12 blocks in front
/// of them, at the selection's height (at most 3 above it, within the build height). None when the box is wider than
/// 48 blocks or too far from that block to be drawn.
pub fn structure_outline(c: &Cuboid, position: [f64; 3], yaw: f32, pitch: f32, min_y: i32, max_y: i32) -> Option<StructureOutline> {
    let size = c.size();
    if size.iter().any(|&n| n > SERVER_CUI_MAX) {
        return None;
    }
    let (rot_x, rot_y) = (f64::from(yaw), f64::from(pitch));
    let xz = rot_y.to_radians().cos();
    let x = (position[0] - (-xz * rot_x.to_radians().sin()) * 12.0) as i32;
    let z = (position[2] - (xz * rot_x.to_radians().cos()) * 12.0) as i32;
    let y = min_y.max((max_y.min(c.min[1] + SERVER_CUI_DISTANCE)).min(c.min[1] + 3));
    let offset = [c.min[0] - x, c.min[1] - y, c.min[2] - z];
    if offset.iter().any(|v| v.abs() > SERVER_CUI_DISTANCE) {
        return None;
    }
    Some(StructureOutline { block: [x, y, z], offset, size })
}

impl Engine {
    /// The structure block to show `player` (who has no CUI mod), if their selection is a cuboid it can draw: the level
    /// the selection is in, and the block. None for any other selection, a player with the CUI mod or with the server
    /// outline off (`useServerCUI`, `server-side-cui`), and a selection that is not in `dimension`.
    pub fn structure_outline_for(&mut self, player: u128, dimension: &str, position: [f64; 3], yaw: f32, pitch: f32, min_y: i32, max_y: i32) -> Option<StructureOutline> {
        if self.sessions.get(player).cui.is_some() {
            return None;
        }
        let (level, c) = self.outline_of(player)?;
        let s = self.sessions.get(player);
        if level != dimension || s.kind != SelKind::Cuboid {
            return None;
        }
        structure_outline(&c, position, yaw, pitch, min_y, max_y)
    }

    /// The client's hello on `worldedit:cui` (`v|<protocol version>`): from now on it is told the player's selection.
    /// A client that says hello again (WorldEditCUI does) is sent the selection again. Anything else is not a hello.
    pub fn cui_hello(&mut self, player: u128, text: &str) -> bool {
        let mut parts = text.trim().split('|');
        if !parts.next().is_some_and(|t| t.eq_ignore_ascii_case("v")) {
            return false;
        }
        let args: Vec<&str> = parts.collect();
        let [version] = args.as_slice() else { return false };
        let Ok(version) = version.trim().parse::<i32>() else { return false };
        let s = self.sessions.get(player);
        if let Some(client) = s.cui.as_mut() {
            client.sent = None;
        } else {
            s.cui = Some(CuiClient { version, sent: None });
        }
        true
    }

    /// `/we cui`: a client that has the mod but sends no version is taken to speak the oldest protocol.
    pub(super) fn cui_enable(&mut self, actor: &Actor) {
        let s = self.sessions.get(actor.uuid);
        if let Some(client) = s.cui.as_mut() {
            client.sent = None;
        } else {
            s.cui = Some(CuiClient { version: UNINITIALIZED, sent: None });
        }
    }

    /// The players who said hello.
    pub fn cui_players(&mut self) -> Vec<u128> {
        self.sessions.with_cui()
    }

    /// The player left: the next connection says hello again.
    pub fn cui_forget(&mut self, player: u128) {
        self.sessions.get(player).cui = None;
    }

    /// The messages to send the player now: their whole selection again when it differs from what was last sent,
    /// else none.
    pub fn cui_update(&mut self, player: u128) -> Vec<String> {
        let s = self.sessions.get(player);
        let Some(client) = s.cui.as_ref() else { return Vec::new() };
        let key = format!("{:?}|{:?}|{:?}|{:?}", s.kind, s.shape, s.pos1, s.pos2);
        if client.sent.as_deref() == Some(key.as_str()) {
            return Vec::new();
        }
        let messages = describe(s.kind, s.shape.as_ref(), s.pos1, s.pos2, client.version);
        if let Some(client) = s.cui.as_mut() {
            client.sent = Some(key);
        }
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::world::fake::FakeWorld;

    fn actor() -> Actor {
        Actor { uuid: 7, name: "Builder".into(), permission: 4, dimension: "minecraft:overworld".into(), ..Actor::default() }
    }

    fn run(e: &mut Engine, w: &mut FakeWorld, line: &str) -> String {
        e.command(w, &actor(), line, &Env::default()).text()
    }

    #[test]
    fn the_structure_block_stands_twelve_blocks_in_front_of_the_player() {
        let c = Cuboid::new([-3, 64, -20], [4, 70, -14]);
        let o = structure_outline(&c, [0.5, 64.0, 0.5], 0.0, 0.0, -64, 319).unwrap();
        // Facing south (yaw 0) the block is 12 blocks to the north of the player's z; y is 3 above the box's floor.
        assert_eq!(o.block, [0, 67, -11]);
        assert_eq!(o.offset, [-3, -3, -9]);
        assert_eq!(o.size, [8, 7, 7]);
        // Wider than 48, or too far to reach from the block: nothing is drawn.
        assert!(structure_outline(&Cuboid::new([0, 64, 0], [48, 64, 0]), [0.5, 64.0, 0.5], 0.0, 0.0, -64, 319).is_none());
        assert!(structure_outline(&c, [100.5, 64.0, 0.5], 0.0, 0.0, -64, 319).is_none());
        // The build height bounds the block's y.
        let low = structure_outline(&Cuboid::new([0, -64, -5], [1, -64, -4]), [0.5, 0.0, 0.5], 0.0, 0.0, -64, 319).unwrap();
        assert_eq!(low.block[1], -61);
    }

    #[test]
    fn only_a_cuboid_of_a_player_without_the_mod_gets_a_structure_block() {
        let mut e = Engine::default();
        let mut w = FakeWorld::new();
        run(&mut e, &mut w, "//pos1 0,64,0");
        run(&mut e, &mut w, "//pos2 1,65,1");
        let shown = e.structure_outline_for(7, "minecraft:overworld", [0.5, 64.0, 12.5], 0.0, 0.0, -64, 319);
        assert_eq!(shown.map(|o| o.size), Some([2, 2, 2]));
        assert!(e.structure_outline_for(7, "minecraft:the_nether", [0.5, 64.0, 12.5], 0.0, 0.0, -64, 319).is_none());
        e.cui_hello(7, "v|4");
        assert!(e.structure_outline_for(7, "minecraft:overworld", [0.5, 64.0, 12.5], 0.0, 0.0, -64, 319).is_none());
    }

    #[test]
    fn a_cuboid_is_two_points_with_their_volume() {
        let lines = describe(SelKind::Cuboid, None, Some([1, 2, 3]), Some([3, 4, 5]), 4);
        assert_eq!(lines, vec!["s|cuboid", "p|0|1|2|3|27", "p|1|3|4|5|27"]);
        assert_eq!(describe(SelKind::Cuboid, None, Some([1, 2, 3]), None, 4), vec!["s|cuboid", "p|0|1|2|3|-1"]);
        assert_eq!(describe(SelKind::Cuboid, None, None, None, 4), vec!["s|cuboid"]);
    }

    #[test]
    fn a_polygon_is_its_points_and_its_height() {
        let shape = Shape::Poly { points: vec![[0, 0], [4, 0], [4, 4], [0, 4]], min_y: 10, max_y: 12 };
        let lines = describe(SelKind::Poly, Some(&shape), Some([0, 10, 0]), Some([4, 12, 4]), 4);
        assert_eq!(lines, vec!["s|polygon2d", "p2|0|0|0|75", "p2|1|4|0|75", "p2|2|4|4|75", "p2|3|0|4|75", "mm|10|12"]);
    }

    #[test]
    fn an_ellipsoid_is_its_centre_and_radii_for_a_new_client_and_a_box_for_an_old_one() {
        let shape = Shape::Ellipsoid { center: [0, 64, 0], radius: [3.0, 3.0, 3.0] };
        assert_eq!(describe(SelKind::Ellipsoid, Some(&shape), None, None, 1), vec!["s|ellipsoid", "e|0|0|64|0", "e|1|3|3|3"]);
        // Volume 179, the same as `//size` says for it.
        assert_eq!(describe(SelKind::Sphere, Some(&shape), None, None, 0), vec!["s|cuboid", "p|0|-3|61|-3|179", "p|1|3|67|3|179"]);
    }

    #[test]
    fn a_cylinder_is_its_centre_radii_and_height() {
        let shape = Shape::Cyl { center: [10, -4], radius: [2.0, 3.5], min_y: 5, max_y: 8 };
        assert_eq!(describe(SelKind::Cyl, Some(&shape), None, None, 4), vec!["s|cylinder", "cyl|10|6|-4|2.0|3.5", "mm|5|8"]);
        let old = describe(SelKind::Cyl, Some(&shape), None, None, 0);
        assert_eq!(old[0], "s|cuboid");
        assert_eq!(old.len(), 3);
    }

    #[test]
    fn a_polyhedron_is_its_vertices_and_the_triangles_of_its_hull() {
        let shape = Shape::Convex { points: vec![[0, 0, 0], [8, 0, 0], [0, 8, 0], [0, 0, 8]] };
        let lines = describe(SelKind::Convex, Some(&shape), None, None, 4);
        assert_eq!(lines[0], "s|polyhedron");
        assert_eq!(lines.iter().filter(|l| l.starts_with("p|")).count(), 4);
        // A tetrahedron has four faces of one triangle each.
        assert_eq!(lines.iter().filter(|l| l.starts_with("poly|")).count(), 4);
        assert!(lines.iter().any(|l| l.starts_with("p|1|8|0|0|")));
        // Too old for a polyhedron: its box, once it has four vertices.
        assert_eq!(describe(SelKind::Convex, Some(&shape), None, None, 2).len(), 3);
        let cube: Vec<Pos> = (0..8).map(|i| [(i & 1) * 2, ((i >> 1) & 1) * 2, ((i >> 2) & 1) * 2]).collect();
        // Six square faces, two triangles each.
        assert_eq!(triangles(&cube).len(), 12);
    }

    #[test]
    fn a_hello_starts_the_outline_and_only_changes_are_sent() {
        let mut e = Engine::default();
        let mut w = FakeWorld::new();
        assert!(e.cui_players().is_empty());
        assert!(!e.cui_hello(7, "x|4"));
        assert!(!e.cui_hello(7, "v|4|5"));
        assert!(!e.cui_hello(7, "v|four"));
        assert!(e.cui_hello(7, "v|4"));
        assert_eq!(e.cui_players(), vec![7]);
        assert_eq!(e.cui_update(7), vec!["s|cuboid"]);
        // Nothing changed: nothing to send.
        assert!(e.cui_update(7).is_empty());
        run(&mut e, &mut w, "/pos1 1,2,3");
        assert_eq!(e.cui_update(7), vec!["s|cuboid", "p|0|1|2|3|-1"]);
        run(&mut e, &mut w, "/pos2 2,3,4");
        assert_eq!(e.cui_update(7), vec!["s|cuboid", "p|0|1|2|3|8", "p|1|2|3|4|8"]);
        // A second hello sends the selection again, and a new selector changes what is sent.
        assert!(e.cui_hello(7, "v|4"));
        assert_eq!(e.cui_update(7).len(), 3);
        run(&mut e, &mut w, "/sel sphere");
        assert!(e.cui_update(7).iter().any(|l| l == "s|ellipsoid"));
        e.cui_forget(7);
        assert!(e.cui_players().is_empty());
    }

    #[test]
    fn we_cui_enables_the_outline_for_the_oldest_protocol() {
        let mut e = Engine::default();
        let mut w = FakeWorld::new();
        run(&mut e, &mut w, "/we cui");
        assert_eq!(e.cui_players(), vec![7]);
        run(&mut e, &mut w, "/sel sphere");
        run(&mut e, &mut w, "/pos1 0,64,0");
        // The client says nothing of its version: it is sent boxes, not an ellipsoid.
        assert_eq!(e.cui_update(7)[0], "s|cuboid");
    }
}
