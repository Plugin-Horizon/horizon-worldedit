//! Blocks out of a saved world, for `/snapshot` and `//restore`: the region files (Anvil, `r.<x>.<z>.mca`) of a world
//! folder or a backup of one, and the paletted block sections of their chunks (Minecraft 1.18 and newer: no `Level`
//! wrapper, `sections[].block_states` with a palette and packed indices). Ported in shape from WorldEdit's
//! `McRegionReader` and `AnvilChunk`.

use crate::horizon_worldedit_core::block::BlockState;
use crate::horizon_worldedit_core::nbt::{self, Tag};
use crate::horizon_worldedit_core::world::Pos;
use std::collections::HashMap;
use std::io::Read;

/// The sector table of a region file is 1024 entries of 4 bytes, then the timestamps: 8 KiB of header.
const HEADER: usize = 8192;
/// A chunk bigger than this (unpacked) is refused: a corrupt length must not eat the server's memory.
const MAX_CHUNK: usize = 64 << 20;

/// The file name of the region holding chunk `cx`, `cz`.
pub fn region_file(cx: i32, cz: i32) -> String {
    format!("r.{}.{}.mca", cx.div_euclid(32), cz.div_euclid(32))
}

/// The NBT of chunk (`cx`, `cz`) from the bytes of the region file that holds it. `Ok(None)`: the region has no such
/// chunk (never saved); `Err`: the file or the chunk is damaged or in a format this reader does not know.
pub fn chunk_nbt(region: &[u8], cx: i32, cz: i32) -> Result<Option<Tag>, String> {
    if region.len() < HEADER {
        return Err("region file is too short".into());
    }
    let slot = 4 * (cx.rem_euclid(32) as usize + 32 * cz.rem_euclid(32) as usize);
    let entry = u32::from_be_bytes(region[slot..slot + 4].try_into().unwrap());
    if entry == 0 {
        return Ok(None);
    }
    let start = (entry >> 8) as usize * 4096;
    let head = region.get(start..start + 5).ok_or("chunk lies outside the region file")?;
    let length = u32::from_be_bytes(head[..4].try_into().unwrap()) as usize;
    if length == 0 {
        return Ok(None);
    }
    let compression = head[4];
    // A chunk too big for its sectors lives in its own `c.<x>.<z>.mcc` file (compression + 128).
    if compression > 127 {
        return Err("chunk is stored in an external .mcc file".into());
    }
    let body = region.get(start + 5..start + 4 + length).ok_or("chunk is cut off")?;
    let mut bytes = Vec::new();
    match compression {
        1 => {
            flate2::read::GzDecoder::new(body).take(MAX_CHUNK as u64).read_to_end(&mut bytes).map_err(|e| format!("chunk data: {e}"))?;
        }
        2 => {
            flate2::read::ZlibDecoder::new(body).take(MAX_CHUNK as u64).read_to_end(&mut bytes).map_err(|e| format!("chunk data: {e}"))?;
        }
        3 => bytes = body.to_vec(),
        4 => return Err("chunk is LZ4-compressed, which is not read".into()),
        other => return Err(format!("unknown chunk compression {other}")),
    }
    nbt::read(&bytes).map(|(_, tag)| Some(tag))
}

/// One 16x16x16 section's blocks: the palette and, unless the palette has one entry, the index of each block.
#[derive(Debug)]
struct Section {
    palette: Vec<BlockState>,
    /// `y * 256 + z * 16 + x`.
    indices: Option<Vec<u16>>,
}

/// The blocks and block entities of one chunk.
#[derive(Debug)]
pub struct Chunk {
    sections: HashMap<i32, Section>,
    entities: HashMap<Pos, Tag>,
    air: BlockState,
}

fn palette_entry(tag: &Tag) -> BlockState {
    let name = tag.get("Name").and_then(Tag::str).unwrap_or("minecraft:air");
    let mut state = BlockState::new(name, &[]);
    if let Some(props) = tag.get("Properties").and_then(Tag::compound) {
        for (k, v) in props {
            if let Some(v) = v.str() {
                state.properties.insert(k.clone(), v.to_owned());
            }
        }
    }
    state
}

/// The bits one index of a palette of `len` entries takes in a block section (at least four).
fn bits_for(len: usize) -> usize {
    let needed = usize::BITS - len.saturating_sub(1).leading_zeros();
    (needed as usize).max(4)
}

impl Chunk {
    /// Reads the sections and block entities of a chunk's NBT.
    pub fn from_nbt(tag: &Tag) -> Result<Chunk, String> {
        // Chunks before 1.18 keep everything under `Level` and number their blocks differently: not read.
        if tag.get("Level").is_some() {
            return Err("chunk is in the format before 1.18".into());
        }
        let mut chunk = Chunk { sections: HashMap::new(), entities: HashMap::new(), air: BlockState::air() };
        for section in tag.get("sections").and_then(Tag::list).unwrap_or(&[]) {
            let y = section.get("Y").and_then(Tag::int).ok_or("section has no Y")? as i32;
            let Some(states) = section.get("block_states") else { continue };
            let palette: Vec<BlockState> = states.get("palette").and_then(Tag::list).unwrap_or(&[]).iter().map(palette_entry).collect();
            if palette.is_empty() {
                continue;
            }
            let indices = if palette.len() == 1 {
                None
            } else {
                let longs = states.get("data").and_then(Tag::longs).ok_or("section has a palette but no data")?;
                let bits = bits_for(palette.len());
                let per_long = 64 / bits;
                if longs.len() * per_long < 4096 {
                    return Err("section data is too short".into());
                }
                let mask = (1u64 << bits) - 1;
                let mut out = Vec::with_capacity(4096);
                for i in 0..4096 {
                    let word = longs[i / per_long] as u64;
                    let index = ((word >> ((i % per_long) * bits)) & mask) as usize;
                    if index >= palette.len() {
                        return Err("section data names a block past the palette".into());
                    }
                    out.push(index as u16);
                }
                Some(out)
            };
            chunk.sections.insert(y, Section { palette, indices });
        }
        for entity in tag.get("block_entities").and_then(Tag::list).unwrap_or(&[]) {
            let coord = |k: &str| entity.get(k).and_then(Tag::int).map(|v| v as i32);
            if let (Some(x), Some(y), Some(z)) = (coord("x"), coord("y"), coord("z")) {
                chunk.entities.insert([x, y, z], entity.clone());
            }
        }
        Ok(chunk)
    }

    /// The block at `p` (absolute coordinates, inside this chunk). A section the chunk does not have is air.
    pub fn block(&self, p: Pos) -> &BlockState {
        let Some(section) = self.sections.get(&p[1].div_euclid(16)) else { return &self.air };
        let at = (p[1].rem_euclid(16) * 256 + p[2].rem_euclid(16) * 16 + p[0].rem_euclid(16)) as usize;
        match &section.indices {
            None => &section.palette[0],
            Some(indices) => &section.palette[indices[at] as usize],
        }
    }

    /// The saved data of the block entity at `p`, `id` and position included.
    pub fn block_entity(&self, p: Pos) -> Option<&Tag> {
        self.entities.get(&p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Write;

    fn compound(pairs: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<BTreeMap<_, _>>())
    }

    /// A chunk with one section (`y` = 4) of stone and glass, glass wherever x + z is even... only the blocks the test reads.
    fn chunk_tag(glass_at: &[usize]) -> Tag {
        let palette = Tag::List(vec![compound(vec![("Name", Tag::String("minecraft:stone".into()))]), compound(vec![("Name", Tag::String("minecraft:glass".into()))])]);
        // Two palette entries: four bits an index, sixteen to a long.
        let mut longs = vec![0i64; 256];
        for &i in glass_at {
            longs[i / 16] |= 1i64 << ((i % 16) * 4);
        }
        let states = compound(vec![("palette", palette), ("data", Tag::LongArray(longs))]);
        let section = compound(vec![("Y", Tag::Byte(4)), ("block_states", states)]);
        let chest = compound(vec![("id", Tag::String("minecraft:chest".into())), ("x", Tag::Int(3)), ("y", Tag::Int(65)), ("z", Tag::Int(2))]);
        compound(vec![("sections", Tag::List(vec![section])), ("block_entities", Tag::List(vec![chest]))])
    }

    #[test]
    fn section_indices_unpack_four_bits_to_an_entry() {
        // Block (x 1, y 1, z 0) is index 256 + 1; block (0, 0, 0) is index 0.
        let chunk = Chunk::from_nbt(&chunk_tag(&[257, 0])).unwrap();
        assert_eq!(chunk.block([1, 65, 0]).name, "minecraft:glass");
        assert_eq!(chunk.block([0, 64, 0]).name, "minecraft:glass");
        assert_eq!(chunk.block([2, 64, 0]).name, "minecraft:stone");
        // Chunks to the west and the north count their blocks from their own corner.
        assert_eq!(chunk.block([-15, 65, -16]).name, "minecraft:glass");
        // No section at y 0..16: air.
        assert!(chunk.block([0, 3, 0]).is_air());
        assert_eq!(chunk.block_entity([3, 65, 2]).and_then(|t| t.get("id")).and_then(Tag::str), Some("minecraft:chest"));
    }

    #[test]
    fn a_single_entry_palette_has_no_data() {
        let states = compound(vec![("palette", Tag::List(vec![compound(vec![("Name", Tag::String("minecraft:dirt".into()))])]))]);
        let tag = compound(vec![("sections", Tag::List(vec![compound(vec![("Y", Tag::Byte(-1)), ("block_states", states)])]))]);
        let chunk = Chunk::from_nbt(&tag).unwrap();
        assert_eq!(chunk.block([5, -3, 9]).name, "minecraft:dirt");
    }

    #[test]
    fn index_widths_follow_the_palette_size() {
        assert_eq!(bits_for(2), 4);
        assert_eq!(bits_for(16), 4);
        assert_eq!(bits_for(17), 5);
        assert_eq!(bits_for(33), 6);
    }

    #[test]
    fn a_chunk_is_found_in_its_sector_and_inflated() {
        let raw = nbt::write("", &chunk_tag(&[0]));
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let packed = z.finish().unwrap();
        let mut file = vec![0u8; HEADER];
        // Chunk (33, -1) is local (1, 31) in region (1, -1); its data starts at sector 2.
        let slot = 4 * (1 + 32 * 31);
        file[slot..slot + 4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
        file.extend_from_slice(&((packed.len() as u32 + 1).to_be_bytes()));
        file.push(2);
        file.extend_from_slice(&packed);
        file.resize(3 * 4096, 0);
        assert_eq!(region_file(33, -1), "r.1.-1.mca");
        let tag = chunk_nbt(&file, 33, -1).unwrap().unwrap();
        assert_eq!(Chunk::from_nbt(&tag).unwrap().block([0, 64, 0]).name, "minecraft:glass");
        assert_eq!(chunk_nbt(&file, 32, -1).unwrap(), None);
        assert!(chunk_nbt(&file[..100], 33, -1).is_err());
    }
}
