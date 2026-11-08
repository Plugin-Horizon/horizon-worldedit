//! Cuboid selections and the directions commands take.

use crate::world::Pos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    North,
    South,
    East,
    West,
    Up,
    Down,
}

impl Dir {
    pub const ALL: [Dir; 6] = [Dir::North, Dir::South, Dir::East, Dir::West, Dir::Up, Dir::Down];
    pub fn vector(self) -> Pos {
        match self {
            Dir::North => [0, 0, -1],
            Dir::South => [0, 0, 1],
            Dir::East => [1, 0, 0],
            Dir::West => [-1, 0, 0],
            Dir::Up => [0, 1, 0],
            Dir::Down => [0, -1, 0],
        }
    }
    pub fn opposite(self) -> Dir {
        match self {
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::East => Dir::West,
            Dir::West => Dir::East,
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
    /// Turned a quarter clockwise, seen from above (up and down stay).
    fn right(self) -> Dir {
        match self {
            Dir::North => Dir::East,
            Dir::East => Dir::South,
            Dir::South => Dir::West,
            Dir::West => Dir::North,
            d => d,
        }
    }
    pub fn axis(self) -> usize {
        match self {
            Dir::East | Dir::West => 0,
            Dir::Up | Dir::Down => 1,
            Dir::North | Dir::South => 2,
        }
    }
    pub fn positive(self) -> bool {
        matches!(self, Dir::East | Dir::Up | Dir::South)
    }
    /// The way a player looks (`yaw`, `pitch` in degrees, as Minecraft gives them): up or down when
    /// steeper than 67.5 degrees, as WorldEdit's `me` is, else the compass side.
    pub fn facing(yaw: f32, pitch: f32) -> Dir {
        if pitch > 67.5 {
            return Dir::Down;
        }
        if pitch < -67.5 {
            return Dir::Up;
        }
        Self::horizontal(yaw)
    }
    /// The compass side of `yaw` (0 is south, 90 west).
    pub fn horizontal(yaw: f32) -> Dir {
        let yaw = yaw.rem_euclid(360.0);
        if !(45.0..315.0).contains(&yaw) {
            Dir::South
        } else if yaw < 135.0 {
            Dir::West
        } else if yaw < 225.0 {
            Dir::North
        } else {
            Dir::East
        }
    }
    /// A direction word: the six names and their first letters, and `me`/`forward`, `back`,
    /// `left`, `right` from where the player looks.
    pub fn parse(word: &str, yaw: f32, pitch: f32) -> Option<Dir> {
        Some(match word.to_ascii_lowercase().as_str() {
            "north" | "n" => Dir::North,
            "south" | "s" => Dir::South,
            "east" | "e" => Dir::East,
            "west" | "w" => Dir::West,
            "up" | "u" => Dir::Up,
            "down" | "d" => Dir::Down,
            "me" | "forward" | "f" => Dir::facing(yaw, pitch),
            "back" | "b" => Dir::facing(yaw, pitch).opposite(),
            "right" | "r" => Dir::horizontal(yaw).right(),
            "left" | "l" => Dir::horizontal(yaw).right().opposite(),
            _ => return None,
        })
    }
}

/// A box of blocks, both corners inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cuboid {
    pub min: Pos,
    pub max: Pos,
}

impl Cuboid {
    pub fn new(a: Pos, b: Pos) -> Self {
        Self {
            min: [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])],
            max: [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])],
        }
    }
    pub fn size(&self) -> Pos {
        [self.max[0] - self.min[0] + 1, self.max[1] - self.min[1] + 1, self.max[2] - self.min[2] + 1]
    }
    pub fn volume(&self) -> u64 {
        self.size().iter().map(|&s| s.max(0) as u64).product()
    }
    pub fn contains(&self, p: Pos) -> bool {
        (0..3).all(|a| p[a] >= self.min[a] && p[a] <= self.max[a])
    }
    /// Cut to the build height; `None` when nothing is left.
    pub fn clamp_y(&self, min_y: i32, max_y: i32) -> Option<Cuboid> {
        let c = Cuboid { min: [self.min[0], self.min[1].max(min_y), self.min[2]], max: [self.max[0], self.max[1].min(max_y), self.max[2]] };
        (c.min[1] <= c.max[1]).then_some(c)
    }
    /// Every position, x then z then y (columns stay together).
    pub fn positions(&self) -> impl Iterator<Item = Pos> + '_ {
        let (lo, hi) = (self.min, self.max);
        (lo[0]..=hi[0]).flat_map(move |x| (lo[2]..=hi[2]).flat_map(move |z| (lo[1]..=hi[1]).map(move |y| [x, y, z])))
    }
    /// The four sides, full height.
    pub fn is_wall(&self, p: Pos) -> bool {
        p[0] == self.min[0] || p[0] == self.max[0] || p[2] == self.min[2] || p[2] == self.max[2]
    }
    /// All six faces.
    pub fn is_face(&self, p: Pos) -> bool {
        self.is_wall(p) || p[1] == self.min[1] || p[1] == self.max[1]
    }
    /// Grown by `amount` toward `dir` (negative: shrunk from that side), `None` when it would be empty.
    pub fn expand(&self, dir: Dir, amount: i32) -> Option<Cuboid> {
        let mut c = *self;
        let a = dir.axis();
        if dir.positive() {
            c.max[a] = c.max[a].saturating_add(amount);
        } else {
            c.min[a] = c.min[a].saturating_sub(amount);
        }
        (c.min[a] <= c.max[a]).then_some(c)
    }
    /// Shrunk by `amount` from the side `dir` points to (WorldEdit's `//contract`, which takes the
    /// face toward `dir` in).
    pub fn contract(&self, dir: Dir, amount: i32) -> Option<Cuboid> {
        let mut c = *self;
        let a = dir.axis();
        if dir.positive() {
            c.min[a] = c.min[a].saturating_add(amount);
        } else {
            c.max[a] = c.max[a].saturating_sub(amount);
        }
        (c.min[a] <= c.max[a]).then_some(c)
    }
    pub fn shifted(&self, by: Pos) -> Cuboid {
        Cuboid { min: add(self.min, by), max: add(self.max, by) }
    }
    /// Grown on every side (negative: shrunk), only horizontally or vertically when asked.
    pub fn outset(&self, amount: i32, horizontal: bool, vertical: bool) -> Option<Cuboid> {
        let mut c = *self;
        for a in 0..3 {
            if (a == 1 && !vertical) || (a != 1 && !horizontal) {
                continue;
            }
            c.min[a] = c.min[a].saturating_sub(amount);
            c.max[a] = c.max[a].saturating_add(amount);
            if c.min[a] > c.max[a] {
                return None;
            }
        }
        Some(c)
    }
    /// The middle block, or the two middle blocks of each even side.
    pub fn center(&self) -> Cuboid {
        // WorldEdit: `(int) center` (truncated) to `roundHalfUp(center)` on each axis.
        let mid = |a: usize| {
            let c = (self.min[a] as f64 + self.max[a] as f64) / 2.0;
            (c.trunc() as i32, (c + 0.5).floor() as i32)
        };
        let (x, y, z) = (mid(0), mid(1), mid(2));
        Cuboid::new([x.0, y.0, z.0], [x.1, y.1, z.1])
    }
}

pub fn add(a: Pos, b: Pos) -> Pos {
    [a[0].saturating_add(b[0]), a[1].saturating_add(b[1]), a[2].saturating_add(b[2])]
}
pub fn sub(a: Pos, b: Pos) -> Pos {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: Pos, k: i32) -> Pos {
    [a[0] * k, a[1] * k, a[2] * k]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_and_sizes() {
        let c = Cuboid::new([2, 70, -3], [0, 64, 1]);
        assert_eq!((c.min, c.max), ([0, 64, -3], [2, 70, 1]));
        assert_eq!(c.size(), [3, 7, 5]);
        assert_eq!(c.volume(), 105);
        assert_eq!(c.positions().count(), 105);
        assert_eq!(c.positions().filter(|&p| c.is_wall(p)).count(), 7 * (3 * 5 - 3));
        assert_eq!(c.clamp_y(66, 68).unwrap().size()[1], 3);
        assert!(c.clamp_y(80, 90).is_none());
    }

    #[test]
    fn expand_contract_shift() {
        let c = Cuboid::new([0, 0, 0], [4, 4, 4]);
        assert_eq!(c.expand(Dir::Up, 3).unwrap().max, [4, 7, 4]);
        assert_eq!(c.expand(Dir::West, 2).unwrap().min, [-2, 0, 0]);
        assert_eq!(c.contract(Dir::East, 1).unwrap().min, [1, 0, 0]);
        assert_eq!(c.contract(Dir::Down, 1).unwrap().max, [4, 3, 4]);
        assert!(c.contract(Dir::East, 9).is_none());
        assert_eq!(c.shifted([1, -1, 0]).min, [1, -1, 0]);
        assert_eq!(c.outset(1, true, false).unwrap().min, [-1, 0, -1]);
        assert!(c.outset(-3, true, true).is_none());
        assert_eq!(Cuboid::new([0, 0, 0], [3, 2, 0]).center(), Cuboid::new([1, 1, 0], [2, 1, 0]));
    }

    #[test]
    fn directions_from_the_view() {
        assert_eq!(Dir::facing(0.0, 0.0), Dir::South);
        assert_eq!(Dir::facing(90.0, 10.0), Dir::West);
        assert_eq!(Dir::facing(180.0, 0.0), Dir::North);
        assert_eq!(Dir::facing(-90.0, 0.0), Dir::East);
        assert_eq!(Dir::facing(0.0, 80.0), Dir::Down);
        assert_eq!(Dir::parse("left", 180.0, 0.0), Some(Dir::West));
        assert_eq!(Dir::parse("right", 180.0, 0.0), Some(Dir::East));
        assert_eq!(Dir::parse("back", 180.0, 0.0), Some(Dir::South));
        assert_eq!(Dir::parse("u", 0.0, 0.0), Some(Dir::Up));
        assert_eq!(Dir::parse("sideways", 0.0, 0.0), None);
    }
}
