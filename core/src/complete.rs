//! Tab-completion words for the arguments of WorldEdit's commands: block ids, patterns, masks,
//! schematic names and command names. No game types: the glue passes the game's block ids.
//!
//! Every function takes the text typed so far for the argument (`prefix`) and returns the whole
//! replacement text for it, so a suggestion keeps what was typed before the word being completed
//! (`50%stone,di` gives `50%stone,diamond_block`).
use std::path::Path;

/// The block ids that start with `token`: with the `minecraft:` namespace when the token has a colon,
/// else without it (`sto` gives `stone`, `stone_bricks`...). A token that opens block properties
/// (`stone[`) gets nothing.
fn blocks(token: &str, ids: &[String]) -> Vec<String> {
    let lower = token.to_ascii_lowercase();
    if lower.contains('[') {
        return Vec::new();
    }
    let mut out = Vec::new();
    if !lower.is_empty() && !lower.contains(':') && "minecraft:".starts_with(lower.as_str()) {
        out.push("minecraft:".to_owned());
    }
    for id in ids {
        if lower.contains(':') {
            if id.starts_with(lower.as_str()) {
                out.push(id.clone());
            }
        } else {
            let bare = id.strip_prefix("minecraft:").unwrap_or(id);
            if bare.starts_with(lower.as_str()) {
                out.push(bare.to_owned());
            }
        }
    }
    out
}

/// `text` split after its last `sep`: what comes before (with the separator) and the rest.
fn split_last(text: &str, sep: char) -> (&str, &str) {
    match text.rfind(sep) {
        Some(i) => text.split_at(i + sep.len_utf8()),
        None => ("", text),
    }
}

/// Pattern words: a block, a list `50%stone,50%dirt` (a weight before `%`), `hand`. Words typed
/// before the last space stay as they are (`//replace stone di`: a mask, then the pattern).
pub fn pattern_words(prefix: &str, ids: &[String]) -> Vec<String> {
    let (lead, last) = split_last(prefix, ' ');
    let (before, entry) = split_last(last, ',');
    let weight_len = match entry.find('%') {
        Some(i) if i > 0 && entry[..i].chars().all(|c| c.is_ascii_digit() || c == '.') => i + 1,
        _ => 0,
    };
    let (weight, token) = entry.split_at(weight_len);
    let head = format!("{lead}{before}{weight}");
    let mut words = Vec::new();
    if "hand".starts_with(token.to_ascii_lowercase().as_str()) {
        words.push("hand".to_owned());
    }
    words.extend(blocks(token, ids));
    words.into_iter().map(|w| format!("{head}{w}")).collect()
}

/// Mask words: the blocks, `#existing`, `#air`, `#solid`, `hand`, a leading `!` to negate, commas for
/// "any of"; each space-separated part must match (the words before the last space stay as typed).
pub fn mask_words(prefix: &str, ids: &[String]) -> Vec<String> {
    let (lead, last) = split_last(prefix, ' ');
    let body = last.trim_start_matches('!');
    let negations = &last[..last.len() - body.len()];
    let (before, token) = split_last(body, ',');
    let head = format!("{lead}{negations}{before}");
    let lower = token.to_ascii_lowercase();
    let mut words: Vec<String> = Vec::new();
    for special in ["#existing", "#air", "#solid", "hand"] {
        if special.starts_with(lower.as_str()) {
            words.push(special.to_owned());
        }
    }
    words.extend(blocks(token, ids));
    words.into_iter().map(|w| format!("{head}{w}")).collect()
}

/// The schematic files under `dir` (two folders deep), as `//schem load` names them: no extension, `/`
/// between folders, those that start with `prefix` (any case).
pub fn schematic_words(dir: &Path, prefix: &str) -> Vec<String> {
    const EXTENSIONS: [&str; 4] = [".schem", ".litematic", ".schematic", ".nbt"];
    let lower = prefix.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut walk: Vec<(std::path::PathBuf, String, u8)> = vec![(dir.to_path_buf(), String::new(), 0)];
    while let Some((folder, name_prefix, depth)) = walk.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if depth < 2 {
                    walk.push((entry.path(), format!("{name_prefix}{file_name}/"), depth + 1));
                }
                continue;
            }
            let lower_name = file_name.to_ascii_lowercase();
            let Some(ext) = EXTENSIONS.iter().find(|e| lower_name.ends_with(**e)) else { continue };
            let stem = &file_name[..file_name.len() - ext.len()];
            let word = format!("{name_prefix}{stem}");
            if word.to_ascii_lowercase().starts_with(lower.as_str()) && !out.contains(&word) {
                out.push(word);
            }
        }
    }
    out.sort();
    out
}

/// Command names for `//help <command>`: the first word only.
pub fn command_words(prefix: &str) -> Vec<String> {
    if prefix.contains(' ') {
        return Vec::new();
    }
    let lower = prefix.trim_start_matches('/').to_ascii_lowercase();
    let mut out: Vec<String> = crate::engine::COMMANDS.iter().map(|(n, _)| (*n).to_owned()).filter(|n| n.starts_with(lower.as_str())).collect();
    out.sort();
    out.dedup();
    out
}

/// The block ids of a game's blocks as the completion wants them: sorted, `minecraft:` kept.
pub fn sorted_ids(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        ["minecraft:stone", "minecraft:stone_bricks", "minecraft:dirt", "minecraft:diamond_block"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn patterns_complete_after_weights_and_commas() {
        assert_eq!(pattern_words("sto", &ids()), vec!["stone", "stone_bricks"]);
        assert_eq!(pattern_words("50%stone,50%di", &ids()), vec!["50%stone,50%dirt", "50%stone,50%diamond_block"]);
        assert_eq!(pattern_words("minecraft:d", &ids()), vec!["minecraft:dirt", "minecraft:diamond_block"]);
        assert_eq!(pattern_words("stone[", &ids()), Vec::<String>::new());
    }

    #[test]
    fn masks_know_their_specials() {
        assert!(mask_words("#e", &ids()).contains(&"#existing".to_owned()));
        assert_eq!(mask_words("!sto", &ids()), vec!["!stone", "!stone_bricks"]);
        assert_eq!(mask_words("#air dirt,di", &ids()), vec!["#air dirt,dirt", "#air dirt,diamond_block"]);
    }
}
