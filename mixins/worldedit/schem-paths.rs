// The folder a world keeps its schematics in, shared by the editor in the game
// binary and the chat completions in this library. Staged into chat.rs with
// `visibility = "pub"`, so the binary reaches it as
// `horizon_client::chat::worldedit_paths` and this library as
// `crate::chat::worldedit_paths`.
//
// It also remembers the world the game last opened, so a completion typed in
// the chat screen — which has no game state — can list the same folder the
// menu lists.
use std::path::{Path, PathBuf};

/// The extensions a schematic file may carry, lower case.
pub const EXTENSIONS: [&str; 3] = ["schem", "litematic", "schematic"];

/// `<world>/schematics`: where `//schem save` writes and `//schem load` looks.
pub fn world_dir(world: &Path) -> PathBuf {
    world.join("schematics")
}

/// The world's schematics folder, made when it is missing. Also remembers
/// `world`, so the completions look here too.
pub fn ensure_world(world: &Path) -> PathBuf {
    let dir = world_dir(world);
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log::warn!("Horizon: could not make {}: {error}", dir.display());
    }
    remember(world);
    dir
}

/// The world the game last opened.
static WORLD: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Notes the world a game is playing, for the folder helpers above.
pub fn remember(world: &Path) {
    if world.as_os_str().is_empty() {
        return;
    }
    if let Ok(mut slot) = WORLD.lock() {
        *slot = Some(world.to_path_buf());
    }
}

/// That world, before one has been opened: None.
pub fn remembered() -> Option<PathBuf> {
    WORLD.lock().ok().and_then(|slot| slot.clone())
}

/// The last world's schematics folder, made when it is missing. None before a
/// world has been opened.
pub fn schematics() -> Option<PathBuf> {
    remembered().map(|world| ensure_world(&world))
}

/// One file of the folder.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The name in the folder, extension and all: `castle.schem`.
    pub file: String,
    /// The name to type, without the extension: `castle`.
    pub name: String,
    /// What reads it, by its extension: `Sponge`, `Litematica`, `MCEdit`.
    pub format: &'static str,
    pub bytes: u64,
}

/// The spelled extension of `file` when it is one of [`EXTENSIONS`].
pub fn extension(file: &str) -> Option<&'static str> {
    let (_, after) = file.rsplit_once('.')?;
    EXTENSIONS
        .iter()
        .copied()
        .find(|known| after.eq_ignore_ascii_case(known))
}

/// The editor a file belongs to, by its name.
pub fn format(file: &str) -> &'static str {
    match extension(file) {
        Some("schem") => "Sponge",
        Some("litematic") => "Litematica",
        Some("schematic") => "MCEdit",
        _ => "unknown",
    }
}

/// `file` without its extension: the name `//schem load` takes.
pub fn stem(file: &str) -> String {
    match file.rsplit_once('.') {
        Some((stem, _)) => stem.to_owned(),
        None => file.to_owned(),
    }
}

/// Every schematic in `dir`, by name. A folder that is not there (or cannot be
/// read) has none.
pub fn entries(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = read
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let format = extension(&file).map(|_| format(&file))?;
            let bytes = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            Some(Entry {
                name: stem(&file),
                file,
                format,
                bytes,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.file
            .to_lowercase()
            .cmp(&b.file.to_lowercase())
            .then_with(|| a.file.cmp(&b.file))
    });
    out
}

/// The names a `load` completes with: each file once, without its extension,
/// in the folder's order.
pub fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in entries(dir) {
        if !out.contains(&entry.name) {
            out.push(entry.name);
        }
    }
    out
}

/// A file's size as the menu shows it: `812 B`, `4.2 KB`, `1.3 MB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_matched_whatever_their_case() {
        assert_eq!(extension("castle.schem"), Some("schem"));
        assert_eq!(extension("Castle.LITEMATIC"), Some("litematic"));
        assert_eq!(extension("old.schematic"), Some("schematic"));
        assert_eq!(extension("notes.txt"), None);
        assert_eq!(extension("castle"), None);
    }

    #[test]
    fn a_name_is_the_file_without_its_extension() {
        assert_eq!(stem("castle.schem"), "castle");
        assert_eq!(stem("my.castle.schem"), "my.castle");
        assert_eq!(stem("castle"), "castle");
    }

    #[test]
    fn sizes_read_as_kilobytes_and_up() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(1024 * 1024 * 3 / 2), "1.5 MB");
    }

    #[test]
    fn the_folder_lists_its_schematics_and_nothing_else() {
        let root = std::env::temp_dir().join(format!("horizon-schem-paths-{}", std::process::id()));
        let dir = ensure_world(&root);
        std::fs::write(dir.join("b_castle.litematic"), b"x").unwrap();
        std::fs::write(dir.join("a_village.schem"), b"xy").unwrap();
        std::fs::write(dir.join("notes.txt"), b"---").unwrap();
        let entries = entries(&dir);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file, "a_village.schem");
        assert_eq!(entries[0].name, "a_village");
        assert_eq!(entries[0].format, "Sponge");
        assert_eq!(entries[0].bytes, 2);
        assert_eq!(entries[1].format, "Litematica");
        assert_eq!(names(&dir), vec!["a_village", "b_castle"]);
        assert_eq!(remembered(), Some(root.clone()));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
