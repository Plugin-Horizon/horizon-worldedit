// WorldEdit for Horizon, native: schematic files.
//
// `horizon-mixins` stages this file into `crates/horizon-client/src/game.rs` as
// `mod worldedit_schem`, beside the editor's own module. Nothing here touches
// `Game`: a reader turns a file into blocks through the name resolver the editor
// hands it, and the writer turns blocks into bytes, so the formats stay testable
// on their own.
//
// Three formats, recognized by what the file holds rather than its extension:
//
//   .schem      Sponge (v1, v2, v3): WorldEdit, FAWE, Axiom, most tools
//   .litematic  Litematica: packed palettes per region, signed region sizes
//   .schematic  MCEdit, WorldEdit's legacy format: 1.12 numeric block ids
//
// Blocks come back relative to the paste origin (where `//paste` puts them): a
// Sponge schematic's `Offset` and a legacy file's `WEOffset` are subtracted, so
// the origin recorded when the copy was made lands at the player.
use horizon_world::compression::Compression;
use horizon_world::nbt::{self, Limits, NamedTag, Tag};
use horizon_world::registry::BlockStateId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use super::worldedit_legacy::LEGACY;

type Pos = [i32; 3];

/// The most blocks a schematic may hold: pasting is one edit, so the same cap
/// as `//set` and the rest, and a file bigger than this is a mistake, not a build.
const MAX_BLOCKS: usize = 250_000;

/// A schematic read from a file.
pub(super) struct Loaded {
    /// The format as a player knows it: `Sponge v2`, `Litematica v6`, `MCEdit`.
    pub format: String,
    /// The blocks' bounding box, in blocks.
    pub size: Pos,
    /// Blocks relative to the paste origin.
    pub blocks: Vec<(Pos, BlockStateId)>,
    /// Palette entries this game has no block for (left out, reported).
    pub unknown: usize,
}

/// Reads `path`: Sponge, Litematica or MCEdit, whichever the file is.
pub(super) fn read(
    path: &Path,
    resolve: &dyn Fn(&str) -> Option<BlockStateId>,
) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let raw = decompress(&bytes)?;
    let (named, _) = nbt::read_named(&raw, Limits::templates()).map_err(|e| e.to_string())?;
    let root = &named.tag;
    let loaded = if root.get("Regions").is_some() {
        read_litematic(root, resolve)?
    } else if matches!(root.get("Blocks"), Some(Tag::ByteArray(_))) {
        read_mcedit(root, resolve)?
    } else {
        read_sponge(root, resolve)?
    };
    Ok(loaded)
}

/// Gzip and zlib NBT, and bare NBT.
fn decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let given = match bytes {
        [0x1f, 0x8b, ..] => Compression::Gzip,
        [0x78, ..] => Compression::Zlib,
        _ => Compression::None,
    };
    given
        .decode(bytes, 1 << 28)
        .map_err(|error| format!("not a gzip, zlib or raw NBT file ({error})"))
}

fn int(tag: Option<&Tag>) -> Option<i64> {
    match tag? {
        Tag::Byte(value) => Some(i64::from(*value)),
        Tag::Short(value) => Some(i64::from(*value)),
        Tag::Int(value) => Some(i64::from(*value)),
        Tag::Long(value) => Some(*value),
        _ => None,
    }
}

fn text(tag: Option<&Tag>) -> Option<String> {
    match tag? {
        Tag::String(value) => Some(value.to_string_lossy()),
        _ => None,
    }
}

fn ints(tag: Option<&Tag>) -> Option<Vec<i32>> {
    match tag? {
        Tag::IntArray(values) => Some(values.clone()),
        Tag::List(values) => Some(
            values
                .iter()
                .filter_map(|value| int(Some(value)).map(|v| v as i32))
                .collect(),
        ),
        Tag::ByteArray(values) => Some(values.iter().map(|v| i32::from(*v)).collect()),
        _ => None,
    }
}

fn position(tag: Option<&Tag>) -> Option<Pos> {
    let compound = match tag? {
        tag @ Tag::Compound(_) => tag,
        _ => return None,
    };
    Some([
        compound.get("x").and_then(|t| int(Some(t)))? as i32,
        compound.get("y").and_then(|t| int(Some(t)))? as i32,
        compound.get("z").and_then(|t| int(Some(t)))? as i32,
    ])
}

/// The size a file declares, checked against the limits of a paste.
fn checked_size(width: i64, height: i64, length: i64) -> Result<Pos, String> {
    if width <= 0 || height <= 0 || length <= 0 {
        return Err("the file holds no blocks".to_owned());
    }
    let volume = (width as u128) * (height as u128) * (length as u128);
    if volume > MAX_BLOCKS as u128 {
        return Err(format!(
            "that schematic is {volume} blocks; the most this can load is {MAX_BLOCKS}"
        ));
    }
    Ok([width as i32, height as i32, length as i32])
}

/// `minecraft:oak_stairs[facing=east,half=top]` from a palette entry's `Name`
/// and `Properties`.
fn spec_of(tag: &Tag) -> Option<String> {
    let name = text(tag.get("Name"))?;
    let mut properties: Vec<(String, String)> = Vec::new();
    if let Some(Tag::Compound(map)) = tag.get("Properties") {
        for (key, value) in map {
            properties.push((key.to_string_lossy(), text(Some(value))?));
        }
    }
    properties.sort();
    if properties.is_empty() {
        return Some(name);
    }
    let pairs: Vec<String> = properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    Some(format!("{name}[{}]", pairs.join(",")))
}

/// Sponge, v1 to v3. v3 keeps the palette and the data under `Blocks`, and every
/// version indexes `(y * length + z) * width + x` with varints.
fn read_sponge(
    root: &Tag,
    resolve: &dyn Fn(&str) -> Option<BlockStateId>,
) -> Result<Loaded, String> {
    let body = match root.get("Schematic") {
        Some(tag @ Tag::Compound(_)) => tag,
        _ => root,
    };
    let version = int(body.get("Version")).unwrap_or(1);
    let width = int(body.get("Width")).ok_or("the schematic has no Width")?;
    let height = int(body.get("Height")).ok_or("the schematic has no Height")?;
    let length = int(body.get("Length")).ok_or("the schematic has no Length")?;
    let size = checked_size(width, height, length)?;
    let offset = ints(body.get("Offset"))
        .filter(|values| values.len() == 3)
        .unwrap_or_else(|| vec![0, 0, 0]);
    let inner = body.get("Blocks");
    let palette_tag = inner
        .and_then(|blocks| blocks.get("Palette"))
        .or_else(|| body.get("Palette"))
        .or_else(|| body.get("Palettes").and_then(|p| p.get("Default")))
        .ok_or("the schematic has no block palette")?;
    let data = match inner.and_then(|blocks| blocks.get("Data")) {
        Some(tag) => tag,
        None => body
            .get("BlockData")
            .ok_or("the schematic has no block data")?,
    };
    let Tag::ByteArray(data) = data else {
        return Err("the block data is not a byte array".to_owned());
    };
    let palette = palette_entries(palette_tag, resolve)?;
    let indices = varints(data);
    let volume = (size[0] as usize) * (size[1] as usize) * (size[2] as usize);
    if indices.len() < volume {
        return Err(format!(
            "the schematic holds {} blocks, not {volume}",
            indices.len()
        ));
    }
    let mut blocks = Vec::with_capacity(volume);
    let mut unknown = 0usize;
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let index = ((y as usize * size[2] as usize) + z as usize) * size[0] as usize
                    + x as usize;
                match palette.get(indices[index] as usize).copied().flatten() {
                    Some(state) => blocks.push((
                        [x - offset[0], y - offset[1], z - offset[2]],
                        state,
                    )),
                    None => unknown += 1,
                }
            }
        }
    }
    Ok(Loaded {
        format: format!("Sponge v{version}"),
        size,
        blocks,
        unknown,
    })
}

/// A Sponge palette: index to state, unknown names kept as `None`.
fn palette_entries(
    tag: &Tag,
    resolve: &dyn Fn(&str) -> Option<BlockStateId>,
) -> Result<Vec<Option<BlockStateId>>, String> {
    let Tag::Compound(map) = tag else {
        return Err("the block palette is not a compound".to_owned());
    };
    let mut entries: Vec<(i64, Option<BlockStateId>)> = Vec::with_capacity(map.len());
    for (name, value) in map {
        let index = int(Some(value)).ok_or("a palette index is not a number")?;
        entries.push((index, resolve(&name.to_string_lossy())));
    }
    let highest = entries.iter().map(|(index, _)| *index).max().unwrap_or(-1);
    if highest < 0 || highest as usize > 1 << 20 {
        return Err("the block palette is empty".to_owned());
    }
    let mut out = vec![None; highest as usize + 1];
    for (index, state) in entries {
        out[index as usize] = state;
    }
    Ok(out)
}

/// Minecraft's varints, as Sponge's block data uses them.
fn varints(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut value = 0u32;
    let mut shift = 0u32;
    for &byte in bytes {
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
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

fn put_varint(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Litematica: one compound per region, each with its own palette and a packed
/// `BlockStates` array. `Size` may be negative, for a mirrored region.
fn read_litematic(
    root: &Tag,
    resolve: &dyn Fn(&str) -> Option<BlockStateId>,
) -> Result<Loaded, String> {
    let Tag::Compound(regions) = root
        .get("Regions")
        .ok_or("the litematic has no Regions")?
    else {
        return Err("the litematic's Regions is not a compound".to_owned());
    };
    if regions.is_empty() {
        return Err("the litematic holds no regions".to_owned());
    }
    let mut absolute: Vec<(Pos, BlockStateId)> = Vec::new();
    let mut unknown = 0usize;
    for (_, region) in regions {
        let start = position(region.get("Position")).ok_or("a region has no Position")?;
        let signed = position(region.get("Size")).ok_or("a region has no Size")?;
        let span = [signed[0].abs(), signed[1].abs(), signed[2].abs()];
        if span.iter().any(|side| *side == 0) {
            continue;
        }
        let Tag::List(palette) = region
            .get("BlockStatePalette")
            .ok_or("a region has no BlockStatePalette")?
        else {
            return Err("a region's BlockStatePalette is not a list".to_owned());
        };
        let states: Vec<Option<BlockStateId>> = palette
            .iter()
            .map(|entry| spec_of(entry).and_then(|spec| resolve(&spec)))
            .collect();
        if states.is_empty() {
            return Err("a region's palette is empty".to_owned());
        }
        let volume = (span[0] as usize) * (span[1] as usize) * (span[2] as usize);
        if volume > MAX_BLOCKS {
            return Err(format!(
                "that region is {volume} blocks; the most this can load is {MAX_BLOCKS}"
            ));
        }
        let longs: Vec<i64> = match region.get("BlockStates") {
            Some(Tag::LongArray(values)) => values.clone(),
            Some(Tag::List(values)) => values
                .iter()
                .filter_map(|value| match value {
                    Tag::Long(v) => Some(*v),
                    Tag::Int(v) => Some(i64::from(*v)),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let values = if longs.is_empty() && states.len() == 1 {
            vec![0u32; volume]
        } else {
            unpack(&longs, bits_for(states.len()), volume).ok_or_else(|| {
                format!(
                    "a region's BlockStates holds {} longs, too few for {volume} blocks",
                    longs.len()
                )
            })?
        };
        for y in 0..span[1] {
            for z in 0..span[2] {
                for x in 0..span[0] {
                    let index = ((y as usize * span[2] as usize) + z as usize) * span[0] as usize
                        + x as usize;
                    let Some(state) = states
                        .get(values[index] as usize)
                        .copied()
                        .flatten()
                    else {
                        unknown += 1;
                        continue;
                    };
                    let local = [
                        if signed[0] < 0 { -x } else { x },
                        if signed[1] < 0 { -y } else { y },
                        if signed[2] < 0 { -z } else { z },
                    ];
                    absolute.push((
                        [
                            start[0] + local[0],
                            start[1] + local[1],
                            start[2] + local[2],
                        ],
                        state,
                    ));
                }
            }
        }
    }
    if absolute.is_empty() {
        return Err("the litematic holds no blocks".to_owned());
    }
    let (size, blocks) = normalize(absolute);
    let version = int(root.get("Version")).unwrap_or(0);
    Ok(Loaded {
        format: if version > 0 {
            format!("Litematica v{version}")
        } else {
            "Litematica".to_owned()
        },
        size,
        blocks,
        unknown,
    })
}

/// Litematica's bit width for a palette: at least two bits, otherwise the bits
/// its highest index needs (`Math.max(2, Bits.calcBits(palette))`).
fn bits_for(palette: usize) -> u32 {
    let highest = palette.max(1) - 1;
    let needed = usize::BITS - (highest | 1).leading_zeros();
    needed.clamp(2, 32)
}

/// Values packed across `long` boundaries, least significant bit first, as
/// Litematica and 1.16+ palettes store them.
fn unpack(longs: &[i64], bits: u32, count: usize) -> Option<Vec<u32>> {
    if bits == 0 || bits > 32 {
        return None;
    }
    let needed = (count as u128 * bits as u128).div_ceil(64) as usize;
    if longs.len() < needed {
        return None;
    }
    let mask = if bits == 32 {
        u64::from(u32::MAX)
    } else {
        (1u64 << bits) - 1
    };
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let start = index as u64 * u64::from(bits);
        let slot = (start / 64) as usize;
        let shift = start % 64;
        let mut value = (longs[slot] as u64) >> shift;
        if shift + u64::from(bits) > 64 {
            let next = longs.get(slot + 1).copied().unwrap_or(0) as u64;
            value |= next << (64 - shift);
        }
        out.push((value & mask) as u32);
    }
    Some(out)
}

/// MCEdit and WorldEdit's legacy `.schematic`: one byte per block from 1.12's
/// numeric ids, nibbles for the data (and the high id bits, when `AddBlocks`
/// holds them).
fn read_mcedit(
    root: &Tag,
    resolve: &dyn Fn(&str) -> Option<BlockStateId>,
) -> Result<Loaded, String> {
    let width = int(root.get("Width")).ok_or("the schematic has no Width")?;
    let height = int(root.get("Height")).ok_or("the schematic has no Height")?;
    let length = int(root.get("Length")).ok_or("the schematic has no Length")?;
    let size = checked_size(width, height, length)?;
    let Some(Tag::ByteArray(ids)) = root.get("Blocks") else {
        return Err("the schematic has no Blocks".to_owned());
    };
    let data: &[u8] = match root.get("Data") {
        Some(Tag::ByteArray(values)) => values,
        _ => &[],
    };
    let added: &[u8] = match root.get("AddBlocks") {
        Some(Tag::ByteArray(values)) => values,
        _ => &[],
    };
    let offset = ints(root.get("WEOffset"))
        .or_else(|| ints(root.get("Offset")))
        .filter(|values| values.len() == 3)
        .unwrap_or_else(|| vec![0, 0, 0]);
    let volume = (size[0] as usize) * (size[1] as usize) * (size[2] as usize);
    if ids.len() < volume {
        return Err(format!(
            "the schematic holds {} blocks, not {volume}",
            ids.len()
        ));
    }
    let mut blocks = Vec::with_capacity(volume);
    let mut unknown = 0usize;
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let index = ((y as usize * size[2] as usize) + z as usize) * size[0] as usize
                    + x as usize;
                let mut id = i32::from(ids[index]);
                if !added.is_empty() {
                    let byte = added[index / 2];
                    let high = if index % 2 == 0 { byte & 0x0f } else { byte >> 4 };
                    id |= i32::from(high) << 8;
                }
                let nibble = match data.get(index / 2) {
                    Some(byte) => {
                        if index % 2 == 0 {
                            byte & 0x0f
                        } else {
                            byte >> 4
                        }
                    }
                    None => 0,
                };
                let legacy = (id << 4) | i32::from(nibble);
                if !(0..4096).contains(&legacy) {
                    unknown += 1;
                    continue;
                }
                let (name, properties) = legacy_state(legacy as usize);
                let spec = if properties.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}[{}]", properties.replace(';', ","))
                };
                match resolve(&spec) {
                    Some(state) => blocks.push((
                        [x - offset[0], y - offset[1], z - offset[2]],
                        state,
                    )),
                    None => unknown += 1,
                }
            }
        }
    }
    Ok(Loaded {
        format: "MCEdit".to_owned(),
        size,
        blocks,
        unknown,
    })
}

/// The 1.12 state `id << 4 | data` as a name and `key=value;...` properties,
/// from the table generated next to this file.
fn legacy_state(id: usize) -> (&'static str, &'static str) {
    static TABLE: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut out = vec![("minecraft:air", ""); 4096];
        for line in LEGACY.lines() {
            let mut fields = line.split('\t');
            let Some(id) = fields.next().and_then(|f| f.trim().parse::<usize>().ok()) else {
                continue;
            };
            let name = fields.next().unwrap_or("minecraft:air");
            let properties = fields.next().unwrap_or("");
            if let Some(slot) = out.get_mut(id) {
                *slot = (name, properties);
            }
        }
        out
    });
    table.get(id).copied().unwrap_or(("minecraft:air", ""))
}

/// Shifts blocks so their low corner is at `0, 0, 0`, and reports the box.
fn normalize(blocks: Vec<(Pos, BlockStateId)>) -> (Pos, Vec<(Pos, BlockStateId)>) {
    let mut low = [i32::MAX; 3];
    let mut high = [i32::MIN; 3];
    for (pos, _) in &blocks {
        for axis in 0..3 {
            low[axis] = low[axis].min(pos[axis]);
            high[axis] = high[axis].max(pos[axis]);
        }
    }
    let shifted = blocks
        .into_iter()
        .map(|(pos, state)| {
            (
                [pos[0] - low[0], pos[1] - low[1], pos[2] - low[2]],
                state,
            )
        })
        .collect();
    (
        [
            high[0] - low[0] + 1,
            high[1] - low[1] + 1,
            high[2] - low[2] + 1,
        ],
        shifted,
    )
}

/// A Sponge v2 `.schem` from `order`, which holds the blocks in Sponge's index
/// order (`(y * length + z) * width + x`), each named by `describe`.
pub(super) fn write_sponge(
    size: Pos,
    order: &[BlockStateId],
    describe: &dyn Fn(BlockStateId) -> String,
) -> Result<Vec<u8>, String> {
    if order.len() != (size[0] as usize) * (size[1] as usize) * (size[2] as usize) {
        return Err("the selection and its blocks disagree".to_owned());
    }
    let mut palette: Vec<(String, i32)> = Vec::new();
    let mut index_of: HashMap<String, i32> = HashMap::new();
    let mut data = Vec::with_capacity(order.len());
    for &state in order {
        let spec = describe(state);
        let index = match index_of.get(&spec) {
            Some(&index) => index,
            None => {
                let index = palette.len() as i32;
                index_of.insert(spec.clone(), index);
                palette.push((spec, index));
                index
            }
        };
        put_varint(&mut data, index as u32);
    }
    let entries: Vec<(&str, Tag)> = palette
        .iter()
        .map(|(spec, index)| (spec.as_str(), Tag::Int(*index)))
        .collect();
    let body = Tag::compound([
        ("Version", Tag::Int(2)),
        ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ("Width", Tag::Short(size[0] as i16)),
        ("Height", Tag::Short(size[1] as i16)),
        ("Length", Tag::Short(size[2] as i16)),
        ("Offset", Tag::IntArray(vec![0, 0, 0])),
        ("PaletteMax", Tag::Int(palette.len() as i32)),
        ("Palette", Tag::compound(entries)),
        ("BlockData", Tag::ByteArray(data)),
    ]);
    let named = NamedTag {
        name: "Schematic".into(),
        tag: body,
    };
    let bytes = nbt::write_named(&named, Limits::templates()).map_err(|e| e.to_string())?;
    Compression::Gzip
        .encode(&bytes)
        .map_err(|error| error.to_string())
}
