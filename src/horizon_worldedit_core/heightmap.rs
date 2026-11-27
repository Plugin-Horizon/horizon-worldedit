//! The image of `/brush heightmap <image>`: WorldEdit's `ImageHeightmap` and the asset folder it reads
//! (`AssetLoaders.getAssetLoader`). Java's `ImageIO` reads png, jpg, bmp, gif and wbmp; this reads PNG (all colour
//! types and bit depths, interlaced or not) and uncompressed 24/32-bit BMP. Other formats are "Failed to load asset".

use std::io::Read;
use std::path::{Path, PathBuf};

/// A decoded image: RGBA, row by row from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<[u8; 4]>,
}

impl HeightImage {
    /// `ImageHeightmap.getHeightAt(x, y, size)`: the image scaled to `size` x `size` (nearest neighbour, as
    /// `Graphics2D.drawImage` does by default, onto a black `TYPE_INT_RGB` image), the Y axis flipped, the mean of
    /// red, green and blue as a fraction of 255.
    pub fn height_at(&self, x: i32, y: i32, size: i32) -> f64 {
        if size <= 0 || x < 0 || y < 0 || x >= size || y >= size || self.width == 0 || self.height == 0 {
            return 0.0;
        }
        let y = size - 1 - y;
        let sx = (((x as f64 + 0.5) * self.width as f64 / f64::from(size)).floor() as usize).min(self.width - 1);
        let sy = (((y as f64 + 0.5) * self.height as f64 / f64::from(size)).floor() as usize).min(self.height - 1);
        let [r, g, b, a] = self.rgba[sy * self.width + sx];
        // Drawn over black: each channel scales with the alpha.
        let over = |c: u8| (f64::from(c) * f64::from(a) / 255.0).round();
        (over(r) + over(g) + over(b)) / 3.0 / 255.0
    }
}

/// The extensions this port reads.
pub const EXTENSIONS: [&str; 2] = ["png", "bmp"];

/// `AssetLoaders.getAssetLoader`: the file `name` means inside `dir`, as typed (if its extension is one we read) or
/// with an extension added. `None`: no such image ("Unknown heightmap brush").
pub fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    // `getSafeOpenFile`: nothing outside the folder.
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains(':') || name.split('/').any(|p| p == ".." || p == ".") {
        // A name that cannot be a file of the folder: the loader is looked up first, and finds nothing.
        return None;
    }
    let direct = dir.join(name);
    let ext = direct.extension().and_then(|e| e.to_str()).unwrap_or("");
    if EXTENSIONS.contains(&ext) && is_file(&direct) {
        return Some(direct);
    }
    for e in EXTENSIONS {
        let p = dir.join(format!("{name}.{e}"));
        if is_file(&p) {
            return Some(p);
        }
    }
    None
}

fn is_file(p: &Path) -> bool {
    // No links out of the folder (WorldEdit's files.allow-symbolic-links: false).
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
}

/// Reads and decodes the file; `None` for anything this does not understand.
pub fn load(path: &Path) -> Option<HeightImage> {
    let bytes = std::fs::read(path).ok()?;
    decode(&bytes)
}

pub fn decode(bytes: &[u8]) -> Option<HeightImage> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)
    } else if bytes.starts_with(b"BM") {
        decode_bmp(bytes)
    } else {
        None
    }
}

fn be32(b: &[u8]) -> usize {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize
}

fn decode_png(bytes: &[u8]) -> Option<HeightImage> {
    let (mut width, mut height, mut depth, mut color, mut interlace) = (0usize, 0usize, 0u8, 0u8, 0u8);
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut alpha: Vec<u8> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let len = be32(&bytes[at..]);
        let kind = &bytes[at + 4..at + 8];
        let body = bytes.get(at + 8..at + 8 + len)?;
        match kind {
            b"IHDR" if len >= 13 => {
                width = be32(body);
                height = be32(&body[4..]);
                depth = body[8];
                color = body[9];
                interlace = body[12];
            }
            b"PLTE" => palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            b"tRNS" => alpha = body.to_vec(),
            b"IDAT" => data.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    if width == 0 || height == 0 || width.checked_mul(height)? > 1 << 26 || !matches!(depth, 1 | 2 | 4 | 8 | 16) {
        return None;
    }
    let channels = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    if (color == 3 && depth > 8) || (matches!(color, 2 | 4 | 6) && depth < 8) {
        return None;
    }
    let mut raw = Vec::new();
    // 16-bit RGBA at most, plus a filter byte per row and the interlace's extra rows.
    let limit = (width * height * 8 + height * 8 + 4096) as u64;
    flate2::read::ZlibDecoder::new(&data[..]).take(limit).read_to_end(&mut raw).ok()?;
    let bits = usize::from(depth) * channels;
    let bpp = bits.div_ceil(8).max(1);
    // The sub-images: one for a plain file, seven for Adam7.
    let passes: Vec<(usize, usize, usize, usize)> = if interlace == 1 {
        vec![(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
    } else {
        vec![(0, 0, 1, 1)]
    };
    let mut rgba = vec![[0u8, 0, 0, 255]; width * height];
    let mut pos = 0;
    for (x0, y0, dx, dy) in passes {
        let (pw, ph) = ((width + dx - 1 - x0) / dx, (height + dy - 1 - y0) / dy);
        if width <= x0 || height <= y0 || pw == 0 || ph == 0 {
            continue;
        }
        let stride = (pw * bits).div_ceil(8);
        let mut prev = vec![0u8; stride];
        for row in 0..ph {
            let filter = *raw.get(pos)?;
            let mut line = raw.get(pos + 1..pos + 1 + stride)?.to_vec();
            pos += 1 + stride;
            for i in 0..stride {
                let a = if i >= bpp { line[i - bpp] } else { 0 };
                let b = prev[i];
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                let add = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    4 => {
                        let (p, pa, pb, pc) = (i32::from(a) + i32::from(b) - i32::from(c), i32::from(a), i32::from(b), i32::from(c));
                        let (da, db, dc) = ((p - pa).abs(), (p - pb).abs(), (p - pc).abs());
                        if da <= db && da <= dc { a } else if db <= dc { b } else { c }
                    }
                    _ => return None,
                };
                line[i] = line[i].wrapping_add(add);
            }
            for col in 0..pw {
                // The sample `k` of this pixel, scaled to 8 bits.
                let sample = |k: usize| -> u8 {
                    let n = col * channels + k;
                    match depth {
                        8 => line[n],
                        16 => line[n * 2],
                        d => {
                            let bit = n * usize::from(d);
                            let v = (line[bit / 8] >> (8 - usize::from(d) - bit % 8)) & ((1u8 << d) - 1);
                            if color == 3 { v } else { (u16::from(v) * 255 / ((1u16 << d) - 1)) as u8 }
                        }
                    }
                };
                let px = match color {
                    0 => [sample(0); 3].into_iter().chain([255]).collect::<Vec<_>>(),
                    2 => vec![sample(0), sample(1), sample(2), 255],
                    3 => {
                        let i = usize::from(sample(0));
                        let c = palette.get(i)?;
                        vec![c[0], c[1], c[2], alpha.get(i).copied().unwrap_or(255)]
                    }
                    4 => vec![sample(0), sample(0), sample(0), sample(1)],
                    _ => vec![sample(0), sample(1), sample(2), sample(3)],
                };
                rgba[(y0 + row * dy) * width + x0 + col * dx] = [px[0], px[1], px[2], px[3]];
            }
            prev = line;
        }
    }
    Some(HeightImage { width, height, rgba })
}

fn decode_bmp(b: &[u8]) -> Option<HeightImage> {
    let le32 = |at: usize| -> Option<i32> { Some(i32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?)) };
    let offset = le32(10)? as usize;
    let (width, raw_height) = (le32(18)?, le32(22)?);
    let bits = u16::from_le_bytes(b.get(28..30)?.try_into().ok()?);
    let compression = le32(30)?;
    if width <= 0 || raw_height == 0 || !matches!(bits, 24 | 32) || compression != 0 || (width as i64) * i64::from(raw_height.abs()) > 1 << 26 {
        return None;
    }
    let (width, height) = (width as usize, raw_height.unsigned_abs() as usize);
    let bytes = usize::from(bits) / 8;
    let stride = (width * bytes).div_ceil(4) * 4;
    let mut rgba = vec![[0u8, 0, 0, 255]; width * height];
    for row in 0..height {
        // A positive height is stored bottom-up.
        let y = if raw_height > 0 { height - 1 - row } else { row };
        let line = b.get(offset + row * stride..offset + row * stride + width * bytes)?;
        for x in 0..width {
            let p = &line[x * bytes..];
            rgba[y * width + x] = [p[2], p[1], p[0], 255];
        }
    }
    Some(HeightImage { width, height, rgba })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A PNG of grey 8-bit pixels (filter 0), built by hand.
    pub fn png_gray(w: usize, h: usize, px: &[u8]) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8], body: &[u8]) {
            out.extend((body.len() as u32).to_be_bytes());
            out.extend(kind);
            out.extend(body);
            out.extend([0, 0, 0, 0]); // the CRC is not checked
        }
        let mut raw = Vec::new();
        for r in 0..h {
            raw.push(0);
            raw.extend(&px[r * w..(r + 1) * w]);
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend((w as u32).to_be_bytes());
        ihdr.extend((h as u32).to_be_bytes());
        ihdr.extend([8, 0, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &z.finish().unwrap());
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn png_grey_is_read_and_the_y_axis_is_flipped() {
        // Row 0 (top) is black, row 1 is white.
        let img = decode(&png_gray(2, 2, &[0, 0, 255, 255])).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        // y = 0 asks for the bottom row after the flip.
        assert_eq!(img.height_at(0, 0, 2), 1.0);
        assert_eq!(img.height_at(0, 1, 2), 0.0);
        // Scaled up, nearest neighbour: four samples per source pixel row.
        assert_eq!(img.height_at(1, 3, 4), 0.0);
        assert_eq!(img.height_at(1, 0, 4), 1.0);
        assert_eq!(img.height_at(5, 0, 4), 0.0, "out of the image");
    }

    #[test]
    fn rgb_is_the_mean_of_the_channels_and_alpha_darkens() {
        let img = HeightImage { width: 1, height: 1, rgba: vec![[255, 0, 0, 255]] };
        assert!((img.height_at(0, 0, 1) - 1.0 / 3.0).abs() < 1e-12);
        let half = HeightImage { width: 1, height: 1, rgba: vec![[255, 255, 255, 0]] };
        assert_eq!(half.height_at(0, 0, 1), 0.0);
    }

    #[test]
    fn unknown_formats_are_not_read() {
        assert!(decode(b"GIF89a....").is_none());
        assert!(decode(&[]).is_none());
    }

    #[test]
    fn the_asset_folder_finds_names_with_and_without_extension() {
        let base = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
        let dir = base.join("we-tests").join("heightmap-assets");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hill.png"), png_gray(1, 1, &[255])).unwrap();
        assert_eq!(find(&dir, "hill"), Some(dir.join("hill.png")));
        assert_eq!(find(&dir, "hill.png"), Some(dir.join("hill.png")));
        assert_eq!(find(&dir, "nothing"), None);
        assert_eq!(find(&dir, "../hill"), None);
        assert!(load(&dir.join("hill.png")).is_some());
    }
}
