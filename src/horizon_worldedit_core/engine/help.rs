//! `//help`, `/we` and its sub-commands (`version`, `reload`, `report`, `trace`, `cui`, `tz`, `help`),
//! and the framed, paged text WorldEdit prints them in.

use super::*;

pub(crate) struct Entry {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub desc: &'static str,
}

/// WorldEdit's own names for the commands this server has, with its descriptions (Piston names: a
/// leading `/` means the player types two slashes).
pub(crate) const ENTRIES: &[Entry] = &[
    Entry { name: "/pos1", aliases: &[], desc: "Set position 1" },
    Entry { name: "/pos2", aliases: &[], desc: "Set position 2" },
    Entry { name: "/hpos1", aliases: &[], desc: "Set position 1 to targeted block" },
    Entry { name: "/hpos2", aliases: &[], desc: "Set position 2 to targeted block" },
    Entry { name: "/sel", aliases: &[";", "/desel", "/deselect"], desc: "Choose a region selector" },
    Entry { name: "/size", aliases: &[], desc: "Get information about the selection" },
    Entry { name: "/expand", aliases: &[], desc: "Expand the selection area" },
    Entry { name: "/contract", aliases: &[], desc: "Contract the selection area" },
    Entry { name: "/shift", aliases: &[], desc: "Shift the selection area" },
    Entry { name: "/outset", aliases: &[], desc: "Outset the selection area" },
    Entry { name: "/inset", aliases: &[], desc: "Inset the selection area" },
    Entry { name: "/chunk", aliases: &[], desc: "Set the selection to your current chunk." },
    Entry { name: "/count", aliases: &[], desc: "Counts the number of blocks matching a mask" },
    Entry { name: "/distr", aliases: &[], desc: "Get the distribution of blocks in the selection" },
    Entry { name: "/wand", aliases: &[], desc: "Get the wand item" },
    Entry { name: "/set", aliases: &[], desc: "Sets all the blocks in the region" },
    Entry { name: "/replace", aliases: &["/re", "/rep"], desc: "Replace all blocks in the selection with another" },
    Entry { name: "/walls", aliases: &[], desc: "Build the four sides of the selection" },
    Entry { name: "/faces", aliases: &["/outline"], desc: "Build the walls, ceiling, and floor of a selection" },
    Entry { name: "/hollow", aliases: &[], desc: "Hollows out the object contained in this selection" },
    Entry { name: "/overlay", aliases: &[], desc: "Set a block on top of blocks in the region" },
    Entry { name: "/center", aliases: &["/middle"], desc: "Set the center block(s)" },
    Entry { name: "/naturalize", aliases: &[], desc: "3 layers of dirt on top then rock below" },
    Entry { name: "/smooth", aliases: &[], desc: "Smooth the elevation in the selection" },
    Entry { name: "/line", aliases: &[], desc: "Draws line segments between cuboid selection corners or convex polyhedral selection vertices" },
    Entry { name: "/stack", aliases: &[], desc: "Repeat the contents of the selection" },
    Entry { name: "/move", aliases: &[], desc: "Move the contents of the selection" },
    Entry { name: "/sphere", aliases: &[], desc: "Generates a filled sphere." },
    Entry { name: "/hsphere", aliases: &[], desc: "Generates a hollow sphere." },
    Entry { name: "/cyl", aliases: &[], desc: "Generates a cylinder." },
    Entry { name: "/hcyl", aliases: &[], desc: "Generates a hollow cylinder." },
    Entry { name: "/cone", aliases: &[], desc: "Generates a cone." },
    Entry { name: "/pyramid", aliases: &[], desc: "Generate a filled pyramid" },
    Entry { name: "/hpyramid", aliases: &[], desc: "Generate a hollow pyramid" },
    Entry { name: "removeabove", aliases: &["/removeabove"], desc: "Remove blocks above your head." },
    Entry { name: "removebelow", aliases: &["/removebelow"], desc: "Remove blocks below you." },
    Entry { name: "removenear", aliases: &["/removenear"], desc: "Remove blocks near you." },
    Entry { name: "replacenear", aliases: &["/replacenear"], desc: "Replace nearby blocks" },
    Entry { name: "/fill", aliases: &[], desc: "Fill a hole" },
    Entry { name: "/fillr", aliases: &[], desc: "Fill a hole recursively" },
    Entry { name: "/drain", aliases: &[], desc: "Drain a pool" },
    Entry { name: "fixlava", aliases: &["/fixlava"], desc: "Fix lava to be stationary" },
    Entry { name: "fixwater", aliases: &["/fixwater"], desc: "Fix water to be stationary" },
    Entry { name: "snow", aliases: &["/snow"], desc: "Simulates snow" },
    Entry { name: "thaw", aliases: &["/thaw"], desc: "Thaws the area" },
    Entry { name: "green", aliases: &["/green"], desc: "Converts dirt to grass blocks in the area" },
    Entry { name: "extinguish", aliases: &["/ex", "/ext", "/extinguish", "ex", "ext"], desc: "Extinguish nearby fire" },
    Entry { name: "/copy", aliases: &[], desc: "Copy the selection to the clipboard" },
    Entry { name: "/cut", aliases: &[], desc: "Cut the selection to the clipboard" },
    Entry { name: "/paste", aliases: &[], desc: "Paste the clipboard's contents" },
    Entry { name: "/rotate", aliases: &[], desc: "Rotate the contents of the clipboard" },
    Entry { name: "/flip", aliases: &["/mirror"], desc: "Flip the contents of the clipboard across the origin" },
    Entry { name: "clearclipboard", aliases: &[], desc: "Clear your clipboard" },
    Entry { name: "undo", aliases: &["/undo"], desc: "Undoes the last action (from history)" },
    Entry { name: "redo", aliases: &["/redo"], desc: "Redoes the last action (from history)" },
    Entry { name: "clearhistory", aliases: &["/clearhistory"], desc: "Clear your history" },
    Entry { name: "schematic", aliases: &["schem", "/schematic", "/schem"], desc: "Schematic commands for saving/loading areas" },
    Entry { name: "brush", aliases: &["br", "/brush", "/br"], desc: "Brushing commands" },
    Entry { name: "mask", aliases: &[], desc: "Set the brush mask" },
    Entry { name: "material", aliases: &["/material"], desc: "Set the brush material" },
    Entry { name: "tool", aliases: &[], desc: "Binds a tool to the item in your hand" },
    Entry { name: "none", aliases: &[], desc: "Unbind a bound brush from your current item" },
    Entry { name: "/limit", aliases: &[], desc: "Modify block change limit" },
    Entry { name: "gmask", aliases: &["/gmask"], desc: "Set the global mask" },
    Entry { name: "toggleplace", aliases: &["/toggleplace"], desc: "Switch between your position and pos1 for placement" },
    Entry { name: "searchitem", aliases: &["/searchitem", "/l", "/search"], desc: "Search for an item" },
    Entry { name: "/calculate", aliases: &["/calc", "/eval", "/evaluate", "/solve"], desc: "Evaluate a mathematical expression" },
    Entry { name: "/help", aliases: &[], desc: "Displays help for WorldEdit commands" },
    Entry { name: "unstuck", aliases: &["!", "/unstuck"], desc: "Escape from being stuck inside a block" },
    Entry { name: "ascend", aliases: &["asc", "/ascend", "/asc"], desc: "Go up a floor" },
    Entry { name: "descend", aliases: &["desc", "/descend", "/desc"], desc: "Go down a floor" },
    Entry { name: "ceil", aliases: &["/ceil"], desc: "Go to the ceiling" },
    Entry { name: "thru", aliases: &["/thru"], desc: "Pass through walls" },
    Entry { name: "jumpto", aliases: &["j", "/jumpto", "/j"], desc: "Teleport to a location" },
    Entry { name: "/up", aliases: &[], desc: "Go upwards some distance" },
    Entry { name: "placement", aliases: &["/placement"], desc: "Select your placement type" },
    Entry { name: "worldedit", aliases: &["we"], desc: "WorldEdit commands" },
];

/// The sub-commands of the commands that have some: name and description.
const SUBS: &[(&str, &[(&str, &str)])] = &[
    (
        "schematic",
        &[
            ("delete", "Delete a saved schematic"),
            ("formats", "List available formats"),
            ("list", "List saved schematics"),
            ("load", "Load a schematic into your clipboard"),
            ("save", "Save your clipboard into a schematic file"),
        ],
    ),
    (
        "brush",
        &[
            ("cylinder", "Choose the cylinder brush"),
            ("none", "Unbind a bound brush from your current item"),
            ("smooth", "Choose the terrain softener brush"),
            ("sphere", "Choose the sphere brush"),
        ],
    ),
    (
        "worldedit",
        &[
            ("cui", "Complete CUI handshake (internal usage)"),
            ("help", "Displays help for WorldEdit commands"),
            ("reload", "Reload configuration"),
            ("report", "Writes a report on WorldEdit"),
            ("trace", "Toggles trace hook"),
            ("tz", "Set your timezone for snapshots"),
            ("version", "Get WorldEdit version"),
        ],
    ),
    (
        "tool",
        &[
            ("cycler", "Block data cycler tool"),
            ("floodfill", "Flood fill tool"),
            ("info", "Block information tool"),
            ("none", "Unbind a bound tool from your current item"),
            ("repl", "Block replacer tool"),
            ("selwand", "Selection wand tool"),
        ],
    ),
];

fn find(word: &str) -> Option<&'static Entry> {
    let w = word.to_ascii_lowercase();
    let named = |n: &str| ENTRIES.iter().find(|e| e.name == n || e.aliases.contains(&n));
    // As WorldEdit looks it up: as typed, then without a slash, then with one.
    named(&w).or_else(|| w.strip_prefix('/').and_then(named)).or_else(|| named(&format!("/{w}")))
}

/// Whether `word` names one of the commands this server has.
pub(super) fn knows(word: &str) -> bool {
    find(word).is_some()
}

/// The player-facing name: `/set` is typed as `//set`, `tool` as `/tool`.
fn typed(name: &str) -> String {
    format!("/{name}")
}

/// A title between rules, centered in 47 columns as WorldEdit's `MessageBox` is.
pub(super) fn titled(title: &str) -> String {
    let side = 47usize.saturating_sub(title.chars().count()) / 2;
    let rule = "-".repeat(side.saturating_sub(1));
    match side {
        0 => title.to_owned(),
        1 => format!(" {title} "),
        _ => format!("{rule} {title} {rule}"),
    }
}

/// `lines` as WorldEdit's `PaginationBox` shows them: eight a page, a footer with the page number.
pub(super) fn paged(title: &str, lines: &[String], page: usize) -> Result<Vec<String>, String> {
    const PER_PAGE: usize = 8;
    if page == 1 && lines.is_empty() {
        return Ok(vec!["No results found.".into()]);
    }
    let pages = lines.len().div_ceil(PER_PAGE);
    if page < 1 || page > pages {
        return Err("Invalid page number".into());
    }
    let mut out = vec![titled(title)];
    out.extend(lines.iter().skip((page - 1) * PER_PAGE).take(PER_PAGE).cloned());
    if pages > 1 {
        let footer = format!("{}Page {page} of {pages}{}", if page > 1 { "<<< " } else { "" }, if page < pages { " >>>" } else { "" });
        out.push(titled(&footer));
    }
    Ok(out)
}

fn clean(name: &str) -> String {
    name.trim_start_matches('/').to_ascii_lowercase()
}

impl Engine {
    /// `//help [-s] [-p page] [command...]`.
    pub(super) fn help(&self, args: &Args) -> R {
        let page: usize = match args.value('p') {
            Some(p) => p.parse().map_err(|_| number_error(p))?,
            None => 1,
        };
        let Some(first) = args.get(0) else {
            let mut all: Vec<&Entry> = ENTRIES.iter().collect();
            all.sort_by_key(|e| clean(e.name));
            let lines: Vec<String> = all.iter().map(|e| format!("? {}: {}", typed(e.name), e.desc)).collect();
            return paged("Help", &lines, page);
        };
        let Some(entry) = find(first) else {
            return Err(format!("The command '{first}' could not be found."));
        };
        let subs: &[(&str, &str)] = SUBS.iter().find(|(n, _)| *n == entry.name).map_or(&[], |(_, s)| s);
        let mut path = typed(entry.name);
        let mut desc = entry.desc;
        let mut sub_of_sub = false;
        if let Some(sub) = args.get(1) {
            if subs.is_empty() {
                let mut out = vec![format!("'{path}' has no sub-commands. (Maybe '{sub}' is for a parameter?)")];
                out.extend(self.usage_box(entry, &path, desc));
                return Err(out.join("\n"));
            }
            match subs.iter().find(|(n, _)| n.eq_ignore_ascii_case(sub)) {
                Some((n, d)) => {
                    path = format!("{path} {n}");
                    desc = d;
                    sub_of_sub = true;
                }
                None => {
                    let lines: Vec<String> = subs.iter().map(|(n, d)| format!("? {n}: {d}")).collect();
                    let mut out = vec![format!("The sub-command '{sub}' under '{path}' could not be found.")];
                    out.extend(paged(&format!("Subcommands: {path}"), &lines, page)?);
                    return Err(out.join("\n"));
                }
            }
        }
        if !sub_of_sub && args.flag('s') && !subs.is_empty() {
            let lines: Vec<String> = subs.iter().map(|(n, d)| format!("? {n}: {d}")).collect();
            return paged(&format!("Subcommands: {path}"), &lines, page);
        }
        if sub_of_sub {
            return Ok(vec![titled(&format!("Help for {path}")), desc.to_owned(), format!("Usage: {path}")]);
        }
        Ok(self.usage_box(entry, &path, desc))
    }

    /// The box for one command: its description and usage.
    fn usage_box(&self, entry: &Entry, path: &str, desc: &str) -> Vec<String> {
        let base = clean(entry.name);
        let full = usage(&base);
        let rest = full.split_once(&base).map_or("", |(_, r)| r);
        let mut out = vec![titled(&format!("Help for {path}")), desc.to_owned(), format!("Usage: {}{}", path, rest.trim_end())];
        if !entry.aliases.is_empty() {
            out.push(format!("Aliases: {}", entry.aliases.iter().map(|a| typed(a)).collect::<Vec<_>>().join(", ")));
        }
        if SUBS.iter().any(|(n, _)| *n == entry.name) {
            out.push("> List Subcommands (//help -s)".into());
        }
        out
    }

    /// `/we <sub-command>`.
    pub(super) fn worldedit(&mut self, world: &mut dyn World, actor: &Actor, args: &Args, env: &Env) -> R {
        let _ = world;
        let Some(sub) = args.get(0).map(str::to_ascii_lowercase) else {
            return Err("Usage: /worldedit <version|reload|report|trace|cui|tz|help> ...".into());
        };
        match sub.as_str() {
            "version" | "ver" => Ok(vec![
                format!("WorldEdit version {} (for Horizon)", env!("CARGO_PKG_VERSION")),
                "https://github.com/EngineHub/WorldEdit/".into(),
                titled("Platforms"),
                "* Horizon (dedicated server)".into(),
                titled("Capabilities"),
                "CONFIGURATION: Horizon".into(),
                "WORLDEDIT_CUI: Horizon".into(),
                "GAME_HOOKS: Horizon".into(),
                "PERMISSIONS: Horizon".into(),
                "USER_COMMANDS: Horizon".into(),
                "WORLD_EDITING: Horizon".into(),
            ]),
            "reload" => {
                let file = env.config_file.file_name().map_or_else(|| "worldedit.toml".to_owned(), |f| f.to_string_lossy().into_owned());
                let text = std::fs::read_to_string(&env.config_file).map_err(|_| format!("{file} could not be read."))?;
                let (config, problem) = Config::parse(&text);
                if let Some(p) = problem {
                    return Err(p);
                }
                self.config = config;
                Ok(vec!["Configuration reloaded!".into()])
            }
            "report" => {
                let dir = env.schematics_dir.parent().map(Path::to_path_buf).ok_or("Failed to write report: no folder to write it in")?;
                let mut text = format!("WorldEdit report (Horizon dedicated server)\nversion {}\n\n{:#?}\n", env!("CARGO_PKG_VERSION"), self.config);
                text.push_str(&format!("\nplayers with a session: {}\n", self.sessions.len()));
                std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join("report.txt"), text)).map_err(|e| format!("Failed to write report: {}", e.kind()))?;
                let shown = dir.file_name().map_or_else(|| "report.txt".to_owned(), |d| format!("{}/report.txt", d.to_string_lossy()));
                Ok(vec![format!("WorldEdit report written to {shown}")])
            }
            "trace" => {
                let wanted = match args.get(1).map(str::to_ascii_lowercase).as_deref() {
                    None => None,
                    Some("active") => Some(true),
                    Some("inactive") => Some(false),
                    Some(other) => return Err(format!("Unknown value '{other}' for the trace mode: active or inactive")),
                };
                let s = self.sessions.get(actor.uuid);
                let now = wanted.unwrap_or(!s.trace);
                if wanted == Some(s.trace) {
                    return Err(if s.trace { "Trace mode already active." } else { "Trace mode already inactive." }.into());
                }
                s.trace = now;
                Ok(vec![if now { "Trace mode now active." } else { "Trace mode now inactive." }.into()])
            }
            // The CUI handshake of the WorldEdit client mod: this server has no plugin channel to answer on.
            "cui" => Ok(Vec::new()),
            "tz" => self.timezone(actor, args, env),
            other => Err(format!("The sub-command '{other}' under 'worldedit' could not be found.")),
        }
    }

    /// `/we tz <timezone>`: `UTC`, `GMT`, `Z` or an offset such as `+01:00`, `-5` or `UTC+2`. Zone names
    /// (`Europe/Berlin`) need a time zone database this server does not have.
    fn timezone(&mut self, actor: &Actor, args: &Args, env: &Env) -> R {
        let zone = args.get(1).ok_or("Usage: /worldedit tz <timezone>")?;
        let Some((minutes, shown)) = parse_offset(zone) else {
            return Err(if zone.contains('/') { "Time zone names are not known here: use an offset such as +01:00 or UTC".into() } else { "Invalid timezone".to_owned() });
        };
        self.sessions.get(actor.uuid).timezone = Some((minutes, shown.clone()));
        let local = env.now_ms / 1000 + i64::from(minutes) * 60;
        let (days, secs) = (local.div_euclid(86_400), local.rem_euclid(86_400));
        let (y, m, d) = civil_from_days(days);
        Ok(vec![format!("Timezone set for this session to: {shown}"), format!("The current time in that timezone is: {y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)])
    }
}

/// `UTC`, `GMT`, `Z`, `+01:00`, `-5`, `UTC+2`: minutes east of UTC, and the name to show.
fn parse_offset(text: &str) -> Option<(i32, String)> {
    let upper = text.to_ascii_uppercase();
    let body = upper.strip_prefix("UTC").or_else(|| upper.strip_prefix("GMT")).unwrap_or(&upper);
    if body.is_empty() || body == "Z" {
        return Some((0, if upper.starts_with("GMT") { "GMT" } else { "UTC" }.to_owned()));
    }
    let sign = match body.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let (h, m) = match body[1..].split_once(':') {
        Some((h, m)) => (h, m),
        None => (&body[1..], "0"),
    };
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    if h > 18 || m > 59 {
        return None;
    }
    Some((sign * (h * 60 + m), format!("{}{:02}:{:02}", if sign < 0 { '-' } else { '+' }, h, m)))
}

/// The date of a day count since 1970-01-01 (Howard Hinnant's civil-from-days).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}
