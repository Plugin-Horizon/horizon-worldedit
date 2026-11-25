//! Snapshots: backups of a world kept in `snapshots.directory`, listed, chosen by name, index or date, and restored
//! into a selection (WorldEdit's `FileSystemSnapshotDatabase`, `SnapshotRepository`, `FolderSnapshot` and
//! `SnapshotRestore`). A snapshot is a world folder, or a `.zip` that holds one, as a plain folder or inside a
//! timestamped folder or archive. `.tar`, `.tar.gz` and `.tar.bz2` archives are not read.

use crate::horizon_worldedit_core::anvil::{self, Chunk};
use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::nbt::Tag;
use crate::horizon_worldedit_core::world::Pos;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------------------------------
// Dates

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Seconds since the epoch of a civil date and time, read as UTC.
fn epoch(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s
}

/// `yyyy-MM-dd HH:mm:ss z`, WorldEdit's date format, for `secs` seen from `offset_secs` east of UTC; `zone` is the
/// name the player gave their time zone.
pub fn format_date(secs: i64, offset_secs: i64, zone: &str) -> String {
    let local = secs + offset_secs;
    let (y, m, d) = civil_from_days(local.div_euclid(86_400));
    let t = local.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} {zone}", t / 3600, t / 60 % 60, t % 60)
}

/// The digits at `i` (one to `max` of them), moving `i` past them.
fn digits(b: &[u8], i: &mut usize, max: usize) -> Option<i64> {
    let start = *i;
    while *i < b.len() && *i - start < max && b[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    std::str::from_utf8(&b[start..*i]).ok()?.parse().ok()
}

fn sep(b: &[u8], i: &mut usize, set: &[u8]) -> Option<()> {
    if *i < b.len() && set.contains(&b[*i]) {
        *i += 1;
        Some(())
    } else {
        None
    }
}

fn valid(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<i64> {
    ((1..=12).contains(&mo) && (1..=31).contains(&d) && (0..24).contains(&h) && (0..60).contains(&mi) && (0..60).contains(&s)).then(|| epoch(y, mo, d, h, mi, s))
}

/// The date a snapshot's file name starts with, as WorldEdit's `FileNameDateTimeParser` reads it: `2024-01-31`,
/// `2024_01_31_23-59-59`, `2024-01-31T23:59:59` (separators one of ` -_:`, the time optional). Also the compact
/// `20240131_235959` that backup scripts write, anywhere in the name. Read as UTC.
pub fn name_date(name: &str) -> Option<i64> {
    let b = name.as_bytes();
    let basic = || -> Option<i64> {
        let mut i = 0;
        let y = digits(b, &mut i, 4).filter(|_| i == 4)?;
        sep(b, &mut i, b" -_:")?;
        let mo = digits(b, &mut i, 2)?;
        sep(b, &mut i, b" -_:")?;
        let d = digits(b, &mut i, 2)?;
        let time = (|| {
            sep(b, &mut i, b" -_:T")?;
            let h = digits(b, &mut i, 2)?;
            sep(b, &mut i, b" -_:")?;
            let mi = digits(b, &mut i, 2)?;
            sep(b, &mut i, b" -_:")?;
            let s = digits(b, &mut i, 2)?;
            Some((h, mi, s))
        })();
        let (h, mi, s) = time.unwrap_or((0, 0, 0));
        valid(y, mo, d, h, mi, s)
    };
    if let Some(t) = basic() {
        return Some(t);
    }
    // `20240131_235959` or `20240131-235959` somewhere in the name.
    for start in 0..b.len().saturating_sub(14) {
        let mut i = start;
        if start > 0 && b[start - 1].is_ascii_digit() {
            continue;
        }
        let (Some(date), Some(())) = (digits(b, &mut i, 8).filter(|_| i == start + 8), sep(b, &mut i, b"_-")) else { continue };
        let Some(time) = digits(b, &mut i, 6).filter(|_| i == start + 15) else { continue };
        if i < b.len() && b[i].is_ascii_digit() {
            continue;
        }
        if let Some(t) = valid(date / 10_000, date / 100 % 100, date % 100, time / 10_000, time / 100 % 100, time % 100) {
            return Some(t);
        }
    }
    None
}

/// A date a player typed (`/snapshot before <date>`), as seconds since the epoch. WorldEdit hands the text to a
/// natural-language parser; this reads the common forms: `2024-01-31`, `2024-01-31 12:30[:45]`, `2024/01/31`,
/// `01/31/2024`, `20240131_123045`, `now`, `today`, `yesterday`, `N seconds|minutes|hours|days|weeks|months|years ago`
/// and `last week|month|year|day|hour`. Dates are in the player's time zone (`offset_secs` east of UTC); `now` is
/// the current time in seconds.
pub fn parse_user_date(text: &str, now: i64, offset_secs: i64) -> Option<i64> {
    let text = text.trim().trim_matches('"').trim().to_ascii_lowercase();
    match text.as_str() {
        "now" | "today" => return Some(now),
        "yesterday" => return Some(now - 86_400),
        "tomorrow" => return Some(now + 86_400),
        _ => {}
    }
    let unit = |u: &str| -> Option<i64> {
        Some(match u.trim_end_matches('s') {
            "second" | "sec" => 1,
            "minute" | "min" => 60,
            "hour" | "hr" => 3600,
            "day" => 86_400,
            "week" => 7 * 86_400,
            "month" => 30 * 86_400,
            "year" => 365 * 86_400,
            _ => return None,
        })
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        [n, u, "ago"] => return Some(now - n.parse::<i64>().ok()? * unit(*u)?),
        ["last", u] => return Some(now - unit(*u)?),
        ["a" | "an", u, "ago"] => return Some(now - unit(*u)?),
        _ => {}
    }
    // The player's own clock: a date without a zone is read in theirs.
    let clock = |y, mo, d, h, mi, s| valid(y, mo, d, h, mi, s).map(|t| t - offset_secs);
    let b = text.as_bytes();
    // `yyyy-MM-dd[ T]HH:mm[:ss]` and `yyyy/MM/dd`.
    let ymd = || -> Option<i64> {
        let mut i = 0;
        let y = digits(b, &mut i, 4).filter(|_| i == 4)?;
        sep(b, &mut i, b"-/._")?;
        let mo = digits(b, &mut i, 2)?;
        sep(b, &mut i, b"-/._")?;
        let d = digits(b, &mut i, 2)?;
        let (mut h, mut mi, mut s) = (0, 0, 0);
        if sep(b, &mut i, b" t_").is_some() {
            h = digits(b, &mut i, 2)?;
            sep(b, &mut i, b":-")?;
            mi = digits(b, &mut i, 2)?;
            if sep(b, &mut i, b":-").is_some() {
                s = digits(b, &mut i, 2)?;
            }
        }
        (i == b.len()).then_some(())?;
        clock(y, mo, d, h, mi, s)
    };
    if let Some(t) = ymd() {
        return Some(t);
    }
    // `MM/dd/yyyy[ HH:mm[:ss]]`, as the American reading of a slash date.
    let mdy = || -> Option<i64> {
        let mut i = 0;
        let mo = digits(b, &mut i, 2)?;
        sep(b, &mut i, b"/")?;
        let d = digits(b, &mut i, 2)?;
        sep(b, &mut i, b"/")?;
        let y = digits(b, &mut i, 4)?;
        let (mut h, mut mi, mut s) = (0, 0, 0);
        if sep(b, &mut i, b" ").is_some() {
            h = digits(b, &mut i, 2)?;
            sep(b, &mut i, b":")?;
            mi = digits(b, &mut i, 2)?;
            if sep(b, &mut i, b":").is_some() {
                s = digits(b, &mut i, 2)?;
            }
        }
        (i == b.len()).then_some(())?;
        clock(y, mo, d, h, mi, s)
    };
    if let Some(t) = mdy() {
        return Some(t);
    }
    name_date(&text).map(|t| t - offset_secs)
}

// ---------------------------------------------------------------------------------------------------------------------
// Zip archives

struct ZipEntry {
    local: u64,
    packed: u64,
    method: u16,
}

/// The central directory of a `.zip` file: enough to read single entries without unpacking the rest.
pub struct Zip {
    path: PathBuf,
    entries: BTreeMap<String, ZipEntry>,
}

fn le16(b: &[u8], at: usize) -> u64 {
    u64::from(u16::from_le_bytes([b[at], b[at + 1]]))
}
fn le32(b: &[u8], at: usize) -> u64 {
    u64::from(u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]))
}

impl Zip {
    pub fn open(path: &Path) -> Result<Zip, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let tail_len = len.min(65_557);
        file.seek(SeekFrom::Start(len - tail_len)).map_err(|e| e.to_string())?;
        let mut tail = vec![0u8; tail_len as usize];
        file.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let eocd = (0..tail.len().saturating_sub(21)).rev().find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06]).ok_or("not a zip file")?;
        let (count, size, offset) = (le16(&tail, eocd + 10), le32(&tail, eocd + 12), le32(&tail, eocd + 16));
        if count == 0xffff || size == 0xffff_ffff || offset == 0xffff_ffff {
            return Err("zip64 archives are not read".into());
        }
        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let mut dir = vec![0u8; size as usize];
        file.read_exact(&mut dir).map_err(|e| e.to_string())?;
        let mut entries = BTreeMap::new();
        let mut at = 0;
        while at + 46 <= dir.len() && dir[at..at + 4] == [0x50, 0x4b, 0x01, 0x02] {
            let (method, packed) = (le16(&dir, at + 10) as u16, le32(&dir, at + 20));
            let (name_len, extra_len, comment_len) = (le16(&dir, at + 28) as usize, le16(&dir, at + 30) as usize, le16(&dir, at + 32) as usize);
            let local = le32(&dir, at + 42);
            let name = dir.get(at + 46..at + 46 + name_len).ok_or("zip directory is cut off")?;
            entries.insert(String::from_utf8_lossy(name).replace('\\', "/"), ZipEntry { local, packed, method });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Zip { path: path.to_owned(), entries })
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(name)?;
        let mut file = File::open(&self.path).ok()?;
        file.seek(SeekFrom::Start(e.local)).ok()?;
        let mut head = [0u8; 30];
        file.read_exact(&mut head).ok()?;
        if head[..4] != [0x50, 0x4b, 0x03, 0x04] {
            return None;
        }
        let skip = le16(&head, 26) + le16(&head, 28);
        file.seek(SeekFrom::Current(skip as i64)).ok()?;
        let mut packed = Vec::new();
        file.take(e.packed).read_to_end(&mut packed).ok()?;
        match e.method {
            0 => Some(packed),
            8 => {
                let mut out = Vec::new();
                flate2::read::DeflateDecoder::new(&packed[..]).read_to_end(&mut out).ok()?;
                Some(out)
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// A folder tree on disk or inside a zip

#[derive(Clone)]
enum Fs {
    Disk,
    Zip(Arc<Zip>),
}

/// A place in a folder tree: a path on disk, or a path inside a zip (`""` is the archive's root).
#[derive(Clone)]
struct Node {
    fs: Fs,
    path: String,
}

impl Node {
    fn disk(path: &Path) -> Node {
        Node { fs: Fs::Disk, path: path.to_string_lossy().into_owned() }
    }

    fn name(&self) -> String {
        self.path.rsplit('/').next().unwrap_or("").to_owned()
    }

    fn child(&self, name: &str) -> Node {
        let path = if self.path.is_empty() { name.to_owned() } else { format!("{}/{name}", self.path) };
        Node { fs: self.fs.clone(), path }
    }

    fn is_dir(&self) -> bool {
        match &self.fs {
            Fs::Disk => Path::new(&self.path).is_dir(),
            Fs::Zip(z) => self.path.is_empty() || z.entries.range(format!("{}/", self.path)..).next().is_some_and(|(k, _)| k.starts_with(&format!("{}/", self.path))),
        }
    }

    fn is_file(&self) -> bool {
        match &self.fs {
            Fs::Disk => Path::new(&self.path).is_file(),
            Fs::Zip(z) => z.entries.contains_key(&self.path) && !self.path.ends_with('/'),
        }
    }

    fn children(&self) -> Vec<Node> {
        match &self.fs {
            Fs::Disk => {
                let mut names: Vec<String> = std::fs::read_dir(&self.path).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
                names.sort();
                names.iter().map(|n| self.child(n)).collect()
            }
            Fs::Zip(z) => {
                let prefix = if self.path.is_empty() { String::new() } else { format!("{}/", self.path) };
                let mut names: Vec<String> = Vec::new();
                for (k, _) in z.entries.range(prefix.clone()..) {
                    let Some(rest) = k.strip_prefix(&prefix) else { break };
                    let first = rest.split('/').next().unwrap_or("");
                    if !first.is_empty() && names.last().map(String::as_str) != Some(first) {
                        names.push(first.to_owned());
                    }
                }
                names.iter().map(|n| self.child(n)).collect()
            }
        }
    }

    fn read(&self) -> Option<Vec<u8>> {
        match &self.fs {
            Fs::Disk => std::fs::read(&self.path).ok(),
            Fs::Zip(z) => z.read(&self.path),
        }
    }

    /// Seconds since the epoch the folder, file or archive was last changed.
    fn modified(&self) -> Option<i64> {
        let path = match &self.fs {
            Fs::Disk => Path::new(&self.path),
            Fs::Zip(z) => z.path.as_path(),
        };
        let t = std::fs::metadata(path).ok()?.modified().ok()?;
        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
    }

    /// The root of the `.zip` this file is.
    fn as_archive(&self) -> Option<Node> {
        if !matches!(self.fs, Fs::Disk) || !self.name().to_ascii_lowercase().ends_with(".zip") || !self.is_file() {
            return None;
        }
        Zip::open(Path::new(&self.path)).ok().map(|z| Node { fs: Fs::Zip(Arc::new(z)), path: String::new() })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Finding snapshots

/// One snapshot of a world.
#[derive(Clone)]
pub struct Snapshot {
    /// The name it is listed and chosen by (`2024-01-31/world`, `world.zip`).
    pub id: String,
    /// Seconds since the epoch, from its name or else its modification time.
    pub date: i64,
    world: Node,
}

fn without_zip(name: &str) -> &str {
    name.strip_suffix(".zip").or_else(|| name.strip_suffix(".ZIP")).unwrap_or(name)
}

/// The world folder inside an archive: the folder named after the world, or the archive's own root when that holds
/// the world, or the one folder that is all it holds.
fn archive_world(root: &Node, world: &str) -> Node {
    let named = root.child(world);
    if named.is_dir() {
        return named;
    }
    if root.child("level.dat").is_file() || root.child("region").is_dir() {
        return root.clone();
    }
    let dirs: Vec<Node> = root.children().into_iter().filter(Node::is_dir).collect();
    if dirs.len() == 1 {
        return dirs[0].clone();
    }
    root.clone()
}

/// A folder that is the world `world`: `<world>/level.dat`, or an archive named `<world>.<ext>`.
fn world_entry(entry: &Node, world: &str) -> Option<Node> {
    let name = entry.name();
    if entry.is_dir() {
        return (name == world && entry.child("level.dat").is_file()).then(|| entry.clone());
    }
    if entry.is_file() && name.starts_with(&format!("{world}.")) {
        return entry.as_archive().map(|root| archive_world(&root, world));
    }
    None
}

/// The worlds inside a folder or archive whose name is a date (`<stamp>/<world>`).
fn timestamped(entry: &Node, world: &str, out: &mut Vec<(String, Node)>) {
    let name = entry.name();
    if name_date(&name).is_none() {
        return;
    }
    if entry.is_dir() {
        for c in entry.children() {
            if let Some(w) = world_entry(&c, world) {
                out.push((format!("{name}/{}", c.name()), w));
            }
        }
    } else if let Some(root) = entry.as_archive() {
        // An archive may keep everything under a folder named like itself.
        let stem = without_zip(&name);
        let inner = root.child(stem);
        let root = if inner.is_dir() && !root.child(world).is_dir() { inner } else { root };
        for c in root.children() {
            if let Some(w) = world_entry(&c, world) {
                out.push((format!("{name}/{}", c.name()), w));
            }
        }
    }
}

/// Every snapshot of `world` under `root`, the newest first. A missing folder holds none.
pub fn list(root: &Path, world: &str) -> Vec<Snapshot> {
    let top = Node::disk(root);
    let mut found: Vec<(String, Node)> = Vec::new();
    for entry in top.children() {
        if let Some(w) = world_entry(&entry, world) {
            found.push((entry.name(), w));
        } else if entry.is_dir() && entry.name().eq_ignore_ascii_case(world) && !entry.child("level.dat").is_file() {
            // A folder named after the world, holding the snapshots: `<world>/<stamp>/<world>`, or `<world>/<stamp>` as
            // a world folder or archive of its own.
            for c in entry.children() {
                let mut inner = Vec::new();
                timestamped(&c, world, &mut inner);
                if inner.is_empty() && name_date(&c.name()).is_some() {
                    if c.is_dir() && c.child("level.dat").is_file() {
                        inner.push((c.name(), c.clone()));
                    } else if let Some(archive) = c.as_archive() {
                        inner.push((c.name(), archive_world(&archive, world)));
                    }
                }
                for (id, w) in inner {
                    found.push((format!("{}/{id}", entry.name()), w));
                }
            }
        } else {
            timestamped(&entry, world, &mut found);
        }
    }
    let mut out: Vec<Snapshot> = found
        .into_iter()
        .map(|(id, node)| {
            let date = id.split('/').rev().find_map(name_date).or_else(|| node.modified()).unwrap_or(0);
            Snapshot { id, date, world: node }
        })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| b.id.cmp(&a.id)));
    out
}

/// The snapshot called `name` (leading slashes ignored).
pub fn find(root: &Path, world: &str, name: &str) -> Option<Snapshot> {
    let name = name.trim_start_matches('/');
    list(root, world).into_iter().find(|s| s.id == name)
}

// ---------------------------------------------------------------------------------------------------------------------
// Restoring

/// The folder of a dimension's region files inside a world folder: the `dimensions/<namespace>/<path>` layout of
/// Minecraft 26, the old `DIM-1` and `DIM1` folders, and the overworld's own `region` folder.
fn region_dir(world: &Node, dimension: &str) -> Option<Node> {
    let (ns, path) = dimension.split_once(':').unwrap_or(("minecraft", dimension));
    let mut candidates = vec![world.child("dimensions").child(ns).child(path).child("region")];
    match dimension {
        "minecraft:overworld" => candidates.push(world.child("region")),
        "minecraft:the_nether" => candidates.push(world.child("DIM-1").child("region")),
        "minecraft:the_end" => candidates.push(world.child("DIM1").child("region")),
        _ => {}
    }
    if dimension == "minecraft:overworld" {
        // A folder of nothing but region files.
        candidates.push(world.clone());
    }
    candidates.into_iter().find(|n| n.is_dir())
}

/// What `restore` found out about the chunks it needed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Chunks the positions are in.
    pub chunks: usize,
    /// Chunks the snapshot does not have.
    pub missing: usize,
    /// Chunks that could not be read.
    pub errors: usize,
    pub last_error: Option<String>,
}

impl Outcome {
    /// WorldEdit's `hadTotalFailure`: not one chunk was read.
    pub fn total_failure(&self) -> bool {
        self.missing + self.errors == self.chunks
    }
}

/// Reads the blocks of `positions` from `snap` (the dimension's region files) and hands each, with its block entity
/// data if it has one, to `visit`, chunk by chunk in the order the chunks first appear.
pub fn restore(snap: &Snapshot, dimension: &str, positions: &[Pos], mut visit: impl FnMut(Pos, &BlockState, Option<&Tag>)) -> Outcome {
    let mut by_chunk: Vec<((i32, i32), Vec<Pos>)> = Vec::new();
    let mut index: HashMap<(i32, i32), usize> = HashMap::new();
    for &p in positions {
        let key = (p[0].div_euclid(16), p[2].div_euclid(16));
        let at = *index.entry(key).or_insert_with(|| {
            by_chunk.push((key, Vec::new()));
            by_chunk.len() - 1
        });
        by_chunk[at].1.push(p);
    }
    let mut outcome = Outcome { chunks: by_chunk.len(), ..Outcome::default() };
    let dir = region_dir(&snap.world, dimension);
    // The region file of the chunk being read, kept while the next chunks are in it too.
    let mut region: Option<(String, Option<Vec<u8>>)> = None;
    for ((cx, cz), blocks) in by_chunk {
        let file = anvil::region_file(cx, cz);
        if region.as_ref().is_none_or(|(name, _)| *name != file) {
            let bytes = dir.as_ref().and_then(|d| d.child(&file).read());
            region = Some((file, bytes));
        }
        let Some(bytes) = region.as_ref().and_then(|(_, b)| b.as_deref()) else {
            outcome.missing += 1;
            continue;
        };
        match anvil::chunk_nbt(bytes, cx, cz).and_then(|tag| tag.map(|t| Chunk::from_nbt(&t)).transpose()) {
            Ok(Some(chunk)) => {
                for p in blocks {
                    visit(p, chunk.block(p), chunk.block_entity(p));
                }
            }
            Ok(None) => outcome.missing += 1,
            Err(e) => {
                outcome.errors += 1;
                outcome.last_error = Some(e);
            }
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_worldedit_core::nbt;
    use std::io::Write;

    #[test]
    fn dates_come_out_of_file_names() {
        // 2024-01-31 23:59:59 UTC.
        assert_eq!(name_date("2024-01-31_23-59-59"), Some(1_706_745_599));
        assert_eq!(name_date("2024-01-31T23:59:59"), Some(1_706_745_599));
        assert_eq!(name_date("2024_01_31"), Some(1_706_659_200));
        assert_eq!(name_date("_backup_world_20240131_235959"), Some(1_706_745_599));
        assert_eq!(name_date("world"), None);
        assert_eq!(name_date("2024-13-01"), None);
    }

    #[test]
    fn typed_dates_follow_the_players_zone() {
        // Two hours east of UTC: midnight there is 22:00 UTC the day before.
        assert_eq!(parse_user_date("2024-01-31", 0, 7200), Some(1_706_659_200 - 7200));
        assert_eq!(parse_user_date("2024-01-31 12:30", 0, 0), Some(1_706_659_200 + 12 * 3600 + 30 * 60));
        assert_eq!(parse_user_date("01/31/2024", 0, 0), Some(1_706_659_200));
        assert_eq!(parse_user_date("2 days ago", 1_000_000, 0), Some(1_000_000 - 172_800));
        assert_eq!(parse_user_date("yesterday", 1_000_000, 0), Some(913_600));
        assert_eq!(parse_user_date("last week", 1_000_000, 0), Some(1_000_000 - 604_800));
        assert_eq!(parse_user_date("nonsense", 0, 0), None);
    }

    #[test]
    fn dates_are_printed_as_worldedit_does() {
        assert_eq!(format_date(1_706_745_599, 0, "UTC"), "2024-01-31 23:59:59 UTC");
        assert_eq!(format_date(1_706_745_599, 3600, "+01:00"), "2024-02-01 00:59:59 +01:00");
        assert_eq!(format_date(0, 0, "UTC"), "1970-01-01 00:00:00 UTC");
    }

    /// A region file with one chunk (0, 0) holding a column of stone and a chest.
    fn region_bytes() -> Vec<u8> {
        use crate::horizon_worldedit_core::nbt::Tag;
        let compound = |pairs: Vec<(&str, Tag)>| Tag::Compound(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        let palette = Tag::List(vec![compound(vec![("Name", Tag::String("minecraft:stone".into()))])]);
        let section = compound(vec![("Y", Tag::Byte(4)), ("block_states", compound(vec![("palette", palette)]))]);
        let chest = compound(vec![("id", Tag::String("minecraft:chest".into())), ("x", Tag::Int(1)), ("y", Tag::Int(64)), ("z", Tag::Int(1))]);
        let chunk = compound(vec![("sections", Tag::List(vec![section])), ("block_entities", Tag::List(vec![chest]))]);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&nbt::write("", &chunk)).unwrap();
        let packed = z.finish().unwrap();
        let mut file = vec![0u8; 8192];
        file[0..4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
        file.extend_from_slice(&(packed.len() as u32 + 1).to_be_bytes());
        file.push(2);
        file.extend_from_slice(&packed);
        file.resize(3 * 4096, 0);
        file
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("we-snap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_world(dir: &Path) {
        std::fs::create_dir_all(dir.join("dimensions/minecraft/overworld/region")).unwrap();
        std::fs::write(dir.join("level.dat"), b"x").unwrap();
        std::fs::write(dir.join("dimensions/minecraft/overworld/region/r.0.0.mca"), region_bytes()).unwrap();
    }

    #[test]
    fn snapshots_are_found_in_every_layout_newest_first() {
        let root = scratch("layouts");
        write_world(&root.join("world"));
        write_world(&root.join("2024-01-02_00-00-00/world"));
        write_world(&root.join("world/2024-03-04_00-00-00"));
        write_world(&root.join("2024-02-03_00-00-00/other"));
        let names: Vec<String> = list(&root, "world").into_iter().map(|s| s.id).collect();
        // The plain world folder holds a level.dat, so it is a snapshot of itself and holds no others; its date is the
        // time it was written (now), so it comes first.
        assert_eq!(names, vec!["world".to_owned(), "2024-01-02_00-00-00/world".to_owned()]);
        assert!(find(&root, "world", "/2024-01-02_00-00-00/world").is_some());
        assert!(find(&root, "world", "nothing").is_none());
        assert_eq!(list(&root, "other").len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_snapshot_inside_a_world_named_folder_is_found() {
        let root = scratch("named");
        std::fs::create_dir_all(root.join("world")).unwrap();
        write_world(&root.join("world/2024-03-04_00-00-00"));
        write_world(&root.join("world/2024-01-01_00-00-00/world"));
        let names: Vec<String> = list(&root, "world").into_iter().map(|s| s.id).collect();
        assert_eq!(names, vec!["world/2024-03-04_00-00-00".to_owned(), "world/2024-01-01_00-00-00/world".to_owned()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_reads_blocks_block_entities_and_counts_missing_chunks() {
        let root = scratch("restore");
        write_world(&root.join("world"));
        let snap = find(&root, "world", "world").unwrap();
        let mut seen: Vec<(Pos, String, bool)> = Vec::new();
        let outcome = restore(&snap, "minecraft:overworld", &[[1, 64, 1], [2, 64, 2], [100, 64, 100]], |p, b, e| seen.push((p, b.name.clone(), e.is_some())));
        assert_eq!(outcome, Outcome { chunks: 2, missing: 1, errors: 0, last_error: None });
        assert!(!outcome.total_failure());
        assert_eq!(seen, vec![([1, 64, 1], "minecraft:stone".to_owned(), true), ([2, 64, 2], "minecraft:stone".to_owned(), false)]);
        // Nothing of the dimension: every chunk is missing, which is a total failure.
        let none = restore(&snap, "minecraft:the_nether", &[[1, 64, 1]], |_, _, _| panic!("no block expected"));
        assert!(none.total_failure());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_zip_snapshot_is_read_without_unpacking_it() {
        let root = scratch("zip");
        let zip_path = root.join("2024-05-06_07-08-09.zip");
        {
            // A stored (uncompressed) entry and a deflated one, written by hand: local headers, then the directory.
            let mut out: Vec<u8> = Vec::new();
            let mut central: Vec<u8> = Vec::new();
            let mut add = |name: &str, data: &[u8], method: u16| {
                let packed = if method == 8 {
                    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                    e.write_all(data).unwrap();
                    e.finish().unwrap()
                } else {
                    data.to_vec()
                };
                let local = out.len() as u32;
                out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
                out.extend_from_slice(&method.to_le_bytes());
                out.extend_from_slice(&[0; 8]);
                out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
                out.extend_from_slice(&(data.len() as u32).to_le_bytes());
                out.extend_from_slice(&(name.len() as u16).to_le_bytes());
                out.extend_from_slice(&[0, 0]);
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&packed);
                central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
                central.extend_from_slice(&method.to_le_bytes());
                central.extend_from_slice(&[0; 8]);
                central.extend_from_slice(&(packed.len() as u32).to_le_bytes());
                central.extend_from_slice(&(data.len() as u32).to_le_bytes());
                central.extend_from_slice(&(name.len() as u16).to_le_bytes());
                central.extend_from_slice(&[0; 12]);
                central.extend_from_slice(&local.to_le_bytes());
                central.extend_from_slice(name.as_bytes());
            };
            add("world/level.dat", b"x", 0);
            add("world/region/r.0.0.mca", &region_bytes(), 8);
            let dir_at = out.len() as u32;
            out.extend_from_slice(&central);
            out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 2, 0, 2, 0]);
            out.extend_from_slice(&(central.len() as u32).to_le_bytes());
            out.extend_from_slice(&dir_at.to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            std::fs::write(&zip_path, out).unwrap();
        }
        let names: Vec<String> = list(&root, "world").into_iter().map(|s| s.id).collect();
        assert_eq!(names, vec!["2024-05-06_07-08-09.zip/world".to_owned()]);
        let snap = find(&root, "world", "2024-05-06_07-08-09.zip/world").unwrap();
        assert_eq!(snap.date, 1_714_979_289);
        let mut stone = 0;
        let outcome = restore(&snap, "minecraft:overworld", &[[1, 64, 1]], |_, b, _| stone += usize::from(b.name == "minecraft:stone"));
        assert_eq!((stone, outcome.missing, outcome.errors), (1, 0, 0));
        let _ = std::fs::remove_dir_all(&root);
    }
}
