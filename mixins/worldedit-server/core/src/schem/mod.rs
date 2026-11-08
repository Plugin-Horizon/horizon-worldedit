//! Schematic files: Sponge `.schem` (v1 to v3, WorldEdit's own), Litematica `.litematic`, MCEdit
//! `.schematic` (1.12 block ids) and vanilla structure `.nbt`, read; Sponge v3 written, the format
//! WorldEdit 7.3 and later writes, so files go back and forth with a Paper server's WorldEdit.
//!
//! The Sponge, Litematica and MCEdit readers and the 1.12 table (`legacy_ids.rs`) come from the
//! upstream bundle's `schematic.rs` (XnotCykoX/WorldEdit-Horizon, MIT), moved onto horizonmatica's
//! NBT (`crate::nbt`). Changed here: where a paste lands. WorldEdit keeps the point the player stood
//! on when copying (the clipboard's origin): a v3 file's `Offset` is the box's corner relative to it,
//! a v2 file keeps it in `Metadata.WEOffsetX/Y/Z`, a `.schematic` in `WEOffsetX/Y/Z`, and a paste
//! puts the origin at the player, as `//paste` in WorldEdit does.

mod legacy_ids;

use crate::block::BlockState;
use crate::nbt::{self, Tag};
use crate::world::Pos;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// A file's blocks: positions relative to the paste origin, each an index into `palette`.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    /// `Sponge v3`, `Litematica v6`, `MCEdit`, `Structure`.
    pub format: String,
    pub size: Pos,
    pub palette: Vec<BlockState>,
    pub blocks: Vec<(Pos, u32)>,
}

/// The most a file may unpack to (bytes).
const MAX_UNPACKED: usize = 1 << 30;

/// Reads a schematic from its bytes, whatever of the four formats it is (the contents decide,
/// not the name); refuses one of more than `max_blocks` blocks.
pub fn read(bytes: &[u8], max_blocks: u64) -> Result<Loaded, String> {
    let root = if bytes.starts_with(&[0x78]) {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(bytes)
            .take(MAX_UNPACKED as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| format!("zlib: {e}"))?;
        nbt::read(&out)?.1
    } else {
        nbt::read_file_bytes(bytes, MAX_UNPACKED)?
    };
    if root.get("Regions").is_some() {
        read_litematic(&root, max_blocks)
    } else if matches!(root.get("Blocks"), Some(Tag::ByteArray(_))) {
        read_mcedit(&root, max_blocks)
    } else if root.get("palette").is_some() || root.get("palettes").is_some() {
        read_structure(&root, max_blocks)
    } else {
        read_sponge(&root, max_blocks)
    }
}

fn int(tag: Option<&Tag>) -> Option<i64> {
    tag?.int()
}

fn ints(tag: Option<&Tag>) -> Option<Vec<i64>> {
    match tag? {
        Tag::IntArray(v) => Some(v.iter().map(|&x| x.into()).collect()),
        Tag::List(v) => Some(v.iter().filter_map(|t| t.int()).collect()),
        Tag::ByteArray(v) => Some(v.iter().map(|&x| x.into()).collect()),
        _ => None,
    }
}

fn triple(tag: Option<&Tag>) -> Option<Pos> {
    let v = ints(tag).filter(|v| v.len() == 3)?;
    Some([v[0] as i32, v[1] as i32, v[2] as i32])
}

fn xyz(tag: Option<&Tag>) -> Option<Pos> {
    let t = tag?;
    Some([int(t.get("x"))? as i32, int(t.get("y"))? as i32, int(t.get("z"))? as i32])
}

fn checked_size(w: i64, h: i64, l: i64, max_blocks: u64) -> Result<Pos, String> {
    if w <= 0 || h <= 0 || l <= 0 {
        return Err("the file holds no blocks".into());
    }
    let volume = (w as u128) * (h as u128) * (l as u128);
    if volume > u128::from(max_blocks) {
        return Err(format!("that schematic is {volume} blocks; the most this server loads is {max_blocks}"));
    }
    Ok([w as i32, h as i32, l as i32])
}

/// Sponge's (and 1.16+ chunks') varints.
fn varints(bytes: &[i8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(bytes.len());
    let (mut value, mut shift) = (0u32, 0u32);
    for &b in bytes {
        let b = b as u8;
        value |= u32::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            out.push(value);
            value = 0;
            shift = 0;
        } else {
            shift += 7;
            if shift > 28 {
                value = 0;
                shift = 0;
            }
        }
    }
    out
}

fn put_varint(out: &mut Vec<i8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte as i8);
            return;
        }
        out.push((byte | 0x80) as i8);
    }
}

/// A Sponge palette (`name[props]` to index) as a list by index; names that don't parse are air.
fn sponge_palette(tag: &Tag) -> Result<Vec<BlockState>, String> {
    let map = tag.compound().ok_or("the block palette is not a compound")?;
    let mut entries: Vec<(i64, BlockState)> = Vec::with_capacity(map.len());
    for (name, value) in map {
        let index = value.int().ok_or("a palette index is not a number")?;
        entries.push((index, BlockState::parse(name).unwrap_or_else(BlockState::air)));
    }
    let highest = entries.iter().map(|(i, _)| *i).max().unwrap_or(-1);
    if highest < 0 || highest > 1 << 20 {
        return Err("the block palette is empty".into());
    }
    let mut out = vec![BlockState::air(); highest as usize + 1];
    for (i, s) in entries {
        if i >= 0 {
            out[i as usize] = s;
        }
    }
    Ok(out)
}

fn read_sponge(root: &Tag, max_blocks: u64) -> Result<Loaded, String> {
    let body = match root.get("Schematic") {
        Some(t @ Tag::Compound(_)) => t,
        _ => root,
    };
    let version = int(body.get("Version")).unwrap_or(1);
    let size = checked_size(
        int(body.get("Width")).ok_or("the schematic has no Width")? & 0xffff,
        int(body.get("Height")).ok_or("the schematic has no Height")? & 0xffff,
        int(body.get("Length")).ok_or("the schematic has no Length")? & 0xffff,
        max_blocks,
    )?;
    let inner = body.get("Blocks");
    let palette_tag = inner
        .and_then(|b| b.get("Palette"))
        .or_else(|| body.get("Palette"))
        .ok_or("the schematic has no block palette")?;
    let data = inner.and_then(|b| b.get("Data")).or_else(|| body.get("BlockData")).ok_or("the schematic has no block data")?;
    let Tag::ByteArray(data) = data else { return Err("the block data is not a byte array".into()) };
    // Where the box lies from the origin (see the module comment).
    let shift = if version >= 3 {
        triple(body.get("Offset")).unwrap_or([0; 3])
    } else {
        let meta = body.get("Metadata");
        match (int(meta.and_then(|m| m.get("WEOffsetX"))), int(meta.and_then(|m| m.get("WEOffsetY"))), int(meta.and_then(|m| m.get("WEOffsetZ")))) {
            (Some(x), Some(y), Some(z)) => [x as i32, y as i32, z as i32],
            _ => [0; 3],
        }
    };
    let palette = sponge_palette(palette_tag)?;
    let indices = varints(data);
    let volume = size.iter().map(|&v| v as usize).product::<usize>();
    if indices.len() < volume {
        return Err(format!("the schematic holds {} blocks, not {volume}", indices.len()));
    }
    let mut blocks = Vec::with_capacity(volume);
    let (w, l) = (size[0] as usize, size[2] as usize);
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let i = (y as usize * l + z as usize) * w + x as usize;
                let p = indices[i].min(palette.len() as u32 - 1);
                blocks.push(([x + shift[0], y + shift[1], z + shift[2]], p));
            }
        }
    }
    Ok(Loaded { format: format!("Sponge v{version}"), size, palette, blocks })
}

/// `minecraft:oak_stairs[facing=east]` from a palette entry's `Name` and `Properties`.
fn named_state(tag: &Tag) -> BlockState {
    let Some(name) = tag.get("Name").and_then(Tag::str) else { return BlockState::air() };
    let mut properties = BTreeMap::new();
    if let Some(map) = tag.get("Properties").and_then(Tag::compound) {
        for (k, v) in map {
            if let Some(v) = v.str() {
                properties.insert(k.clone(), v.to_owned());
            }
        }
    }
    let name = if name.contains(':') { name.to_owned() } else { format!("minecraft:{name}") };
    BlockState { name, properties }
}

/// Litematica's bit width for a palette (`Math.max(2, Bits.calcBits(palette))`).
fn bits_for(palette: usize) -> u32 {
    let highest = palette.max(1) - 1;
    (usize::BITS - (highest | 1).leading_zeros()).clamp(2, 32)
}

/// Values packed across long boundaries, least significant bit first.
fn unpack(longs: &[i64], bits: u32, count: usize) -> Option<Vec<u32>> {
    let needed = (count as u128 * bits as u128).div_ceil(64) as usize;
    if bits == 0 || bits > 32 || longs.len() < needed {
        return None;
    }
    let mask = if bits == 32 { u64::from(u32::MAX) } else { (1u64 << bits) - 1 };
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let start = i as u64 * u64::from(bits);
        let (slot, shift) = ((start / 64) as usize, start % 64);
        let mut v = (longs[slot] as u64) >> shift;
        if shift + u64::from(bits) > 64 {
            v |= (longs.get(slot + 1).copied().unwrap_or(0) as u64) << (64 - shift);
        }
        out.push((v & mask) as u32);
    }
    Some(out)
}

fn read_litematic(root: &Tag, max_blocks: u64) -> Result<Loaded, String> {
    let regions = root.get("Regions").and_then(Tag::compound).ok_or("the litematic's Regions is not a compound")?;
    let mut palette: Vec<BlockState> = vec![BlockState::air()];
    let mut slot: BTreeMap<BlockState, u32> = BTreeMap::from([(BlockState::air(), 0)]);
    let mut absolute: Vec<(Pos, u32)> = Vec::new();
    let mut total: u64 = 0;
    for region in regions.values() {
        let start = xyz(region.get("Position")).ok_or("a region has no Position")?;
        let signed = xyz(region.get("Size")).ok_or("a region has no Size")?;
        let span = signed.map(i32::abs);
        if span.contains(&0) {
            continue;
        }
        let states: Vec<u32> = region
            .get("BlockStatePalette")
            .and_then(Tag::list)
            .ok_or("a region has no BlockStatePalette")?
            .iter()
            .map(|e| {
                let s = named_state(e);
                *slot.entry(s.clone()).or_insert_with(|| {
                    palette.push(s);
                    palette.len() as u32 - 1
                })
            })
            .collect();
        let volume = span.iter().map(|&v| v as usize).product::<usize>();
        total += volume as u64;
        if total > max_blocks {
            return Err(format!("that litematic is more than {max_blocks} blocks, the most this server loads"));
        }
        let longs = region.get("BlockStates").and_then(Tag::longs).unwrap_or(&[]);
        let values = if longs.is_empty() && states.len() == 1 {
            vec![0u32; volume]
        } else {
            unpack(longs, bits_for(states.len()), volume).ok_or("a region's BlockStates is too short")?
        };
        // Position is one corner; a negative Size runs from it toward smaller coordinates.
        let min: Pos = std::array::from_fn(|a| if signed[a] < 0 { start[a] + signed[a] + 1 } else { start[a] });
        let (w, l) = (span[0] as usize, span[2] as usize);
        for y in 0..span[1] {
            for z in 0..span[2] {
                for x in 0..span[0] {
                    let i = (y as usize * l + z as usize) * w + x as usize;
                    let state = states.get(values[i] as usize).copied().unwrap_or(0);
                    absolute.push(([min[0] + x, min[1] + y, min[2] + z], state));
                }
            }
        }
    }
    if absolute.is_empty() {
        return Err("the litematic holds no blocks".into());
    }
    let (size, blocks) = normalize(absolute);
    let version = int(root.get("Version")).unwrap_or(0);
    let format = if version > 0 { format!("Litematica v{version}") } else { "Litematica".into() };
    Ok(Loaded { format, size, palette, blocks })
}

/// The 1.12 state `id << 4 | data` as a 1.13 name and `key=value;...`.
fn legacy_state(id: usize) -> BlockState {
    static TABLE: OnceLock<Vec<BlockState>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut out = vec![BlockState::air(); 4096];
        for line in legacy_ids::LEGACY.lines() {
            let mut f = line.split('\t');
            let Some(i) = f.next().and_then(|v| v.trim().parse::<usize>().ok()) else { continue };
            let name = f.next().unwrap_or("minecraft:air");
            let props = f.next().unwrap_or("").replace(';', ",");
            let spec = if props.is_empty() { name.to_owned() } else { format!("{name}[{props}]") };
            if let (Some(slot), Some(s)) = (out.get_mut(i), BlockState::parse(&spec)) {
                *slot = s;
            }
        }
        out
    });
    table.get(id).cloned().unwrap_or_else(BlockState::air)
}

fn read_mcedit(root: &Tag, max_blocks: u64) -> Result<Loaded, String> {
    let size = checked_size(
        int(root.get("Width")).ok_or("the schematic has no Width")?,
        int(root.get("Height")).ok_or("the schematic has no Height")?,
        int(root.get("Length")).ok_or("the schematic has no Length")?,
        max_blocks,
    )?;
    let Some(Tag::ByteArray(ids)) = root.get("Blocks") else { return Err("the schematic has no Blocks".into()) };
    let data: &[i8] = match root.get("Data") {
        Some(Tag::ByteArray(v)) => v,
        _ => &[],
    };
    let added: &[i8] = match root.get("AddBlocks") {
        Some(Tag::ByteArray(v)) => v,
        _ => &[],
    };
    let shift: Pos = match (int(root.get("WEOffsetX")), int(root.get("WEOffsetY")), int(root.get("WEOffsetZ"))) {
        (Some(x), Some(y), Some(z)) => [x as i32, y as i32, z as i32],
        _ => [0; 3],
    };
    let volume = size.iter().map(|&v| v as usize).product::<usize>();
    if ids.len() < volume {
        return Err(format!("the schematic holds {} blocks, not {volume}", ids.len()));
    }
    let nibble = |v: &[i8], i: usize| v.get(i / 2).map_or(0u8, |&b| if i % 2 == 0 { b as u8 & 0x0f } else { b as u8 >> 4 });
    let mut palette: Vec<BlockState> = Vec::new();
    let mut slot: BTreeMap<usize, u32> = BTreeMap::new();
    let mut blocks = Vec::with_capacity(volume);
    let (w, l) = (size[0] as usize, size[2] as usize);
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let i = (y as usize * l + z as usize) * w + x as usize;
                let mut id = usize::from(ids[i] as u8);
                if !added.is_empty() {
                    id |= usize::from(nibble(added, i)) << 8;
                }
                let legacy = ((id << 4) | usize::from(nibble(data, i))).min(4095);
                let p = *slot.entry(legacy).or_insert_with(|| {
                    palette.push(legacy_state(legacy));
                    palette.len() as u32 - 1
                });
                blocks.push(([x + shift[0], y + shift[1], z + shift[2]], p));
            }
        }
    }
    Ok(Loaded { format: "MCEdit".into(), size, palette, blocks })
}

/// A vanilla structure (`/place template`, a structure block's save): `size`, `palette` (or the
/// first of `palettes`) and `blocks` with `pos` and `state`. Cells it doesn't list stay as they are
/// in the world, as structure voids do.
fn read_structure(root: &Tag, max_blocks: u64) -> Result<Loaded, String> {
    let size = triple(root.get("size")).ok_or("the structure has no size")?;
    checked_size(size[0].into(), size[1].into(), size[2].into(), max_blocks)?;
    let palette_list = root
        .get("palette")
        .and_then(Tag::list)
        .or_else(|| root.get("palettes").and_then(Tag::list).and_then(|p| p.first()).and_then(Tag::list))
        .ok_or("the structure has no palette")?;
    let palette: Vec<BlockState> = palette_list.iter().map(named_state).collect();
    let mut blocks = Vec::new();
    for b in root.get("blocks").and_then(Tag::list).unwrap_or(&[]) {
        let (Some(p), Some(s)) = (triple(b.get("pos")), int(b.get("state"))) else { continue };
        if (0..palette.len() as i64).contains(&s) {
            blocks.push((p, s as u32));
        }
    }
    Ok(Loaded { format: "Structure".into(), size, palette, blocks })
}

/// Blocks shifted so their low corner is at 0, 0, 0, and the box they fill.
fn normalize(blocks: Vec<(Pos, u32)>) -> (Pos, Vec<(Pos, u32)>) {
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for (p, _) in &blocks {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let shifted = blocks.into_iter().map(|(p, s)| ([p[0] - lo[0], p[1] - lo[1], p[2] - lo[2]], s)).collect();
    ([hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1], shifted)
}

/// A Sponge v3 `.schem`: `cells` in Sponge's order (`(y * length + z) * width + x`), each an index
/// into `palette` (full state names); `offset` is the box's low corner relative to the origin and
/// `origin` where that was in the world (WorldEdit's `Metadata.WorldEdit.Origin`).
pub fn write_sponge3(size: Pos, offset: Pos, origin: Pos, data_version: i32, palette: &[String], cells: &[u32], date_ms: i64) -> Result<Vec<u8>, String> {
    if size.iter().any(|&s| !(1..=65535).contains(&s)) {
        return Err("a schematic is at most 65535 blocks along each side".into());
    }
    if cells.len() != size.iter().map(|&v| v as usize).product::<usize>() {
        return Err("the blocks and the size disagree".into());
    }
    let mut data = Vec::with_capacity(cells.len());
    for &c in cells {
        put_varint(&mut data, c);
    }
    let palette_tag = Tag::Compound(palette.iter().enumerate().map(|(i, s)| (s.clone(), Tag::Int(i as i32))).collect());
    let worldedit = Tag::Compound(BTreeMap::from([
        ("Origin".to_string(), Tag::IntArray(origin.to_vec())),
        ("Version".to_string(), Tag::String(concat!("horizon-worldedit ", env!("CARGO_PKG_VERSION")).into())),
        ("EditingPlatform".to_string(), Tag::String("horizon".into())),
    ]));
    let body = Tag::Compound(BTreeMap::from([
        ("Version".to_string(), Tag::Int(3)),
        ("DataVersion".to_string(), Tag::Int(data_version)),
        ("Width".to_string(), Tag::Short(size[0] as u16 as i16)),
        ("Height".to_string(), Tag::Short(size[1] as u16 as i16)),
        ("Length".to_string(), Tag::Short(size[2] as u16 as i16)),
        ("Offset".to_string(), Tag::IntArray(offset.to_vec())),
        ("Metadata".to_string(), Tag::Compound(BTreeMap::from([("WorldEdit".to_string(), worldedit), ("Date".to_string(), Tag::Long(date_ms))]))),
        (
            "Blocks".to_string(),
            Tag::Compound(BTreeMap::from([
                ("Palette".to_string(), palette_tag),
                ("Data".to_string(), Tag::ByteArray(data)),
                ("BlockEntities".to_string(), Tag::List(Vec::new())),
            ])),
        ),
    ]));
    let root = Tag::Compound(BTreeMap::from([("Schematic".to_string(), body)]));
    Ok(nbt::write_gzip("", &root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(items: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    #[test]
    fn sponge_v3_round_trip_keeps_the_origin() {
        let palette = vec!["minecraft:air".to_string(), "minecraft:stone".to_string(), "minecraft:oak_stairs[facing=east,half=bottom]".to_string()];
        // A 2x1x2 box whose low corner lay 1 west and 2 below the player.
        let cells = vec![1, 0, 2, 1];
        let bytes = write_sponge3([2, 1, 2], [-1, -2, 0], [100, 64, 100], 4440, &palette, &cells, 0).unwrap();
        let l = read(&bytes, 1000).unwrap();
        assert_eq!(l.format, "Sponge v3");
        assert_eq!(l.size, [2, 1, 2]);
        let at = |p: Pos| l.palette[l.blocks.iter().find(|b| b.0 == p).unwrap().1 as usize].to_string_full();
        assert_eq!(at([-1, -2, 0]), "minecraft:stone");
        assert_eq!(at([-1, -2, 1]), "minecraft:oak_stairs[facing=east,half=bottom]");
        assert_eq!(at([0, -2, 0]), "minecraft:air");
        assert!(read(&bytes, 3).is_err());
    }

    #[test]
    fn sponge_v2_uses_the_worldedit_offset() {
        let root = compound(vec![
            ("Version", Tag::Int(2)),
            ("Width", Tag::Short(1)),
            ("Height", Tag::Short(1)),
            ("Length", Tag::Short(2)),
            ("Offset", Tag::IntArray(vec![500, 70, -20])),
            ("Metadata", compound(vec![("WEOffsetX", Tag::Int(3)), ("WEOffsetY", Tag::Int(0)), ("WEOffsetZ", Tag::Int(-1))])),
            ("Palette", compound(vec![("minecraft:dirt", Tag::Int(0)), ("minecraft:glass", Tag::Int(1))])),
            ("BlockData", Tag::ByteArray(vec![1, 0])),
        ]);
        let l = read(&nbt::write_gzip("Schematic", &root), 100).unwrap();
        assert_eq!(l.format, "Sponge v2");
        assert_eq!(l.blocks, vec![([3, 0, -1], 1), ([3, 0, 0], 0)]);
        assert_eq!(l.palette[1].name, "minecraft:glass");
    }

    #[test]
    fn litematic_regions() {
        let region = |pos: Pos, size: Pos, longs: Vec<i64>| {
            compound(vec![
                ("Position", compound(vec![("x", Tag::Int(pos[0])), ("y", Tag::Int(pos[1])), ("z", Tag::Int(pos[2]))])),
                ("Size", compound(vec![("x", Tag::Int(size[0])), ("y", Tag::Int(size[1])), ("z", Tag::Int(size[2]))])),
                ("BlockStatePalette", Tag::List(vec![compound(vec![("Name", Tag::String("minecraft:air".into()))]), compound(vec![("Name", Tag::String("minecraft:stone".into()))])])),
                ("BlockStates", Tag::LongArray(longs)),
            ])
        };
        // Two bits a block: 0b01 is stone. Region a: 2x1x1 both stone; region b mirrored in x.
        let root = compound(vec![
            ("Version", Tag::Int(6)),
            ("Regions", compound(vec![("a", region([0, 0, 0], [2, 1, 1], vec![0b0101])), ("b", region([5, 0, 0], [-2, 1, 1], vec![0b0001]))])),
        ]);
        let l = read(&nbt::write_gzip("", &root), 100).unwrap();
        assert_eq!(l.format, "Litematica v6");
        assert_eq!(l.size, [6, 1, 1]);
        let stone: Vec<Pos> = l.blocks.iter().filter(|b| l.palette[b.1 as usize].name == "minecraft:stone").map(|b| b.0).collect();
        assert_eq!(stone, vec![[0, 0, 0], [1, 0, 0], [4, 0, 0]]);
    }

    #[test]
    fn mcedit_and_structure() {
        let root = compound(vec![
            ("Width", Tag::Short(2)),
            ("Height", Tag::Short(1)),
            ("Length", Tag::Short(1)),
            ("Blocks", Tag::ByteArray(vec![1, 5])),
            ("Data", Tag::ByteArray(vec![0x00])),
            ("WEOffsetX", Tag::Int(-1)),
            ("WEOffsetY", Tag::Int(0)),
            ("WEOffsetZ", Tag::Int(0)),
        ]);
        let l = read(&nbt::write_gzip("Schematic", &root), 100).unwrap();
        assert_eq!(l.format, "MCEdit");
        let names: Vec<String> = l.blocks.iter().map(|b| l.palette[b.1 as usize].name.clone()).collect();
        assert_eq!(names, vec!["minecraft:stone", "minecraft:oak_planks"]);
        assert_eq!(l.blocks[0].0, [-1, 0, 0]);
        let s = compound(vec![
            ("size", Tag::List(vec![Tag::Int(1), Tag::Int(2), Tag::Int(1)])),
            ("palette", Tag::List(vec![compound(vec![("Name", Tag::String("minecraft:glass".into()))])])),
            ("blocks", Tag::List(vec![compound(vec![("pos", Tag::List(vec![Tag::Int(0), Tag::Int(1), Tag::Int(0)])), ("state", Tag::Int(0))])])),
        ]);
        let l = read(&nbt::write_gzip("", &s), 100).unwrap();
        assert_eq!((l.format.as_str(), l.blocks.clone()), ("Structure", vec![([0, 1, 0], 0)]));
        assert!(read(b"not nbt", 100).is_err());
    }
}
