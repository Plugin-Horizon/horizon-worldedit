//! Argument errors in the words WorldEdit's command layer (Piston) prints: `Missing argument for <x>.`,
//! `Invalid value for [x] (reason), acceptable values are ...`, `Too many arguments.`, and the sub-command lines of the
//! group commands, each followed by `Usage: /name ...`. The parameter table is read from the original's command
//! classes by `tools/gen_usage.py` (WorldEdit, GPL-3.0).
use super::usage_table::{GROUP_WORDS, TABLE};

/// The commands whose arguments are checked here (the others keep their own messages).
const CHECKED: &[&str] = &[
    "up", "ascend", "asc", "descend", "desc", "extinguish", "ex", "ext", "fixlava", "fixwater", "floodfill", "flood", "forestgen", "green", "snow",
    "thaw", "gmask", "mask", "tracemask", "material", "lrbuild", "searchitem", "chunkinfo", "delchunks", "listchunks", "toggleeditwand", "toggleplace",
    "pumpkins", "range", "remove", "rem", "rement", "replacenear", "butcher", "ceil", "jumpto", "j", "thru", "unstuck", "info", "cycler", "deltree",
    "farwand", "navwand", "selwand", "tree", "undo", "redo", "biomeinfo", "biomelist", "biomels", "repl", "placement", "size",
];

/// The tool commands that also exist as global words (`/selwand`), and their names under `/tool`.
pub fn global_tool(name: &str) -> Option<&'static str> {
    Some(match name {
        "selwand" => "selwand",
        "navwand" => "navwand",
        "info" => "info",
        "cycler" => "cycler",
        "repl" => "repl",
        "floodfill" | "flood" => "floodfill",
        "farwand" => "farwand",
        "tree" => "tree",
        "deltree" => "deltree",
        "lrbuild" => "lrbuild",
        "none" => "none",
        _ => return None,
    })
}

/// `/tree [type]` and `/forestgen [size] [type] [density]`: Paper's tree type conversion fails for its own default
/// (`tree`) and for any word that is no tree type. Returns the lines, `bare` telling the global notice comes first.
pub fn tree_check(typed: &str, words: &[&str], known: impl Fn(&str) -> bool) -> Option<Vec<String>> {
    let typed_l = typed.to_ascii_lowercase();
    let (index, usage) = match typed_l.as_str() {
        "tree" => (0, "Usage: /tree [type]"),
        "forestgen" => (1, "Usage: /forestgen [size] [type] [density]"),
        _ => return None,
    };
    let word = words.get(index).copied().unwrap_or("tree");
    if word != "tree" && known(word) {
        return None;
    }
    Some(vec![format!("Invalid value for [type] (Not a valid tree type: {word}), acceptable values are any tree type"), usage.to_owned()])
}

pub const REMOVERS: &[&str] = &["projectiles", "items", "paintings", "itemframes", "boats", "minecarts", "tnt", "xp", "all"];

pub const DEPRECATION: &str = "Global tool names cause conflicts and will be removed in WorldEdit 8. Please use /tool {} instead.";

/// The group commands: (words typed, Paper's sub-command order, key of `GROUP_WORDS`).
const GROUPS: &[(&[&str], &str, &str)] = &[
    (&["brush", "br"], "forest, splatter, butcher, paint, none, clipboard, snowsmooth, gravity, heightmap, extinguish, feature, sphere, raise, smooth, cylinder, set, apply, deform, lower, erode, dilate, snow, biome, morph", "brush"),
    (&["tool"], "selwand, farwand, navwand, floodfill, repl, info, lrbuild, tree, cycler, deltree, none, stacker", "tool"),
    (&["we", "worldedit"], "help, version, trace, reload, cui, tz, report", "we"),
    (&["schem", "schematic"], "list, formats, load, delete, save, share", "schem"),
    (&["snap", "snapshot"], "before, use, sel, after, list", "snap"),
    (&["pickaxe", "sp", "superpickaxe"], "single, area, recursive", "pickaxe"),
];

fn row(name: &str, numbered: bool) -> Option<&'static super::usage_table::Row> {
    TABLE.iter().find(|r| r.0.contains(&name) && (!numbered || !r.3.is_empty()))
}

/// `Usage: /name [-flags] [-p <page>] <a> [b]`.
fn usage_line(typed: &str, r: &super::usage_table::Row) -> String {
    let mut s = format!("Usage: /{typed}");
    if !r.1.is_empty() {
        s.push_str(&format!(" [-{}]", r.1));
    }
    for (c, n) in r.2 {
        s.push_str(&format!(" [-{c} <{n}>]"));
    }
    for (n, optional, _) in r.3 {
        let shown = if *n == "query" { "query...".to_owned() } else { (*n).to_owned() };
        let n = &shown;
        s.push_str(&if *optional { format!(" [{n}]") } else { format!(" <{n}>") });
    }
    s
}

/// The checks of the typed words of a command (flags already taken out). `typed` is the word as typed, `canon` the
/// engine's name for it. `None`: nothing wrong here.
pub fn check(typed: &str, canon: &str, words: &[&str]) -> Option<Vec<String>> {
    let typed_l = typed.to_ascii_lowercase();
    if !CHECKED.contains(&typed_l.as_str()) && !CHECKED.contains(&canon) {
        return None;
    }
    // `/size help`: not a number, so the brush size's own error (the selection's `//size` takes no words).
    let numbered = canon == "brushsize" || (typed_l == "size" && words.len() == 1);
    let lookup = if numbered { "size" } else { typed_l.as_str() };
    let r = row(lookup, numbered).or_else(|| row(canon, numbered))?;
    // `<query...>`-like text parameters take everything.
    let params = r.3;
    if r.0.contains(&"remove") {
        if let Some(w) = words.first() {
            if !REMOVERS.contains(&w.to_ascii_lowercase().as_str()) {
                let list = "projectiles, items, paintings, itemframes, boats, minecarts, tnt, xp, or all";
                return Some(vec![format!("Invalid value for <remover> (Acceptable types: {list}), acceptable values are {list}"), usage_line(&typed_l, r)]);
            }
        }
    }
    let has_text = params.iter().any(|p| p.0 == "query" || p.0.ends_with("..."));
    let required = params.iter().filter(|p| !p.1).count();
    let usage = usage_line(&typed_l, r);
    if words.len() > params.len() && !has_text {
        return Some(vec!["Too many arguments.".into(), usage]);
    }
    // A required word takes its place first; the words left over go to the last optional parameters (the order
    // Piston fills them in, as `/snow help` and `/forestgen help` show).
    let mut extra = words.len().saturating_sub(required);
    let optionals = params.iter().filter(|p| p.1).count();
    let mut skip = optionals - extra.min(optionals);
    let mut at = 0;
    for p in params {
        if p.1 {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            extra = extra.saturating_sub(1);
        }
        let Some(word) = words.get(at).copied() else { break };
        at += 1;
        let (accepted, bad) = match p.2 {
            "int" => (if r.0.contains(&"snow") || r.0.contains(&"thaw") || r.0.contains(&"green") { "Any integer" } else { "any integer" }, word.parse::<f64>().is_err()),
            "double" => ("any double", word.parse::<f64>().is_err()),
            _ => continue,
        };
        if bad {
            let (open, close) = if p.1 { ('[', ']') } else { ('<', '>') };
            return Some(vec![format!("Invalid value for {open}{}{close} (For input string: \"{word}\"), acceptable values are {accepted}", p.0), usage]);
        }
    }
    if words.len() < required {
        let missing = params.iter().filter(|p| !p.1).nth(words.len())?;
        return Some(vec![format!("Missing argument for <{}>.", if missing.0 == "query" { "query..." } else { missing.0 }), usage]);
    }
    None
}

/// An error from a mask or pattern argument of `typed`, in Piston's frame: the message goes into the
/// `Invalid value for ...` line of the parameter that holds the word it names.
pub fn wrap_parse(typed: &str, canon: &str, words: &[&str], error: &str) -> Option<Vec<String>> {
    let typed_l = typed.to_ascii_lowercase();
    let r = row(&typed_l, false).or_else(|| row(canon, false))?;
    if !CHECKED.contains(&typed_l.as_str()) && !CHECKED.contains(&canon) {
        return None;
    }
    let accepted = |kind: &str| match kind {
        "mask" => Some("any mask"),
        "pattern" => Some("any pattern"),
        _ => None,
    };
    let required = r.3.iter().filter(|p| !p.1).count();
    let mut skip = r.3.iter().filter(|p| p.1).count().saturating_sub(words.len().saturating_sub(required));
    let mut at = 0;
    for p in r.3 {
        if p.1 {
            if skip > 0 {
                skip -= 1;
                continue;
            }
        }
        let word = *words.get(at)?;
        at += 1;
        let Some(acc) = accepted(p.2) else { continue };
        if error.contains(&format!("'{word}'")) {
            let (open, close) = if p.1 { ('[', ']') } else { ('<', '>') };
            return Some(vec![format!("Invalid value for {open}{}{close} ({error}), acceptable values are {acc}", p.0), usage_line(&typed_l, r)]);
        }
    }
    None
}

/// `No sub-command provided` / `Invalid sub-command` for a group command (`/brush`, `/tool`...).
pub fn group(typed: &str, words: &[&str]) -> Option<Vec<String>> {
    let typed_l = typed.to_ascii_lowercase();
    let (_, options, key) = GROUPS.iter().find(|g| g.0.contains(&typed_l.as_str()))?;
    let usage = format!("Usage: /{typed_l} <{}>", options.replace(", ", "|"));
    let Some(first) = words.first() else {
        return Some(vec![format!("No sub-command provided. Options: {options}"), usage]);
    };
    let first_l = first.to_ascii_lowercase();
    let known = GROUP_WORDS.iter().find(|g| g.0 == *key).is_some_and(|g| g.1.contains(&first_l.as_str()))
        || matches!((*key, first_l.as_str()), ("brush", "apply" | "paint") | ("tool", "unbind") | ("brush", "unbind") | ("pickaxe", "area" | "single" | "recursive" | "recur"));
    if known {
        return None;
    }
    Some(vec![format!("Invalid sub-command '{first}'. Options: {options}"), usage])
}
